#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use tauri::{State, ipc::Channel};
use voldiq_core::{
    NodeId, Progress, ScanOptions, ScanPhase,
    index::{Details, FileType, Index, Page, Query},
    scanner,
    windows::Volume,
};

#[derive(Default)]
struct Engine {
    index: Mutex<Option<Arc<Index>>>,
    active: Mutex<Option<(u64, Arc<AtomicBool>)>>,
    sequence: AtomicU64,
}

impl Engine {
    fn index(&self) -> Result<Arc<Index>, String> {
        self.index
            .lock()
            .map_err(|e| e.to_string())?
            .clone()
            .ok_or_else(|| "Scan a drive or folder first.".into())
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
    progress: Channel<Progress>,
    engine: State<'_, Arc<Engine>>,
) -> Result<u64, String> {
    let engine = engine.inner().clone();
    let id = engine.sequence.fetch_add(1, Ordering::Relaxed) + 1;
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut active = engine.active.lock().map_err(|e| e.to_string())?;
        if active.is_some() {
            return Err("A scan is already running. Cancel it or wait for it to finish.".into());
        }
        *active = Some((id, cancel.clone()));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let mut final_progress = None;
        let result = scanner::scan(&PathBuf::from(path), &options, &cancel, |update| {
            if update.phase == ScanPhase::Scanning {
                let _ = progress.send(update);
            } else {
                final_progress = Some(update);
            }
        });
        let mut failure = None;
        match result {
            Ok(index) => match engine.index.lock() {
                Ok(mut slot) => *slot = Some(Arc::new(index)),
                Err(e) => failure = Some(e.to_string()),
            },
            Err(error) => failure = Some(error.to_string()),
        }
        if let Ok(mut active) = engine.active.lock() {
            *active = None;
        }
        if let Some(error) = failure {
            let _ = progress.send(Progress {
                phase: ScanPhase::Failed,
                entries: 0,
                files: 0,
                logical: 0,
                elapsed_ms: 0,
                errors: 1,
                method: error,
            });
        } else if let Some(update) = final_progress {
            let _ = progress.send(update);
        }
    });
    Ok(id)
}

#[tauri::command]
fn cancel_scan(id: u64, engine: State<'_, Arc<Engine>>) -> Result<(), String> {
    if let Some((active, cancel)) = &*engine.active.lock().map_err(|e| e.to_string())?
        && *active == id
    {
        cancel.store(true, Ordering::Relaxed);
    }
    Ok(())
}

#[tauri::command]
async fn query_files(query: Query, engine: State<'_, Arc<Engine>>) -> Result<Page, String> {
    let index = engine.index()?;
    tauri::async_runtime::spawn_blocking(move || index.query(&query).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn get_details(id: NodeId, engine: State<'_, Arc<Engine>>) -> Result<Details, String> {
    engine.index()?.details(id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_file_types(
    id: NodeId,
    engine: State<'_, Arc<Engine>>,
) -> Result<Vec<FileType>, String> {
    let index = engine.index()?;
    tauri::async_runtime::spawn_blocking(move || index.file_types(id).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn get_issues(engine: State<'_, Arc<Engine>>) -> Result<Vec<voldiq_core::Issue>, String> {
    Ok(engine.index()?.issues.clone())
}

#[tauri::command]
async fn reveal_item(id: NodeId, engine: State<'_, Arc<Engine>>) -> Result<(), String> {
    let path = engine.index()?.path(id).map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", path.display()))
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn main() {
    tauri::Builder::default()
        .manage(Arc::new(Engine::default()))
        .invoke_handler(tauri::generate_handler![
            get_volumes,
            choose_folder,
            start_scan,
            cancel_scan,
            query_files,
            get_details,
            get_file_types,
            get_issues,
            reveal_item
        ])
        .run(tauri::generate_context!())
        .expect("Voldiq could not start");
}
