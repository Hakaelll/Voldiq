use crate::{Error, Kind, NodeId, Result, index::Index, windows};
use serde::Serialize;
#[cfg(all(test, windows))]
mod disposition_tests;
pub mod rules;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Serialize)]
pub struct PreviewItem {
    pub id: NodeId,
    pub path: String,
    pub logical: u64,
    pub reclaimable: u64,
    pub hard_link: bool,
}

#[derive(Clone, Serialize)]
pub struct Preview {
    pub rule: &'static str,
    pub risk: &'static str,
    pub policy: &'static str,
    pub items: Vec<PreviewItem>,
    pub logical: u64,
    pub reclaimable: u64,
    pub retained: Vec<String>,
}

#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    identity: (u64, u64),
    size: u64,
    modified: SystemTime,
    change_time: u64,
}

struct PlannedFile {
    id: NodeId,
    path: PathBuf,
    stamp: Stamp,
    parents: Vec<(PathBuf, (u64, u64))>,
}

pub struct Plan {
    preview: Preview,
    files: Vec<PlannedFile>,
}

impl Plan {
    pub fn duplicates(
        index: &Index,
        analysis: &crate::duplicates::Analysis,
        selection: &[NodeId],
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<Self> {
        let retained = analysis.review_selection(index, selection, cancel)?;
        let keep = Self::prepare(index, &retained, cancel)?;
        let mut plan = Self::prepare(index, selection, cancel)?;
        plan.preview.rule = "duplicate-files";
        plan.preview.retained = keep
            .preview
            .items
            .into_iter()
            .map(|item| item.path)
            .collect();
        Ok(plan)
    }

    pub fn manual(index: &Index, selection: &[NodeId]) -> Result<Self> {
        Self::prepare(index, selection, &std::sync::atomic::AtomicBool::new(false))
    }

    fn prepare(
        index: &Index,
        selection: &[NodeId],
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<Self> {
        check_cancelled(cancel)?;
        if index.snapshot_id.is_some() {
            return Err(Error::Invalid(
                "Rescan before managing historical files.".into(),
            ));
        }
        if selection.is_empty() || selection.len() > 500 {
            return Err(Error::Invalid("Select between 1 and 500 files.".into()));
        }
        let root_key = index
            .root_identity
            .ok_or_else(|| Error::Invalid("Root identity unavailable.".into()))?;
        if windows::identity(&index.root)?.key != root_key {
            return Err(Error::Invalid("Scan root changed. Rescan first.".into()));
        }
        let excluded: HashSet<_> = index
            .exclusions
            .iter()
            .map(|path| windows::identity(path).map(|identity| identity.key))
            .collect::<Result<_>>()?;
        let protected = protected_identities()?;
        let mut unique = HashSet::new();
        let mut files = Vec::new();
        let mut preview = Preview {
            rule: "manual-files",
            risk: "review",
            policy: "recycle",
            items: Vec::new(),
            logical: 0,
            reclaimable: 0,
            retained: Vec::new(),
        };
        for &id in selection {
            check_cancelled(cancel)?;
            if !unique.insert(id) {
                return Err(Error::Invalid("Duplicate selection.".into()));
            }
            let entry = index.entry(id)?;
            if entry.kind != Kind::File || entry.incomplete {
                return Err(Error::Invalid(
                    "Select complete, regular files only.".into(),
                ));
            }
            let path = index.path(id)?;
            if path.to_str().is_none() {
                return Err(Error::Invalid(
                    "Filename cannot be represented exactly in the preview.".into(),
                ));
            }
            if !path.starts_with(&index.root) {
                return Err(Error::Invalid("File is outside the scan root.".into()));
            }
            let parents = capture_parents(&path, &protected, &excluded, cancel)?;
            let (metadata, identity) = windows::metadata_identity(&path)?;
            validate_file(&metadata)?;
            let expected = entry.file_state.ok_or_else(|| {
                Error::Invalid("File identity was not captured. Rescan first.".into())
            })?;
            if identity.key != expected.identity || identity.change_time != expected.change_time {
                return Err(Error::Invalid(
                    "File changed since scanning. Rescan first.".into(),
                ));
            }
            if excluded.contains(&identity.key) {
                return Err(Error::Invalid("File is excluded.".into()));
            }
            if metadata.len() != entry.logical
                || crate::scanner::modified(&metadata) != entry.modified
            {
                return Err(Error::Invalid(
                    "File changed since scanning. Rescan first.".into(),
                ));
            }
            let reclaimable = if identity.links == 1 {
                identity.allocated
            } else {
                0
            };
            preview.logical = preview
                .logical
                .checked_add(metadata.len())
                .ok_or_else(|| Error::Invalid("Selection size overflow.".into()))?;
            preview.reclaimable = preview
                .reclaimable
                .checked_add(reclaimable)
                .ok_or_else(|| Error::Invalid("Selection allocation overflow.".into()))?;
            preview.items.push(PreviewItem {
                id,
                path: path.to_string_lossy().into_owned(),
                logical: metadata.len(),
                reclaimable,
                hard_link: identity.links > 1,
            });
            files.push(PlannedFile {
                id,
                path,
                stamp: Stamp {
                    identity: identity.key,
                    size: metadata.len(),
                    modified: metadata.modified()?,
                    change_time: identity.change_time,
                },
                parents,
            });
        }
        check_cancelled(cancel)?;
        Ok(Self { preview, files })
    }

    pub fn preview(&self) -> &Preview {
        &self.preview
    }

    /// Validates a preview without changing disk state. Execution must keep the returned handles alive.
    #[cfg(windows)]
    pub fn verify(&self, id: NodeId) -> Result<VerifiedFile> {
        let planned = self
            .files
            .iter()
            .find(|file| file.id == id)
            .ok_or_else(|| Error::Invalid("File is not in the confirmed plan.".into()))?;
        let mut parents = Vec::new();
        // Pin ancestor names from the volume root inward; independently check the opened leaf's identity.
        for (path, expected) in planned.parents.iter().rev() {
            let file = match parents.last() {
                Some(parent) => windows::cleanup_io::open_child(
                    parent,
                    path.file_name()
                        .ok_or_else(|| Error::Invalid("Parent name unavailable.".into()))?,
                    true,
                )?,
                None => windows::cleanup_io::open_root(path)?,
            };
            let metadata = file.metadata()?;
            if !metadata.is_dir()
                || windows::is_reparse(&metadata)
                || windows::requires_recall(&metadata)
                || windows::file_identity(&file)?.key != *expected
            {
                return Err(Error::Invalid(
                    "Parent directory changed since preview.".into(),
                ));
            }
            parents.push(file);
        }
        let parent = parents
            .last()
            .ok_or_else(|| Error::Invalid("Parent authority unavailable.".into()))?;
        let file = windows::cleanup_io::open_child(
            parent,
            planned
                .path
                .file_name()
                .ok_or_else(|| Error::Invalid("File name unavailable.".into()))?,
            false,
        )?;
        let metadata = file.metadata()?;
        validate_file(&metadata)?;
        let identity = windows::file_identity(&file)?;
        let current = Stamp {
            identity: identity.key,
            size: metadata.len(),
            modified: metadata.modified()?,
            change_time: identity.change_time,
        };
        if current != planned.stamp {
            return Err(Error::Invalid("File changed since preview.".into()));
        }
        Ok(VerifiedFile {
            file,
            parents,
            path: planned.path.clone(),
            stamp: current,
        })
    }
}

#[cfg(windows)]
pub struct VerifiedFile {
    pub(crate) file: std::fs::File,
    pub(crate) parents: Vec<std::fs::File>,
    pub(crate) path: PathBuf,
    stamp: Stamp,
}

#[cfg(windows)]
impl VerifiedFile {
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn identity(&self) -> Result<(u64, u64)> {
        Ok(windows::file_identity(&self.file)?.key)
    }
    pub fn pinned_parents(&self) -> usize {
        self.parents.len()
    }

    /// Permanently unlinks this verified file handle. This never recycles or falls back to a path API.
    /// The caller must provide explicit permanent-deletion confirmation and durable operation logging.
    pub fn remove_permanently(self) -> Result<()> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_DISPOSITION_FLAG_DELETE, FILE_DISPOSITION_FLAG_FORCE_IMAGE_SECTION_CHECK,
            FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO_EX, FileDispositionInfoEx,
            SetFileInformationByHandle,
        };
        let metadata = self.file.metadata()?;
        validate_file(&metadata)?;
        let identity = windows::file_identity(&self.file)?;
        let stamp = Stamp {
            identity: identity.key,
            size: metadata.len(),
            modified: metadata.modified()?,
            change_time: identity.change_time,
        };
        if stamp != self.stamp || metadata.permissions().readonly() {
            return Err(Error::Invalid("File changed or is read-only.".into()));
        }
        let info = FILE_DISPOSITION_INFO_EX {
            Flags: FILE_DISPOSITION_FLAG_DELETE
                | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS
                | FILE_DISPOSITION_FLAG_FORCE_IMAGE_SECTION_CHECK,
        };
        // The verified handle and all ancestor handles stay owned until this synchronous call finishes.
        let success = unsafe {
            SetFileInformationByHandle(
                self.file.as_raw_handle(),
                FileDispositionInfoEx,
                (&info as *const FILE_DISPOSITION_INFO_EX).cast(),
                std::mem::size_of::<FILE_DISPOSITION_INFO_EX>() as u32,
            )
        };
        if success == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // POSIX disposition removes this link when its deleting handle closes; other links/readers may retain data.
        drop(self);
        Ok(())
    }
}

