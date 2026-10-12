use std::{ffi::OsString, path::PathBuf};
use tauri::Manager;
use voldiq_core::developer::CacheLocation;

pub(super) fn cache_locations(app: &tauri::AppHandle) -> Vec<CacheLocation> {
    let cargo = configured_path(
        std::env::var_os("CARGO_HOME"),
        app.path().home_dir().ok().map(|path| path.join(".cargo")),
    );
    let npm = configured_path(
        std::env::var_os("NPM_CONFIG_CACHE"),
        app.path()
            .local_data_dir()
            .ok()
            .map(|path| path.join("npm-cache")),
    );
    let mut locations = Vec::new();
    for (path, tool) in [
        (cargo.as_ref().map(|path| path.join("registry")), "Cargo"),
        (cargo.as_ref().map(|path| path.join("git")), "Cargo"),
        (npm, "npm"),
    ] {
        if let Some(path) = path.and_then(|path| path.canonicalize().ok()) {
            locations.push(CacheLocation { path, tool });
        }
    }
    locations
}

fn configured_path(value: Option<OsString>, default: Option<PathBuf>) -> Option<PathBuf> {
    match value.filter(|value| !value.is_empty()) {
        Some(value) => {
            let path = PathBuf::from(value);
            path.is_absolute().then_some(path)
        }
        None => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_overrides_do_not_guess_a_base_for_relative_paths() {
        let default = std::env::temp_dir().join("default-cache");
        let custom = std::env::temp_dir().join("custom-cache");
        assert_eq!(
            configured_path(None, Some(default.clone())),
            Some(default.clone())
        );
        assert_eq!(
            configured_path(Some(OsString::new()), Some(default.clone())),
            Some(default.clone())
        );
        assert_eq!(
            configured_path(Some(custom.clone().into_os_string()), Some(default.clone())),
            Some(custom)
        );
        assert_eq!(
            configured_path(Some(OsString::from("relative/cache")), Some(default)),
            None
        );
        assert_eq!(configured_path(None, None), None);
    }
}
