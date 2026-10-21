use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

pub(super) fn cache_location(
    home: Option<&PathBuf>,
    local: Option<&PathBuf>,
    environment: &impl Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    let default = local.map(|path| path.join("npm-cache"));
    if let Some(value) = environment("NPM_CONFIG_CACHE").filter(|value| !value.is_empty()) {
        return super::configured_path(Some(value), None);
    }
    let user = super::configured_path(
        environment("NPM_CONFIG_USERCONFIG"),
        home.map(|path| path.join(".npmrc")),
    );
    if user.is_none() && environment("NPM_CONFIG_USERCONFIG").is_some_and(|v| !v.is_empty()) {
        return None;
    }
    for config in [
        user,
        super::configured_path(environment("NPM_CONFIG_GLOBALCONFIG"), None),
    ]
    .into_iter()
    .flatten()
    {
        match read_cache(&config) {
            Ok(Some(value)) => return expand_path(&value, home, environment),
            Ok(None) => {}
            Err(()) => return None,
        }
    }
    if environment("NPM_CONFIG_GLOBALCONFIG")
        .is_some_and(|v| !v.is_empty() && !Path::new(&v).is_absolute())
    {
        return None;
    }
    default
}

fn read_cache(path: &Path) -> Result<Option<String>, ()> {
    match crate::config_file::read(path)? {
        Some(text) => parse_cache(text.trim_start_matches('\u{feff}')),
        None => Ok(None),
    }
}

fn parse_cache(text: &str) -> Result<Option<String>, ()> {
    let mut cache = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            break;
        }
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        let (key, value) = line.split_once('=').unwrap_or((line, ""));
        let key = ini_value(key)?;
        if key == "cache[]" {
            return Err(());
        }
        if key == "cache" {
            cache = Some(ini_value(value)?);
        }
    }
    Ok(cache)
}

fn ini_value(value: &str) -> Result<String, ()> {
    let value = value.trim();
    if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
        return serde_json::from_str(value).map_err(|_| ());
    }
    if value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2 {
        return Ok(value[1..value.len() - 1].to_owned());
    }
    let mut result = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '#' | ';' => break,
            '\\' => match chars.next() {
                Some(next @ ('\\' | '#' | ';')) => result.push(next),
                Some(next) => {
                    result.push('\\');
                    result.push(next);
                }
                None => result.push('\\'),
            },
            _ => result.push(ch),
        }
    }
    Ok(result.trim().to_owned())
}

fn expand_path(
    value: &str,
    home: Option<&PathBuf>,
    environment: &impl Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    let mut path = OsString::new();
    let mut remainder = value;
    while let Some(start) = remainder.find("${") {
        let prefix = &remainder[..start];
        let escapes = prefix
            .bytes()
            .rev()
            .take_while(|byte| *byte == b'\\')
            .count();
        path.push(&prefix[..prefix.len() - escapes]);
        path.push("\\".repeat(escapes / 2));
        let variable = &remainder[start + 2..];
        let end = variable.find('}')?;
        let name = &variable[..end];
        let (name, optional) = name
            .strip_suffix('?')
            .map_or((name, false), |name| (name, true));
        if name.is_empty() || name.contains(['$', '{', '?']) {
            return None;
        }
        if escapes % 2 == 1 {
            path.push(&remainder[start..start + 2 + end + 1]);
        } else {
            match environment(name) {
                Some(value) => path.push(value),
                None if optional => {}
                None => return None,
            }
        }
        remainder = &variable[end + 1..];
    }
    path.push(remainder);
    let path = PathBuf::from(path);
    if path == Path::new("~") {
        return None;
    }
    if let Ok(suffix) = path.strip_prefix("~") {
        return home.map(|home| home.join(suffix));
    }
    path.is_absolute().then_some(path)
}
