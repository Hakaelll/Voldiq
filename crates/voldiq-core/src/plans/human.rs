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
        if self.closed.load(Ordering::Acquire) {
            return Err(Error::NeedsHumanApproval);
        }
        let mut grants = self.grants.lock().map_err(|_| invalid())?;
        grants.retain(|_, grant| grant.expires > now);
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
        self.closed.store(true, Ordering::Release);
        if let Ok(mut grants) = self.grants.lock() {
            grants.clear();
        }
    }
}

pub enum Begin {
    Started(super::ExecutionLease),
    Existing(State),
}
