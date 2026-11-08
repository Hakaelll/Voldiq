use std::{
    ffi::{OsStr, OsString},
    io,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Component, Path, PathBuf, Prefix},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS},
    Storage::FileSystem::GetDriveTypeW,
    System::{
        Registry::{REG_SZ, RegQueryValueExW},
        WindowsProgramming::DRIVE_FIXED,
    },
};
use winreg::{
    RegKey,
    enums::{KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_OPEN_LINK},
};

const INSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Voldiq";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "Voldiq";

#[derive(serde::Serialize, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Status {
    Ready { registered: bool },
    NotInstalled,
    UnsupportedPath,
    Conflict,
}

enum Entry {
    Missing,
    Text(OsString),
    Foreign,
}

pub(crate) fn preference(
    user: &RegKey,
    executable: &Path,
    change: Option<bool>,
) -> io::Result<Status> {
    let command = match installed_command(user, executable)? {
        Ok(command) => command,
        Err(status) => return Ok(status),
    };
    let access = if change.is_some() {
        KEY_QUERY_VALUE | KEY_SET_VALUE
    } else {
        KEY_QUERY_VALUE
    };
    let key = match open_key(user, RUN_KEY, access)? {
        Some(key) => key,
        None if change != Some(true) => return Ok(Status::Ready { registered: false }),
        None => {
            user.create_subkey_with_flags(RUN_KEY, access)?;
            open_key(user, RUN_KEY, access)?
                .ok_or_else(|| io::Error::other("Windows startup settings changed. Try again."))?
        }
    };
    let state = read_status(&key, &command)?;
    match (change, &state) {
        (Some(true), Status::Ready { registered: false }) => key.set_value(RUN_VALUE, &command)?,
        (Some(false), Status::Ready { registered: true }) => match key.delete_value(RUN_VALUE) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            result => result?,
        },
        _ => return Ok(state),
    }
    read_status(&key, &command)
}

fn installed_command(user: &RegKey, executable: &Path) -> io::Result<Result<OsString, Status>> {
    let Some(key) = open_key(user, INSTALL_KEY, KEY_QUERY_VALUE)? else {
        return Ok(Err(Status::NotInstalled));
    };
    let Entry::Text(location) = entry(&key, "InstallLocation")? else {
        return Ok(Err(Status::NotInstalled));
    };
    Ok(startup_command(executable, &location))
}

fn startup_command(executable: &Path, location: &OsStr) -> Result<OsString, Status> {
    let location: Vec<_> = location.encode_wide().collect();
    let Some(directory) = location
        .strip_prefix(&[u16::from(b'"')])
        .and_then(|value| value.strip_suffix(&[u16::from(b'"')]))
    else {
        return Err(Status::NotInstalled);
    };
    let installed = PathBuf::from(OsString::from_wide(directory)).join("voldiq.exe");
    // Only the executable reported by Windows can become a startup target.
    if executable.as_os_str() != installed.as_os_str() {
        return Err(Status::NotInstalled);
    }
    let Some(Component::Prefix(prefix)) = executable.components().next() else {
        return Err(Status::UnsupportedPath);
    };
    let Prefix::Disk(drive) = prefix.kind() else {
        return Err(Status::UnsupportedPath);
    };
    let units: Vec<_> = executable.as_os_str().encode_wide().collect();
    if !executable.is_absolute()
        || executable
            .components()
            .any(|part| part == Component::ParentDir)
        || units.len() + 2 > 260
        || units
            .iter()
            .any(|&unit| unit < 32 || unit == u16::from(b'"'))
    {
        return Err(Status::UnsupportedPath);
    }
    let root = [u16::from(drive), u16::from(b':'), u16::from(b'\\'), 0];
    if unsafe { GetDriveTypeW(root.as_ptr()) } != DRIVE_FIXED {
        return Err(Status::UnsupportedPath);
    }
    let mut command = OsString::from("\"");
    command.push(executable);
    command.push("\"");
    Ok(command)
}

fn read_status(key: &RegKey, command: &OsStr) -> io::Result<Status> {
    if !matches!(entry(key, "SymbolicLinkValue")?, Entry::Missing) {
        return Ok(Status::Conflict);
    }
    Ok(match entry(key, RUN_VALUE)? {
        Entry::Missing => Status::Ready { registered: false },
        Entry::Text(value) if value == command => Status::Ready { registered: true },
        _ => Status::Conflict,
    })
}

fn open_key(user: &RegKey, path: &str, access: u32) -> io::Result<Option<RegKey>> {
    match user.open_subkey_with_options_flags(path, REG_OPTION_OPEN_LINK, access) {
        Ok(key) => Ok(Some(key)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn entry(key: &RegKey, name: &str) -> io::Result<Entry> {
    let name: Vec<_> = name.encode_utf16().chain([0]).collect();
    let mut buffer = [0u16; 1024];
    let mut bytes = std::mem::size_of_val(&buffer) as u32;
    let mut kind = 0;
    // The bounded buffer preserves the original registry type and terminator.
    match unsafe {
        RegQueryValueExW(
            key.raw_handle(),
            name.as_ptr(),
            ptr::null(),
            &mut kind,
            buffer.as_mut_ptr().cast(),
            &mut bytes,
        )
    } {
        ERROR_FILE_NOT_FOUND => Ok(Entry::Missing),
        ERROR_MORE_DATA => Ok(Entry::Foreign),
        ERROR_SUCCESS if kind == REG_SZ && bytes.is_multiple_of(2) => Ok(buffer
            .get(..bytes as usize / 2)
            .and_then(|text| text.strip_suffix(&[0]))
            .filter(|text| !text.contains(&0))
            .map_or(Entry::Foreign, |text| {
                Entry::Text(OsString::from_wide(text))
            })),
        ERROR_SUCCESS => Ok(Entry::Foreign),
        code => Err(io::Error::from_raw_os_error(code as i32)),
    }
}
