//! One OS-released operation lease per pinned state directory, shared by every adapter.
use crate::{
    Error, Result,
    agents::{AuthorizedPath, pin_directory},
    windows,
};
use std::{cell::Cell, fs::File, path::Path, sync::Arc};

#[derive(Clone)]
pub struct Coordinator {
    directory: Arc<AuthorizedPath>,
}

pub struct Lease {
    _file: File,
    directory: Arc<AuthorizedPath>,
    pub(crate) recovered: Cell<bool>,
}

impl Coordinator {
    pub fn open(directory: &Path) -> Result<Self> {
        Ok(Self {
            directory: Arc::new(pin_directory(&std::fs::canonicalize(directory)?)?),
        })
    }

    pub fn directory(&self) -> &Path {
        self.directory.path()
    }

    pub(crate) fn identity(&self) -> (u64, u64) {
        self.directory.identity()
    }

    pub fn acquire(&self) -> Result<Lease> {
        self.try_acquire()?.ok_or_else(|| {
            Error::Invalid(
                "Another Voldiq operation is active. Wait for it to finish or cancel it.".into(),
            )
        })
    }

    fn try_acquire(&self) -> Result<Option<Lease>> {
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::*;
            options
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_OPEN_NO_RECALL);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        // Retain the existing filename so older scan clients participate in the same exclusion.
        let file = options.open(self.directory.path().join("scan.lock"))?;
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || windows::is_reparse(&metadata)
            || windows::requires_recall(&metadata)
        {
            return Err(Error::Invalid(
                "Operation lock must be a regular local file.".into(),
            ));
        }
        match file.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => return Ok(None),
            Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
        }
        Ok(Some(Lease {
            _file: file,
            directory: self.directory.clone(),
            recovered: Cell::new(false),
        }))
    }
}

impl Lease {
    pub(crate) fn directory(&self) -> &Path {
        self.directory.path()
    }

    pub fn acquire(directory: &Path) -> Result<Self> {
        Coordinator::open(directory)?.acquire()
    }

    pub(crate) fn try_acquire(directory: &Path) -> Result<Option<Self>> {
        Coordinator::open(directory)?.try_acquire()
    }

    pub(crate) fn directory_identity(&self) -> (u64, u64) {
        self.directory.identity()
    }
}
