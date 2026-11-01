use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::Manager;
use voldiq_core::agents::{Capability, ConnectionInfo, Limits, Policy, Registry};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionRequest {
    name: String,
    roots: Vec<PathBuf>,
    exclusions: Vec<PathBuf>,
    scan: bool,
    plan: bool,
    #[serde(default)]
    content: bool,
    page_size: usize,
    scan_entries: usize,
    plan_files: usize,
    plan_bytes: u64,
}

#[derive(Serialize)]
pub struct RevealedCredential {
    id: i64,
    credential: String,
}

fn registry(app: &tauri::AppHandle) -> Result<Registry, String> {
    Registry::open(
        &app.path()
            .app_local_data_dir()
            .map_err(|error| error.to_string())?
            .join("agents.db"),
    )
    .map_err(|error| error.to_string())
}

fn confirm(message: &str) -> bool {
    rfd::MessageDialog::new()
        .set_title("Voldiq · Agent permissions")
        .set_description(message)
        .set_buttons(rfd::MessageButtons::YesNo)
        .set_level(rfd::MessageLevel::Warning)
        .show()
        == rfd::MessageDialogResult::Yes
}

#[tauri::command]
pub async fn list_agent_connections(app: tauri::AppHandle) -> Result<Vec<ConnectionInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        registry(&app)?.list().map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn create_agent_connection(
    app: tauri::AppHandle,
    request: ConnectionRequest,
) -> Result<Option<RevealedCredential>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if request.roots.is_empty() || request.roots.len() > 32 || request.exclusions.len() > 256 || request.name.trim().is_empty() || request.name.len() > 128 || request.name.chars().any(char::is_control) {
            return Err("Enter a connection name and 1–32 allowed folders.".into());
        }
        let mut capabilities = vec![Capability::Metadata];
        if request.scan { capabilities.push(Capability::Scan); }
        if request.content { capabilities.push(Capability::ReadContent); }
        if request.plan { capabilities.extend([Capability::PreparePlans, Capability::RequestApproval]); }
        let permission_names = capabilities.iter().map(|capability| match capability { Capability::Metadata => "metadata", Capability::Scan => "scanning", Capability::PreparePlans => "prepare recycling plans", Capability::RequestApproval => "request human review", Capability::ReadContent => "read folder notes and file content", Capability::ExecuteApproved => "execute human-approved plans", Capability::Export => "export metadata" }).collect::<Vec<_>>().join(", ");
        let policy = Policy::new(request.roots.clone(), request.exclusions.clone(), capabilities, Limits { page_size: request.page_size, scan_entries: request.scan_entries, plan_files: request.plan_files, plan_bytes: request.plan_bytes }).map_err(|error| error.to_string())?;
        let roots = request.roots.iter().map(|path| path.to_string_lossy()).collect::<Vec<_>>().join("\n");
        let exclusions = request.exclusions.iter().map(|path| path.to_string_lossy()).collect::<Vec<_>>().join("\n");
        let message = format!("Create connection: {}\n\nAllowed folders:\n{}\n\nExcluded folders:\n{}\n\nPermissions: {}\nPage limit: {} entries\nScan limit: {} entries\nPlan limit: {} files / {} logical bytes\n\nThe client may send metadata and explicitly permitted note/content data to its AI provider. Creating this connection does not approve any plan. File execution is not enabled.", request.name, roots, if exclusions.is_empty() { "None" } else { &exclusions }, permission_names, request.page_size, request.scan_entries, request.plan_files, request.plan_bytes);
        if !confirm(&message) { return Ok(None); }
        let result = registry(&app)?.provision(&request.name, policy).map_err(|error| error.to_string())?;
        Ok(Some(RevealedCredential { id: result.id, credential: result.credential.expose().to_owned() }))
    }).await.map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn rotate_agent_credential(
    app: tauri::AppHandle,
    id: i64,
) -> Result<Option<RevealedCredential>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut registry = registry(&app)?;
        let connection = registry.list().map_err(|error| error.to_string())?.into_iter().find(|connection| connection.id == id && !connection.revoked).ok_or("Connection unavailable.")?;
        if !confirm(&format!("Replace the credential for {}?\n\nThe previous credential stops working for every connected client. The new credential is shown once in Voldiq.", connection.name)) { return Ok(None); }
        let credential = registry.rotate(id).map_err(|error| error.to_string())?;
        Ok(Some(RevealedCredential { id, credential: credential.expose().to_owned() }))
    }).await.map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn revoke_agent_connection(app: tauri::AppHandle, id: i64) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        registry(&app)?
            .revoke(id)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}
