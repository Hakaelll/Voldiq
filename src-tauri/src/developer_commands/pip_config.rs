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
        Some(path) => crate::config_file::read(path)?,
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
        if let Some(text) = crate::config_file::read(&file)? {
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
