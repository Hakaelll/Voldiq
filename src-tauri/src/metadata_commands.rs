use crate::Engine;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc};
use tauri::State;
use voldiq_core::metadata::{Note, Store};

#[derive(Serialize)]
pub struct FolderMetadata {
    folder: Note,
    protected_by_parent: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveNote {
    path: PathBuf,
    note: String,
}

#[derive(Serialize)]
pub struct ProtectedPage {
    items: Vec<Note>,
    next_offset: Option<usize>,
}

fn read(store: &mut Store, path: &std::path::Path) -> voldiq_core::Result<FolderMetadata> {
    let folder = store.get(path)?;
    let protected_by_parent = !folder.protected && store.protections()?.protects(path)?;
    Ok(FolderMetadata {
        folder,
        protected_by_parent,
    })
}

#[tauri::command]
pub async fn get_folder_metadata(
    engine: State<'_, Arc<Engine>>,
    path: PathBuf,
) -> Result<FolderMetadata, String> {
    let directory = engine.operations.clone();
    tauri::async_runtime::spawn_blocking(move || {
        read(&mut Store::open(directory.directory())?, &path)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn save_folder_note(
    engine: State<'_, Arc<Engine>>,
    request: SaveNote,
) -> Result<FolderMetadata, String> {
    let directory = engine.operations.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut store = Store::open(directory.directory())?;
        store.save_note(&request.path, &request.note)?;
        read(&mut store, &request.path)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn protect_folder(
    engine: State<'_, Arc<Engine>>,
    path: PathBuf,
) -> Result<FolderMetadata, String> {
    let directory = engine.operations.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut store = Store::open(directory.directory())?;
        store.protect_folder(&path)?;
        read(&mut store, &path)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn list_protected_folders(
    engine: State<'_, Arc<Engine>>,
    offset: usize,
    limit: usize,
) -> Result<ProtectedPage, String> {
    let directory = engine.operations.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut items = Store::open(directory.directory())?.protected_folders(offset, limit)?;
        let next_offset = (items.len() > limit).then_some(offset + limit);
        items.truncate(limit);
        Ok::<_, voldiq_core::Error>(ProtectedPage { items, next_offset })
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

// Human GUI only. No equivalent is registered in the CLI or MCP adapter.
#[tauri::command]
pub async fn remove_folder_protection(
    engine: State<'_, Arc<Engine>>,
    id: i64,
    version: String,
) -> Result<bool, String> {
    let directory = engine.operations.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut store = Store::open(directory.directory()).map_err(|error| error.to_string())?;
        let folder = store.protection(id,&version).map_err(|error| error.to_string())?;
        let answer = rfd::MessageDialog::new().set_title("Voldiq · Remove folder protection")
            .set_description(format!("Allow cleanup plans to select this folder's contents?\n\nRecorded path: {}\n\nProtection follows the folder if renamed and also guards this recorded path. Removing it invalidates existing agent plans. Notes are kept.",folder.path.display()))
            .set_buttons(rfd::MessageButtons::YesNo).set_level(rfd::MessageLevel::Warning).show();
        if answer != rfd::MessageDialogResult::Yes { return Ok(false); }
        store.remove_protection(id,&version).map_err(|error| error.to_string())?;
        Ok(true)
    }).await.map_err(|error| error.to_string())?
}
