use crate::Result;
use serde::Serialize;
use std::path::Path;

#[cfg(windows)]
pub mod cleanup_channel;
#[cfg(windows)]
pub(crate) mod cleanup_io;
#[cfg(windows)]
pub mod cleanup_stage;
pub mod recycle_bin;
#[cfg(windows)]
mod security;
mod storage;
pub use storage::storage_seek_penalty;
mod settings;
pub use settings::{open_recycle_bin, open_storage_settings};

#[derive(Serialize)]
pub struct Volume {
    pub path: String,
    pub name: String,
    pub filesystem: String,
    pub total: u64,
    pub free: u64,
    pub drive_type: String,
    pub accessible: bool,
}

pub struct FileIdentity {
    pub key: (u64, u64),
    pub allocated: u64,
    pub links: u32,
    pub change_time: u64,
}

#[cfg(windows)]
pub fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(windows)]
pub fn identity(path: &Path) -> Result<FileIdentity> {
    file_identity(&open_metadata(path)?)
}

#[cfg(windows)]
fn open_metadata(path: &Path) -> Result<std::fs::File> {
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::{Foundation::*, Storage::FileSystem::*};
    let path = wide(path);
    // Opening metadata only with OPEN_REPARSE_POINT does not read cloud-only file contents.
    unsafe {
        let handle = CreateFileW(
            path.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_OPEN_NO_RECALL,
            std::ptr::null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(std::fs::File::from_raw_handle(handle))
    }
}

#[cfg(windows)]
pub fn metadata_identity(path: &Path) -> Result<(std::fs::Metadata, FileIdentity)> {
    let file = open_metadata(path)?;
    let before = file_identity(&file)?;
    let metadata = file.metadata()?;
    let identity = file_identity(&file)?;
    if before.key != identity.key || before.change_time != identity.change_time {
        return Err(crate::Error::Invalid(
            "File changed while reading metadata.".into(),
        ));
    }
    Ok((metadata, identity))
}

#[cfg(unix)]
pub fn metadata_identity(path: &Path) -> Result<(std::fs::Metadata, FileIdentity)> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let before = file_identity(&file)?;
    let metadata = file.metadata()?;
    let identity = file_identity(&file)?;
    if before.key != identity.key || before.change_time != identity.change_time {
        return Err(crate::Error::Invalid(
            "File changed while reading metadata.".into(),
        ));
    }
    Ok((metadata, identity))
}

#[cfg(windows)]
pub fn protected_directories() -> Result<Vec<std::path::PathBuf>> {
    use windows_sys::Win32::UI::Shell::*;
    [
        FOLDERID_Windows,
        FOLDERID_ProgramFiles,
        FOLDERID_ProgramFilesX86,
        FOLDERID_ProgramData,
    ]
    .iter()
    .map(known_folder_path)
    .collect()
}

#[cfg(windows)]
pub fn pip_configuration_directories() -> Result<[std::path::PathBuf; 2]> {
    use windows_sys::Win32::UI::Shell::{FOLDERID_ProgramData, FOLDERID_RoamingAppData};
    Ok([
        known_folder_path(&FOLDERID_ProgramData)?,
        known_folder_path(&FOLDERID_RoamingAppData)?,
    ])
}

#[cfg(windows)]
pub fn local_app_data() -> Result<std::path::PathBuf> {
    known_folder_path(&windows_sys::Win32::UI::Shell::FOLDERID_LocalAppData)
}

#[cfg(windows)]
fn known_folder_path(folder: &windows_sys::core::GUID) -> Result<std::path::PathBuf> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    use windows_sys::Win32::{System::Com::CoTaskMemFree, UI::Shell::*};
    let mut raw = std::ptr::null_mut();
    let status = unsafe {
        SHGetKnownFolderPath(
            folder,
            KF_FLAG_DEFAULT as u32,
            std::ptr::null_mut(),
            &mut raw,
        )
    };
    if status < 0 || raw.is_null() {
        if !raw.is_null() {
            unsafe { CoTaskMemFree(raw.cast()) };
        }
        return Err(crate::Error::Invalid(format!(
            "Protected Windows location unavailable: {status:#x}."
        )));
    }
    let mut length = 0;
    while length < 32768 && unsafe { *raw.add(length) } != 0 {
        length += 1;
    }
    let path = if length < 32768 {
        Some(std::path::PathBuf::from(OsString::from_wide(unsafe {
            std::slice::from_raw_parts(raw, length)
        })))
    } else {
        None
    };
    unsafe { CoTaskMemFree(raw.cast()) };
    let path = path
        .filter(|path| path.is_absolute())
        .ok_or_else(|| crate::Error::Invalid("Invalid protected Windows location.".into()))?;
    Ok(path)
}

