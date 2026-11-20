use crate::Engine;
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use tauri::State;
use voldiq_core::cloud_accounts::{Account, Action, BrowserAccount, BrowserProvider, View};

#[derive(Default)]
pub(crate) struct Cloud(Arc<Accounts>);

struct Accounts {
    one_drive: Mutex<Account>,
    google_drive: Mutex<BrowserAccount>,
    dropbox: Mutex<BrowserAccount>,
}

impl Default for Accounts {
    fn default() -> Self {
        Self {
            one_drive: Mutex::default(),
            google_drive: Mutex::new(BrowserAccount::new(BrowserProvider::GoogleDrive)),
            dropbox: Mutex::new(BrowserAccount::new(BrowserProvider::Dropbox)),
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Provider {
    OneDrive,
    GoogleDrive,
    Dropbox,
}

#[tauri::command]
pub(crate) async fn cloud_account(
    provider: Provider,
    action: Action,
    engine: State<'_, Arc<Engine>>,
    cloud: State<'_, Cloud>,
) -> Result<View, String> {
    engine.require_durable_state()?;
    let directory = engine.operations.directory().to_path_buf();
    let accounts = cloud.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let unavailable = || String::from("The cloud account is unavailable in this window.");
        let result = match provider {
            Provider::OneDrive => accounts
                .one_drive
                .lock()
                .map_err(|_| unavailable())?
                .run(&directory, action),
            Provider::GoogleDrive | Provider::Dropbox => {
                let account = if matches!(provider, Provider::GoogleDrive) {
                    &accounts.google_drive
                } else {
                    &accounts.dropbox
                };
                account
                    .lock()
                    .map_err(|_| unavailable())?
                    .run(&directory, action)
            }
        };
        result.map_err(|error| error.to_string())
    })
    .await
    .map_err(|_| String::from("The cloud account operation ended unexpectedly."))?
}
