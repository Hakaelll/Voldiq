use super::{State, invalid, random_id};
use crate::{
    Error, Result,
    agents::{AuthorizedPath, pin_directory},
};
use std::{
    collections::HashMap,
    fs::File,
    path::Path,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

struct Grant {
    hash: String,
    owner: i64,
    binding: String,
    expires: i64,
    issued_at: i64,
    issued: std::time::Instant,
    maximum: std::time::Duration,
}

/// Held only by the human desktop process. SQLite stores audit, never this authority.
pub struct HumanAuthority {
    _file: File,
    directory: AuthorizedPath,
    issuer: String,
    grants: Mutex<HashMap<String, Grant>>,
    closed: AtomicBool,
}

impl HumanAuthority {
    /// Availability observation only. The desktop must still consume its private live grant.
    pub fn is_active(directory: &Path) -> Result<bool> {
        let directory = pin_directory(directory)?;
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::*;
            options
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_OPEN_NO_RECALL);
        }
        let file = match options.open(directory.path().join("approval.lock")) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || crate::windows::is_reparse(&metadata)
            || crate::windows::requires_recall(&metadata)
        {
            return Err(invalid());
        }
        match file.try_lock_shared() {
            Ok(()) => Ok(false),
            Err(std::fs::TryLockError::WouldBlock) => Ok(true),
            Err(std::fs::TryLockError::Error(error)) => Err(error.into()),
        }
    }
    pub fn acquire(directory: &Path) -> Result<Self> {
        let directory = pin_directory(directory)?;
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options
                .share_mode(
                    windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ
                        | windows_sys::Win32::Storage::FileSystem::FILE_SHARE_WRITE,
                )
                .custom_flags(
                    windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT,
                );
        }
        let file = options.open(directory.path().join("approval.lock"))?;
        if !file.metadata()?.is_file() || crate::windows::is_reparse(&file.metadata()?) {
            return Err(invalid());
        }
        file.try_lock()
            .map_err(|_| Error::Invalid("Another desktop process owns human approvals.".into()))?;
        Ok(Self {
            _file: file,
            directory,
            issuer: random_id()?,
            grants: Mutex::new(HashMap::new()),
            closed: AtomicBool::new(false),
        })
    }

    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    pub(super) fn check_directory(&self, expected: (u64, u64)) -> Result<()> {
        if self.closed.load(Ordering::Acquire) || self.directory.identity() != expected {
            return Err(invalid());
        }
        Ok(())
    }

    pub(super) fn grant(
        &self,
        id: &str,
        hash: &str,
        owner: i64,
        binding: &str,
        now: i64,
        expires: i64,
    ) -> Result<()> {
        if self.closed.load(Ordering::Acquire)
            || now < 0
            || expires <= now
            || expires - now > super::APPROVAL_TTL
        {
            return Err(Error::NeedsHumanApproval);
        }
        let mut grants = self.grants.lock().map_err(|_| invalid())?;
        grants.retain(|_, grant| grant.expires > now && grant.issued.elapsed() < grant.maximum);
        if grants.len() >= 500 {
            return Err(invalid());
        }
        grants.insert(
            id.to_owned(),
            Grant {
                hash: hash.to_owned(),
                owner,
                binding: binding.to_owned(),
                expires,
                issued_at: now,
                issued: std::time::Instant::now(),
                maximum: std::time::Duration::from_secs((expires - now) as u64),
            },
        );
        Ok(())
    }

    pub(super) fn consume(
        &self,
        id: &str,
        hash: &str,
        owner: i64,
        binding: &str,
        now: i64,
    ) -> Result<()> {
        if self.closed.load(Ordering::Acquire) {
            return Err(Error::NeedsHumanApproval);
        }
        let grant = self
            .grants
            .lock()
            .map_err(|_| invalid())?
            .remove(id)
            .ok_or(Error::NeedsHumanApproval)?;
        if grant.hash != hash
            || grant.owner != owner
            || grant.binding != binding
            || now >= grant.expires
            || now < grant.issued_at
            || grant.issued.elapsed() >= grant.maximum
        {
            return Err(Error::NeedsHumanApproval);
        }
        Ok(())
    }

    pub(super) fn forget(&self, id: &str) -> Result<()> {
        self.grants.lock().map_err(|_| invalid())?.remove(id);
        Ok(())
    }

    pub(super) fn close(&self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Ok(mut grants) = self.grants.lock() {
            grants.clear();
        }
        let _ = self._file.unlock();
    }
}

pub enum Begin {
    Started(super::ExecutionLease),
    Existing(State),
}
