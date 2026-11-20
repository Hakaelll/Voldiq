use crate::Engine;
use std::sync::{Arc, Mutex};
use tauri::State;
use voldiq_core::cloud_accounts::{Account, Action, View};

#[derive(Default)]
pub(crate) struct Cloud(Arc<Mutex<Account>>);

#[tauri::command]
pub(crate) async fn one_drive(
    action: Action,
    engine: State<'_, Arc<Engine>>,
    cloud: State<'_, Cloud>,
) -> Result<View, String> {
    engine.require_durable_state()?;
    let directory = engine.operations.directory().to_path_buf();
    let account = cloud.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        account
            .lock()
            .map_err(|_| String::from("OneDrive is unavailable in this window."))?
            .run(&directory, action)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|_| String::from("The OneDrive operation ended unexpectedly."))?
}
