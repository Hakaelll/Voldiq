use super::{RecycledItem, check_content, check_shell_identity, file_identity};
use crate::{
    Error, Result,
    cleanup::journal::Journal,
    windows::{
        cleanup_io,
        cleanup_stage::{StageEntry, StageIdentity, restore_to_parent},
    },
};
use std::{
    fs::File,
    path::{Path, PathBuf},
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

pub struct RestoredItem {
    pub(super) path: PathBuf,
    pub(super) identity: StageIdentity,
    pub(super) journal_error: Option<String>,
}

impl RestoredItem {
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn identity(&self) -> StageIdentity {
        self.identity
    }
    pub fn recorded(&self) -> bool {
        self.journal_error.is_none()
    }
    pub fn journal_error(&self) -> Option<&str> {
        self.journal_error.as_deref()
    }
}

impl RecycledItem<'_, '_> {
    /// One explicit original-location restoration while the source and original ancestors remain retained.
    pub fn restore(&mut self, journal: &mut Journal) -> Result<RestoredItem> {
        if self.restore_attempted || self.journal_error.is_some() {
            return Err(Error::Invalid(
                "Recycling receipt must be reconciled before restoration.".into(),
            ));
        }
        let strict = self.source.strict_guard()?;
        check_shell_identity(&self._item, self.identity(), false)?;
        let parent = self
            .source
            ._ancestors
            .last()
            .ok_or_else(|| Error::Invalid("Missing retained restoration parent.".into()))?;
        let name = StageEntry::parse(
            self.source
                .original_path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| Error::Invalid("Inexact original restoration filename.".into()))?,
        )?;
        match at_parent(parent, &name) {
            Ok(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "Restoration destination is occupied.",
                )
                .into());
            }
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        self.restore_attempted = true;
        journal.start_restore(self.relocation, self.identity(), &self.source.original_path)?;
        let restored = (|| {
            restore_to_parent(&strict, parent, &name)?;
            let returned = at_parent(parent, &name)?;
            if file_identity(&returned)? != self.identity() {
                return Err(Error::Invalid(
                    "Restoration destination identity is unresolved.".into(),
                ));
            }
            check_content(&strict, &self.source.stamp)
        })();
        if let Err(error) = restored {
            if let Err(recording) =
                journal.unresolved_relocation(self.relocation, &error.to_string())
            {
                return Err(Error::Invalid(format!(
                    "{error}; restoration journal: {recording}"
                )));
            }
            return Err(error);
        }
        let journal_error = journal
            .record_restored(self.relocation, self.identity(), &self.source.original_path)
            .err()
            .map(|error| error.to_string());
        Ok(RestoredItem {
            path: self.source.original_path.clone(),
            identity: self.identity(),
            journal_error,
        })
    }
}

pub(super) fn at_parent(parent: &File, name: &StageEntry) -> Result<File> {
    cleanup_io::open_component(
        parent,
        std::ffi::OsStr::new(name.as_str()),
        FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
    )
}
