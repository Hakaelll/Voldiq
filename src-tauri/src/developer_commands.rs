use std::{ffi::OsString, path::PathBuf};
use tauri::Manager;
use voldiq_core::developer::{CacheLocation, Category};

pub(super) fn cache_locations(app: &tauri::AppHandle) -> Vec<CacheLocation> {
    resolve_cache_locations(
        app.path().home_dir().ok(),
        app.path().local_data_dir().ok(),
        |name| std::env::var_os(name),
    )
}

fn resolve_cache_locations(
    home: Option<PathBuf>,
    local: Option<PathBuf>,
    environment: impl Fn(&str) -> Option<OsString>,
) -> Vec<CacheLocation> {
    let cargo = configured_path(
        environment("CARGO_HOME"),
        home.as_ref().map(|path| path.join(".cargo")),
    );
    let npm = configured_path(
        environment("NPM_CONFIG_CACHE"),
        local.as_ref().map(|path| path.join("npm-cache")),
    );
    let pip = configured_path(
        environment("PIP_CACHE_DIR"),
        local.as_ref().map(|path| path.join("pip/Cache")),
    );
    let gradle = configured_path(
        environment("GRADLE_USER_HOME"),
        home.as_ref().map(|path| path.join(".gradle")),
    );
    let packages = configured_path(
        environment("NUGET_PACKAGES"),
        home.as_ref().map(|path| path.join(".nuget/packages")),
    );
    let nuget_http = configured_path(
        environment("NUGET_HTTP_CACHE_PATH"),
        local.as_ref().map(|path| path.join("NuGet/v3-cache")),
    );
    let nuget_plugins = configured_path(
        environment("NUGET_PLUGINS_CACHE_PATH"),
        local.as_ref().map(|path| path.join("NuGet/plugins-cache")),
    );
    let mut locations = Vec::new();
    for (path, tool, category) in [
        (
            cargo.as_ref().map(|path| path.join("registry")),
            "Cargo",
            Category::Cache,
        ),
        (
            cargo.as_ref().map(|path| path.join("git")),
            "Cargo",
            Category::Cache,
        ),
        (npm, "npm", Category::Cache),
        (pip, "pip", Category::Cache),
        (
            gradle.map(|path| path.join("caches")),
            "Gradle",
            Category::Cache,
        ),
        (packages, "NuGet", Category::Dependencies),
        (nuget_http, "NuGet", Category::Cache),
        (nuget_plugins, "NuGet", Category::Cache),
    ] {
        if let Some(path) = path.and_then(|path| path.canonicalize().ok()) {
            locations.push(CacheLocation {
                path,
                tool,
                category,
            });
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
    use std::sync::atomic::AtomicBool;
    use voldiq_core::{NodeId, ScanOptions, developer, scanner::scan};

    #[test]
    fn tool_locations_classify_measured_dependencies_and_caches_with_overrides() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path();
        let home = root.join("profile");
        let local = root.join("local");
        for (path, size) in [
            ("profile/.cargo/registry/pkg", 6),
            ("profile/.cargo/git/repo", 5),
            ("local/npm-cache/data", 7),
            ("local/pip/Cache/wheel", 40),
            ("profile/.gradle/caches/build", 200),
            ("profile/.gradle/jdks/runtime", 1000),
            ("profile/.gradle/gradle.properties", 900),
            ("profile/.nuget/packages/pkg/content", 300),
            ("local/NuGet/v3-cache/http", 20),
            ("local/NuGet/plugins-cache/result", 10),
            ("custom pip/wheel", 60),
            ("custom gradle/caches/build", 500),
            ("custom gradle/init.d/script", 999),
            ("custom packages/pkg/content", 700),
        ] {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, vec![1; size]).unwrap();
        }
        let mut index = scan(
            root,
            &ScanOptions::default(),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();
        let defaults = resolve_cache_locations(Some(home.clone()), Some(local.clone()), |_| None);
        let page = developer::query_with_caches(&index, NodeId(0), 0, 100, &defaults).unwrap();
        assert_eq!(page.total, 8);
        assert_eq!(page.logical, 588);
        let packages = page.items.iter().find(|item| item.logical == 300).unwrap();
        assert_eq!(packages.tool, "NuGet");
        assert_eq!(packages.category, Category::Dependencies);
        assert!(
            page.items
                .iter()
                .filter(|item| item.id != packages.id)
                .all(|item| item.category == Category::Cache)
        );
        let overridden = resolve_cache_locations(Some(home), Some(local), |name| {
            let path = match name {
                "PIP_CACHE_DIR" => "custom pip",
                "GRADLE_USER_HOME" => "custom gradle",
                "NUGET_PACKAGES" => "custom packages",
                _ => return None,
            };
            Some(root.join(path).into_os_string())
        });
        let page = developer::query_with_caches(&index, NodeId(0), 0, 100, &overridden).unwrap();
        assert_eq!(page.total, 8);
        assert_eq!(page.logical, 1308);
        assert!(page.items.iter().any(|item| item.tool == "NuGet"
            && item.logical == 700
            && item.category == Category::Dependencies));
        assert!(
            page.items
                .iter()
                .any(|item| item.tool == "Gradle" && item.logical == 500)
        );
        assert!(
            page.items
                .iter()
                .any(|item| item.tool == "pip" && item.logical == 60)
        );
        index.snapshot_id = Some(1);
        assert_eq!(
            developer::query_with_caches(&index, NodeId(0), 0, 100, &overridden)
                .unwrap()
                .total,
            0
        );
    }

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
