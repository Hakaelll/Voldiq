use crate::{Engine, analysis_status::IndexId, snapshot_commands::SnapshotDatabase};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::State;
use voldiq_core::{
    NodeId,
    cleanup::{Plan, Preview},
    scanner::SharedIndex,
};

#[cfg(test)]
mod execution_tests {
    use super::*;
    use std::sync::{RwLock, atomic::AtomicBool};

    #[test]
    fn confirmation_requires_the_same_live_index_and_consumes_the_plan_once() {
        assert!(
            !tauri::is_dev(),
            "Verify the native application with --features voldiq/production."
        );
        crate::embedded_ui::verify(&tauri::generate_context!()).unwrap();
        assert!(cleanup_capabilities().recycling);
        let fixture = tempfile::tempdir().unwrap();
        std::fs::write(fixture.path().join("selected"), b"original").unwrap();
        let scan = || {
            voldiq_core::scanner::scan(
                fixture.path(),
                &voldiq_core::ScanOptions::default(),
                &AtomicBool::new(false),
                |_| {},
            )
            .unwrap()
        };
        let index = scan();
        let id = index
            .entries
            .iter()
            .find(|entry| entry.name == "selected")
            .unwrap()
            .id;
        let plan = Plan::manual(&index, &[id]).unwrap();
        let current = Arc::new(RwLock::new(index));
        let plans = Mutex::new(Some(Session {
            id: 42,
            _index: current.clone(),
            _plan: plan,
        }));
        for confirmation in ["", "delete", "RECYCLE"] {
            assert!(
                consume_plan(&plans, &current, 42, confirmation, CleanupAction::Permanent).is_err()
            );
        }
        assert!(consume_plan(&plans, &current, 43, "DELETE", CleanupAction::Permanent).is_err());
        let rescan = Arc::new(RwLock::new(scan()));
        assert!(consume_plan(&plans, &rescan, 42, "DELETE", CleanupAction::Permanent).is_err());
        assert!(plans.lock().unwrap().is_some());
        let session =
            consume_plan(&plans, &current, 42, "DELETE", CleanupAction::Permanent).unwrap();
        assert_eq!(session._plan.preview().items[0].id, id);
        assert!(consume_plan(&plans, &current, 42, "DELETE", CleanupAction::Permanent).is_err());
        *plans.lock().unwrap() = Some(Session {
            id: 44,
            _index: current.clone(),
            _plan: Plan::manual(&current.read().unwrap(), &[id]).unwrap(),
        });
        for confirmation in ["", "recycle", "DELETE"] {
            assert!(
                consume_plan(&plans, &current, 44, confirmation, CleanupAction::Recycle).is_err()
            );
        }
        assert!(consume_plan(&plans, &rescan, 44, "RECYCLE", CleanupAction::Recycle).is_err());
        let recycling =
            consume_plan(&plans, &current, 44, "RECYCLE", CleanupAction::Recycle).unwrap();
        assert_eq!(recycling._plan.preview().items[0].id, id);
        assert!(consume_plan(&plans, &current, 44, "RECYCLE", CleanupAction::Recycle).is_err());
        assert_eq!(
            std::fs::read(fixture.path().join("selected")).unwrap(),
            b"original"
        );
        let engine = Engine::new(fixture.path()).unwrap();
        let first = engine.publish(current.clone(), None).unwrap();
        assert!(engine.index_is_current(first, &current).unwrap());
        assert_eq!(engine.publish(current.clone(), Some(123)).unwrap(), first);
        let other = fixture.path().join("other");
        std::fs::create_dir(&other).unwrap();
        std::fs::write(other.join("unrelated"), b"other").unwrap();
        let other_index = Arc::new(RwLock::new(
            voldiq_core::scanner::scan(
                &other,
                &voldiq_core::ScanOptions::default(),
                &AtomicBool::new(false),
                |_| {},
            )
            .unwrap(),
        ));
        assert_eq!(
            other_index.read().unwrap().entry(id).unwrap().name,
            "unrelated"
        );
        let second = engine.publish(other_index.clone(), None).unwrap();
        assert_ne!(first, second);
        assert!(engine.index_for(first).is_err());
        assert!(!engine.index_is_current(first, &current).unwrap());
        assert!(Arc::ptr_eq(
            &engine.index_for(second).unwrap(),
            &other_index
        ));
        *engine.index.lock().unwrap() = None;
        assert!(engine.index_for(second).is_err());
        let reinstalled = engine.publish(other_index, None).unwrap();
        assert_ne!(reinstalled, second);
        let retained = engine.index_for(reinstalled).unwrap();
        assert!(
            crate::snapshot_commands::finish_snapshot_load(
                &engine,
                Ok(scan()),
                &AtomicBool::new(true)
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(engine.index_id().unwrap(), Some(reinstalled));
        assert!(Arc::ptr_eq(
            &engine.index_for(reinstalled).unwrap(),
            &retained
        ));
        let loaded = crate::snapshot_commands::finish_snapshot_load(
            &engine,
            Ok(scan()),
            &AtomicBool::new(false),
        )
        .unwrap()
        .unwrap();
        assert_ne!(loaded, reinstalled);
        assert!(!Arc::ptr_eq(&engine.index_for(loaded).unwrap(), &retained));
        crate::docker_commands::verify_disk_usage_versions();
        crate::lifecycle_trace::verify_fixture();
        crate::developer_commands::verify_pnpm_configuration();
    }
}

struct Session {
    id: u64,
    _index: SharedIndex,
    _plan: Plan,
}
#[derive(Default)]
pub struct CleanupPlans(Arc<Mutex<Option<Session>>>);

fn clear_loaded_analysis(
    engine: Arc<Engine>,
    plans: &Mutex<Option<Session>>,
    database: &Mutex<voldiq_core::snapshots::Store>,
) -> Result<(), String> {
    let _operation = engine.begin()?;
    let mut index = engine.index.lock().map_err(|error| error.to_string())?;
    let mut duplicates = engine
        .duplicates
        .lock()
        .map_err(|error| error.to_string())?;
    let mut plans = plans.lock().map_err(|error| error.to_string())?;
    database
        .lock()
        .map_err(|error| error.to_string())?
        .save_hash_cache(
            &voldiq_core::duplicates::HashCache::default(),
            &std::sync::atomic::AtomicBool::new(false),
        )
        .map_err(|error| error.to_string())?;
    *plans = None;
    *duplicates = None;
    *index = None;
    Ok(())
}

#[tauri::command]
pub async fn clear_current_analysis(
    engine: State<'_, Arc<Engine>>,
    plans: State<'_, CleanupPlans>,
    database: State<'_, SnapshotDatabase>,
) -> Result<(), String> {
    let engine = engine.inner().clone();
    let plans = plans.0.clone();
    let database = database.0.clone();
    tauri::async_runtime::spawn_blocking(move || clear_loaded_analysis(engine, &plans, &database))
        .await
        .map_err(|error| error.to_string())?
}

pub struct CleanupJournal(
    pub(crate) Arc<Mutex<Result<voldiq_core::cleanup::journal::Journal, String>>>,
);
impl CleanupJournal {
    pub fn open(path: &std::path::Path) -> Self {
        Self(Arc::new(Mutex::new(
            voldiq_core::cleanup::journal::Journal::open(path).map_err(|error| error.to_string()),
        )))
    }
}

#[derive(Clone, Serialize)]
pub struct CleanupProgress {
    id: u64,
    completed: usize,
    total: usize,
}

#[derive(Clone, Copy)]
enum CleanupAction {
    Permanent,
    Recycle,
}

fn consume_plan(
    plans: &Mutex<Option<Session>>,
    current: &SharedIndex,
    id: u64,
    confirmation: &str,
    action: CleanupAction,
) -> Result<Session, String> {
    let expected = match action {
        CleanupAction::Permanent => "DELETE",
        CleanupAction::Recycle => "RECYCLE",
    };
    if confirmation != expected {
        return Err(match action {
            CleanupAction::Permanent => "Type DELETE to confirm permanent deletion.",
            CleanupAction::Recycle => "Confirm moving the reviewed files to the Recycle Bin.",
        }
        .into());
    }
    let mut slot = plans.lock().map_err(|error| error.to_string())?;
    let session = slot
        .as_ref()
        .ok_or("Cleanup preview is no longer available.")?;
    if session.id != id || !Arc::ptr_eq(&session._index, current) {
        return Err("Cleanup preview belongs to a previous analysis. Review again.".into());
    }
    slot.take()
        .ok_or_else(|| "Cleanup preview is no longer available.".into())
}

#[tauri::command]
pub async fn execute_permanent_cleanup(
    id: u64,
    confirmation: String,
    progress: tauri::ipc::Channel<CleanupProgress>,
    engine: State<'_, Arc<Engine>>,
    plans: State<'_, CleanupPlans>,
    journal: State<'_, CleanupJournal>,
) -> Result<voldiq_core::cleanup::execute::Report, String> {
    let engine = engine.inner().clone();
    let operation = engine.begin()?;
    let operation_id = operation.id;
    let cancel = operation.cancel.clone();
    let current = engine.index()?;
    let session = consume_plan(
        &plans.0,
        &current,
        id,
        &confirmation,
        CleanupAction::Permanent,
    )?;
    let journal = journal.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        let mut journal = journal.lock().map_err(|error| error.to_string())?;
        let journal = journal.as_mut().map_err(|error| error.clone())?;
        let total = session._plan.preview().items.len();
        progress
            .send(CleanupProgress {
                id: operation_id,
                completed: 0,
                total,
            })
            .map_err(|error| error.to_string())?;
        *engine.index.lock().map_err(|error| error.to_string())? = None;
        *engine
            .duplicates
            .lock()
            .map_err(|error| error.to_string())? = None;
        voldiq_core::cleanup::execute::permanent(session._plan, journal, &cancel, |completed| {
            if progress
                .send(CleanupProgress {
                    id: operation_id,
                    completed,
                    total,
                })
                .is_err()
            {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        })
        .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[derive(Serialize)]
pub struct CleanupCapabilities {
    recycling: bool,
}

#[tauri::command]
pub fn cleanup_capabilities() -> CleanupCapabilities {
    CleanupCapabilities { recycling: true }
}

#[tauri::command]
pub async fn execute_recycling_cleanup(
    id: u64,
    confirmation: String,
    progress: tauri::ipc::Channel<CleanupProgress>,
    engine: State<'_, Arc<Engine>>,
    plans: State<'_, CleanupPlans>,
    journal: State<'_, CleanupJournal>,
) -> Result<voldiq_core::cleanup::recycle::batch::Report, String> {
    let engine = engine.inner().clone();
    let operation = engine.begin()?;
    let operation_id = operation.id;
    let cancel = operation.cancel.clone();
    let current = engine.index()?;
    let session = consume_plan(
        &plans.0,
        &current,
        id,
        &confirmation,
        CleanupAction::Recycle,
    )?;
    let journal = journal.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        let mut journal = journal.lock().map_err(|error| error.to_string())?;
        let journal = journal.as_mut().map_err(|error| error.clone())?;
        let total = session._plan.preview().items.len();
        progress
            .send(CleanupProgress {
                id: operation_id,
                completed: 0,
                total,
            })
            .map_err(|error| error.to_string())?;
        *engine.index.lock().map_err(|error| error.to_string())? = None;
        *engine
            .duplicates
            .lock()
            .map_err(|error| error.to_string())? = None;
        voldiq_core::cleanup::recycle::batch::run(session._plan, journal, &cancel, |completed| {
            if progress
                .send(CleanupProgress {
                    id: operation_id,
                    completed,
                    total,
                })
                .is_err()
            {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        })
        .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn query_cleanup_history(
    offset: usize,
    limit: usize,
    journal: State<'_, CleanupJournal>,
) -> Result<Vec<voldiq_core::cleanup::journal::ActivityItem>, String> {
    let journal = journal.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        journal
            .lock()
            .map_err(|error| error.to_string())?
            .as_ref()
            .map_err(|error| error.clone())?
            .activity(offset, limit)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[derive(Serialize)]
pub struct CleanupPreview {
    id: u64,
    preview: Preview,
}

#[tauri::command]
pub async fn query_recycle_bin(
    drive: String,
) -> Result<voldiq_core::windows::recycle_bin::Summary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        voldiq_core::windows::recycle_bin::query(&drive).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn query_cleanup_rule(
    index_id: Option<IndexId>,
    app: tauri::AppHandle,
    rule: voldiq_core::cleanup::rules::RuleKind,
    offset: usize,
    limit: usize,
    engine: State<'_, Arc<Engine>>,
) -> Result<voldiq_core::cleanup::rules::Page, String> {
    let caches = crate::developer_commands::cache_locations(&app);
    let directory = engine.operations.clone();
    let index = match index_id {
        Some(id) => Some(engine.index_for(id)?),
        None if engine.index_id()?.is_none() => None,
        None => return Err("Analysis changed. Refresh the view before continuing.".into()),
    };
    tauri::async_runtime::spawn_blocking(move || {
        let root = std::env::temp_dir();
        let protections = voldiq_core::metadata::Store::open(directory.directory())
            .and_then(|mut store| store.protections())
            .map_err(|e| e.to_string())?;
        let guard = index
            .as_ref()
            .map(|index| index.read())
            .transpose()
            .map_err(|e| e.to_string())?;
        let now = voldiq_core::cleanup::rules::now().map_err(|e| e.to_string())?;
        match rule {
            voldiq_core::cleanup::rules::RuleKind::UserTemp => voldiq_core::cleanup::rules::query(
                guard.as_deref(),
                &root,
                now,
                offset,
                limit,
                &protections,
            ),
            voldiq_core::cleanup::rules::RuleKind::OldFiles => {
                voldiq_core::cleanup::rules::query_old(
                    guard.as_deref(),
                    now,
                    offset,
                    limit,
                    &protections,
                )
            }
            voldiq_core::cleanup::rules::RuleKind::BrowserCache => {
                let local_data =
                    voldiq_core::windows::local_app_data().map_err(|e| e.to_string())?;
                voldiq_core::cleanup::rules::query_browser_cache(
                    guard.as_deref(),
                    &local_data,
                    now,
                    offset,
                    limit,
                    &protections,
                )
            }
            voldiq_core::cleanup::rules::RuleKind::ProjectTemp => {
                voldiq_core::cleanup::rules::query_project(
                    guard.as_deref(),
                    now,
                    offset,
                    limit,
                    &caches,
                    &protections,
                )
            }
        }
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn preview_cleanup_rule(
    index_id: IndexId,
    app: tauri::AppHandle,
    rule: voldiq_core::cleanup::rules::RuleKind,
    selection: Vec<NodeId>,
    engine: State<'_, Arc<Engine>>,
    plans: State<'_, CleanupPlans>,
) -> Result<CleanupPreview, String> {
    let caches = crate::developer_commands::cache_locations(&app);
    let engine = engine.inner().clone();
    let operation = engine.begin()?;
    let id = operation.id;
    let index = engine.index_for(index_id)?;
    let plans = plans.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        let guard = index.read().map_err(|e| e.to_string())?;
        let now = voldiq_core::cleanup::rules::now().map_err(|e| e.to_string())?;
        let plan = match rule {
            voldiq_core::cleanup::rules::RuleKind::UserTemp => {
                voldiq_core::cleanup::rules::preview(&guard, &std::env::temp_dir(), now, &selection)
            }
            voldiq_core::cleanup::rules::RuleKind::OldFiles => {
                voldiq_core::cleanup::rules::preview_old(&guard, now, &selection)
            }
            voldiq_core::cleanup::rules::RuleKind::BrowserCache => {
                let local_data =
                    voldiq_core::windows::local_app_data().map_err(|e| e.to_string())?;
                voldiq_core::cleanup::rules::preview_browser_cache(
                    &guard,
                    &local_data,
                    now,
                    &selection,
                )
            }
            voldiq_core::cleanup::rules::RuleKind::ProjectTemp => {
                voldiq_core::cleanup::rules::preview_project(&guard, now, &selection, &caches)
            }
        }
        .and_then(|plan| plan.with_protections(engine.operations.directory()))
        .map_err(|e| e.to_string())?;
        drop(guard);
        let preview = plan.preview().clone();
        *plans.lock().map_err(|e| e.to_string())? = Some(Session {
            id,
            _index: index,
            _plan: plan,
        });
        Ok(CleanupPreview { id, preview })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn preview_cleanup(
    index_id: IndexId,
    selection: Vec<NodeId>,
    engine: State<'_, Arc<Engine>>,
    plans: State<'_, CleanupPlans>,
) -> Result<CleanupPreview, String> {
    let engine = engine.inner().clone();
    let operation = engine.begin()?;
    let id = operation.id;
    let index = engine.index_for(index_id)?;
    let plans = plans.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        let guard = index.read().map_err(|e| e.to_string())?;
        let plan = Plan::manual(&guard, &selection)
            .and_then(|plan| plan.with_protections(engine.operations.directory()))
            .map_err(|e| e.to_string())?;
        drop(guard);
        let preview = plan.preview().clone();
        *plans.lock().map_err(|e| e.to_string())? = Some(Session {
            id,
            _index: index,
            _plan: plan,
        });
        Ok(CleanupPreview { id, preview })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn discard_cleanup(id: u64, plans: State<'_, CleanupPlans>) -> Result<(), String> {
    let mut slot = plans.0.lock().map_err(|e| e.to_string())?;
    if slot.as_ref().is_some_and(|plan| plan.id == id) {
        *slot = None;
    }
    Ok(())
}

#[tauri::command]
pub async fn preview_duplicate_cleanup(
    index_id: IndexId,
    selection: Vec<NodeId>,
    progress: tauri::ipc::Channel<u64>,
    engine: State<'_, Arc<Engine>>,
    plans: State<'_, CleanupPlans>,
) -> Result<CleanupPreview, String> {
    let engine = engine.inner().clone();
    let operation = engine.begin()?;
    let id = operation.id;
    let cancel = operation.cancel.clone();
    let index = engine.index_for(index_id)?;
    let run = engine
        .duplicates
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or("Analyze duplicates first.")?;
    if !Arc::ptr_eq(&index, &run.index) {
        return Err("Duplicate analysis belongs to an older scan.".into());
    }
    progress.send(id).map_err(|e| e.to_string())?;
    let plans = plans.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        let guard = index.read().map_err(|e| e.to_string())?;
        run.analysis
            .check_protections(&engine.protections()?)
            .map_err(|error| error.to_string())?;
        let plan = Plan::duplicates(&guard, &run.analysis, &selection, &cancel)
            .and_then(|plan| plan.with_protections(engine.operations.directory()))
            .map_err(|e| e.to_string())?;
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("Duplicate review cancelled.".into());
        }
        drop(guard);
        let preview = plan.preview().clone();
        *plans.lock().map_err(|e| e.to_string())? = Some(Session {
            id,
            _index: index,
            _plan: plan,
        });
        Ok(CleanupPreview { id, preview })
    })
    .await
    .map_err(|e| e.to_string())?
}
