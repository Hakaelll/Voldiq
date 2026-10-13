use std::{ffi::OsString, path::PathBuf};
use tauri::Manager;
use voldiq_core::developer::{CacheLocation, Category};
mod config_file;
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

#[cfg(test)]
fn resolve_cache_locations(
    home: Option<PathBuf>,
    local: Option<PathBuf>,
    environment: impl Fn(&str) -> Option<OsString>,
) -> Vec<CacheLocation> {
    resolve_cache_locations_with_pip(home, local, None, environment)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use voldiq_core::{NodeId, ScanOptions, developer, scanner::scan};

    #[test]
    fn developer_query_defaults_and_ipc_filters_are_strict() {
        let query: developer::Query = serde_json::from_str("{}").unwrap();
        assert_eq!(query.scope, NodeId(0));
        assert_eq!(query.offset, 0);
        assert_eq!(query.limit, 50);
        assert_eq!(query.category, None);
        assert_eq!(query.sort, developer::Sort::Size);
        let filtered: developer::Query =
            serde_json::from_str(r#"{"category":"build","sort":"name","offset":2,"limit":1}"#)
                .unwrap();
        assert_eq!(filtered.category, Some(Category::Build));
        assert_eq!(filtered.sort, developer::Sort::Name);
        assert_eq!(filtered.offset, 2);
        for invalid in [
            r#"{"category":"other"}"#,
            r#"{"sort":"tool"}"#,
            r#"{"offset":-1}"#,
            r#"{"scope":-1}"#,
            r#"{"limit":1.5}"#,
            r#"{"unknown":true}"#,
        ] {
            assert!(serde_json::from_str::<developer::Query>(invalid).is_err());
        }
    }

    #[test]
    fn pnpm_environment_precedence_selects_only_measured_store_storage() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path();
        let local = root.join("local");
        for (path, size) in [
            ("local/pnpm/store/v10/files/pkg", 11),
            ("local/pnpm/config/settings", 90),
            ("home/store/v11/files/pkg", 22),
            ("home/pnpm.exe", 100),
            ("xdg/pnpm/store/v11/files/pkg", 33),
            ("configured/v11/files/pkg", 44),
            ("configured/v11/index/metadata", 2),
            ("configured/v11/links/pkg/content", 17),
            ("configured/user-data", 400),
            ("configured/v11/index.db", 500),
            ("legacy base/v10/files/unrelated", 66),
            ("legacy base/v10/v11/files/data", 55),
        ] {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, vec![1; size]).unwrap();
        }
        let index = scan(
            root,
            &ScanOptions::default(),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();
        for (variables, expected) in [
            (Vec::new(), 11),
            (vec![("XDG_DATA_HOME", "xdg")], 33),
            (vec![("XDG_DATA_HOME", "xdg"), ("PNPM_HOME", "home")], 22),
            (
                vec![
                    ("PNPM_HOME", "home"),
                    ("PNPM_CONFIG_STORE_DIR", "configured"),
                ],
                63,
            ),
        ] {
            let locations = resolve_cache_locations(None, Some(local.clone()), |name| {
                variables
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| root.join(value).into_os_string())
            });
            let page = developer::query_with_caches(
                &index,
                &developer::Query {
                    scope: NodeId(0),
                    offset: 0,
                    limit: 100,
                    ..Default::default()
                },
                &locations,
            )
            .unwrap();
            assert_eq!(page.total, if expected == 63 { 3 } else { 1 });
            assert_eq!(page.logical, expected);
            assert_eq!(page.items[0].tool, "pnpm");
            assert_eq!(page.items[0].category, Category::Cache);
            if expected == 63 {
                assert!(
                    page.items
                        .iter()
                        .any(|item| item.logical == 17 && item.category == Category::Dependencies)
                );
            }
        }
        let versioned = resolve_cache_locations(None, None, |name| {
            (name == "PNPM_CONFIG_STORE_DIR").then(|| root.join("configured/v11").into_os_string())
        });
        let versioned_page = developer::query_with_caches(
            &index,
            &developer::Query {
                scope: NodeId(0),
                offset: 0,
                limit: 100,
                ..Default::default()
            },
            &versioned,
        )
        .unwrap();
        assert_eq!(versioned_page.logical, 63);
        assert_eq!(versioned_page.total, 3);
        let legacy_named = resolve_cache_locations(None, None, |name| {
            (name == "PNPM_CONFIG_STORE_DIR").then(|| root.join("legacy base/v10").into_os_string())
        });
        let legacy_page = developer::query_with_caches(
            &index,
            &developer::Query {
                scope: NodeId(0),
                offset: 0,
                limit: 100,
                ..Default::default()
            },
            &legacy_named,
        )
        .unwrap();
        assert_eq!(legacy_page.total, 1);
        assert_eq!(legacy_page.logical, 55);
        let relative = resolve_cache_locations(None, Some(local.clone()), |name| {
            (name == "PNPM_CONFIG_STORE_DIR").then(|| OsString::from("relative/store"))
        });
        assert!(relative.is_empty());
        let missing = resolve_cache_locations(None, Some(local), |name| {
            (name == "PNPM_CONFIG_STORE_DIR").then(|| root.join("absent").into_os_string())
        });
        assert!(missing.is_empty());
    }

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
        let page = developer::query_with_caches(
            &index,
            &developer::Query {
                scope: NodeId(0),
                offset: 0,
                limit: 100,
                ..Default::default()
            },
            &defaults,
        )
        .unwrap();
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
        let page = developer::query_with_caches(
            &index,
            &developer::Query {
                scope: NodeId(0),
                offset: 0,
                limit: 100,
                ..Default::default()
            },
            &overridden,
        )
        .unwrap();
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
            developer::query_with_caches(
                &index,
                &developer::Query {
                    scope: NodeId(0),
                    offset: 0,
                    limit: 100,
                    ..Default::default()
                },
                &overridden
            )
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
