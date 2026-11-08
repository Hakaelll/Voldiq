use super::{Locations, PagingLocations, RegisteredDisk, RegistrationCoverage, WslLocations};
use crate::{
    Error, Result,
    agents::ordinary_path,
    windows::{self, registry::Key},
};
use std::{
    ffi::OsString,
    os::windows::ffi::OsStringExt,
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    sync::atomic::{AtomicBool, Ordering},
};
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS},
    Storage::FileSystem::GetVolumePathNameW,
    System::Registry::{
        HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_ENUMERATE_SUB_KEYS, KEY_QUERY_VALUE,
        RegEnumKeyExW,
    },
    UI::Shell::{FOLDERID_LocalAppData, FOLDERID_ProgramData, FOLDERID_Windows},
};

const WSL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Lxss";
const REGISTRATION_LIMIT: u32 = 128;

pub(super) fn discover(
    root: &Path,
    selected_docker_disk: Option<PathBuf>,
    cancel: &AtomicBool,
) -> Locations {
    let windows_directory = windows::known_folder_location(&FOLDERID_Windows).ok();
    Locations {
        scan_volume_root: volume_root(root),
        windows_volume_root: windows_directory.as_deref().and_then(volume_root),
        windows_directory,
        local_app_data: windows::known_folder_location(&FOLDERID_LocalAppData).ok(),
        program_data: windows::known_folder_location(&FOLDERID_ProgramData).ok(),
        wsl: wsl_locations(cancel),
        paging: paging_locations(cancel),
        selected_docker_disk,
    }
}

const MEMORY_MANAGEMENT: &str =
    r"SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management";
const PAGING_LIMIT: usize = 32;

fn paging_locations(cancel: &AtomicBool) -> PagingLocations {
    let mut result = PagingLocations {
        coverage: RegistrationCoverage::Unavailable,
        paths: Vec::new(),
    };
    if cancel.load(Ordering::Relaxed) {
        return result;
    }
    let Ok(key) = Key::open(HKEY_LOCAL_MACHINE, MEMORY_MANAGEMENT, KEY_QUERY_VALUE) else {
        return result;
    };
    let revision = key.revision().ok();
    let Ok(Some(entries)) = key.multi_string("ExistingPageFiles") else {
        return result;
    };
    result.coverage =
        if entries.len() > PAGING_LIMIT || revision.is_none() || key.revision().ok() != revision {
            RegistrationCoverage::Partial
        } else {
            RegistrationCoverage::Complete
        };
    for entry in entries.iter().take(PAGING_LIMIT) {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let name = entry.strip_prefix(r"\??\").unwrap_or(entry);
        match ordinary_path(Path::new(name)) {
            Ok(path) if path.file_name().is_some() => result.paths.push(path),
            _ => result.coverage = RegistrationCoverage::Partial,
        }
    }
    result
}

fn volume_root(path: &Path) -> Option<PathBuf> {
    let input = windows::wide(path);
    let mut buffer = vec![0; 32_768];
    if unsafe { GetVolumePathNameW(input.as_ptr(), buffer.as_mut_ptr(), buffer.len() as u32) } == 0
    {
        return None;
    }
    let end = buffer.iter().position(|&unit| unit == 0)?;
    ordinary_path(Path::new(&OsString::from_wide(&buffer[..end]))).ok()
}

fn wsl_locations(cancel: &AtomicBool) -> WslLocations {
    let mut result = WslLocations {
        coverage: RegistrationCoverage::Complete,
        disks: Vec::new(),
    };
    let root = match Key::open(
        HKEY_CURRENT_USER,
        WSL_KEY,
        KEY_QUERY_VALUE | KEY_ENUMERATE_SUB_KEYS,
    ) {
        Ok(key) => key,
        Err(ERROR_FILE_NOT_FOUND) => return result,
        Err(_) => {
            result.coverage = RegistrationCoverage::Unavailable;
            return result;
        }
    };
    let revision = root.revision().ok();
    for position in 0..=REGISTRATION_LIMIT {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let mut name = [0; 256];
        let mut length = name.len() as u32;
        let status = unsafe {
            RegEnumKeyExW(
                root.0,
                position,
                name.as_mut_ptr(),
                &mut length,
                null(),
                null_mut(),
                null_mut(),
                null_mut(),
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            break;
        }
        if status != ERROR_SUCCESS
            || position == REGISTRATION_LIMIT
            || length as usize >= name.len()
        {
            result.coverage = RegistrationCoverage::Partial;
            break;
        }
        match String::from_utf16(&name[..length as usize])
            .ok()
            .filter(|name| is_guid(name))
        {
            Some(name) => match registration(&root, &name) {
                Ok(Some(disk)) => result.disks.push(disk),
                Ok(None) => {}
                Err(_) => result.coverage = RegistrationCoverage::Partial,
            },
            None => result.coverage = RegistrationCoverage::Partial,
        }
    }
    if revision.is_none() || root.revision().ok() != revision {
        result.coverage = RegistrationCoverage::Partial;
    }
    result
}

fn is_guid(key: &str) -> bool {
    let Some(text) = key
        .strip_prefix('{')
        .and_then(|text| text.strip_suffix('}'))
    else {
        return false;
    };
    text.len() == 36
        && text
            .split('-')
            .zip([8, 4, 4, 4, 12])
            .all(|(piece, length)| {
                piece.len() == length && piece.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
}

fn registration(root: &Key, id: &str) -> Result<Option<RegisteredDisk>> {
    let invalid = || Error::Invalid("WSL registration is unavailable or unsupported.".into());
    let key = Key::open(root.0, id, KEY_QUERY_VALUE).map_err(|_| invalid())?;
    let before = key.revision()?;
    let version = key.dword("Version")?;
    let disk = match version {
        Some(1) => None,
        Some(2) => {
            let name = key.string("DistributionName", 256)?.ok_or_else(invalid)?;
            let base = ordinary_path(Path::new(
                &key.string("BasePath", 4096)?.ok_or_else(invalid)?,
            ))?;
            let file = key
                .string("VhdFileName", 1024)?
                .unwrap_or_else(|| "ext4.vhdx".into());
            let file = Path::new(&file);
            if file.file_name() != Some(file.as_os_str())
                || !file
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("vhdx"))
            {
                return Err(invalid());
            }
            Some(RegisteredDisk {
                name,
                path: ordinary_path(&base.join(file))?,
            })
        }
        _ => return Err(invalid()),
    };
    if key.revision()? != before {
        return Err(invalid());
    }
    Ok(disk)
}
