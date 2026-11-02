use std::{
    ffi::{OsStr, OsString},
    path::PathBuf,
};
use voldiq_core::developer::{CacheLocation, Category};
use yaml_rust2::{
    Yaml, YamlLoader,
    parser::{Event, Parser},
};

#[derive(Default)]
struct Settings {
    store: Option<String>,
    cache: Option<String>,
}

enum StoreLocation {
    Configured(PathBuf),
    Conventional(PathBuf),
}

pub(super) fn cache_locations(
    home: Option<&PathBuf>,
    local: Option<&PathBuf>,
    environment: &impl Fn(&str) -> Option<OsString>,
) -> Vec<CacheLocation> {
    let local = directory_environment("LOCALAPPDATA", environment)
        .map(|path| path.or_else(|| local.cloned()));
    let settings = read_settings(home, &local, environment);
    let store = match nonempty("PNPM_CONFIG_STORE_DIR", environment) {
        Some(value) => absolute_path(&value, home).map(StoreLocation::Configured),
        None => settings
            .as_ref()
            .and_then(|settings| match &settings.store {
                Some(value) => expand_path(value, home, environment).map(StoreLocation::Configured),
                None => data_directory(home, &local, environment)
                    .map(|path| StoreLocation::Conventional(path.join("store"))),
            }),
    };
    let metadata = match nonempty("PNPM_CONFIG_CACHE_DIR", environment) {
        Some(value) => absolute_path(&value, home),
        None => settings
            .as_ref()
            .and_then(|settings| match &settings.cache {
                Some(value) => expand_path(value, home, environment),
                None => cache_directory(home, &local, environment),
            }),
    };
    let mut locations = Vec::new();
    if let Some(store) = store {
        let (base, versions): (_, &[&str]) = match store {
            StoreLocation::Configured(path) => (path, &["v11"]),
            StoreLocation::Conventional(path) => (path, &["v3", "v10", "v11"]),
        };
        let roots: Vec<_> = if base
            .file_name()
            .is_some_and(|name| versions.iter().any(|v| name == *v))
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
                add(&mut locations, root.join(directory), category);
            }
        }
    }
    if let Some(path) = metadata {
        add(&mut locations, path, Category::Cache);
    }
    locations
}

fn add(locations: &mut Vec<CacheLocation>, path: PathBuf, category: Category) {
    if let Ok(path) = path.canonicalize()
        && path.is_dir()
    {
        locations.push(CacheLocation {
            path,
            tool: "pnpm",
            category,
        });
    }
}

fn nonempty(name: &str, environment: &impl Fn(&str) -> Option<OsString>) -> Option<OsString> {
    environment(name).filter(|value| !value.is_empty())
}

fn directory_environment(
    name: &str,
    environment: &impl Fn(&str) -> Option<OsString>,
) -> Result<Option<PathBuf>, ()> {
    nonempty(name, environment)
        .map(|value| {
            let path = PathBuf::from(&value);
            // Unexpanded Windows directory variables have no reliable absolute location here.
            (path.is_absolute() && !value.to_string_lossy().contains('%'))
                .then_some(path)
                .ok_or(())
        })
        .transpose()
}

fn read_settings(
    home: Option<&PathBuf>,
    local: &Result<Option<PathBuf>, ()>,
    environment: &impl Fn(&str) -> Option<OsString>,
) -> Option<Settings> {
    let directory = match directory_environment("XDG_CONFIG_HOME", environment).ok()? {
        Some(path) => path.join("pnpm"),
        None => match local.as_ref().ok()? {
            Some(path) => path.join("pnpm/config"),
            None => home?.join(".config/pnpm"),
        },
    };
    match crate::config_file::read(&directory.join("config.yaml")).ok()? {
        Some(text) => parse_settings(text.trim_start_matches('\u{feff}')).ok(),
        None => Some(Settings::default()),
    }
}

