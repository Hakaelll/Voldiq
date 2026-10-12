use crate::{Engine, Operation};
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
            assert!(consume_plan(&plans, &current, 42, confirmation).is_err());
        }
        assert!(consume_plan(&plans, &current, 43, "DELETE").is_err());
        let rescan = Arc::new(RwLock::new(scan()));
        assert!(consume_plan(&plans, &rescan, 42, "DELETE").is_err());
        assert!(plans.lock().unwrap().is_some());
        let session = consume_plan(&plans, &current, 42, "DELETE").unwrap();
        assert_eq!(session._plan.preview().items[0].id, id);
        assert!(consume_plan(&plans, &current, 42, "DELETE").is_err());
        assert_eq!(
            std::fs::read(fixture.path().join("selected")).unwrap(),
            b"original"
        );
    }
}

struct Session {
    id: u64,
    _index: SharedIndex,
    _plan: Plan,
}
#[derive(Default)]
pub struct CleanupPlans(Arc<Mutex<Option<Session>>>);

pub struct CleanupJournal(Arc<Mutex<Result<voldiq_core::cleanup::journal::Journal, String>>>);
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

fn consume_plan(
    plans: &Mutex<Option<Session>>,
    current: &SharedIndex,
    id: u64,
    confirmation: &str,
) -> Result<Session, String> {
    if confirmation != "DELETE" {
        return Err("Type DELETE to confirm permanent deletion.".into());
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
    let (operation_id, cancel) = engine.begin()?;
    let operation = Operation {
        engine: engine.clone(),
        id: operation_id,
    };
    let current = engine.index()?;
    let session = consume_plan(&plans.0, &current, id, &confirmation)?;
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

#[tauri::command]
pub async fn query_cleanup_history(
    offset: usize,
    limit: usize,
    journal: State<'_, CleanupJournal>,
) -> Result<Vec<voldiq_core::cleanup::journal::Item>, String> {
    let journal = journal.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        journal
            .lock()
            .map_err(|error| error.to_string())?
            .as_ref()
            .map_err(|error| error.clone())?
            .items(offset, limit)
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
    rule: voldiq_core::cleanup::rules::RuleKind,
    offset: usize,
    limit: usize,
    engine: State<'_, Arc<Engine>>,
) -> Result<voldiq_core::cleanup::rules::Page, String> {
    let index = engine.index.lock().map_err(|e| e.to_string())?.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let root = std::env::temp_dir();
        let guard = index
            .as_ref()
            .map(|index| index.read())
            .transpose()
            .map_err(|e| e.to_string())?;
        let now = voldiq_core::cleanup::rules::now().map_err(|e| e.to_string())?;
        match rule {
            voldiq_core::cleanup::rules::RuleKind::UserTemp => {
                voldiq_core::cleanup::rules::query(guard.as_deref(), &root, now, offset, limit)
            }
            voldiq_core::cleanup::rules::RuleKind::OldFiles => {
                voldiq_core::cleanup::rules::query_old(guard.as_deref(), now, offset, limit)
            }
        }
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn preview_cleanup_rule(
    rule: voldiq_core::cleanup::rules::RuleKind,
    selection: Vec<NodeId>,
    engine: State<'_, Arc<Engine>>,
    plans: State<'_, CleanupPlans>,
) -> Result<CleanupPreview, String> {
    let engine = engine.inner().clone();
    let (id, _) = engine.begin()?;
    let operation = Operation {
        engine: engine.clone(),
        id,
    };
    let index = engine.index()?;
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
        }
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
    selection: Vec<NodeId>,
    engine: State<'_, Arc<Engine>>,
    plans: State<'_, CleanupPlans>,
) -> Result<CleanupPreview, String> {
    let engine = engine.inner().clone();
    let index = engine.index()?;
    let (id, _) = engine.begin()?;
    let operation = Operation { engine, id };
    let plans = plans.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        let guard = index.read().map_err(|e| e.to_string())?;
        let plan = Plan::manual(&guard, &selection).map_err(|e| e.to_string())?;
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
    selection: Vec<NodeId>,
    progress: tauri::ipc::Channel<u64>,
    engine: State<'_, Arc<Engine>>,
    plans: State<'_, CleanupPlans>,
) -> Result<CleanupPreview, String> {
    let engine = engine.inner().clone();
    let (id, cancel) = engine.begin()?;
    let operation = Operation {
        engine: engine.clone(),
        id,
    };
    let index = engine.index()?;
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
        let plan = Plan::duplicates(&guard, &run.analysis, &selection, &cancel)
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
