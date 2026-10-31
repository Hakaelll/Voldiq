#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use tauri::Manager;
use tauri::{State, ipc::Channel};
mod analysis_status;
mod cleanup_commands;
mod config_file;
mod developer_commands;
mod docker_commands;
mod overview;
mod query_commands;
mod restore_commands;
mod snapshot_commands;
use analysis_status::IndexId;
use cleanup_commands::*;
use snapshot_commands::*;
use voldiq_core::{
    NodeId, Progress, ScanOptions, ScanPhase,
    duplicates::{self, Analysis, HashCache},
    scanner::{self, SharedIndex},
    windows::Volume,
};

#[derive(Default)]
struct Engine {
    queries: query_commands::Queries,
    index: Mutex<Option<analysis_status::LoadedAnalysis>>,
    active: Mutex<Option<(u64, Arc<AtomicBool>)>>,
    sequence: AtomicU64,
    duplicates: Mutex<Option<Arc<DuplicateRun>>>,
    hash_cache: Mutex<HashCache>,
}

struct DuplicateRun {
    index: SharedIndex,
    analysis: Analysis,
}

struct Operation {
    engine: Arc<Engine>,
    id: u64,
}
impl Drop for Operation {
    fn drop(&mut self) {
        if let Ok(mut active) = self.engine.active.lock()
            && active.as_ref().is_some_and(|(id, _)| *id == self.id)
        {
            *active = None;
        }
    }
}

impl Engine {
    fn begin(&self) -> Result<(u64, Arc<AtomicBool>), String> {
        let mut active = self.active.lock().map_err(|e| e.to_string())?;
        if active.is_some() {
            return Err("Another analysis is running.".into());
        }
        let id = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        let cancel = Arc::new(AtomicBool::new(false));
        *active = Some((id, cancel.clone()));
        Ok((id, cancel))
    }
    fn index(&self) -> Result<SharedIndex, String> {
        self.index
            .lock()
            .map_err(|e| e.to_string())?
            .as_ref()
            .map(|loaded| loaded.index.clone())
            .ok_or_else(|| "Scan a drive or folder first.".into())
    }
    fn index_for(&self, expected: IndexId) -> Result<SharedIndex, String> {
        self.index
            .lock()
            .map_err(|error| error.to_string())?
            .as_ref()
            .filter(|loaded| loaded.id == expected)
            .map(|loaded| loaded.index.clone())
            .ok_or_else(|| "Analysis changed. Refresh the view before continuing.".into())
    }
    fn index_id(&self) -> Result<Option<IndexId>, String> {
        Ok(self
            .index
            .lock()
            .map_err(|error| error.to_string())?
            .as_ref()
            .map(|loaded| loaded.id))
    }
    fn index_is_current(&self, expected: IndexId, index: &SharedIndex) -> Result<bool, String> {
        Ok(self
            .index
            .lock()
            .map_err(|error| error.to_string())?
            .as_ref()
            .is_some_and(|loaded| loaded.id == expected && Arc::ptr_eq(&loaded.index, index)))
    }
    fn publish(&self, index: SharedIndex, analyzed_at: Option<u64>) -> Result<IndexId, String> {
        let mut slot = self.index.lock().map_err(|error| error.to_string())?;
        let id = match slot
            .as_ref()
            .filter(|loaded| Arc::ptr_eq(&loaded.index, &index))
        {
            Some(loaded) => loaded.id,
            None => IndexId(self.sequence.fetch_add(1, Ordering::Relaxed) + 1),
        };
        *slot = Some(analysis_status::LoadedAnalysis {
            id,
            index,
            analyzed_at,
        });
        Ok(id)
    }
}

