use crate::{Engine, analysis_status::IndexId};
use std::sync::Arc;
use tauri::State;
use voldiq_core::{
    NodeId,
    budgets::{Budget, Store},
};

#[tauri::command]
pub(crate) async fn save_folder_budget(
    engine: State<'_, Arc<Engine>>,
    index_id: IndexId,
    node_id: NodeId,
    version: String,
    expression: String,
) -> Result<Budget, String> {
    let index = engine.index_for(index_id)?;
    let directory = engine.operations.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let index = index.read().map_err(|e| e.to_string())?;
        Store::open(directory.directory())
            .and_then(|mut store| store.save(&index, node_id, &version, &expression))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) async fn remove_folder_budget(
    engine: State<'_, Arc<Engine>>,
    index_id: IndexId,
    node_id: NodeId,
    budget_id: i64,
    version: String,
) -> Result<(), String> {
    let index = engine.index_for(index_id)?;
    let directory = engine.operations.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let index = index.read().map_err(|e| e.to_string())?;
        Store::open(directory.directory())
            .and_then(|mut store| store.remove(&index, node_id, budget_id, &version))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
