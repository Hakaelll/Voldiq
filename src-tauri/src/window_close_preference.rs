use std::{fs, path::Path};

#[derive(Clone, Copy, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CloseBehavior {
    Exit,
    Tray,
}

const PREFERENCE: &str = "window-close-to-tray";

pub(crate) fn read(directory: &Path) -> Result<CloseBehavior, String> {
    match crate::config_file::read(&directory.join(PREFERENCE)) {
        Ok(None) => Ok(CloseBehavior::Exit),
        Ok(Some(marker)) if marker.is_empty() => Ok(CloseBehavior::Tray),
        _ => Err("Could not read the window close preference.".into()),
    }
}

pub(crate) fn save(directory: &Path, behavior: CloseBehavior) -> Result<CloseBehavior, String> {
    if read(directory)? == behavior {
        return Ok(behavior);
    }
    let path = directory.join(PREFERENCE);
    let result = match behavior {
        CloseBehavior::Exit => fs::remove_file(path),
        CloseBehavior::Tray => fs::File::create_new(path).map(drop),
    };
    // Another Voldiq window can have already applied the same choice.
    if result.is_ok() || read(directory).is_ok_and(|saved| saved == behavior) {
        Ok(behavior)
    } else {
        Err("Could not save the window close preference.".into())
    }
}