#[tauri::command]
async fn get_volumes() -> Result<Vec<Volume>, String> {
    tauri::async_runtime::spawn_blocking(voldiq_core::windows::volumes)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn choose_folder() -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_title("Choose a folder to analyze")
            .pick_folder()
            .map(|path| path.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn start_scan(
    path: String,
    options: ScanOptions,
    progress: Channel<analysis_status::ScanProgress>,
    engine: State<'_, Arc<Engine>>,
    database: State<'_, SnapshotDatabase>,
) -> Result<u64, String> {
    let engine = engine.inner().clone();
    let (id, cancel) = engine.begin()?;
    let operation = Operation {
        engine: engine.clone(),
        id,
    };
    if let Ok(mut result) = engine.duplicates.lock() {
        *result = None;
    }
    let previous = engine.index.lock().map_err(|e| e.to_string())?.clone();
    let previous_date = previous.as_ref().and_then(|loaded| loaded.analyzed_at);
    let store = database.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        let mut final_progress = None;
        let path = PathBuf::from(path);
        let start = std::time::Instant::now();
        let resumed = if options.mode == voldiq_core::ScanMode::Auto && previous.is_none() {
            store
                .lock()
                .ok()
                .and_then(|store| store.resume_latest(&path, &options, &cancel).ok().flatten())
        } else {
            None
        };
        let result = if let Some(index) = resumed {
            final_progress = Some(Progress {
                previous: false,
                phase: ScanPhase::Complete,
                entries: index.entries.len(),
                files: index.entries[0].files,
                logical: index.entries[0].logical,
                errors: index.error_count,
                elapsed_ms: start.elapsed().as_millis() as u64,
                method: "USN Journal".into(),
            });
            Ok(Some(Arc::new(std::sync::RwLock::new(index))))
        } else if options.mode == voldiq_core::ScanMode::Auto
            && previous.is_none()
            && cancel.load(Ordering::Relaxed)
        {
            final_progress = Some(Progress {
                previous: false,
                phase: ScanPhase::Cancelled,
                entries: 0,
                files: 0,
                logical: 0,
                errors: 0,
                elapsed_ms: start.elapsed().as_millis() as u64,
                method: "USN Journal".into(),
            });
            Ok(None)
        } else {
            scanner::refresh_or_scan(
                &path,
                &options,
                &cancel,
                |update| {
                    if update.phase == ScanPhase::Scanning {
                        let analyzed_at = if update.previous { previous_date } else { None };
                        let _ = progress.send(analysis_status::ScanProgress {
                            index_id: engine.index_id().ok().flatten(),
                            progress: update,
                            analyzed_at,
                        });
                    } else {
                        final_progress = Some(update);
                    }
                },
                |index| {
                    let _ = engine.publish(index, None);
                },
                previous.as_ref().map(|loaded| &loaded.index),
            )
            .map(Some)
        };
        let mut failure = None;
        match result {
            Ok(Some(index)) => {
                let date = final_progress.as_ref().and_then(|update| {
                    analysis_status::analyzed_at(
                        update,
                        previous_date,
                        std::time::SystemTime::now(),
                    )
                });
                if let Err(error) = engine.publish(index, date) {
                    failure = Some(error);
                }
            }
            Ok(None) => {}
            Err(error) => failure = Some(error.to_string()),
        }
        let (index_id, analyzed_at) = engine
            .index
            .lock()
            .ok()
            .and_then(|slot| {
                slot.as_ref()
                    .map(|loaded| (Some(loaded.id), loaded.analyzed_at))
            })
            .unwrap_or((None, None));
        if let Ok(mut active) = engine.active.lock() {
            *active = None;
        }
        if let Some(error) = failure {
            let _ = progress.send(analysis_status::ScanProgress {
                index_id,
                progress: Progress {
                    previous: false,
                    phase: ScanPhase::Failed,
                    entries: 0,
                    files: 0,
                    logical: 0,
                    elapsed_ms: 0,
                    errors: 1,
                    method: error,
                },
                analyzed_at,
            });
        } else if let Some(update) = final_progress {
            let _ = progress.send(analysis_status::ScanProgress {
                index_id,
                progress: update,
                analyzed_at,
            });
        }
    });
    Ok(id)
}

#[tauri::command]
fn cancel_operation(id: u64, engine: State<'_, Arc<Engine>>) -> Result<(), String> {
    if let Some((active, cancel)) = &*engine.active.lock().map_err(|e| e.to_string())?
        && *active == id
    {
        cancel.store(true, Ordering::Relaxed);
    }
    Ok(())
}

