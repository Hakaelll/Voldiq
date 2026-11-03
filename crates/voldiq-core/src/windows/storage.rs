use std::path::Path;

#[cfg(windows)]
pub(crate) fn volume_paths(
    file: &std::fs::File,
) -> crate::Result<(std::path::PathBuf, std::path::PathBuf)> {
    use std::os::windows::{ffi::OsStringExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_NAME_NORMALIZED, GetFinalPathNameByHandleW, VOLUME_NAME_GUID,
    };
    let mut buffer = vec![0_u16; 32768];
    let length = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            VOLUME_NAME_GUID | FILE_NAME_NORMALIZED,
        )
    } as usize;
    if length == 0 || length >= buffer.len() {
        return Err(std::io::Error::last_os_error().into());
    }
    buffer.truncate(length);
    let native = std::ffi::OsString::from_wide(&buffer);
    let text = native
        .to_str()
        .ok_or_else(|| crate::Error::Invalid("Native volume path unavailable.".into()))?;
    let suffix = text
        .strip_prefix(r"\\?\Volume{")
        .ok_or_else(|| crate::Error::Invalid("Stable volume GUID unavailable.".into()))?;
    let close = suffix
        .find("}\\")
        .ok_or_else(|| crate::Error::Invalid("Invalid native volume root.".into()))?;
    let root = format!(r"\\?\Volume{{{}}}\", &suffix[..close]);
    Ok((native.into(), root.into()))
}

#[cfg(not(windows))]
pub fn storage_seek_penalty(_path: &Path) -> Option<bool> {
    None
}

#[cfg(windows)]
pub fn storage_seek_penalty(path: &Path) -> Option<bool> {
    use std::path::{Component, Prefix};
    if !path.is_absolute()
        || matches!(path.components().next(), Some(Component::Prefix(prefix))
        if matches!(prefix.kind(), Prefix::UNC(_, _) | Prefix::VerbatimUNC(_, _)))
    {
        return None;
    }
    use std::{
        fs::File,
        os::windows::io::{AsRawHandle, FromRawHandle},
    };
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        Storage::FileSystem::*,
        System::{IO::DeviceIoControl, Ioctl::*, WindowsProgramming::DRIVE_FIXED},
    };
    let input = super::wide(path);
    let mut mount = vec![0_u16; 32768];
    if unsafe { GetVolumePathNameW(input.as_ptr(), mount.as_mut_ptr(), mount.len() as u32) } == 0
        || unsafe { GetDriveTypeW(mount.as_ptr()) } != DRIVE_FIXED
    {
        return None;
    }
    let mut device = [0_u16; 128];
    if unsafe {
        GetVolumeNameForVolumeMountPointW(mount.as_ptr(), device.as_mut_ptr(), device.len() as u32)
    } == 0
    {
        return None;
    }
    let length = device.iter().position(|unit| *unit == 0)?;
    if length < 2 || device[length - 1] != u16::from(b'\\') {
        return None;
    }
    device[length - 1] = 0;
    let handle = unsafe {
        CreateFileW(
            device.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return None;
    }
    let handle = unsafe { File::from_raw_handle(handle) };
    let query = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceSeekPenaltyProperty,
        QueryType: PropertyStandardQuery,
        ..Default::default()
    };
    let mut descriptor = [0_u8; 12];
    let mut returned = 0;
    if unsafe {
        DeviceIoControl(
            handle.as_raw_handle(),
            IOCTL_STORAGE_QUERY_PROPERTY,
            (&query as *const STORAGE_PROPERTY_QUERY).cast(),
            std::mem::size_of_val(&query) as u32,
            descriptor.as_mut_ptr().cast(),
            descriptor.len() as u32,
            &mut returned,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return None;
    }
    parse_descriptor(descriptor.get(..usize::try_from(returned).ok()?)?)
}

#[cfg(any(windows, test))]
fn parse_descriptor(bytes: &[u8]) -> Option<bool> {
    let version = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?);
    let size = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?);
    if version < 12 || size < 12 || bytes.len() < 12 {
        return None;
    }
    match bytes[8] {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}
