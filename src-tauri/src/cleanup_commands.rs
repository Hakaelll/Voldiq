use crate::{Engine, Operation};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::State;
use voldiq_core::{
    NodeId,
    cleanup::{Plan, Preview},
    scanner::SharedIndex,
};

struct Session {
    id: u64,
    _index: SharedIndex,
    _plan: Plan,
}
#[derive(Default)]
pub struct CleanupPlans(Arc<Mutex<Option<Session>>>);

#[derive(Serialize)]
pub struct CleanupPreview {
    id: u64,
    preview: Preview,
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
