use crate::{Engine, SnapshotDatabase, profile::Mode};
use serde::Deserialize;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, State};
use voldiq_core::snapshots::schedule::{ScanSettings, Schedule, time_now};

#[derive(Default, Clone)]
pub struct Scheduler(Arc<AtomicBool>);

impl Scheduler {
    pub fn stop(&self) {
        self.0.store(true, Ordering::Release);
    }
}

pub fn start(app: &AppHandle) -> Result<(), String> {
    let engine = app.state::<Arc<Engine>>().inner().clone();
    let database = app.state::<SnapshotDatabase>().inner().clone();
    let scheduler = app.state::<Scheduler>().inner().clone();
    let app = app.clone();
    std::thread::Builder::new()
        .name("voldiq-scheduled-scans".into())
        .spawn(move || {
            while !scheduler.0.load(Ordering::Acquire) {
                let result = (|| -> Result<(), String> {
                    let now = time_now().map_err(|error| error.to_string())?;
                    if !database
                        .0
                        .lock()?
                        .scan_schedule_due(now)
                        .map_err(|error| error.to_string())?
                    {
                        return Ok(());
                    }
                    let Ok(mut operation) = engine.begin() else {
                        return Ok(());
                    };
                    if scheduler.0.load(Ordering::Acquire) {
                        return Ok(());
                    }
                    let run = database
                        .0
                        .lock()?
                        .claim_scheduled_scan(&mut operation._lease, now)
                        .map_err(|error| error.to_string())?;
                    let _ = app.emit("scheduled-scans-changed", ());
                    let saved = if let Some(run) = run {
                        let measured = run.measure(&scheduler.0);
                        database
                            .0
                            .lock()?
                            .finish_scheduled_scan(run, measured, &scheduler.0)
                            .map_err(|error| error.to_string())?
                    } else {
                        None
                    };
                    drop(operation);
                    let _ = app.emit("scheduled-scans-changed", ());
                    if let Some(saved) = saved.filter(|saved| saved.growth_alert) {
                        let _ = app.emit("scheduled-growth-alert", saved.snapshot);
                        #[cfg(windows)]
                        if engine.mode == Mode::Installed {
                            crate::windows_notifications::scheduled_growth(
                                engine.operations.directory(),
                            );
                        }
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    eprintln!("Scheduled scan unavailable: {error}");
                }
                std::thread::sleep(Duration::from_secs(15));
            }
        })
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn list_scan_schedules(
    database: State<'_, SnapshotDatabase>,
) -> Result<Vec<Schedule>, String> {
    let store = database.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .lock()?
            .scan_schedules()
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScheduleChange {
    Add {
        path: PathBuf,
        settings: ScanSettings,
    },
    Edit {
        id: i64,
        revision: i64,
        settings: ScanSettings,
    },
    Enable {
        id: i64,
        revision: i64,
        enabled: bool,
    },
    Remove {
        id: i64,
        revision: i64,
    },
}

#[tauri::command]
pub async fn change_scan_schedule(
    change: ScheduleChange,
    engine: State<'_, Arc<Engine>>,
    database: State<'_, SnapshotDatabase>,
    app: AppHandle,
) -> Result<Vec<Schedule>, String> {
    if engine.mode == Mode::Ephemeral {
        return Err("Scheduled scans require a persistent profile.".into());
    }
    let operation = engine.inner().begin()?;
    let store = database.0.clone();
    let schedules = tauri::async_runtime::spawn_blocking(move || {
        let mut store = store.lock()?;
        let lease = &operation._lease;
        let now = time_now().map_err(|error| error.to_string())?;
        match change {
            ScheduleChange::Add { path, settings } => store
                .add_scan_schedule(lease, &path, &settings, now)
                .map(|_| ()),
            ScheduleChange::Edit {
                id,
                revision,
                settings,
            } => store.edit_scan_schedule(lease, id, revision, &settings, now),
            ScheduleChange::Enable {
                id,
                revision,
                enabled,
            } => store.set_scan_schedule_enabled(lease, id, revision, enabled, now),
            ScheduleChange::Remove { id, revision } => {
                store.remove_scan_schedule(lease, id, revision)
            }
        }
        .map_err(|error| error.to_string())?;
        store.scan_schedules().map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())??;
    let _ = app.emit("scheduled-scans-changed", ());
    Ok(schedules)
}
