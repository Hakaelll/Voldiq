use crate::{Engine, profile, windows_startup};
use reqwest::{Client, Response, Url, redirect::Policy};
use serde::Serialize;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Runtime, State};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::Mutex;

const RELEASES: &str = "https://github.com/Hakaelll/Voldiq/releases";
const MANIFEST: &str = "https://github.com/Hakaelll/Voldiq/releases/latest/download/latest.json";
const TARGET: &str = "windows-x86_64";
const MANIFEST_LIMIT: usize = 64 * 1024;
const INSTALLER_LIMIT: usize = 64 * 1024 * 1024;
const REVIEW_TIME: Duration = Duration::from_secs(600);

#[derive(Default)]
pub(crate) struct Updates(Mutex<Option<Offer>>);

struct Offer {
    update: Update,
    checked: Instant,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Availability {
    Ready { version: String },
    NotInstalled,
    Unconfigured,
}

fn availability(app: &AppHandle, engine: &Engine) -> Result<Availability, String> {
    if !cfg!(feature = "production") || engine.mode != profile::Mode::Installed {
        return Ok(Availability::NotInstalled);
    }
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let user = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    if windows_startup::installed_target(&user, &executable)
        .map_err(|error| error.to_string())?
        .is_err()
    {
        return Ok(Availability::NotInstalled);
    }
    if public_key(app).is_none() {
        return Ok(Availability::Unconfigured);
    }
    Ok(Availability::Ready {
        version: app.package_info().version.to_string(),
    })
}

fn public_key(app: &AppHandle) -> Option<&str> {
    app.config()
        .plugins
        .0
        .get("updater")?
        .get("pubkey")?
        .as_str()
        .map(str::trim)
        .filter(|key| !key.is_empty())
}

fn require_update_access(app: &AppHandle, engine: &Engine) -> Result<(), String> {
    match availability(app, engine)? {
        Availability::Ready { .. } => Ok(()),
        Availability::Unconfigured => Err("This build has no trusted release key.".into()),
        Availability::NotInstalled => Err("Use the installed copy to update Voldiq.".into()),
    }
}

fn client(timeout: Duration) -> Result<Client, String> {
    Client::builder()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(10))
        .https_only(true)
        .user_agent(concat!("Voldiq/", env!("CARGO_PKG_VERSION")))
        .redirect(Policy::custom(|attempt| {
            let url = attempt.url();
            if attempt.previous().len() < 5
                && url.scheme() == "https"
                && url.username().is_empty()
                && url.password().is_none()
                && url.port().is_none()
                && match url.host_str() {
                    Some("github.com") => url.path().starts_with("/Hakaelll/Voldiq/releases/"),
                    Some(
                        "release-assets.githubusercontent.com" | "objects.githubusercontent.com",
                    ) => true,
                    _ => false,
                }
            {
                attempt.follow()
            } else {
                attempt.error("Untrusted release redirect")
            }
        }))
        .build()
        .map_err(|_| "Could not start the secure update connection.".into())
}

async fn read_bounded(mut response: Response, limit: usize) -> Result<Vec<u8>, String> {
    if !response.status().is_success() {
        return Err(format!(
            "GitHub Releases returned HTTP {}.",
            response.status().as_u16()
        ));
    }
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err("The update download exceeds the size limit.".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "The release download was interrupted.")?
    {
        if chunk.len() > limit - bytes.len() {
            return Err("The update download exceeds the size limit.".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn release_url(update: &Update) -> Result<&Url, String> {
    let version = &update.version;
    let expected = format!("{RELEASES}/download/v{version}/Voldiq_{version}_x64-setup.exe");
    if update.download_url.as_str() != expected {
        return Err("The release did not provide the expected Voldiq Windows installer.".into());
    }
    Ok(&update.download_url)
}

fn parse_offer<R: Runtime>(
    app: &AppHandle<R>,
    raw: serde_json::Value,
) -> Result<Option<Update>, String> {
    let updater = app
        .updater_builder()
        .target(TARGET)
        .restart_after_install(false)
        .endpoints(vec![
            Url::parse(MANIFEST).map_err(|error| error.to_string())?,
        ])
        .and_then(|builder| builder.build())
        .map_err(|error| error.to_string())?;
    let update = updater
        .restore_update(raw)
        .map_err(|_| "The release manifest is invalid.")?;
    if let Some(update) = &update {
        release_url(update)?;
    }
    Ok(update)
}

#[tauri::command]
pub(crate) fn update_availability(
    app: AppHandle,
    engine: State<'_, Arc<Engine>>,
) -> Result<Availability, String> {
    availability(&app, &engine)
}

#[tauri::command]
pub(crate) async fn check_windows_update(
    app: AppHandle,
    engine: State<'_, Arc<Engine>>,
    updates: State<'_, Updates>,
) -> Result<Option<String>, String> {
    require_update_access(&app, &engine)?;
    let mut offer = updates
        .0
        .try_lock()
        .map_err(|_| "An update request is already running.")?;
    *offer = None;
    let response = client(Duration::from_secs(25))?
        .get(MANIFEST)
        .send()
        .await
        .map_err(|_| "Could not reach GitHub Releases.")?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err("GitHub Releases has no update manifest for Voldiq.".into());
    }
    let raw = serde_json::from_slice(&read_bounded(response, MANIFEST_LIMIT).await?)
        .map_err(|_| "The release manifest is invalid.")?;
    let Some(update) = parse_offer(&app, raw)? else {
        return Ok(None);
    };
    let version = update.version.clone();
    *offer = Some(Offer {
        update,
        checked: Instant::now(),
    });
    Ok(Some(version))
}

#[tauri::command]
pub(crate) async fn install_windows_update(
    app: AppHandle,
    engine: State<'_, Arc<Engine>>,
    updates: State<'_, Updates>,
    version: String,
) -> Result<(), String> {
    require_update_access(&app, &engine)?;
    let mut slot = updates
        .0
        .try_lock()
        .map_err(|_| "An update request is already running.")?;
    let Some(offer) = slot
        .take()
        .filter(|offer| offer.update.version == version && offer.checked.elapsed() < REVIEW_TIME)
    else {
        return Err("The update offer expired or changed. Check for updates again.".into());
    };
    let update = offer.update;
    let response = client(Duration::from_secs(180))?
        .get(release_url(&update)?.clone())
        .send()
        .await
        .map_err(|_| "Could not download the update from GitHub Releases.")?;
    let bytes = read_bounded(response, INSTALLER_LIMIT).await?;
    let engine = engine.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        update.verify(&bytes).map_err(|_| "The installer signature or signed version is invalid.")?;
        let _operation = engine.begin()
            .map_err(|_| "Finish the active Voldiq operation, then check for updates again.")?;
        let description = format!(
            "Install verified Voldiq {version}?\n\nVoldiq will close and the Windows installer will open."
        );
        let confirmed = rfd::MessageDialog::new()
            .set_title("Voldiq update")
            .set_description(&description)
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();
        if confirmed != rfd::MessageDialogResult::Yes {
            return Ok(());
        }
        if offer.checked.elapsed() >= REVIEW_TIME {
            return Err("The update offer expired. Check for updates again.".into());
        }
        require_update_access(&app, &engine)?;
        update.install(bytes).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}
