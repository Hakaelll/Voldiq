use std::{ffi::OsString, path::PathBuf};
use tauri::Manager;
use voldiq_core::developer::{CacheLocation, Category};
mod npm_config;
mod pip_config;

enum PnpmLocation {
    Configured(PathBuf),
    Conventional(PathBuf),
}

pub(super) fn cache_locations(app: &tauri::AppHandle) -> Vec<CacheLocation> {
    #[cfg(windows)]
    let pip_directories = voldiq_core::windows::pip_configuration_directories().ok();
    #[cfg(not(windows))]
    let pip_directories = None;
    resolve_cache_locations_with_pip(
        app.path().home_dir().ok(),
        app.path().local_data_dir().ok(),
        pip_directories,
        |name| std::env::var_os(name),
    )
}

fn resolve_cache_locations_with_pip(
    home: Option<PathBuf>,
    local: Option<PathBuf>,
    pip_directories: Option<[PathBuf; 2]>,
    environment: impl Fn(&str) -> Option<OsString>,
) -> Vec<CacheLocation> {
    let cargo = configured_path(
        environment("CARGO_HOME"),
        home.as_ref().map(|path| path.join(".cargo")),
    );
    let npm = npm_config::cache_location(home.as_ref(), local.as_ref(), &environment);
    let pnpm = pnpm_store_location(local.as_ref(), &environment);
    let pip = pip_config::cache_locations(
        home.as_ref(),
        local.as_ref(),
        pip_directories.as_ref(),
        &environment,
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
    for path in pip {
        if let Ok(path) = path.canonicalize() {
            locations.push(CacheLocation {
                path,
                tool: "pip",
                category: Category::Cache,
            });
        }
    }
    if let Some(location) = pnpm {
        let (base, versions): (PathBuf, &[&str]) = match location {
            PnpmLocation::Configured(base) => (base, &["v11"]),
            PnpmLocation::Conventional(base) => (base, &["v3", "v10", "v11"]),
        };
        let roots: Vec<_> = if base
            .file_name()
            .is_some_and(|name| versions.iter().any(|version| name == *version))
        {
            vec![base]
        } else {
            versions.iter().map(|version| base.join(version)).collect()
        };
        for root in roots {
            for (directory, category) in [
                ("files", Category::Cache),
                ("index", Category::Cache),
                ("links", Category::Dependencies),
            ] {
                if let Ok(path) = root.join(directory).canonicalize() {
                    locations.push(CacheLocation {
                        path,
                        tool: "pnpm",
                        category,
                    });
                }
            }
        }
    }
    locations
}

fn pnpm_store_location(
    local: Option<&PathBuf>,
    environment: &impl Fn(&str) -> Option<OsString>,
) -> Option<PnpmLocation> {
    if let Some(value) = environment("PNPM_CONFIG_STORE_DIR").filter(|value| !value.is_empty()) {
        return configured_path(Some(value), None).map(PnpmLocation::Configured);
    }
    if let Some(value) = environment("PNPM_HOME").filter(|value| !value.is_empty()) {
        return configured_path(Some(value), None)
            .map(|path| PnpmLocation::Conventional(path.join("store")));
    }
    if let Some(value) = environment("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        return configured_path(Some(value), None)
            .map(|path| PnpmLocation::Conventional(path.join("pnpm/store")));
    }
    local.map(|path| PnpmLocation::Conventional(path.join("pnpm/store")))
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
