use std::{
    collections::{BTreeMap, HashSet},
    ffi::OsString,
    path::{Path, PathBuf},
};

pub(super) fn cache_locations(
    home: Option<&PathBuf>,
    local: Option<&PathBuf>,
    directories: Option<&[PathBuf; 2]>,
    environment: &impl Fn(&str) -> Option<OsString>,
) -> Vec<PathBuf> {
    resolve(home, local, directories, environment).unwrap_or_default()
}

fn resolve(
    home: Option<&PathBuf>,
    local: Option<&PathBuf>,
    directories: Option<&[PathBuf; 2]>,
    environment: &impl Fn(&str) -> Option<OsString>,
) -> Result<Vec<PathBuf>, ()> {
    let default = local.map(|path| path.join("pip/Cache"));
    if let Some(value) = environment("PIP_CACHE_DIR").filter(|value| !value.is_empty()) {
        let path = PathBuf::from(&value);
        return Ok(vec![if path.is_absolute() {
            path
        } else {
            cache_path(value.to_str().ok_or(())?, home).ok_or(())?
        }]);
    }
    let override_value = environment("PIP_CONFIG_FILE").filter(|value| !value.is_empty());
    if override_value.as_ref().is_some_and(|value| value == "nul") {
        return Ok(default.into_iter().collect());
    }
    let explicit = override_value.map(PathBuf::from);
    if explicit.as_ref().is_some_and(|path| !path.is_absolute()) {
        return Err(());
    }
    let explicit_text = match &explicit {
        Some(path) => super::config_file::read(path)?,
        None => None,
    };
    let environment_path = |key: &str| -> Result<Option<PathBuf>, ()> {
        let value = environment(key).filter(|value| !value.is_empty());
        let path = value.map(PathBuf::from);
        if path.as_ref().is_some_and(|path| !path.is_absolute()) {
            return Err(());
        }
        Ok(path)
    };
    let mut files = vec![directories.map(|paths| paths[0].join("pip/pip.ini"))];
    if explicit_text.is_none() {
        files.push(home.map(|path| path.join("pip/pip.ini")));
        files.push(directories.map(|paths| paths[1].join("pip/pip.ini")));
    }
    files.push(environment_path("VIRTUAL_ENV")?.map(|path| path.join("pip.ini")));
    let mut settings = BTreeMap::new();
    for file in files.into_iter().flatten() {
        if let Some(text) = super::config_file::read(&file)? {
            settings.extend(parse(&text)?);
        }
    }
    if let Some(text) = explicit_text {
        settings.extend(parse(&text)?);
    }
    let mut paths = Vec::new();
    if !settings.contains_key("global") {
        paths.extend(default);
    }
    for value in settings.values() {
        paths.push(cache_path(value, home).ok_or(())?);
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn cache_path(value: &str, home: Option<&PathBuf>) -> Option<PathBuf> {
    let path = Path::new(value);
    if value.starts_with("~/") || value.starts_with("~\\") {
        return home.map(|home| home.join(&value[2..]));
    }
    path.is_absolute().then(|| path.to_path_buf())
}

fn supported_section(section: &str) -> bool {
    matches!(
        section,
        "global"
            | "install"
            | "lock"
            | "download"
            | "uninstall"
            | "freeze"
            | "inspect"
            | "list"
            | "show"
            | "check"
            | "config"
            | "search"
            | "cache"
            | "index"
            | "wheel"
            | "hash"
            | "completion"
            | "debug"
            | "help"
    )
}

fn parse(text: &str) -> Result<BTreeMap<String, String>, ()> {
    let mut sections = HashSet::new();
    let mut keys = HashSet::new();
    let mut section: Option<&str> = None;
    let mut previous: Option<(usize, bool)> = None;
    let mut caches = BTreeMap::new();
    let mut default = None;
    for raw in text.trim_start_matches('\u{feff}').lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        let indent = raw.len() - raw.trim_start().len();
        if let Some((previous_indent, cache)) = previous
            && indent > previous_indent
        {
            if cache {
                return Err(());
            }
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            if name.is_empty() || !sections.insert(name) {
                return Err(());
            }
            section = Some(name);
            previous = None;
            continue;
        }
        let section = section.ok_or(())?;
        let (key, value) = line.split_once(['=', ':']).ok_or(())?;
        let key = key.trim().to_lowercase();
        if key.is_empty() || !keys.insert((section, key.clone())) {
            return Err(());
        }
        let cache = key.trim_start_matches("--").replace('_', "-") == "cache-dir";
        previous = Some((indent, cache));
        if cache {
            let value = value.trim();
            if value.is_empty() {
                return Err(());
            }
            if section == "DEFAULT" {
                default = Some(value.to_owned());
            } else if supported_section(section) {
                caches.insert(section.to_owned(), value.to_owned());
            }
        }
    }
    if let Some(default) = default {
        for section in sections
            .into_iter()
            .filter(|section| supported_section(section))
        {
            caches
                .entry(section.to_owned())
                .or_insert_with(|| default.clone());
        }
    }
    Ok(caches)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use voldiq_core::{
        NodeId, ScanOptions,
        developer::{self, Category},
        scanner::scan,
    };

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn configured_pip_caches_follow_file_order_and_use_only_measured_directories() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path();
        let home = root.join("profile");
        let local = root.join("local");
        for (folder, size) in [
            ("global cache", 10),
            ("legacy cache", 20),
            ("user cache", 30),
            ("site cache", 40),
            ("install cache", 50),
            ("explicit cache", 60),
            ("unrelated cache", 800),
            ("uppercase cache", 900),
            ("environment cache", 70),
        ] {
            write(&root.join(folder).join("data"), &"x".repeat(size));
        }
        write(
            &root.join("system/pip/pip.ini"),
            &format!(
                "[global]\ncache-dir={}\n",
                root.join("global cache").display()
            ),
        );
        write(
            &home.join("pip/pip.ini"),
            &format!(
                "[global]\ncache-dir={}\n",
                root.join("legacy cache").display()
            ),
        );
        write(
            &root.join("roaming/pip/pip.ini"),
            &format!(
                "[global]\ncache-dir={}\n[install]\ncache-dir={}\nindex-url=https://credentials.invalid/private\n[unrelated]\ncache-dir={}\n[GLOBAL]\ncache-dir={}\n",
                root.join("user cache").display(),
                root.join("install cache").display(),
                root.join("unrelated cache").display(),
                root.join("uppercase cache").display()
            ),
        );
        write(
            &root.join("venv/pip.ini"),
            &format!(
                "[global]\ncache-dir={}\n",
                root.join("site cache").display()
            ),
        );
        write(
            &root.join("override.ini"),
            &format!(
                "[global]\ncache-dir={}\n",
                root.join("explicit cache").display()
            ),
        );
        write(
            &root.join("forged/pip/pip.ini"),
            &format!(
                "[global]\ncache-dir={}\n",
                root.join("unrelated cache").display()
            ),
        );
        let index = scan(
            root,
            &ScanOptions::default(),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();
        for (explicit, env_cache, expected) in [
            (None, None, 90),
            (Some("override.ini"), None, 60),
            (Some("missing.ini"), None, 90),
            (Some("override.ini"), Some("environment cache"), 70),
        ] {
            let locations = super::super::resolve_cache_locations_with_pip(
                Some(home.clone()),
                Some(local.clone()),
                Some([root.join("system"), root.join("roaming")]),
                |key| {
                    let value = match key {
                        "PROGRAMDATA" | "APPDATA" => Some("forged"),
                        "VIRTUAL_ENV" => Some("venv"),
                        "PIP_CONFIG_FILE" => explicit,
                        "PIP_CACHE_DIR" => env_cache,
                        _ => None,
                    }?;
                    Some(root.join(value).into_os_string())
                },
            );
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
            assert_eq!(page.logical, expected);
            assert!(
                page.items
                    .iter()
                    .all(|item| item.tool == "pip" && item.category == Category::Cache)
            );
            assert!(page.items.iter().all(|item| item.path.ends_with("cache")));
        }
    }

    #[test]
    fn null_override_disables_files_and_bad_paths_never_fall_back_to_other_caches() {
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path().join("profile");
        let local = fixture.path().join("local");
        write(&home.join("pip/pip.ini"), "invalid configuration");
        let disabled = cache_locations(Some(&home), Some(&local), None, &|key| {
            (key == "PIP_CONFIG_FILE").then(|| OsString::from("nul"))
        });
        assert_eq!(disabled, vec![local.join("pip/Cache")]);
        for key in ["PIP_CONFIG_FILE", "PIP_CACHE_DIR", "VIRTUAL_ENV"] {
            assert!(
                cache_locations(Some(&home), Some(&local), None, &|name| {
                    (name == key).then(|| OsString::from("relative/path"))
                })
                .is_empty()
            );
        }
        let expanded = cache_locations(Some(&home), Some(&local), None, &|key| {
            (key == "PIP_CACHE_DIR").then(|| OsString::from("~/cache"))
        });
        assert_eq!(expanded, vec![home.join("cache")]);
    }

    #[test]
    fn ini_sections_defaults_and_literal_values_keep_pip_semantics() {
        let values = parse("[DEFAULT]\ncache-dir = C:\\shared\n[global]\nindex-url=https://private.invalid\n[install]\nCACHE_DIR: D:\\install # literal\n").unwrap();
        assert_eq!(values["global"], "C:\\shared");
        assert_eq!(values["install"], "D:\\install # literal");
        assert!(
            parse("[private-data]\ncache-dir=C:\\unrelated\n")
                .unwrap()
                .is_empty()
        );
        assert!(
            parse("[DEFAULT]\ncache-dir=C:\\unrelated\n[private-data]\nkey=value\n")
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            parse("[global]\ncache-dir=C:\\${NAME}\\cache\n").unwrap()["global"],
            "C:\\${NAME}\\cache"
        );
        for text in [
            "cache-dir=C:\\cache",
            "[global]\ncache-dir=C:\\a\ncache-dir=C:\\b",
            "[global]\n[global]",
            "[global]\ncache-dir=C:\\a\n  continuation",
            "[global]\ncache-dir=",
        ] {
            assert!(parse(text).is_err());
        }
        assert!(cache_path("relative", None).is_none());
        assert!(cache_path("\"C:\\quoted\"", None).is_none());
    }

    #[test]
    fn unsafe_config_reads_omit_classification_without_exposing_file_contents() {
        let fixture = tempfile::tempdir().unwrap();
        let config = fixture.path().join("config.ini");
        for contents in [vec![b'x'; 65_537], vec![0xff]] {
            std::fs::write(&config, contents).unwrap();
            assert!(
                cache_locations(None, Some(&fixture.path().to_path_buf()), None, &|key| {
                    (key == "PIP_CONFIG_FILE").then(|| config.clone().into_os_string())
                })
                .is_empty()
            );
        }
        std::fs::remove_file(&config).unwrap();
        std::fs::create_dir(&config).unwrap();
        assert!(
            cache_locations(None, Some(&fixture.path().to_path_buf()), None, &|key| {
                (key == "PIP_CONFIG_FILE").then(|| config.clone().into_os_string())
            })
            .is_empty()
        );
    }

    #[cfg(windows)]
    #[test]
    fn native_pip_configuration_directories_are_absolute_existing_windows_folders() {
        let directories = voldiq_core::windows::pip_configuration_directories().unwrap();
        assert!(
            directories
                .iter()
                .all(|path| path.is_absolute() && path.is_dir())
        );
        assert_ne!(directories[0], directories[1]);
    }
}
