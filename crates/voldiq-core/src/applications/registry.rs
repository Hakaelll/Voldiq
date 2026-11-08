use super::{Catalog, MAX_REGISTRATIONS, Registration, Source, SourceCoverage, check};
use crate::{
    Error, Result,
    windows::{self, registry::Key},
};
use std::{
    path::PathBuf,
    sync::atomic::AtomicBool,
    time::{SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::{Foundation::*, System::Registry::*};

const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
const MAX_VISITED_KEYS: usize = 4096;

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
    let local =
        windows::known_folder_location(&windows_sys::Win32::UI::Shell::FOLDERID_LocalAppData).ok();
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
            match record(&child, source, name, local.as_deref()) {
                Ok(Record::Application(application)) => {
                    if application.install_location.is_none() {
                        status.missing_location += 1;
                    }
                    registrations.push(application);
                }
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
fn record(
    key: &Key,
    source: Source,
    name: String,
    local: Option<&std::path::Path>,
) -> Result<Record> {
    let before = key.revision()?;
    let Some(display_name) = key.string("DisplayName", 1024)? else {
        return Ok(Record::NoDisplayName);
    };
    let location = key.string("InstallLocation", 4096)?;
    let mut record = Registration {
        source,
        key: name,
        name: display_name,
        install_location: location.map(PathBuf::from),
        profile: None,
        publisher: key.string("Publisher", 1024)?,
        version: key.string("DisplayVersion", 1024)?,
    };
    if let Some(local) = local
        && let Some(layout) = super::BrowserLayout::registered(&record)
    {
        record.profile = Some(super::ProfileLocation {
            path: layout.default_path(local),
            layout,
        });
    }
    if record.install_location.is_none() && record.profile.is_none() {
        return Ok(Record::MissingLocation);
    }
    if !super::valid_registration(&record) {
        return Err(Error::Invalid("Unsupported registration metadata.".into()));
    }
    if key.revision()? != before {
        return Err(Error::Invalid(
            "Registration changed while being read.".into(),
        ));
    }
    Ok(Record::Application(record))
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
