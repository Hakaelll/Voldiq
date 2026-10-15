use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{ffi::OsString, path::PathBuf};

#[derive(Default, Deserialize)]
struct Config {
    #[serde(default, rename = "currentContext")]
    current: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Metadata {
    name: String,
    endpoints: Endpoints,
}

#[derive(Deserialize)]
struct Endpoints {
    docker: DockerEndpoint,
}

#[derive(Deserialize)]
struct DockerEndpoint {
    #[serde(rename = "Host")]
    host: String,
}

pub(super) fn resolve(
    home: Option<PathBuf>,
    environment: impl Fn(&str) -> Option<OsString>,
) -> Result<String, String> {
    let value = |key| -> Result<Option<String>, String> {
        environment(key)
            .filter(|value| !value.is_empty())
            .map(|value| {
                value
                    .into_string()
                    .map_err(|_| "Docker setting is not valid Unicode.".into())
            })
            .transpose()
    };
    let host = value("DOCKER_HOST")?;
    let context = value("DOCKER_CONTEXT")?;
    // CLI releases and documentation disagree about two simultaneous environment overrides.
    if host.is_some() && context.as_deref().is_some_and(|name| name != "default") {
        return Err("Set either DOCKER_HOST or DOCKER_CONTEXT for Docker analysis.".into());
    }
    if host.is_some() || context.as_deref() == Some("default") {
        return super::endpoint(host.as_deref());
    }
    let root = match value("DOCKER_CONFIG")? {
        Some(path) => PathBuf::from(path),
        None => home
            .ok_or("Docker profile is unavailable.")?
            .join(".docker"),
    };
    if !root.is_absolute() {
        return Err("Docker configuration requires an absolute local directory.".into());
    }
    #[cfg(windows)]
    if !matches!(root.components().next(), Some(std::path::Component::Prefix(prefix))
        if matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)))
    {
        return Err("Docker configuration requires an absolute local directory.".into());
    }
    let name = match context {
        Some(name) => name,
        None => {
            let text = crate::config_file::read(&root.join("config.json"))
                .map_err(|_| "Docker configuration could not be read safely.")?;
            match text {
                Some(text) => serde_json::from_str::<Config>(&text)
                    .map_err(|_| "Docker configuration is invalid.")?,
                None => Config::default(),
            }
            .current
        }
    };
    if name.is_empty() || name == "default" {
        return super::endpoint(None);
    }
    if name.len() < 2
        || name.len() > 200
        || !name.as_bytes()[0].is_ascii_alphanumeric()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'+' | b'-'))
    {
        return Err("Docker context name is invalid.".into());
    }
    let digest = format!("{:x}", Sha256::digest(name.as_bytes()));
    let path = root.join("contexts/meta").join(digest).join("meta.json");
    let text = crate::config_file::read(&path)
        .map_err(|_| "Docker context could not be read safely.")?
        .ok_or("Docker context was not found.")?;
    let metadata: Metadata =
        serde_json::from_str(&text).map_err(|_| "Docker context is invalid.")?;
    if metadata.name != name || metadata.endpoints.docker.host.is_empty() {
        return Err("Docker context does not match its stored endpoint.".into());
    }
    super::endpoint(Some(&metadata.endpoints.docker.host))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path};

    const CONTEXT_ID: &str = "fe9c6bd7a66301f49ca9b6a70b217107cd1284598bfc254700c989b916da791e";
    const LOCAL: &str = r#"{"Name":"desktop-linux","Endpoints":{"docker":{"Host":"npipe:////./pipe/dockerDesktopLinuxEngine"}}}"#;

    fn resolve_at(home: &Path, settings: &[(&str, &str)]) -> Result<String, String> {
        resolve(Some(home.to_owned()), |key| {
            settings
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| OsString::from(value))
        })
    }

    fn fixture(home: &Path) -> PathBuf {
        let root = home.join(".docker");
        let metadata = root.join("contexts/meta").join(CONTEXT_ID);
        fs::create_dir_all(&metadata).unwrap();
        fs::write(
            root.join("config.json"),
            br#"{"currentContext":"desktop-linux","auths":{"private":{"auth":"fixture-secret"}}}"#,
        )
        .unwrap();
        fs::write(metadata.join("meta.json"), LOCAL).unwrap();
        root
    }

    #[test]
    fn saved_and_explicit_contexts_use_the_hashed_local_endpoint() {
        let home = tempfile::tempdir().unwrap();
        let root = fixture(home.path());
        assert_eq!(
            resolve_at(home.path(), &[]).unwrap(),
            r"\\.\pipe\dockerDesktopLinuxEngine"
        );
        fs::write(root.join("config.json"), b"invalid private fixture-secret").unwrap();
        assert_eq!(
            resolve_at(home.path(), &[("DOCKER_CONTEXT", "desktop-linux")]).unwrap(),
            r"\\.\pipe\dockerDesktopLinuxEngine"
        );
        assert_eq!(
            resolve_at(home.path(), &[("DOCKER_HOST", "npipe://./pipe/override")]).unwrap(),
            r"\\.\pipe\override"
        );
        assert_eq!(
            resolve_at(home.path(), &[("DOCKER_CONTEXT", "default")]).unwrap(),
            r"\\.\pipe\docker_engine"
        );
        assert_eq!(
            resolve_at(
                home.path(),
                &[
                    ("DOCKER_CONTEXT", "default"),
                    ("DOCKER_HOST", "npipe://./pipe/override")
                ]
            )
            .unwrap(),
            r"\\.\pipe\override"
        );
        assert!(
            resolve_at(
                home.path(),
                &[
                    ("DOCKER_CONTEXT", "desktop-linux"),
                    ("DOCKER_HOST", "npipe://./pipe/override")
                ]
            )
            .is_err()
        );
        let error = resolve_at(home.path(), &[]).unwrap_err();
        assert!(!error.contains("fixture-secret"));
    }

    #[test]
    fn custom_config_roots_and_missing_default_config_are_distinct() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve_at(home.path(), &[]).unwrap(),
            r"\\.\pipe\docker_engine"
        );
        let other = tempfile::tempdir().unwrap();
        let root = fixture(other.path());
        assert_eq!(
            resolve_at(home.path(), &[("DOCKER_CONFIG", root.to_str().unwrap())]).unwrap(),
            r"\\.\pipe\dockerDesktopLinuxEngine"
        );
        assert!(resolve_at(home.path(), &[("DOCKER_CONFIG", "relative")]).is_err());
        #[cfg(windows)]
        assert!(resolve_at(home.path(), &[("DOCKER_CONFIG", r"\\server\share")]).is_err());
    }

    #[test]
    fn invalid_or_missing_selected_metadata_never_uses_default_engine() {
        let home = tempfile::tempdir().unwrap();
        let root = fixture(home.path());
        let metadata = root
            .join("contexts/meta")
            .join(CONTEXT_ID)
            .join("meta.json");
        for content in [
            r#"{"Name":"other","Endpoints":{"docker":{"Host":"npipe://./pipe/local"}}}"#,
            r#"{"Name":"desktop-linux","Endpoints":{"docker":{"Host":"ssh://remote"}}}"#,
            r#"{"Name":"desktop-linux","Endpoints":{"docker":{"Host":"tcp://localhost:2375"}}}"#,
            r#"{"Name":"desktop-linux","Endpoints":{"docker":{"Host":""}}}"#,
            r#"{"Name":"desktop-linux","Endpoints":{}}"#,
            r#"{"Name":"desktop-linux","Endpoints":{"docker":{"Host":"npipe:////server/pipe/remote"}}}"#,
            "null",
            "invalid",
        ] {
            fs::write(&metadata, content).unwrap();
            assert!(resolve_at(home.path(), &[]).is_err(), "{content}");
        }
        fs::remove_file(&metadata).unwrap();
        assert!(resolve_at(home.path(), &[]).is_err());
        for name in [
            "../escape",
            "x",
            "bad/context",
            "bad\\context",
            "bad\ncontext",
        ] {
            assert!(resolve_at(home.path(), &[("DOCKER_CONTEXT", name)]).is_err());
        }
    }

    #[test]
    fn bounded_reader_rejects_oversize_non_utf8_and_duplicate_settings() {
        let home = tempfile::tempdir().unwrap();
        let root = fixture(home.path());
        let config = root.join("config.json");
        for content in [
            vec![b' '; 65_537],
            vec![0xff],
            br#"{"currentContext":"default","currentContext":"desktop-linux"}"#.to_vec(),
        ] {
            fs::write(&config, content).unwrap();
            assert!(resolve_at(home.path(), &[]).is_err());
        }
        fs::write(&config, br#"{"currentContext":"desktop-linux"}"#).unwrap();
        fs::write(
            root.join("contexts/meta")
                .join(CONTEXT_ID)
                .join("meta.json"),
            vec![b' '; 65_537],
        )
        .unwrap();
        assert!(resolve_at(home.path(), &[]).is_err());
    }
}
