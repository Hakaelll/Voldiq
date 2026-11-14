use serde::Serialize;
use std::{
    ffi::OsString,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tempfile::{Builder, TempDir};

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Mode {
    Installed,
    Portable,
    Ephemeral,
}

pub(crate) struct Profile {
    pub(crate) mode: Mode,
    pub(crate) path: PathBuf,
    pub(crate) temporary: Option<TempDir>,
}

#[derive(Clone)]
pub(crate) struct Operations {
    path: PathBuf,
    coordinator: Arc<Mutex<Option<voldiq_core::operations::Coordinator>>>,
}

impl Operations {
    pub(crate) fn open(path: &Path) -> voldiq_core::Result<Self> {
        let coordinator = voldiq_core::operations::Coordinator::open(path)?;
        Ok(Self {
            path: coordinator.directory().to_owned(),
            coordinator: Arc::new(Mutex::new(Some(coordinator))),
        })
    }

    pub(crate) fn directory(&self) -> &Path {
        &self.path
    }

    pub(crate) fn acquire(&self) -> Result<voldiq_core::operations::Lease, String> {
        self.coordinator
            .lock()
            .map_err(|error| error.to_string())?
            .as_ref()
            .ok_or("Voldiq storage is closing.")?
            .acquire()
            .map_err(|error| error.to_string())
    }

    pub(crate) fn close(&self) {
        self.coordinator
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
    }
}

pub(crate) fn requested(mut arguments: impl Iterator<Item = OsString>) -> io::Result<Mode> {
    let first = arguments.next();
    let mode = match first.as_ref().and_then(|value| value.to_str()) {
        Some("--portable") => Mode::Portable,
        Some("--ephemeral") => Mode::Ephemeral,
        _ => return Ok(Mode::Installed),
    };
    if arguments
        .next()
        .is_some_and(|argument| argument != "--trace-lifecycle")
    {
        return Err(io::Error::other(
            "Pass --portable or --ephemeral on its own.",
        ));
    }
    Ok(mode)
}

impl Profile {
    pub(crate) fn resolve(mode: Mode, installed: &Path, executable: &Path) -> io::Result<Self> {
        let path = match mode {
            Mode::Installed => installed.to_owned(),
            Mode::Portable => executable
                .parent()
                .ok_or_else(|| io::Error::other("Voldiq executable has no parent directory."))?
                .join(".voldiq-data"),
            Mode::Ephemeral => return Self::ephemeral(),
        };
        match writable(&path) {
            Ok(()) => Ok(Self {
                mode,
                path,
                temporary: None,
            }),
            Err(error) if is_unwritable(&error) => Self::ephemeral(),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn ephemeral() -> io::Result<Self> {
        let temporary = Builder::new().prefix(".voldiq-session-").tempdir()?;
        Ok(Self {
            mode: Mode::Ephemeral,
            path: temporary.path().to_owned(),
            temporary: Some(temporary),
        })
    }
}

pub(crate) fn is_unwritable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::PermissionDenied
            | io::ErrorKind::ReadOnlyFilesystem
            | io::ErrorKind::StorageFull
    )
}

pub(crate) fn snapshot_unwritable(error: &voldiq_core::Error) -> bool {
    use rusqlite::{Error, ErrorCode};
    match error {
        voldiq_core::Error::Io(error) => is_unwritable(error),
        voldiq_core::Error::Database(Error::SqliteFailure(error, _)) => {
            matches!(
                error.code,
                ErrorCode::ReadOnly | ErrorCode::PermissionDenied | ErrorCode::DiskFull
            )
        }
        _ => false,
    }
}

pub(crate) fn close_temporary(temporary: TempDir, snapshots: &crate::SnapshotDatabase) {
    // WebView2 can hold the profile briefly after the window and event loop are gone.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        snapshots.close();
        match std::fs::remove_dir_all(temporary.path()) {
            Ok(()) => return,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return,
            Err(error) if std::time::Instant::now() >= deadline => {
                eprintln!("Could not remove the temporary Voldiq profile: {error}");
                return;
            }
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(150)),
        }
    }
}

fn writable(path: &Path) -> io::Result<()> {
    if !path.is_absolute() {
        return Err(io::Error::other("Voldiq data directory must be absolute."));
    }
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("Missing data directory parent."))?;
    for component in parent.ancestors().collect::<Vec<_>>().into_iter().rev() {
        check_directory(component)?;
    }
    match std::fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    check_directory(path)?;
    write_probe(path)?;
    for name in [
        "snapshots.db",
        "snapshots.db-wal",
        "snapshots.db-shm",
        "snapshots.db-journal",
    ] {
        match std::fs::symlink_metadata(path.join(name)) {
            Ok(metadata) => {
                if !metadata.is_file()
                    || voldiq_core::windows::is_reparse(&metadata)
                    || voldiq_core::windows::requires_recall(&metadata)
                {
                    return Err(io::Error::other(
                        "Voldiq snapshot storage contains an unsafe file.",
                    ));
                }
                if metadata.permissions().readonly() {
                    return Err(io::ErrorKind::ReadOnlyFilesystem.into());
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    let webview = path.join("EBWebView");
    match std::fs::symlink_metadata(&webview) {
        Ok(_) => {
            check_directory(&webview)?;
            write_probe(&webview)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn check_directory(path: &Path) -> io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || voldiq_core::windows::is_reparse(&metadata)
        || voldiq_core::windows::requires_recall(&metadata)
    {
        return Err(io::Error::other(
            "Voldiq data path contains an unsafe directory.",
        ));
    }
    Ok(())
}

fn write_probe(directory: &Path) -> io::Result<()> {
    let mut probe = Builder::new()
        .prefix(".voldiq-write-")
        .tempfile_in(directory)?;
    probe.write_all(b"voldiq")?;
    probe.close()
}