#[cfg(windows)]
pub fn file_identity(file: &std::fs::File) -> Result<FileIdentity> {
    use std::os::windows::io::AsRawHandle;
    handle_identity(file.as_raw_handle())
}

#[cfg(windows)]
fn handle_identity(handle: windows_sys::Win32::Foundation::HANDLE) -> Result<FileIdentity> {
    use windows_sys::Win32::Storage::FileSystem::*;
    unsafe {
        let mut info: BY_HANDLE_FILE_INFORMATION = std::mem::zeroed();
        let mut standard: FILE_STANDARD_INFO = std::mem::zeroed();
        let mut basic: FILE_BASIC_INFO = std::mem::zeroed();
        let ok = GetFileInformationByHandle(handle, &mut info) != 0
            && GetFileInformationByHandleEx(
                handle,
                FileStandardInfo,
                &mut standard as *mut _ as *mut _,
                std::mem::size_of_val(&standard) as u32,
            ) != 0
            && GetFileInformationByHandleEx(
                handle,
                FileBasicInfo,
                &mut basic as *mut _ as *mut _,
                std::mem::size_of_val(&basic) as u32,
            ) != 0;
        let error = std::io::Error::last_os_error();
        if !ok {
            return Err(error.into());
        }
        Ok(FileIdentity {
            key: (
                info.dwVolumeSerialNumber as u64,
                ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64,
            ),
            allocated: standard.AllocationSize.max(0) as u64,
            links: info.nNumberOfLinks,
            change_time: basic.ChangeTime as u64,
        })
    }
}

#[cfg(unix)]
pub fn identity(path: &Path) -> Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::symlink_metadata(path)?;
    Ok(FileIdentity {
        key: (metadata.dev(), metadata.ino()),
        allocated: metadata.blocks() * 512,
        links: metadata.nlink() as u32,
        change_time: (metadata.ctime() as u64)
            .wrapping_mul(1_000_000_000)
            .wrapping_add(metadata.ctime_nsec() as u64),
    })
}

#[cfg(unix)]
pub fn file_identity(file: &std::fs::File) -> Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    Ok(FileIdentity {
        key: (metadata.dev(), metadata.ino()),
        allocated: metadata.blocks() * 512,
        links: metadata.nlink() as u32,
        change_time: (metadata.ctime() as u64)
            .wrapping_mul(1_000_000_000)
            .wrapping_add(metadata.ctime_nsec() as u64),
    })
}

#[cfg(windows)]
pub fn volumes() -> Vec<Volume> {
    use windows_sys::Win32::Storage::FileSystem::*;
    use windows_sys::Win32::System::WindowsProgramming::{
        DRIVE_CDROM, DRIVE_FIXED, DRIVE_REMOTE, DRIVE_REMOVABLE,
    };
    let mut result = Vec::new();
    unsafe {
        let drives = GetLogicalDrives();
        for letter in 0..26 {
            if drives & (1 << letter) == 0 {
                continue;
            }
            let path = format!("{}:\\", (b'A' + letter as u8) as char);
            let wide = wide(Path::new(&path));
            let (mut total, mut free, mut available) = (0, 0, 0);
            let accessible =
                GetDiskFreeSpaceExW(wide.as_ptr(), &mut available, &mut total, &mut free) != 0;
            let (mut name, mut filesystem) = ([0_u16; 261], [0_u16; 261]);
            GetVolumeInformationW(
                wide.as_ptr(),
                name.as_mut_ptr(),
                name.len() as u32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                filesystem.as_mut_ptr(),
                filesystem.len() as u32,
            );
            let decode = |text: &[u16]| {
                String::from_utf16_lossy(
                    &text[..text.iter().position(|c| *c == 0).unwrap_or(text.len())],
                )
            };
            let drive_type = match GetDriveTypeW(wide.as_ptr()) {
                DRIVE_FIXED => "Local",
                DRIVE_REMOVABLE => "Removable",
                DRIVE_REMOTE => "Network",
                DRIVE_CDROM => "Optical",
                _ => "Other",
            }
            .into();
            result.push(Volume {
                path,
                name: decode(&name),
                filesystem: decode(&filesystem),
                total,
                free,
                drive_type,
                accessible,
            });
        }
    }
    result
}

