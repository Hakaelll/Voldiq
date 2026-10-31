use crate::{Engine, cleanup_commands::CleanupJournal, snapshot_commands::SnapshotDatabase};
use std::{
    ffi::OsStr,
    fs::{File, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::{Arc, Mutex, TryLockError},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::Manager;

#[derive(Clone, Default)]
pub(crate) struct Trace(Option<Arc<File>>);

impl Trace {
    pub fn from_args() -> io::Result<Self> {
        let mut arguments = std::env::args_os().skip(1);
        if arguments.next().as_deref() != Some(OsStr::new("--trace-lifecycle")) {
            return Ok(Self::default());
        }
        let path = arguments.next().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Supply a new absolute trace path.",
            )
        })?;
        if arguments.next().is_some() || !Path::new(&path).is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Use --trace-lifecycle <new absolute JSONL path>.",
            ));
        }
        Self::open(Path::new(&path))
    }

    fn open(path: &Path) -> io::Result<Self> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map(|file| Self(Some(Arc::new(file))))
    }

    pub fn record(&self, event: &'static str, app: Option<&tauri::AppHandle>) {
        let Some(file) = &self.0 else { return };
        let state = app.map(|app| {
            serde_json::json!({
                "operation": app.try_state::<Arc<Engine>>()
                    .map(|engine| operation_state(&engine)).unwrap_or("unavailable"),
                "snapshots": app.try_state::<SnapshotDatabase>()
                    .map(|database| lock_state(&database.0)).unwrap_or("unavailable"),
                "cleanupJournal": app.try_state::<CleanupJournal>()
                    .map(|journal| lock_state(&journal.0)).unwrap_or("unavailable"),
            })
        });
        let row = serde_json::json!({
            "event": event,
            "pid": std::process::id(),
            "unixMs": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
            "state": state,
        });
        if let Err(error) = writeln!(file.as_ref(), "{row}") {
            eprintln!("Lifecycle trace write failed: {error}");
        }
    }
}

fn operation_state(engine: &Engine) -> &'static str {
    match engine.active.try_lock() {
        Ok(active) => {
            if active.is_some() {
                "active"
            } else {
                "idle"
            }
        }
        Err(TryLockError::WouldBlock) => "busy",
        Err(TryLockError::Poisoned(_)) => "poisoned",
    }
}

fn lock_state<T>(lock: &Mutex<T>) -> &'static str {
    match lock.try_lock() {
        Ok(_) => "available",
        Err(TryLockError::WouldBlock) => "busy",
        Err(TryLockError::Poisoned(_)) => "poisoned",
    }
}

#[cfg(test)]
pub(crate) fn verify_fixture() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("events.jsonl");
    let trace = Trace::open(&path).unwrap();
    trace.record("fixture-start", None);
    trace.clone().record("fixture-end", None);
    let before = std::fs::read(&path).unwrap();
    let rows: Vec<serde_json::Value> = std::str::from_utf8(&before)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["event"], "fixture-start");
    assert_eq!(rows[1]["event"], "fixture-end");
    assert_eq!(rows[0]["pid"], std::process::id());
    assert!(rows[0]["state"].is_null());
    assert!(
        matches!(Trace::open(&path), Err(error) if error.kind() == io::ErrorKind::AlreadyExists)
    );
    Trace::default().record("disabled", None);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let engine = Engine::default();
    assert_eq!(operation_state(&engine), "idle");
    let _ = engine.begin().unwrap();
    assert_eq!(operation_state(&engine), "active");
    let _held = engine.active.lock().unwrap();
    assert_eq!(operation_state(&engine), "busy");
    assert_eq!(lock_state(&engine.active), "busy");
    drop(trace);
}
