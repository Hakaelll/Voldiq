//! Persisted selection evidence. Loading it never grants execution authority.
use super::{Plan, PlannedFile, Stamp, capture_parents, protected_identities, validate_file};
use crate::{Error, NodeId, Result, index::Index, snapshots::Scope, windows};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::PathBuf, sync::atomic::AtomicBool};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    root: PathBuf,
    root_identity: (u64, u64),
    exclusions: Vec<(PathBuf, (u64, u64))>,
    files: Vec<PlannedFile>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FileEvidence {
    pub id: NodeId,
    pub path: String,
    pub identity: String,
    pub logical_bytes: String,
    pub allocated_bytes: String,
    pub hard_links: u32,
    pub modified_unix_ns: String,
    pub change_time: String,
}

impl Record {
    pub(crate) fn capture(index: &Index, selection: &[NodeId]) -> Result<Self> {
        // A saved scan supplies evidence only; every selected file is checked against the live filesystem.
        let plan = Plan::prepare_verified(index, selection, &AtomicBool::new(false))?;
        let exclusions = index
            .exclusions
            .iter()
            .map(|path| Ok((path.clone(), windows::identity(path)?.key)))
            .collect::<Result<_>>()?;
        Ok(Self {
            root: index.root.clone(),
            root_identity: index.root_identity.ok_or_else(invalid)?,
            exclusions,
            files: plan.files,
        })
    }

    pub(crate) fn scope(&self) -> Scope {
        Scope {
            root: self.root.clone(),
            identity: self.root_identity,
            exclusions: self
                .exclusions
                .iter()
                .map(|(path, _)| path.clone())
                .collect(),
        }
    }

    pub(crate) fn evidence(&self) -> Result<Vec<FileEvidence>> {
        self.files
            .iter()
            .map(|file| {
                Ok(FileEvidence {
                    id: file.id,
                    path: file.path.to_str().ok_or_else(invalid)?.to_owned(),
                    identity: format!(
                        "{:016x}:{:016x}",
                        file.stamp.identity.0, file.stamp.identity.1
                    ),
                    logical_bytes: file.stamp.size.to_string(),
                    allocated_bytes: file.allocated.to_string(),
                    hard_links: file.links,
                    modified_unix_ns: file
                        .stamp
                        .modified
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(|_| invalid())?
                        .as_nanos()
                        .to_string(),
                    change_time: file.stamp.change_time.to_string(),
                })
            })
            .collect()
    }

    pub(crate) fn validate(&self, state_directory: (u64, u64)) -> Result<()> {
        if self.files.is_empty()
            || self.files.len() > 500
            || windows::identity(&self.root)?.key != self.root_identity
        {
            return Err(invalid());
        }
        let mut protected = protected_identities()?;
        protected.insert(state_directory);
        let mut excluded = HashSet::new();
        for (path, expected) in &self.exclusions {
            if windows::identity(path)?.key != *expected {
                return Err(invalid());
            }
            excluded.insert(*expected);
        }
        let mut unique = HashSet::new();
        for file in &self.files {
            if !unique.insert(file.id)
                || !file.path.starts_with(&self.root)
                || file.digest.is_some()
                || sensitive(&file.path)
            {
                return Err(invalid());
            }
            let parents =
                capture_parents(&file.path, &protected, &excluded, &AtomicBool::new(false))?;
            if parents != file.parents {
                return Err(invalid());
            }
            let (metadata, identity) = windows::metadata_identity(&file.path)?;
            validate_file(&metadata)?;
            let current = Stamp {
                identity: identity.key,
                size: metadata.len(),
                modified: metadata.modified()?,
                change_time: identity.change_time,
            };
            if current != file.stamp
                || identity.allocated != file.allocated
                || identity.links != file.links
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
}

fn sensitive(path: &std::path::Path) -> bool {
    path.components().any(|part| {
        let name = part.as_os_str().to_string_lossy().to_ascii_lowercase();
        matches!(
            name.as_str(),
            ".git"
                | ".ssh"
                | ".aws"
                | ".azure"
                | ".gnupg"
                | ".codex"
                | "$recycle.bin"
                | "system volume information"
        ) || name == ".env"
            || name.starts_with(".env.")
            || name.starts_with(".voldiq-")
            || name.ends_with(".vhd")
            || name.ends_with(".vhdx")
            || matches!(
                name.as_str(),
                "credentials"
                    | "credentials.json"
                    | "auth.json"
                    | "login data"
                    | "key4.db"
                    | "logins.json"
                    | "ntuser.dat"
                    | "pagefile.sys"
                    | "hiberfil.sys"
                    | "swapfile.sys"
            )
    })
}

fn invalid() -> Error {
    Error::Invalid("Selection evidence or protection changed. Prepare a new plan.".into())
}