fn validate_file(metadata: &std::fs::Metadata) -> Result<()> {
    if !metadata.is_file() || windows::is_reparse(metadata) || windows::requires_recall(metadata) {
        return Err(Error::Invalid(
            "Reparse, offline and non-file targets are excluded.".into(),
        ));
    }
    Ok(())
}

fn capture_parents(
    path: &Path,
    protected: &HashSet<(u64, u64)>,
    excluded: &HashSet<(u64, u64)>,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<Vec<(PathBuf, (u64, u64))>> {
    let mut parents = Vec::new();
    let mut next = path.parent();
    while let Some(parent) = next {
        check_cancelled(cancel)?;
        let metadata = std::fs::symlink_metadata(parent)?;
        if !metadata.is_dir()
            || windows::is_reparse(&metadata)
            || windows::requires_recall(&metadata)
        {
            return Err(Error::Invalid(
                "Redirected parent directory is excluded.".into(),
            ));
        }
        let key = windows::identity(parent)?.key;
        if protected.contains(&key) {
            return Err(Error::Invalid(
                "Protected system directory is excluded.".into(),
            ));
        }
        if excluded.contains(&key) {
            return Err(Error::Invalid("Directory is excluded.".into()));
        }
        parents.push((parent.to_owned(), key));
        next = parent.parent();
    }
    Ok(parents)
}

fn check_cancelled(cancel: &std::sync::atomic::AtomicBool) -> Result<()> {
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(Error::Invalid("Cleanup review cancelled.".into()));
    }
    Ok(())
}

fn protected_identities() -> Result<HashSet<(u64, u64)>> {
    let mut identities = HashSet::new();
    #[cfg(windows)]
    for path in windows::protected_directories()? {
        identities.insert(windows::identity(&path)?.key);
    }
    Ok(identities)
}
