use crate::Result;
use serde::Serialize;
use std::path::Path;

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
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().into());
        }
        let result = handle_identity(handle);
        CloseHandle(handle);
        result
    }
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
