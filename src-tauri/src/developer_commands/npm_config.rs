use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

#[cfg(test)]
const MAX_CONFIG_BYTES: u64 = 65_536;

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
    match super::config_file::read(path)? {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaped_environment_tokens_classify_the_literal_directory_instead_of_its_namesake() {
        use std::sync::atomic::AtomicBool;
        use voldiq_core::{NodeId, ScanOptions, developer, scanner::scan};
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path().to_path_buf();
        for (name, size) in [("${NAME}", 31), ("expanded", 59)] {
            let path = home.join("base").join(name).join("cache");
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("data"), vec![1; size]).unwrap();
        }
        std::fs::write(
            home.join(".npmrc"),
            format!("cache='{}/base/\\${{NAME}}/cache'", home.display()),
        )
        .unwrap();
        let locations = super::super::resolve_cache_locations(Some(home.clone()), None, |name| {
            (name == "NAME").then(|| OsString::from("expanded"))
        });
        let index = scan(
            &home,
            &ScanOptions::default(),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();
        let page = developer::query_with_caches(&index, NodeId(0), 0, 100, &locations).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.logical, 31);
        assert_eq!(
            expand_path("C:/base/\\\\${NAME}/cache", None, &|_| Some(
                OsString::from("expanded")
            )),
            Some(PathBuf::from("C:/base/\\expanded/cache"))
        );
    }

    #[test]
    fn configured_caches_use_environment_user_and_explicit_global_precedence_with_real_totals() {
        use std::sync::atomic::AtomicBool;
        use voldiq_core::{NodeId, ScanOptions, developer, scanner::scan};
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path();
        let home = root.join("home");
        let local = root.join("local");
        std::fs::create_dir_all(&home).unwrap();
        for (directory, size) in [
            ("local/npm-cache", 11),
            ("user", 23),
            ("global", 37),
            ("environment", 53),
            ("alternate", 71),
        ] {
            let path = root.join(directory);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("data"), vec![1; size]).unwrap();
        }
        let global = root.join("global.npmrc");
        std::fs::write(
            &global,
            "cache=${ROOT}/global\n//registry.test/:_authToken=not-exported",
        )
        .unwrap();
        let alternate = root.join("alternate.npmrc");
        std::fs::write(&alternate, "cache=${ROOT}/alternate").unwrap();
        let user = home.join(".npmrc");
        std::fs::write(&user, "cache=${ROOT}/user").unwrap();
        let index = scan(
            root,
            &ScanOptions::default(),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();
        for (variables, expected) in [
            (
                vec![
                    ("ROOT", root.to_path_buf()),
                    ("NPM_CONFIG_GLOBALCONFIG", global.clone()),
                ],
                23,
            ),
            (
                vec![
                    ("ROOT", root.to_path_buf()),
                    ("NPM_CONFIG_USERCONFIG", alternate.clone()),
                ],
                71,
            ),
            (
                vec![
                    ("NPM_CONFIG_CACHE", root.join("environment")),
                    ("NPM_CONFIG_USERCONFIG", root.join("missing")),
                ],
                53,
            ),
        ] {
            let locations = super::super::resolve_cache_locations(
                Some(home.clone()),
                Some(local.clone()),
                |name| {
                    variables
                        .iter()
                        .find(|(key, _)| *key == name)
                        .map(|(_, path)| path.clone().into_os_string())
                },
            );
            let page = developer::query_with_caches(&index, NodeId(0), 0, 100, &locations).unwrap();
            assert_eq!(page.total, 1);
            assert_eq!(page.logical, expected);
            assert_eq!(page.items[0].tool, "npm");
            assert_eq!(page.items[0].category, developer::Category::Cache);
        }
        std::fs::write(&user, "registry=https://example.test").unwrap();
        let locations = super::super::resolve_cache_locations(
            Some(home.clone()),
            Some(local.clone()),
            |name| match name {
                "ROOT" => Some(root.to_path_buf().into_os_string()),
                "NPM_CONFIG_GLOBALCONFIG" => Some(global.clone().into_os_string()),
                _ => None,
            },
        );
        assert_eq!(
            developer::query_with_caches(&index, NodeId(0), 0, 100, &locations)
                .unwrap()
                .logical,
            37
        );
        assert_eq!(
            cache_location(Some(&home), Some(&local), &|_| None),
            Some(local.join("npm-cache"))
        );
        std::fs::write(&user, "cache=relative/cache").unwrap();
        assert!(cache_location(Some(&home), Some(&local), &|_| None).is_none());
        std::fs::write(&user, "cache=${MISSING}/cache").unwrap();
        assert!(cache_location(Some(&home), Some(&local), &|_| None).is_none());
        assert_eq!(
            cache_location(Some(&home), Some(&local), &|name| (name
                == "NPM_CONFIG_CACHE")
                .then(|| root.join("environment").into_os_string())),
            Some(root.join("environment"))
        );
    }

    #[test]
    fn ini_paths_preserve_comments_escapes_unicode_sections_and_last_scalar() {
        assert_eq!(parse_cache("# ignored\ncache='D:/caché #1'\ncache=C:\\cache\\folder\\#two ; comment\n[other]\ncache=ignored").unwrap(), Some("C:\\cache\\folder#two".into()));
        assert_eq!(
            parse_cache("cache=\"C:\\\\cache\\\\data\"").unwrap(),
            Some("C:\\cache\\data".into())
        );
        assert!(parse_cache("cache[]=C:/one\ncache=C:/two").is_err());
        assert!(parse_cache("cache=\"invalid\\path\"").is_err());
        assert_eq!(
            parse_cache("registry=https://example.test\n//example.test/:_authToken=SECRET")
                .unwrap(),
            None
        );
    }

    #[test]
    fn config_reads_are_bounded_and_environment_expansion_never_guesses_relative_bases() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("npmrc");
        std::fs::write(&path, "\u{feff}cache='~/cache #1'").unwrap();
        assert_eq!(read_cache(&path).unwrap(), Some("~/cache #1".into()));
        let home = fixture.path().to_path_buf();
        assert_eq!(
            expand_path("~/cache #1", Some(&home), &|_| None),
            Some(home.join("cache #1"))
        );
        assert_eq!(
            expand_path("${ROOT}/cache", None, &|name| (name == "ROOT")
                .then(|| home.clone().into_os_string())),
            Some(home.join("cache"))
        );
        assert!(expand_path("${MISSING}/cache", None, &|_| None).is_none());
        assert!(expand_path("relative/cache", Some(&home), &|_| None).is_none());
        assert!(expand_path("~", Some(&home), &|_| None).is_none());
        std::fs::write(&path, vec![b'x'; MAX_CONFIG_BYTES as usize + 1]).unwrap();
        assert!(read_cache(&path).is_err());
        std::fs::write(&path, [0xff]).unwrap();
        assert!(read_cache(&path).is_err());
        assert!(read_cache(fixture.path()).is_err());
        assert_eq!(read_cache(&fixture.path().join("missing")).unwrap(), None);
    }
}
