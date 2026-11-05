use crate::{Error, Result, windows};
use std::{fs::File, path::Path};

pub(super) fn directory(
    path: &Path,
    root: &Path,
    root_identity: Option<(u64, u64)>,
    expected: Option<(u64, u64)>,
) -> Result<Vec<File>> {
    let changed = || Error::Invalid("Directory identity changed or is unavailable.".into());
    let expected = expected.ok_or_else(changed)?;
    let root_identity = root_identity.ok_or_else(changed)?;
    #[cfg(windows)]
    {
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        let mut pins = Vec::new();
        let mut saw_root = false;
        for ancestor in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
            let file = match pins.last() {
                None => windows::cleanup_io::open_root(ancestor)?,
                Some(parent) => windows::cleanup_io::open_component(
                    parent,
                    ancestor.file_name().ok_or_else(changed)?,
                    FILE_READ_ATTRIBUTES
                        | if ancestor == path {
                            FILE_LIST_DIRECTORY
                        } else {
                            0
                        },
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                )?,
            };
            let metadata = file.metadata()?;
            if !metadata.is_dir() || windows::is_reparse(&metadata) {
                return Err(changed());
            }
            let identity = windows::file_identity(&file)?.key;
            if ancestor == root {
                if identity != root_identity {
                    return Err(changed());
                }
                saw_root = true;
            }
            if ancestor == path && identity != expected {
                return Err(changed());
            }
            pins.push(file);
        }
        if !saw_root {
            return Err(changed());
        }
        Ok(pins)
    }
    #[cfg(not(windows))]
    {
        let (metadata, identity) = windows::metadata_identity(path)?;
        if !metadata.is_dir()
            || windows::is_reparse(&metadata)
            || identity.key != expected
            || windows::identity(root)?.key != root_identity
        {
            return Err(changed());
        }
        Ok(Vec::new())
    }
}
