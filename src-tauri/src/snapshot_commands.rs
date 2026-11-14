use crate::{Engine, analysis_status::IndexId};
use std::{
    path::Path,
    sync::{
        Arc, Mutex, MutexGuard, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{State, ipc::Channel};
use voldiq_core::snapshots::{Snapshot, Store};

#[derive(Clone)]
pub struct SnapshotDatabase(pub(super) Arc<SnapshotStore>);

pub(super) struct SnapshotStore(Mutex<Option<Store>>);

pub(super) struct OpenSnapshotStore<'a>(MutexGuard<'a, Option<Store>>);

impl std::ops::Deref for OpenSnapshotStore<'_> {
    type Target = Store;

    fn deref(&self) -> &Store {
        self.0.as_ref().expect("Open snapshot store guard")
    }
}

impl std::ops::DerefMut for OpenSnapshotStore<'_> {
    fn deref_mut(&mut self) -> &mut Store {
        self.0.as_mut().expect("Open snapshot store guard")
    }
}

impl SnapshotStore {
    pub(super) fn lock(&self) -> Result<OpenSnapshotStore<'_>, String> {
        let slot = self.0.lock().map_err(|error| error.to_string())?;
        if slot.is_none() {
            return Err("Snapshot storage is closing.".into());
        }
        Ok(OpenSnapshotStore(slot))
    }
}

#[tauri::command]
pub async fn get_snapshot_retention(
    snapshot: i64,
    database: State<'_, SnapshotDatabase>,
) -> Result<voldiq_core::snapshots::RetentionSettings, String> {
    let store = database.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .lock()
            .map_err(|e| e.to_string())?
            .retention_settings(snapshot)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn set_snapshot_retention(
    snapshot: i64,
    version: String,
    keep_latest: Option<u32>,
    engine: State<'_, Arc<Engine>>,
    database: State<'_, SnapshotDatabase>,
) -> Result<voldiq_core::snapshots::RetentionSettings, String> {
    let operation = engine.begin()?;
    let store = database.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        store
            .lock()
            .map_err(|e| e.to_string())?
            .set_retention(snapshot, &version, keep_latest)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn apply_snapshot_retention(
    snapshot: i64,
    fingerprint: String,
    operation: Channel<u64>,
    engine: State<'_, Arc<Engine>>,
    database: State<'_, SnapshotDatabase>,
) -> Result<Option<voldiq_core::snapshots::RetentionResult>, String> {
    let active = engine.begin()?;
    operation.send(active.id).map_err(|e| e.to_string())?;
    let store = database.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let operation = active;
        store
            .lock()
            .map_err(|e| e.to_string())?
            .apply_retention(snapshot, &fingerprint, &operation.cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

impl SnapshotDatabase {
    pub fn open(path: &Path) -> voldiq_core::Result<Self> {
        Ok(Self(Arc::new(SnapshotStore(Mutex::new(Some(
            Store::open(path)?,
        ))))))
    }

    pub(super) fn close(&self) {
        if let Ok(mut slot) = self.0.0.try_lock() {
            slot.take();
        }
    }

    pub(super) fn lock_state(&self) -> &'static str {
        match self.0.0.try_lock() {
            Ok(slot) if slot.is_some() => "available",
            Ok(_) => "closed",
            Err(std::sync::TryLockError::WouldBlock) => "busy",
            Err(std::sync::TryLockError::Poisoned(_)) => "poisoned",
        }
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
    let active_operation = engine.begin()?;
    let id = active_operation.id;
    let cancel = active_operation.cancel.clone();
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
        let evidence = voldiq_core::libraries::capture(&index, &cancel, |_| true)
            .map_err(|error| error.to_string())?;
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let saved = store
            .lock()
            .map_err(|error| error.to_string())?
            .save_with_libraries(&index, &cancel, &caches, evidence.as_ref());
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
    let active_operation = engine.begin()?;
    let operation_id = active_operation.id;
    let cancel = active_operation.cancel.clone();
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
    let index_id = engine.publish(Arc::new(RwLock::new(index)), None, false)?;
    *engine
        .duplicates
        .lock()
        .map_err(|error| error.to_string())? = None;
    Ok(Some(index_id))
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
