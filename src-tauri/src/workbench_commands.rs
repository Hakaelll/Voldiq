use crate::{
    Engine, SnapshotDatabase,
    analysis_status::{IndexId, ScanProgress},
};
use serde::Deserialize;
use std::sync::Arc;
use tauri::{State, ipc::Channel};
use voldiq_core::{
    NodeId, ScanOptions,
    workbench::{Catalog, RecordRef, SavedView, Store, ViewSpec},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SaveWorkspace {
    id: Option<i64>,
    revision: String,
    name: String,
    spec: ViewSpec,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RecordKind {
    View,
    Favorite,
}

#[tauri::command]
pub(crate) async fn list_workspaces(engine: State<'_, Arc<Engine>>) -> Result<Catalog, String> {
    let directory = engine.operations.clone();
    tauri::async_runtime::spawn_blocking(move || Store::open(directory.directory())?.catalog())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub(crate) async fn save_workspace(
    engine: State<'_, Arc<Engine>>,
    request: SaveWorkspace,
) -> Result<Catalog, String> {
    let directory = engine.operations.clone();
    tauri::async_runtime::spawn_blocking(move || {
        Store::open(directory.directory())?.save_view(
            request.id,
            &request.revision,
            &request.name,
            request.spec,
        )
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}
#[tauri::command]
pub(crate) async fn load_workspace(
    engine: State<'_, Arc<Engine>>,
    reference: RecordRef,
) -> Result<SavedView, String> {
    let directory = engine.operations.clone();
    tauri::async_runtime::spawn_blocking(move || {
        Store::open(directory.directory())?.view(&reference)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}
#[tauri::command]
pub(crate) async fn remove_workspace(
    engine: State<'_, Arc<Engine>>,
    reference: RecordRef,
    kind: RecordKind,
) -> Result<Catalog, String> {
    let directory = engine.operations.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut store = Store::open(directory.directory())?;
        match kind {
            RecordKind::View => store.remove_view(&reference),
            RecordKind::Favorite => store.remove_favorite(&reference),
        }
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}
#[tauri::command]
pub(crate) async fn favorite_folder(
    engine: State<'_, Arc<Engine>>,
    index_id: IndexId,
    node_id: NodeId,
    revision: String,
) -> Result<Catalog, String> {
    let index = engine.index_for(index_id)?;
    let directory = engine.operations.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let index = index.read().map_err(|e| e.to_string())?;
        Store::open(directory.directory())
            .and_then(|mut store| store.add_favorite(&revision, &index, node_id))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

// Human GUI only; a stored shortcut grants no agent capability or cleanup permission.
#[tauri::command]
pub(crate) async fn start_favorite_scan(
    engine: State<'_, Arc<Engine>>,
    database: State<'_, SnapshotDatabase>,
    reference: RecordRef,
    options: ScanOptions,
    progress: Channel<ScanProgress>,
) -> Result<u64, String> {
    let directory = engine.operations.clone();
    let folder = tauri::async_runtime::spawn_blocking(move || {
        Store::open(directory.directory()).and_then(|store| store.pin_favorite(&reference))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let path = folder
        .path()
        .to_str()
        .ok_or("Favorite path cannot be represented exactly.")?
        .to_owned();
    crate::launch_scan(
        path,
        options,
        progress,
        engine.inner().clone(),
        &database,
        Some(folder),
    )
}
