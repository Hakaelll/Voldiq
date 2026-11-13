use crate::windows_startup::{Entry, Status, entry, installed_target, open_key};
use std::{
    ffi::{OsStr, OsString},
    io,
    path::{Path, PathBuf},
};
use windows_sys::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
use winreg::{
    RegKey,
    enums::{KEY_READ, KEY_WRITE, REG_CREATED_NEW_KEY},
};

const VERB: &str = r"Software\Classes\Directory\shell\Voldiq.Analyze";
const LABEL: &str = "Analizar con Voldiq";

pub(crate) fn folder_argument(
    mut args: impl Iterator<Item = OsString>,
) -> io::Result<Option<PathBuf>> {
    if args.next().as_deref() != Some(OsStr::new("--analyze-folder")) {
        return Ok(None);
    }
    let path = args
        .next()
        .filter(|path| !path.is_empty() && args.next().is_none())
        .ok_or_else(|| io::Error::other("Pass one folder to --analyze-folder."))?;
    Ok(Some(PathBuf::from(path)))
}

pub(crate) fn preference(
    user: &RegKey,
    executable: &Path,
    change: Option<bool>,
) -> io::Result<Status> {
    let mut command = match installed_target(user, executable)? {
        Ok(target) if !target.to_string_lossy().contains('%') => target,
        Ok(_) => return Ok(Status::UnsupportedPath),
        Err(status) => return Ok(status),
    };
    command.push(r#" --analyze-folder "%1""#);
    match open_key(user, VERB, KEY_READ)? {
        Some(verb) if !owned(&verb, &command)? => Ok(Status::Conflict),
        Some(verb) if change == Some(false) => {
            verb.delete_subkey("command")?;
            drop(verb);
            user.delete_subkey(VERB)?;
            Ok(Status::Ready { registered: false })
        }
        Some(_) => Ok(Status::Ready { registered: true }),
        None if change == Some(true) => register(user, &command),
        None => Ok(Status::Ready { registered: false }),
    }
}

fn owned(verb: &RegKey, expected: &OsStr) -> io::Result<bool> {
    let meta = verb.query_info()?;
    if meta.sub_keys != 1
        || meta.values != 2
        || !matches!(entry(verb, "")?, Entry::Text(label) if label == LABEL)
        || !matches!(entry(verb, "MultiSelectModel")?, Entry::Text(model) if model == "Single")
    {
        return Ok(false);
    }
    let Some(command) = open_key(verb, "command", KEY_READ)? else {
        return Ok(false);
    };
    let meta = command.query_info()?;
    Ok(meta.sub_keys == 0
        && meta.values == 1
        && matches!(entry(&command, "")?, Entry::Text(value) if value == expected))
}

fn register(user: &RegKey, command: &OsStr) -> io::Result<Status> {
    let (verb, disposition) = user.create_subkey_with_flags(VERB, KEY_READ | KEY_WRITE)?;
    if !matches!(disposition, REG_CREATED_NEW_KEY) {
        return Ok(Status::Conflict);
    }
    let result = (|| -> io::Result<()> {
        verb.set_value("", &LABEL)?;
        verb.set_value("MultiSelectModel", &"Single")?;
        let (key, _) = verb.create_subkey("command")?;
        key.set_value("", &command)
    })();
    if let Err(error) = result {
        let _ = verb.delete_subkey("command");
        drop(verb);
        let _ = user.delete_subkey(VERB);
        return Err(error);
    }
    Ok(Status::Ready { registered: true })
}

pub(crate) fn notify_explorer() {
    unsafe {
        SHChangeNotify(
            SHCNE_ASSOCCHANGED as i32,
            SHCNF_IDLIST,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
}
