use crate::{Error, Kind, NodeId, Result, index::Index, windows};
use serde::Serialize;
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
    pub fn manual(index: &Index, selection: &[NodeId]) -> Result<Self> {
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
        };
        for &id in selection {
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
            let parents = capture_parents(&path, &protected, &excluded)?;
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
        Ok(Self { preview, files })
    }

    pub fn preview(&self) -> &Preview {
        &self.preview
    }

    /// Validates a preview without changing disk state. Execution must keep the returned handles alive.
    #[cfg(windows)]
    pub fn verify(&self, id: NodeId) -> Result<VerifiedFile> {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::*;
        let planned = self
            .files
            .iter()
            .find(|file| file.id == id)
            .ok_or_else(|| Error::Invalid("File is not in the confirmed plan.".into()))?;
        let mut parents = Vec::new();
        // Pin ancestors from the volume root inward so a path cannot be redirected while opening its leaf.
        for (path, expected) in planned.parents.iter().rev() {
            let file = std::fs::OpenOptions::new()
                .access_mode(FILE_READ_ATTRIBUTES)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(
                    FILE_FLAG_OPEN_REPARSE_POINT
                        | FILE_FLAG_BACKUP_SEMANTICS
                        | FILE_FLAG_OPEN_NO_RECALL,
                )
                .open(path)?;
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
        let file = std::fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES | DELETE)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_OPEN_NO_RECALL)
            .open(&planned.path)?;
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
        })
    }
}

#[cfg(windows)]
pub struct VerifiedFile {
    pub(crate) file: std::fs::File,
    pub(crate) parents: Vec<std::fs::File>,
    pub(crate) path: PathBuf,
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
) -> Result<Vec<(PathBuf, (u64, u64))>> {
    let mut parents = Vec::new();
    let mut next = path.parent();
    while let Some(parent) = next {
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

fn protected_identities() -> Result<HashSet<(u64, u64)>> {
    let mut identities = HashSet::new();
    #[cfg(windows)]
    for path in windows::protected_directories()? {
        identities.insert(windows::identity(&path)?.key);
    }
    Ok(identities)
}
