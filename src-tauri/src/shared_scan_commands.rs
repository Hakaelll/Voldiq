use crate::{Engine, IndexId, SnapshotDatabase, snapshot_commands::finish_snapshot_load};
use serde::Serialize;
use std::sync::Arc;
use tauri::{State, ipc::Channel};
use voldiq_core::{
    jobs::{DesktopJob, Jobs},
    snapshots::Snapshot,
};

#[derive(Serialize)]
pub struct SharedScan {
    #[serde(flatten)]
    job: DesktopJob,
    saved_result: bool,
}

#[derive(Serialize)]
pub struct OpenedSharedScan {
    snapshot: Snapshot,
    index_id: IndexId,
}

fn parse_id(value: &str) -> Result<i64, String> {
    value
        .parse::<i64>()
        .ok()
        .filter(|id| *id > 0 && id.to_string() == value)
        .ok_or_else(|| "Scan job is unavailable.".into())
}

#[tauri::command]
pub async fn list_shared_scans(
    engine: State<'_, Arc<Engine>>,
    database: State<'_, SnapshotDatabase>,
) -> Result<Vec<SharedScan>, String> {
    engine.require_durable_state()?;
    let directory = engine.operations.directory().to_owned();
    let store = database.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let jobs = Jobs::recent_for_desktop(&directory).map_err(|error| error.to_string())?;
        let store = store.lock().map_err(|error| error.to_string())?;
        jobs.into_iter()
            .map(|job| {
                let saved_result = job
                    .snapshot_id
                    .map(|id| store.contains(id))
                    .transpose()
                    .map_err(|error| error.to_string())?
                    .unwrap_or(false);
                Ok(SharedScan { job, saved_result })
            })
            .collect()
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn cancel_shared_scan(id: String, engine: State<'_, Arc<Engine>>) -> Result<(), String> {
    engine.require_durable_state()?;
    let id = parse_id(&id)?;
    let directory = engine.operations.directory().to_owned();
    tauri::async_runtime::spawn_blocking(move || Jobs::cancel_for_desktop(&directory, id))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn open_shared_scan(
    id: String,
    operation: Channel<u64>,
    engine: State<'_, Arc<Engine>>,
    database: State<'_, SnapshotDatabase>,
) -> Result<Option<OpenedSharedScan>, String> {
    engine.require_durable_state()?;
    let id = parse_id(&id)?;
    let engine = engine.inner().clone();
    let active = engine.begin()?;
    operation
        .send(active.id)
        .map_err(|error| error.to_string())?;
    let store = database.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let snapshot =
            Jobs::snapshot_for_desktop(&active._lease, id).map_err(|error| error.to_string())?;
        let (summary, index) = {
            let store = store.lock().map_err(|error| error.to_string())?;
            let summary = store
                .summary(snapshot)
                .map_err(|error| error.to_string())?
                .ok_or("The saved result is no longer available.")?;
            (summary, store.load(snapshot, &active.cancel))
        };
        Ok(
            finish_snapshot_load(&engine, index, &active.cancel)?.map(|index_id| {
                OpenedSharedScan {
                    snapshot: summary,
                    index_id,
                }
            }),
        )
    })
    .await
    .map_err(|error| error.to_string())?
}