#[cfg(not(windows))]
pub fn volumes() -> Vec<Volume> {
    Vec::new()
}

pub fn is_reparse(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.is_symlink()
    }
}

pub fn requires_recall(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::*;
        metadata.file_attributes()
            & (FILE_ATTRIBUTE_OFFLINE
                | FILE_ATTRIBUTE_RECALL_ON_OPEN
                | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
            != 0
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}

#[cfg(windows)]
pub fn final_path(file: &std::fs::File) -> Result<std::path::PathBuf> {
    use std::os::windows::{ffi::OsStringExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::*;
    let mut buffer = vec![0_u16; 260];
    loop {
        let length = unsafe {
            GetFinalPathNameByHandleW(
                file.as_raw_handle(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
            )
        } as usize;
        if length == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        if length >= buffer.len() {
            if length >= 32768 {
                return Err(crate::Error::Invalid(
                    "Native path exceeds Windows bounds.".into(),
                ));
            }
            buffer.resize(length + 1, 0);
            continue;
        }
        return Ok(std::path::PathBuf::from(std::ffi::OsString::from_wide(
            &buffer[..length],
        )));
    }
}

#[cfg(windows)]
pub fn hard_link_names(path: &Path) -> Result<Vec<std::path::PathBuf>> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::{Foundation::*, Storage::FileSystem::*};
    struct Enumeration(HANDLE);
    impl Drop for Enumeration {
        fn drop(&mut self) {
            unsafe {
                FindClose(self.0);
            }
        }
    }
    let input = wide(path);
    let mut root = vec![0_u16; 32768];
    if unsafe { GetVolumePathNameW(input.as_ptr(), root.as_mut_ptr(), root.len() as u32) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let root_length = root
        .iter()
        .position(|unit| *unit == 0)
        .ok_or_else(|| crate::Error::Invalid("Invalid hard-link volume path.".into()))?;
    let root = std::fs::canonicalize(std::path::PathBuf::from(std::ffi::OsString::from_wide(
        &root[..root_length],
    )))?;
    let mut buffer = vec![0_u16; 260];
    let handle = loop {
        let mut length = buffer.len() as u32;
        let handle =
            unsafe { FindFirstFileNameW(input.as_ptr(), 0, &mut length, buffer.as_mut_ptr()) };
        if handle != INVALID_HANDLE_VALUE {
            break Enumeration(handle);
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_MORE_DATA as i32) {
            return Err(error.into());
        }
        grow_name_buffer(&mut buffer, length)?;
    };
    let mut names = Vec::new();
    loop {
        let length = buffer
            .iter()
            .position(|unit| *unit == 0)
            .ok_or_else(|| crate::Error::Invalid("Unterminated hard-link name.".into()))?;
        let relative = std::path::PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length]));
        let relative = relative
            .strip_prefix(Path::new("\\"))
            .map_err(|_| crate::Error::Invalid("Invalid hard-link name prefix.".into()))?;
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(crate::Error::Invalid(
                "Invalid hard-link filename components.".into(),
            ));
        }
        names.push(root.join(relative));
        if names.len() >= 65536 {
            return Err(crate::Error::Invalid(
                "Hard-link name limit reached.".into(),
            ));
        }
        loop {
            let mut length = buffer.len() as u32;
            if unsafe { FindNextFileNameW(handle.0, &mut length, buffer.as_mut_ptr()) } != 0 {
                break;
            }
            let error = std::io::Error::last_os_error();
            match error.raw_os_error() {
                Some(code) if code == ERROR_HANDLE_EOF as i32 => return Ok(names),
                Some(code) if code == ERROR_MORE_DATA as i32 => {
                    grow_name_buffer(&mut buffer, length)?
                }
                _ => return Err(error.into()),
            }
        }
    }
}

#[cfg(windows)]
fn grow_name_buffer(buffer: &mut Vec<u16>, required: u32) -> Result<()> {
    let required = required as usize;
    if required <= buffer.len() || required > 32768 {
        return Err(crate::Error::Invalid(
            "Invalid native filename buffer length.".into(),
        ));
    }
    buffer.resize(required, 0);
    Ok(())
}
