use crate::{Engine, analysis_status::IndexId, snapshot_commands::SnapshotDatabase};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::State;
use voldiq_core::{
    cleanup::{Plan, Preview},
    scanner::SharedIndex,
};

struct Session {
    id: u64,
    index_id: IndexId,
    _index: SharedIndex,
    _plan: Plan,
}
#[derive(Default)]
pub struct CleanupPlans(Arc<Mutex<Option<Session>>>);

fn clear_loaded_analysis(
    engine: Arc<Engine>,
    plans: &Mutex<Option<Session>>,
    database: &crate::snapshot_commands::SnapshotStore,
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
    pub fn unavailable() -> Self {
        Self(Arc::new(Mutex::new(Err(
            "Cleanup history is unavailable in a temporary session.".into(),
        ))))
    }

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
    current_id: IndexId,
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
    if session.id != id || session.index_id != current_id || !Arc::ptr_eq(&session._index, current)
    {
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
    engine.require_durable_state()?;
    let engine = engine.inner().clone();
    let operation = engine.begin()?;
    let operation_id = operation.id;
    let cancel = operation.cancel.clone();
    let current_id = engine
        .index_id()?
        .ok_or("Analyze a drive or folder first.")?;
    let current = engine.index_for(current_id)?;
    let session = consume_plan(
        &plans.0,
        &current,
        current_id,
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
    engine.require_durable_state()?;
    let engine = engine.inner().clone();
    let operation = engine.begin()?;
    let operation_id = operation.id;
    let cancel = operation.cancel.clone();
    let current_id = engine
        .index_id()?
        .ok_or("Analyze a drive or folder first.")?;
    let current = engine.index_for(current_id)?;
    let session = consume_plan(
        &plans.0,
        &current,
        current_id,
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
    origins: Vec<voldiq_core::cleanup::tray::Origin>,
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
    request: voldiq_core::cleanup::rules::Query,
    engine: State<'_, Arc<Engine>>,
) -> Result<voldiq_core::cleanup::rules::Page, String> {
    let caches = crate::developer_commands::cache_locations(&app);
    let engine = engine.inner().clone();
    let index = match index_id {
        Some(id) => Some(engine.index_for(id)?),
        None if engine.index_id()?.is_none() => None,
        None => return Err("Analysis changed. Refresh the view before continuing.".into()),
    };
    let duplicates = engine
        .duplicates
        .lock()
        .map_err(|error| error.to_string())?
        .clone()
        .filter(|run| {
            Some(run.index_id) == index_id
                && index
                    .as_ref()
                    .is_some_and(|index| Arc::ptr_eq(&run.index, index))
        });
    tauri::async_runtime::spawn_blocking(move || {
        use voldiq_core::cleanup::rules;
        let protections = engine.protections()?;
        let guard = index
            .as_ref()
            .map(|index| index.read())
            .transpose()
            .map_err(|error| error.to_string())?;
        let temporary = std::env::temp_dir();
        let local_data = if matches!(rule, rules::RuleKind::BrowserCache) {
            voldiq_core::windows::local_app_data().map_err(|error| error.to_string())?
        } else {
            temporary.clone()
        };
        let page = rules::query(
            guard.as_deref(),
            rule,
            request,
            &rules::Environment {
                temporary: &temporary,
                local_data: &local_data,
                caches: &caches,
                now: rules::now().map_err(|error| error.to_string())?,
                protections: &protections,
                duplicates: duplicates.as_ref().map(|run| &run.analysis),
            },
            &std::sync::atomic::AtomicBool::new(false),
        )
        .map_err(|error| error.to_string())?;
        if engine.index_id()? != index_id {
            return Err("Analysis changed. Refresh the view before continuing.".into());
        }
        Ok(page)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn preview_cleanup_tray(
    index_id: IndexId,
    app: tauri::AppHandle,
    groups: Vec<voldiq_core::cleanup::tray::Group>,
    progress: tauri::ipc::Channel<u64>,
    engine: State<'_, Arc<Engine>>,
    plans: State<'_, CleanupPlans>,
) -> Result<CleanupPreview, String> {
    let caches = crate::developer_commands::cache_locations(&app);
    let engine = engine.inner().clone();
    engine.require_durable_state()?;
    let operation = engine.begin()?;
    let id = operation.id;
    let index = engine.index_for(index_id)?;
    let duplicates = engine
        .duplicates
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
    if groups
        .iter()
        .any(|group| matches!(group.source, voldiq_core::cleanup::tray::Source::Duplicates))
        && duplicates
            .as_ref()
            .is_none_or(|run| run.index_id != index_id || !Arc::ptr_eq(&run.index, &index))
    {
        return Err("Analyze duplicates in the current analysis first.".into());
    }
    progress.send(id).map_err(|error| error.to_string())?;
    let plans = plans.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        let guard = index.read().map_err(|error| error.to_string())?;
        let prepared = voldiq_core::cleanup::tray::prepare(
            &guard,
            &groups,
            &voldiq_core::cleanup::tray::Environment {
                temporary: &std::env::temp_dir(),
                local_data: &voldiq_core::windows::local_app_data()
                    .map_err(|error| error.to_string())?,
                caches: &caches,
                state: engine.operations.directory(),
                now: voldiq_core::cleanup::rules::now().map_err(|error| error.to_string())?,
                duplicates: duplicates.as_ref().map(|run| &run.analysis),
            },
            &_operation.cancel,
        )
        .map_err(|error| error.to_string())?;
        if _operation.cancel.load(std::sync::atomic::Ordering::Relaxed)
            || !engine.index_is_current(index_id, &index)?
        {
            return Err("Cleanup review cancelled or analysis changed.".into());
        }
        drop(guard);
        let preview = prepared.plan.preview().clone();
        *plans.lock().map_err(|error| error.to_string())? = Some(Session {
            id,
            index_id,
            _index: index,
            _plan: prepared.plan,
        });
        Ok(CleanupPreview {
            id,
            preview,
            origins: prepared.origins,
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn discard_cleanup(id: u64, plans: State<'_, CleanupPlans>) -> Result<(), String> {
    let mut slot = plans.0.lock().map_err(|error| error.to_string())?;
    if slot.as_ref().is_some_and(|plan| plan.id == id) {
        *slot = None;
    }
    Ok(())
}