fn parse_settings(text: &str) -> Result<Settings, ()> {
    // shortcut: Alias/merge configurations stay unclassified until expansion can be bounded.
    // Bound parsing before the loader materializes nodes or clones anchored values.
    let mut parser = Parser::new_from_str(text);
    let mut depth = 0usize;
    let mut ended = false;
    for _ in 0..4096 {
        match parser.next_token().map_err(|_| ())?.0 {
            Event::Alias(_) => return Err(()),
            Event::MappingStart(..) | Event::SequenceStart(..) => {
                depth += 1;
                if depth > 32 {
                    return Err(());
                }
            }
            Event::MappingEnd | Event::SequenceEnd => depth = depth.checked_sub(1).ok_or(())?,
            Event::StreamEnd => {
                ended = true;
                break;
            }
            _ => {}
        }
    }
    if !ended {
        return Err(());
    }
    let documents = YamlLoader::load_from_str(text).map_err(|_| ())?;
    if documents.is_empty() {
        return Ok(Settings::default());
    }
    if documents.len() != 1 {
        return Err(());
    }
    let document = &documents[0];
    if document == &Yaml::Null {
        return Ok(Settings::default());
    }
    let mapping = document.as_hash().ok_or(())?;
    if mapping.contains_key(&Yaml::String("<<".into())) {
        return Err(());
    }
    let setting = |key: &str| match mapping.get(&Yaml::String(key.into())) {
        None | Some(Yaml::Null) => Ok(None),
        Some(Yaml::String(value)) => Ok((!value.is_empty()).then(|| value.clone())),
        _ => Err(()),
    };
    Ok(Settings {
        store: setting("storeDir")?,
        cache: setting("cacheDir")?,
    })
}

fn data_directory(
    home: Option<&PathBuf>,
    local: &Result<Option<PathBuf>, ()>,
    environment: &impl Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    if let Some(path) = directory_environment("PNPM_HOME", environment).ok()? {
        return Some(path);
    }
    if let Some(path) = directory_environment("XDG_DATA_HOME", environment).ok()? {
        return Some(path.join("pnpm"));
    }
    match local.as_ref().ok()? {
        Some(path) => Some(path.join("pnpm")),
        None => home.map(|path| path.join(".pnpm")),
    }
}

fn cache_directory(
    home: Option<&PathBuf>,
    local: &Result<Option<PathBuf>, ()>,
    environment: &impl Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    if let Some(path) = directory_environment("XDG_CACHE_HOME", environment).ok()? {
        return Some(path.join("pnpm"));
    }
    match local.as_ref().ok()? {
        Some(path) => Some(path.join("pnpm-cache")),
        None => home.map(|path| path.join(".pnpm-cache")),
    }
}

fn absolute_path(value: &OsStr, home: Option<&PathBuf>) -> Option<PathBuf> {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        return Some(path);
    }
    let suffix = path.strip_prefix("~").ok()?;
    if suffix.as_os_str().is_empty() {
        return None;
    }
    home.map(|home| home.join(suffix))
}

fn expand_path(
    value: &str,
    home: Option<&PathBuf>,
    environment: &impl Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    let mut expanded = OsString::new();
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        let prefix = &rest[..start];
        let escapes = prefix
            .bytes()
            .rev()
            .take_while(|byte| *byte == b'\\')
            .count();
        expanded.push(&prefix[..prefix.len() - escapes]);
        expanded.push("\\".repeat(escapes / 2));
        let variable = &rest[start + 2..];
        let end = variable.find('}')?;
        let name = &variable[..end];
        if name.is_empty() || name.contains(['$', '{']) {
            return None;
        }
        if escapes % 2 == 1 {
            expanded.push(&rest[start..start + end + 3]);
        } else {
            let fallback = name.split_once('-').and_then(|(name, fallback)| {
                let (name, on_empty) = name
                    .strip_suffix(':')
                    .map_or((name, false), |name| (name, true));
                (!name.is_empty() && !name.contains(':') && !fallback.is_empty())
                    .then_some((name, fallback, on_empty))
            });
            let replacement = match fallback {
                Some((name, fallback, on_empty)) => environment(name)
                    .filter(|value| !on_empty || !value.is_empty())
                    .unwrap_or_else(|| fallback.into()),
                None => environment(name)?,
            };
            expanded.push(replacement);
        }
        if expanded.len() > 32767 {
            return None;
        }
        rest = &variable[end + 1..];
    }
    expanded.push(rest);
    if expanded.len() > 32767 {
        return None;
    }
    absolute_path(&expanded, home)
}
