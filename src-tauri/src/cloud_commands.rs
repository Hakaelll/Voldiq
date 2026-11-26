use crate::Engine;
use std::sync::{Arc, Mutex};
use tauri::State;
use voldiq_core::cloud_accounts::{
    Account, Action, BrowserAccount, BrowserProvider, View,
    recycle::{self, Provider},
};

#[derive(Default)]
pub(crate) struct Cloud(Arc<Accounts>);

struct Accounts {
    one_drive: Mutex<Account>,
    google_drive: Mutex<BrowserAccount>,
    dropbox: Mutex<BrowserAccount>,
    recycle: Mutex<recycle::Session>,
}

impl Default for Accounts {
    fn default() -> Self {
        Self {
            one_drive: Mutex::default(),
            google_drive: Mutex::new(BrowserAccount::new(BrowserProvider::GoogleDrive)),
            dropbox: Mutex::new(BrowserAccount::new(BrowserProvider::Dropbox)),
            recycle: Mutex::default(),
        }
    }
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
        if matches!(action, Action::Disconnect | Action::Revoke) {
            accounts
                .recycle
                .lock()
                .map_err(|_| unavailable())?
                .run(
                    &directory,
                    provider,
                    recycle::Action::Cancel { review: None },
                )
                .map_err(|error| error.to_string())?;
        }
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

#[tauri::command]
pub(crate) async fn cloud_recycle(
    provider: Provider,
    action: recycle::Action,
    engine: State<'_, Arc<Engine>>,
    cloud: State<'_, Cloud>,
) -> Result<recycle::View, String> {
    engine.require_durable_state()?;
    let directory = engine.operations.directory().to_path_buf();
    let accounts = cloud.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let unavailable = || String::from("The cloud review is unavailable in this window.");
        let mut session = accounts.recycle.lock().map_err(|_| unavailable())?;
        let result = match action {
            recycle::Action::Review { id } => {
                let source = match provider {
                    Provider::OneDrive => accounts
                        .one_drive
                        .lock()
                        .map_err(|_| unavailable())?
                        .recycle_source(&directory, &id),
                    Provider::GoogleDrive | Provider::Dropbox => {
                        let account = if provider == Provider::GoogleDrive {
                            &accounts.google_drive
                        } else {
                            &accounts.dropbox
                        };
                        account
                            .lock()
                            .map_err(|_| unavailable())?
                            .recycle_source(&directory, &id)
                    }
                }
                .map_err(|error| error.to_string())?;
                session.prepare(&directory, source)
            }
            recycle::Action::Confirm { review } => {
                session.confirm(&directory, provider, &review, |description| {
                    rfd::MessageDialog::new()
                        .set_title("Voldiq · Recycle cloud file")
                        .set_description(description)
                        .set_level(rfd::MessageLevel::Warning)
                        .set_buttons(rfd::MessageButtons::YesNo)
                        .show()
                        == rfd::MessageDialogResult::Yes
                })
            }
            action => session.run(&directory, provider, action),
        };
        result.map_err(|error| error.to_string())
    })
    .await
    .map_err(|_| String::from("The cloud review ended unexpectedly."))?
}