#[tauri::command]
fn start_duplicates(
    index_id: IndexId,
    scope: NodeId,
    min_size: u64,
    progress: Channel<duplicates::Progress>,
    engine: State<'_, Arc<Engine>>,
) -> Result<u64, String> {
    let engine = engine.inner().clone();
    let (id, cancel) = engine.begin()?;
    let operation = Operation {
        engine: engine.clone(),
        id,
    };
    let index = engine.index_for(index_id)?;
    index
        .read()
        .map_err(|e| e.to_string())?
        .entry(scope)
        .map_err(|e| e.to_string())?;
    if let Ok(mut result) = engine.duplicates.lock() {
        *result = None;
    }
    tauri::async_runtime::spawn_blocking(move || {
        let mut terminal = None;
        let result = engine
            .hash_cache
            .lock()
            .map_err(|e| e.to_string())
            .and_then(|mut cache| {
                duplicates::find(&index, scope, min_size, &cancel, &mut cache, |update| {
                    if update.phase == "complete" || update.phase == "cancelled" {
                        terminal = Some(update);
                    } else {
                        let _ = progress.send(update);
                    }
                })
                .map_err(|e| e.to_string())
            });
        let error = match result {
            Ok(result) => match engine.duplicates.lock() {
                Ok(mut slot) => {
                    *slot = Some(Arc::new(DuplicateRun {
                        index,
                        analysis: result,
                    }));
                    None
                }
                Err(error) => Some(error.to_string()),
            },
            Err(error) => Some(error),
        };
        drop(operation);
        if let Some(message) = error {
            let _ = progress.send(duplicates::Progress {
                phase: "failed".into(),
                examined: 0,
                candidates: 0,
                bytes_read: 0,
                cache_hits: 0,
                errors: 1,
                elapsed_ms: 0,
                message,
            });
        } else if let Some(update) = terminal {
            let _ = progress.send(update);
        }
    });
    Ok(id)
}

#[derive(Serialize)]
struct DuplicateGroup {
    key: usize,
    name: String,
    digest: String,
    logical: u64,
    recoverable: u64,
    copies: usize,
}

#[derive(Serialize)]
struct DuplicatePage {
    groups: Vec<DuplicateGroup>,
    total: usize,
    recoverable: u64,
    errors: u64,
    cancelled: bool,
    bytes_read: u64,
    cache_hits: u64,
}

fn duplicate_group(
    key: usize,
    group: &duplicates::Group,
    index: &voldiq_core::index::Index,
) -> Result<DuplicateGroup, String> {
    let member = group
        .members
        .first()
        .ok_or("Duplicate group has no members.")?;
    let entry = index.entry(member.id).map_err(|e| e.to_string())?;
    Ok(DuplicateGroup {
        key,
        name: entry.name.clone(),
        digest: group.digest.clone(),
        logical: group.logical,
        recoverable: group.recoverable,
        copies: group.members.len(),
    })
}

