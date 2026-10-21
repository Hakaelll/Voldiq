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
