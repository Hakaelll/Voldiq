use super::denied;
use crate::{Result, windows};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Root {
    pub(super) path: PathBuf,
    identity: (u64, u64),
}

impl Root {
    pub(super) fn reopen(&self) -> Result<AuthorizedPath> {
        let current = AuthorizedPath::open(&self.path)?;
        if current.identity != self.identity {
            return Err(denied());
        }
        Ok(current)
    }
    pub(super) fn capture(path: &Path) -> Result<Self> {
        let checked = AuthorizedPath::open(path)?;
        if !checked
            .handles
            .last()
            .ok_or_else(denied)?
            .metadata()?
            .is_dir()
        {
            return Err(denied());
        }
        Ok(Self {
            path: checked.path.clone(),
            identity: checked.identity,
        })
    }

    pub(super) fn validate(&self) -> Result<()> {
        validate_path(&self.path)
    }

    pub(super) fn contains(&self, path: &Path) -> bool {
        path.starts_with(&self.path)
    }
}

/// Metadata authorization with pinned ancestors. It does not grant mutation authority.
pub struct AuthorizedPath {
    path: PathBuf,
    identity: (u64, u64),
    handles: Vec<File>,
}

impl AuthorizedPath {
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn identity(&self) -> (u64, u64) {
        self.identity
    }

    fn open(path: &Path) -> Result<Self> {
        validate_path(path)?;
        let mut handles = Vec::new();
        for ancestor in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
            let file = open_metadata(ancestor)?;
            let metadata = file.metadata()?;
            if windows::is_reparse(&metadata) || (ancestor != path && !metadata.is_dir()) {
                return Err(denied());
            }
            handles.push(file);
        }
        let file = handles.last().ok_or_else(denied)?;
        let identity = windows::file_identity(file)?.key;
        #[cfg(windows)]
        let canonical = {
            let final_path = windows::final_path(file)?;
            ordinary_path(&final_path)?
        };
        #[cfg(not(windows))]
        let canonical = std::fs::canonicalize(path)?;
        Ok(Self {
            path: canonical,
            identity,
            handles,
        })
    }
}

pub(crate) fn ordinary_path(path: &Path) -> Result<PathBuf> {
    #[cfg(windows)]
    {
        use std::path::Prefix;
        let mut components = path.components();
        if let Some(Component::Prefix(prefix)) = components.next()
            && let Prefix::VerbatimDisk(drive) = prefix.kind()
        {
            let mut ordinary = PathBuf::from(format!("{}:\\", char::from(drive)));
            ordinary.extend(
                components
                    .filter(|part| !matches!(part, Component::RootDir))
                    .map(|part| part.as_os_str()),
            );
            validate_path(&ordinary)?;
            return Ok(ordinary);
        }
    }
    validate_path(path)?;
    Ok(path.to_owned())
}

pub(crate) fn pin_directory(path: &Path) -> Result<AuthorizedPath> {
    let pin = AuthorizedPath::open(&ordinary_path(path)?)?;
    if !pin.handles.last().ok_or_else(denied)?.metadata()?.is_dir() {
        return Err(denied());
    }
    Ok(pin)
}

pub(super) fn authorize(
    roots: &[Root],
    exclusions: &[Root],
    path: &Path,
) -> Result<AuthorizedPath> {
    let checked = AuthorizedPath::open(path)?;
    let root = roots
        .iter()
        .find(|root| root.contains(&checked.path))
        .ok_or_else(denied)?;
    let current = AuthorizedPath::open(&root.path)?;
    if current.identity != root.identity
        || exclusions.iter().any(|excluded| {
            excluded.contains(&checked.path)
                || checked.handles.iter().any(|file| {
                    windows::file_identity(file)
                        .map_or(true, |identity| identity.key == excluded.identity)
                })
        })
    {
        return Err(denied());
    }
    Ok(checked)
}

fn validate_path(path: &Path) -> Result<()> {
    if !path.is_absolute() || path.as_os_str().len() > 32_000 {
        return Err(denied());
    }
    for part in path.components() {
        match part {
            Component::Normal(name) => {
                let name = name.to_str().ok_or_else(denied)?;
                if name.chars().any(|character| {
                    character.is_control()
                        || matches!(character, ':' | '<' | '>' | '"' | '|' | '?' | '*')
                }) || name.ends_with(['.', ' '])
                {
                    return Err(denied());
                }
                #[cfg(windows)]
                {
                    let stem = name
                        .split('.')
                        .next()
                        .unwrap_or("")
                        .trim_end()
                        .to_uppercase();
                    if matches!(
                        stem.as_str(),
                        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
                    ) || ["COM", "LPT"].iter().any(|prefix| {
                        stem.strip_prefix(prefix).is_some_and(|suffix| {
                            matches!(
                                suffix,
                                "1" | "2"
                                    | "3"
                                    | "4"
                                    | "5"
                                    | "6"
                                    | "7"
                                    | "8"
                                    | "9"
                                    | "¹"
                                    | "²"
                                    | "³"
                            )
                        })
                    }) {
                        return Err(denied());
                    }
                }
            }
            Component::RootDir => {}
            #[cfg(windows)]
            Component::Prefix(prefix) if matches!(prefix.kind(), std::path::Prefix::Disk(_)) => {}
            _ => return Err(denied()),
        }
    }
    Ok(())
}

fn open_metadata(path: &Path) -> Result<File> {
    let mut options = std::fs::OpenOptions::new();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::*;
        options
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(
                FILE_FLAG_BACKUP_SEMANTICS
                    | FILE_FLAG_OPEN_REPARSE_POINT
                    | FILE_FLAG_OPEN_NO_RECALL,
            );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.read(true).custom_flags(libc::O_NOFOLLOW);
    }
    Ok(options.open(path)?)
}
