use super::{Catalog, MAX_REGISTRATIONS, Registration, Source, SourceCoverage, check};
use crate::{Error, Result, windows};
use std::{
    path::PathBuf,
    sync::atomic::AtomicBool,
    time::{SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::{Foundation::*, System::Registry::*};

const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
const MAX_VISITED_KEYS: usize = 4096;

struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.0) };
    }
}
impl Key {
    fn open(parent: HKEY, name: &str, access: u32) -> std::result::Result<Self, u32> {
        let name: Vec<_> = name.encode_utf16().chain(Some(0)).collect();
        let mut key = std::ptr::null_mut();
        // The terminated name and output pointer remain valid for this local, read-only open.
        let code = unsafe {
            RegOpenKeyExW(
                parent,
                name.as_ptr(),
                REG_OPTION_OPEN_LINK,
                access,
                &mut key,
            )
        };
        if code == ERROR_SUCCESS {
            Ok(Self(key))
        } else {
            Err(code)
        }
    }
}

fn now() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs())
        .map_err(|_| Error::Invalid("Registration observation date is unavailable.".into()))
}

pub(super) fn read(cancel: &AtomicBool) -> Result<Catalog> {
    let started_at = now()?;
    let mut registrations = Vec::new();
    let mut coverage = Vec::new();
    let mut visited = 0;
    for (source, hive, view) in [
        (Source::CurrentUser64, HKEY_CURRENT_USER, KEY_WOW64_64KEY),
        (Source::CurrentUser32, HKEY_CURRENT_USER, KEY_WOW64_32KEY),
        (Source::Machine64, HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY),
        (Source::Machine32, HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY),
    ] {
        check(cancel)?;
        let mut status = SourceCoverage {
            source,
            complete: true,
            problem: None,
            visited: 0,
            missing_location: 0,
            rejected: 0,
        };
        let key = match Key::open(
            hive,
            UNINSTALL,
            KEY_QUERY_VALUE | KEY_ENUMERATE_SUB_KEYS | view,
        ) {
            Ok(key) => key,
            Err(ERROR_FILE_NOT_FOUND) => {
                coverage.push(status);
                continue;
            }
            Err(code) => {
                status.complete = false;
                status.problem = Some(format!("Windows registration source unavailable ({code})."));
                coverage.push(status);
                continue;
            }
        };
        for position in 0.. {
            check(cancel)?;
            if visited == MAX_VISITED_KEYS || registrations.len() == MAX_REGISTRATIONS {
                status.complete = false;
                status.problem =
                    Some("Registration work limit reached; this source may be incomplete.".into());
                break;
            }
            let mut name = [0u16; 256];
            let mut length = name.len() as u32;
            // The fixed output buffer is bounded by its supplied length; no registry value is executed.
            let code = unsafe {
                RegEnumKeyExW(
                    key.0,
                    position,
                    name.as_mut_ptr(),
                    &mut length,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if code == ERROR_NO_MORE_ITEMS {
                break;
            }
            visited += 1;
            status.visited += 1;
            if code != ERROR_SUCCESS {
                status.complete = false;
                status.problem = Some(format!(
                    "Windows registration enumeration incomplete ({code})."
                ));
                break;
            }
            let Ok(name) = String::from_utf16(&name[..length as usize]) else {
                status.rejected += 1;
                status.complete = false;
                continue;
            };
            let child = match Key::open(key.0, &name, KEY_QUERY_VALUE | view) {
                Ok(child) => child,
                Err(_) => {
                    status.rejected += 1;
                    status.complete = false;
                    continue;
                }
            };
            match record(&child, source, name) {
                Ok(Record::Application(application)) => registrations.push(application),
                Ok(Record::MissingLocation) => status.missing_location += 1,
                Ok(Record::NoDisplayName) => {}
                Err(_) => {
                    status.rejected += 1;
                    status.complete = false;
                }
            }
        }
        if !status.complete && status.problem.is_none() {
            status.problem =
                Some("Some registration records were unavailable or unsupported.".into());
        }
        coverage.push(status);
    }
    Ok(Catalog {
        started_at,
        observed_at: now()?,
        registrations,
        coverage,
    })
}

enum Record {
    Application(Registration),
    MissingLocation,
    NoDisplayName,
}
fn record(key: &Key, source: Source, name: String) -> Result<Record> {
    let before = revision(key)?;
    let Some(display_name) = string(key, "DisplayName", 1024)? else {
        return Ok(Record::NoDisplayName);
    };
    let Some(location) = string(key, "InstallLocation", 4096)? else {
        return Ok(Record::MissingLocation);
    };
    let record = Registration {
        source,
        key: name,
        name: display_name,
        install_location: PathBuf::from(location),
        publisher: string(key, "Publisher", 1024)?,
        version: string(key, "DisplayVersion", 1024)?,
    };
    if !super::valid_registration(&record) {
        return Err(Error::Invalid("Unsupported registration metadata.".into()));
    }
    if revision(key)? != before {
        return Err(Error::Invalid(
            "Registration changed while being read.".into(),
        ));
    }
    Ok(Record::Application(record))
}

fn revision(key: &Key) -> Result<(u32, u32)> {
    let mut time = FILETIME::default();
    // The retained local key and timestamp output remain alive through this metadata-only call.
    let code = unsafe {
        RegQueryInfoKeyW(
            key.0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut time,
        )
    };
    if code != ERROR_SUCCESS {
        return Err(Error::Invalid("Registration revision unavailable.".into()));
    }
    Ok((time.dwHighDateTime, time.dwLowDateTime))
}

fn string(key: &Key, name: &str, max_bytes: usize) -> Result<Option<String>> {
    let name: Vec<_> = name.encode_utf16().chain(Some(0)).collect();
    let mut buffer = [0u16; 4096];
    let mut bytes = std::mem::size_of_val(&buffer) as u32;
    // REG_SZ only: never expand environment references, read commands or accept arbitrary value types.
    let code = unsafe {
        RegGetValueW(
            key.0,
            std::ptr::null(),
            name.as_ptr(),
            RRF_RT_REG_SZ | RRF_NOEXPAND,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if code == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if code != ERROR_SUCCESS
        || bytes as usize > std::mem::size_of_val(&buffer)
        || !bytes.is_multiple_of(2)
    {
        return Err(Error::Invalid(
            "Registration value is unavailable or unsupported.".into(),
        ));
    }
    let units = &buffer[..bytes as usize / 2];
    let units = units
        .strip_suffix(&[0])
        .ok_or_else(|| Error::Invalid("Unterminated registration value.".into()))?;
    if units.contains(&0) {
        return Err(Error::Invalid("Embedded registration terminator.".into()));
    }
    let text = String::from_utf16(units)
        .map_err(|_| Error::Invalid("Registration text is not Unicode.".into()))?;
    if text.len() > max_bytes || text.chars().any(char::is_control) {
        return Err(Error::Invalid(
            "Registration text exceeds its supported bounds.".into(),
        ));
    }
    Ok((!text.trim().is_empty()).then_some(text))
}

pub(super) fn generic_roots() -> Result<Vec<(u64, u64)>> {
    use windows_sys::Win32::UI::Shell::*;
    let mut paths = Vec::new();
    for folder in [
        &FOLDERID_Windows,
        &FOLDERID_ProgramFiles,
        &FOLDERID_ProgramFilesX86,
        &FOLDERID_ProgramData,
        &FOLDERID_Profile,
        &FOLDERID_UserProfiles,
        &FOLDERID_LocalAppData,
        &FOLDERID_RoamingAppData,
    ] {
        paths.push(windows::known_folder_location(folder)?);
    }
    paths
        .iter()
        .filter(|path| super::local_install_path(path))
        .map(|path| windows::identity(path).map(|identity| identity.key))
        .collect()
}