#[tauri::command]
async fn query_duplicate_groups(
    index_id: IndexId,
    offset: usize,
    limit: usize,
    engine: State<'_, Arc<Engine>>,
) -> Result<Option<DuplicatePage>, String> {
    let current = engine.index_for(index_id)?;
    let result = engine.duplicates.lock().map_err(|e| e.to_string())?.clone();
    let Some(run) = result else {
        return Ok(None);
    };
    if !Arc::ptr_eq(&run.index, &current) {
        return Err("Duplicate results belong to another analysis.".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let result = &run.analysis;
        let index = run.index.read().map_err(|e| e.to_string())?;
        Ok(Some(DuplicatePage {
            groups: result
                .groups
                .iter()
                .enumerate()
                .skip(offset)
                .take(limit.clamp(1, 100))
                .map(|(key, group)| duplicate_group(key, group, &index))
                .collect::<Result<Vec<_>, String>>()?,
            total: result.groups.len(),
            recoverable: result.groups.iter().map(|g| g.recoverable).sum(),
            errors: result.errors,
            cancelled: result.cancelled,
            bytes_read: result.bytes_read,
            cache_hits: result.cache_hits,
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
struct DuplicateItem {
    id: NodeId,
    name: String,
    path: String,
    logical: u64,
    allocated: u64,
    links: u32,
    modified: Option<u64>,
}

#[tauri::command]
async fn query_duplicate_members(
    index_id: IndexId,
    group: usize,
    offset: usize,
    limit: usize,
    engine: State<'_, Arc<Engine>>,
) -> Result<Vec<DuplicateItem>, String> {
    let current = engine.index_for(index_id)?;
    let result = engine
        .duplicates
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or("Analyze duplicates first.")?;
    if !Arc::ptr_eq(&result.index, &current) {
        return Err("Duplicate results belong to another analysis.".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let group = result
            .analysis
            .groups
            .get(group)
            .ok_or("Duplicate group is no longer available.")?;
        let index = result.index.read().map_err(|e| e.to_string())?;
        group
            .members
            .iter()
            .skip(offset)
            .take(limit.clamp(1, 200))
            .map(|member| {
                let details = index.details(member.id).map_err(|e| e.to_string())?;
                Ok(DuplicateItem {
                    id: member.id,
                    name: details.entry.name,
                    path: details.path,
                    logical: group.logical,
                    allocated: member.allocated,
                    links: member.links,
                    modified: details.entry.modified,
                })
            })
            .collect()
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn get_duplicate_issues(
    index_id: IndexId,
    engine: State<'_, Arc<Engine>>,
) -> Result<Vec<voldiq_core::Issue>, String> {
    let current = engine.index_for(index_id)?;
    let result = engine
        .duplicates
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
    match result {
        Some(run) if Arc::ptr_eq(&run.index, &current) => Ok(run.analysis.issues.clone()),
        Some(_) => Err("Duplicate results belong to another analysis.".into()),
        None => Ok(Vec::new()),
    }
}

#[tauri::command]
fn get_issues(
    index_id: IndexId,
    engine: State<'_, Arc<Engine>>,
) -> Result<Vec<voldiq_core::Issue>, String> {
    let current = engine.index_for(index_id)?;
    Ok(current
        .read()
        .map_err(|error| error.to_string())?
        .issues
        .clone())
}

#[tauri::command]
async fn reveal_item(
    index_id: IndexId,
    id: NodeId,
    engine: State<'_, Arc<Engine>>,
) -> Result<(), String> {
    let path = engine
        .index_for(index_id)?
        .read()
        .map_err(|e| e.to_string())?
        .path(id)
        .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut argument = std::ffi::OsString::from("/select,");
        argument.push(path.as_os_str());
        std::process::Command::new("explorer.exe")
            .arg(argument)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn open_windows_storage() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| {
        voldiq_core::windows::open_storage_settings().map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn open_windows_recycle_bin() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| {
        voldiq_core::windows::open_recycle_bin().map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn query_developer_storage(
    index_id: IndexId,
    app: tauri::AppHandle,
    query: voldiq_core::developer::Query,
    engine: State<'_, Arc<Engine>>,
) -> Result<voldiq_core::developer::Page, String> {
    let shared = engine.index_for(index_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let historical = shared
            .read()
            .map_err(|e| e.to_string())?
            .snapshot_id
            .is_some();
        let caches = if historical {
            Vec::new()
        } else {
            developer_commands::cache_locations(&app)
        };
        let index = shared.read().map_err(|e| e.to_string())?;
        voldiq_core::developer::query_with_caches(&index, &query, &caches)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn main() {
    #[cfg(windows)]
    if let Some(code) = voldiq_core::cleanup::helper::run_cli() {
        std::process::exit(code);
    }
    tauri::Builder::default()
        .setup(|app| {
            let directory = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&directory)?;
            app.manage(SnapshotDatabase::open(&directory.join("snapshots.db"))?);
            app.manage(CleanupJournal::open(&directory.join("cleanup.db")));
            Ok(())
        })
        .manage(Arc::new(Engine::default()))
        .manage(CleanupPlans::default())
        .manage(restore_commands::RestorationReviews::default())
        .invoke_handler(tauri::generate_handler![
            get_volumes,
            choose_folder,
            start_scan,
            cancel_operation,
            start_duplicates,
            query_duplicate_groups,
            query_duplicate_members,
            get_duplicate_issues,
            query_commands::query_files_cancellable,
            query_commands::query_overview,
            query_commands::query_details_cancellable,
            query_commands::cancel_query,
            query_commands::open_query_session,
            preview_cleanup,
            query_cleanup_rule,
            query_recycle_bin,
            preview_cleanup_rule,
            preview_duplicate_cleanup,
            discard_cleanup,
            clear_current_analysis,
            execute_permanent_cleanup,
            cleanup_capabilities,
            execute_recycling_cleanup,
            query_cleanup_history,
            restore_commands::preview_restoration,
            restore_commands::discard_restoration,
            restore_commands::execute_restoration,
            query_developer_storage,
            docker_commands::query_docker_storage,
            get_issues,
            reveal_item,
            open_windows_storage,
            open_windows_recycle_bin,
            save_snapshot,
            list_snapshots,
            load_snapshot,
            compare_snapshots,
            delete_snapshot,
            snapshot_storage,
            compact_snapshots
        ])
        .run(tauri::generate_context!())
        .expect("Voldiq could not start");
}
