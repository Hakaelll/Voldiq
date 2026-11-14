use crate::{Error, Result, agents::AuthorizedPath};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Identity of a folder presented for a human scan. It grants no cleanup or agent access.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScanFolder {
    path: String,
    identity: [String; 2],
}

impl ScanFolder {
    pub fn path(&self) -> &Path {
        Path::new(&self.path)
    }

    pub fn prepare(path: &Path) -> Result<Self> {
        let directory = pin(path)?;
        let path = directory
            .path()
            .to_str()
            .ok_or_else(|| Error::Invalid("Folder path cannot be represented exactly.".into()))?;
        Ok(Self {
            path: path.to_owned(),
            identity: identity(&directory),
        })
    }

    pub fn reopen(&self) -> Result<AuthorizedPath> {
        let directory = pin(self.path())?;
        if identity(&directory) != self.identity {
            return Err(Error::Invalid(
                "The folder changed. Select it again to scan its current contents.".into(),
            ));
        }
        Ok(directory)
    }
}

fn identity(directory: &AuthorizedPath) -> [String; 2] {
    let (volume, file) = directory.identity();
    [volume.to_string(), file.to_string()]
}

fn pin(path: &Path) -> Result<AuthorizedPath> {
    let invalid = || Error::Invalid("Choose one accessible local folder.".into());
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        use windows_sys::Win32::{
            Storage::FileSystem::GetDriveTypeW,
            System::WindowsProgramming::{DRIVE_FIXED, DRIVE_REMOVABLE},
        };
        let Some(Component::Prefix(prefix)) = path.components().next() else {
            return Err(invalid());
        };
        let (Prefix::Disk(drive) | Prefix::VerbatimDisk(drive)) = prefix.kind() else {
            return Err(invalid());
        };
        let root = [u16::from(drive), u16::from(b':'), u16::from(b'\\'), 0];
        if !matches!(
            unsafe { GetDriveTypeW(root.as_ptr()) },
            DRIVE_FIXED | DRIVE_REMOVABLE
        ) {
            return Err(invalid());
        }
    }
    let directory = crate::agents::pin_directory(path).map_err(|_| invalid())?;
    #[cfg(windows)]
    directory.directory_handle().map_err(|_| invalid())?;
    Ok(directory)
}
