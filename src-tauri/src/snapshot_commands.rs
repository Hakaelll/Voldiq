use crate::{Engine, Operation, analysis_status::IndexId};
use std::{
    path::Path,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{State, ipc::Channel};
use voldiq_core::snapshots::{Comparison, Snapshot, Store};

pub struct SnapshotDatabase(pub(super) Arc<Mutex<Store>>);

impl SnapshotDatabase {
    pub fn open(path: &Path) -> voldiq_core::Result<Self> {
        Ok(Self(Arc::new(Mutex::new(Store::open(path)?))))
    }
}

#[tauri::command]
pub async fn save_snapshot(
    index_id: IndexId,
    operation: Channel<u64>,
    app: tauri::AppHandle,
    engine: State<'_, Arc<Engine>>,
    database: State<'_, SnapshotDatabase>,
) -> Result<Option<i64>, String> {
    let engine = engine.inner().clone();
    let (id, cancel) = engine.begin()?;
    let active_operation = Operation {
        engine: engine.clone(),
        id,
    };
    let index = engine.index_for(index_id)?;
    operation.send(id).map_err(|error| error.to_string())?;
    let store = database.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = active_operation;
        let index = index.read().map_err(|e| e.to_string())?;
        if index.snapshot_id.is_some() {
            return Err("Rescan before saving a new snapshot.".into());
        }
        let caches = crate::developer_commands::cache_locations(&app);
        let saved = store
            .lock()
            .map_err(|error| error.to_string())?
            .save(&index, &cancel, &caches);
        match saved {
            Ok(id) => Ok(Some(id)),
            Err(_) if cancel.load(Ordering::Relaxed) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
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
    operation: Channel<u64>,
    engine: State<'_, Arc<Engine>>,
    database: State<'_, SnapshotDatabase>,
) -> Result<Option<IndexId>, String> {
    let engine = engine.inner().clone();
    let (operation_id, cancel) = engine.begin()?;
    let active_operation = Operation {
        engine: engine.clone(),
        id: operation_id,
    };
    operation
        .send(operation_id)
        .map_err(|error| error.to_string())?;
    let store = database.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = active_operation;
        let loaded = store
            .lock()
            .map_err(|error| error.to_string())?
            .load(id, &cancel);
        finish_snapshot_load(&engine, loaded, &cancel)
    })
    .await
    .map_err(|e| e.to_string())?
}

pub(crate) fn finish_snapshot_load(
    engine: &Engine,
    loaded: voldiq_core::Result<voldiq_core::index::Index>,
    cancel: &AtomicBool,
) -> Result<Option<IndexId>, String> {
    // Serialize publication with cancellation acknowledgement.
    let _publication = engine.active.lock().map_err(|error| error.to_string())?;
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let index = loaded.map_err(|error| error.to_string())?;
    let index_id = engine.publish(Arc::new(RwLock::new(index)), None)?;
    *engine
        .duplicates
        .lock()
        .map_err(|error| error.to_string())? = None;
    Ok(Some(index_id))
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
