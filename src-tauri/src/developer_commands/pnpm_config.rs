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

#[cfg(test)]
pub(super) fn verify_fixture() {
    use std::collections::BTreeMap;
    use voldiq_core::developer::{Query, query_with_caches};
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("home");
    let local = fixture.path().join("local");
    let store = fixture.path().join("configured-store");
    let metadata = fixture.path().join("configured-metadata");
    let override_store = fixture.path().join("override-store");
    let override_metadata = fixture.path().join("override-metadata");
    for (path, bytes) in [
        (store.join("v11/files/content"), 11),
        (store.join("v11/index/entry"), 21),
        (store.join("v11/links/dependency"), 31),
        (metadata.join("entry"), 41),
        (local.join("pnpm/store/v11/files/content"), 13),
        (local.join("pnpm-cache/entry"), 23),
        (override_store.join("v11/files/content"), 17),
        (override_metadata.join("entry"), 19),
    ] {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![0; bytes]).unwrap();
    }
    let configuration = local.join("pnpm/config/config.yaml");
    std::fs::create_dir_all(configuration.parent().unwrap()).unwrap();
    let settings = format!(
        "storeDir: {}\ncacheDir: {}\n",
        serde_json::to_string(store.to_str().unwrap()).unwrap(),
        serde_json::to_string(metadata.to_str().unwrap()).unwrap()
    );
    std::fs::write(&configuration, &settings).unwrap();
    let index = voldiq_core::scanner::scan(
        fixture.path(),
        &Default::default(),
        &std::sync::atomic::AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let mut environment = BTreeMap::<String, OsString>::new();
    let resolve = |environment: &BTreeMap<String, OsString>| {
        cache_locations(Some(&home), Some(&local), &|key| {
            environment.get(key).cloned()
        })
    };
    let totals = |locations: Vec<CacheLocation>| {
        let page = query_with_caches(&index, &Query::default(), &locations).unwrap();
        let bytes = |category| {
            page.categories
                .iter()
                .find(|total| total.category == category)
                .unwrap()
                .logical
        };
        (
            bytes(Category::Cache),
            bytes(Category::Dependencies),
            page.total,
        )
    };
    assert_eq!(totals(resolve(&environment)), (73, 31, 4));
    assert_eq!(std::fs::read_to_string(&configuration).unwrap(), settings);
    environment.insert(
        "PNPM_CONFIG_STORE_DIR".into(),
        override_store.clone().into(),
    );
    environment.insert(
        "PNPM_CONFIG_CACHE_DIR".into(),
        override_metadata.clone().into(),
    );
    assert_eq!(totals(resolve(&environment)), (36, 0, 2));
    let _ = environment.remove("PNPM_CONFIG_STORE_DIR");
    let _ = environment.remove("PNPM_CONFIG_CACHE_DIR");
    let overlap = format!(
        "storeDir: {}\ncacheDir: {}\n",
        serde_json::to_string(store.to_str().unwrap()).unwrap(),
        serde_json::to_string(store.to_str().unwrap()).unwrap()
    );
    std::fs::write(&configuration, overlap).unwrap();
    assert_eq!(totals(resolve(&environment)), (32, 31, 3));
    std::fs::write(&configuration, "storeDir: [broken").unwrap();
    assert!(resolve(&environment).is_empty());
    environment.insert("PNPM_CONFIG_STORE_DIR".into(), override_store.into());
    assert_eq!(totals(resolve(&environment)), (17, 0, 1));
    let _ = environment.remove("PNPM_CONFIG_STORE_DIR");
    std::fs::write(&configuration, " ".repeat(65_537)).unwrap();
    assert!(resolve(&environment).is_empty());
    let xdg = fixture.path().join("xdg");
    std::fs::create_dir_all(xdg.join("pnpm")).unwrap();
    std::fs::write(xdg.join("pnpm/config.yaml"), &settings).unwrap();
    environment.insert("XDG_CONFIG_HOME".into(), xdg.into());
    assert_eq!(totals(resolve(&environment)), (73, 31, 4));
    std::fs::write(&configuration, &settings).unwrap();
    environment.insert("XDG_CONFIG_HOME".into(), "relative".into());
    assert!(resolve(&environment).is_empty());
    let _ = environment.remove("XDG_CONFIG_HOME");
    std::fs::remove_file(&configuration).unwrap();
    assert_eq!(totals(resolve(&environment)), (36, 0, 2));
    for text in [
        "storeDir: one\nstoreDir: two",
        "base: &base /cache\nstoreDir: *base",
        "<<: {storeDir: /cache}",
        "storeDir: []",
    ] {
        assert!(parse_settings(text).is_err());
    }
    assert!(parse_settings(&format!("nested: {}0{}", "[".repeat(33), "]".repeat(33))).is_err());
    environment.insert("EMPTY".into(), "".into());
    let lookup = |key: &str| environment.get(key).cloned();
    assert_eq!(
        expand_path(
            &format!("${{UNSET-{}}}", store.display()),
            Some(&home),
            &lookup
        ),
        Some(store.clone())
    );
    assert_eq!(
        expand_path(
            &format!("${{EMPTY:-{}}}", store.display()),
            Some(&home),
            &lookup
        ),
        Some(store)
    );
    assert!(expand_path("${MISSING}", Some(&home), &lookup).is_none());
    assert_eq!(
        expand_path("~/packages", Some(&home), &lookup),
        Some(home.join("packages"))
    );
    let project = fixture.path().join("cleanup-project");
    let target = project.join("target");
    std::fs::create_dir_all(target.join("v11/files")).unwrap();
    std::fs::create_dir_all(target.join("v11/links")).unwrap();
    std::fs::write(
        project.join("Cargo.toml"),
        "[package]\nname='fixture'\nversion='0.0.0'\n",
    )
    .unwrap();
    std::fs::write(target.join("v11/files/content"), b"cache").unwrap();
    std::fs::write(target.join("v11/links/dependency"), b"dependency").unwrap();
    let temporary = target.join("old.tmp");
    std::fs::write(&temporary, b"owned").unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&temporary)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(
            std::time::SystemTime::now() - std::time::Duration::from_secs(14 * 86400),
        ))
        .unwrap();
    let path = serde_json::to_string(target.to_str().unwrap()).unwrap();
    std::fs::write(
        &configuration,
        format!("storeDir: {path}\ncacheDir: {path}\n"),
    )
    .unwrap();
    let protected_index = voldiq_core::scanner::scan(
        &project,
        &Default::default(),
        &std::sync::atomic::AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let now = voldiq_core::cleanup::rules::now().unwrap();
    let rule_state = tempfile::tempdir().unwrap();
    let protections = voldiq_core::metadata::Store::open(rule_state.path())
        .unwrap()
        .protections()
        .unwrap();
    assert_eq!(
        voldiq_core::cleanup::rules::query_project(
            Some(&protected_index),
            now,
            0,
            50,
            &[],
            &protections
        )
        .unwrap()
        .total,
        1
    );
    assert_eq!(
        voldiq_core::cleanup::rules::query_project(
            Some(&protected_index),
            now,
            0,
            50,
            &resolve(&environment),
            &protections,
        )
        .unwrap()
        .total,
        0
    );
}
