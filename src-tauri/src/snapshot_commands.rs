use crate::{Engine, Operation};
use std::{
    path::Path,
    sync::{Arc, Mutex, RwLock},
};
use tauri::State;
use voldiq_core::snapshots::{Comparison, Snapshot, Store};

pub struct SnapshotDatabase(pub(super) Arc<Mutex<Store>>);

impl SnapshotDatabase {
    pub fn open(path: &Path) -> voldiq_core::Result<Self> {
        Ok(Self(Arc::new(Mutex::new(Store::open(path)?))))
    }
}

#[tauri::command]
pub async fn save_snapshot(
    engine: State<'_, Arc<Engine>>,
    database: State<'_, SnapshotDatabase>,
) -> Result<i64, String> {
    let engine = engine.inner().clone();
    let (id, cancel) = engine.begin()?;
    let operation = Operation {
        engine: engine.clone(),
        id,
    };
    let index = engine.index()?;
    let store = database.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        let index = index.read().map_err(|e| e.to_string())?;
        if index.snapshot_id.is_some() {
            return Err("Rescan before saving a new snapshot.".into());
        }
        store
            .lock()
            .map_err(|e| e.to_string())?
            .save(&index, &cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn list_snapshots(
    offset: usize,
    limit: usize,
    database: State<'_, SnapshotDatabase>,
) -> Result<Vec<Snapshot>, String> {
    let store = database.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .lock()
            .map_err(|e| e.to_string())?
            .list(offset, limit)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn load_snapshot(
    id: i64,
    engine: State<'_, Arc<Engine>>,
    database: State<'_, SnapshotDatabase>,
) -> Result<(), String> {
    let engine = engine.inner().clone();
    let (operation_id, _) = engine.begin()?;
    let operation = Operation {
        engine: engine.clone(),
        id: operation_id,
    };
    let store = database.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        let index = store
            .lock()
            .map_err(|e| e.to_string())?
            .load(id)
            .map_err(|e| e.to_string())?;
        engine.publish(Arc::new(RwLock::new(index)), None)?;
        *engine.duplicates.lock().map_err(|e| e.to_string())? = None;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn compare_snapshots(
    before: i64,
    after: i64,
    offset: usize,
    limit: usize,
    database: State<'_, SnapshotDatabase>,
) -> Result<Comparison, String> {
    let store = database.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .lock()
            .map_err(|e| e.to_string())?
            .compare(before, after, offset, limit)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn delete_snapshot(id: i64, database: State<'_, SnapshotDatabase>) -> Result<(), String> {
    let store = database.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .lock()
            .map_err(|e| e.to_string())?
            .delete(id)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn snapshot_storage(database: State<'_, SnapshotDatabase>) -> Result<u64, String> {
    let store = database.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .lock()
            .map_err(|e| e.to_string())?
            .disk_bytes()
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn compact_snapshots(database: State<'_, SnapshotDatabase>) -> Result<(), String> {
    let store = database.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .lock()
            .map_err(|e| e.to_string())?
            .compact()
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
