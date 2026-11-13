use std::path::Path;

#[derive(Clone, Copy, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CloseBehavior {
    Exit,
    Tray,
}

const PREFERENCE: &str = "window-close-to-tray";

pub(crate) fn read(directory: &Path) -> Result<CloseBehavior, String> {
    match crate::config_file::read_marker(&directory.join(PREFERENCE)) {
        Ok(false) => Ok(CloseBehavior::Exit),
        Ok(true) => Ok(CloseBehavior::Tray),
        Err(()) => Err("Could not read the window close preference.".into()),
    }
}

pub(crate) fn save(directory: &Path, behavior: CloseBehavior) -> Result<CloseBehavior, String> {
    crate::config_file::set_marker(&directory.join(PREFERENCE), behavior == CloseBehavior::Tray)
        .map(|_| behavior)
        .map_err(|()| "Could not save the window close preference.".into())
}
