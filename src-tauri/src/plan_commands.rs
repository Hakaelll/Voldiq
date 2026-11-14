use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use voldiq_core::plans::{Desktop, Review, ReviewPage, State, View};

#[derive(Clone)]
pub struct PlanReviews {
    desktop: Arc<Mutex<ReviewSession>>,
    directory: PathBuf,
    closed: Arc<AtomicBool>,
}

enum ReviewSession {
    Dormant,
    Ready(Arc<Desktop>),
    Closed,
}

impl PlanReviews {
    pub fn disabled(directory: &Path) -> Self {
        Self {
            desktop: Arc::new(Mutex::new(ReviewSession::Closed)),
            directory: directory.to_owned(),
            closed: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn open(directory: &Path) -> Self {
        Self {
            desktop: Arc::new(Mutex::new(match Desktop::open(directory) {
                Ok(desktop) => ReviewSession::Ready(Arc::new(desktop)),
                Err(_) => ReviewSession::Dormant,
            })),
            directory: directory.to_owned(),
            closed: Arc::new(AtomicBool::new(false)),
        }
    }
    fn desktop(&self) -> Result<Arc<Desktop>, String> {
        if self.closed.load(Ordering::Acquire) {
            return Err("Human review session ended.".into());
        }
        let mut state = self
            .desktop
            .lock()
            .map_err(|_| "Human review state unavailable.".to_owned())?;
        match &*state {
            ReviewSession::Ready(desktop) => return Ok(desktop.clone()),
            ReviewSession::Closed => return Err("Human review session ended.".into()),
            ReviewSession::Dormant => {}
        }
        let desktop = Arc::new(Desktop::open(&self.directory).map_err(|error| error.to_string())?);
        *state = ReviewSession::Ready(desktop.clone());
        Ok(desktop)
    }
    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        if let Ok(mut state) = self.desktop.lock() {
            let previous = std::mem::replace(&mut *state, ReviewSession::Closed);
            if let ReviewSession::Ready(desktop) = previous {
                desktop.close();
            }
        }
    }
}

pub fn start_dispatcher(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri::{Emitter, Manager};
    let reviews = app.state::<PlanReviews>().inner().clone();
    let engine = app.state::<Arc<crate::Engine>>().inner().clone();
    let journal = app
        .state::<crate::cleanup_commands::CleanupJournal>()
        .0
        .clone();
    let app = app.clone();
    std::thread::Builder::new().name("voldiq-agent-dispatch".into()).spawn(move || {
        struct EndSession(PlanReviews);
        impl Drop for EndSession { fn drop(&mut self) { self.0.close(); } }
        let _session = EndSession(reviews.clone());
        let apartment = match voldiq_core::cleanup::recycle::ShellApartment::new() {
            Ok(apartment)=>apartment, Err(error)=>{eprintln!("Agent dispatcher unavailable: {error}");return;}
        };
        while !reviews.closed.load(Ordering::Acquire) {
            let result = (|| -> Result<(),String> {
                let desktop = match reviews.desktop() { Ok(desktop)=>desktop,Err(_)=>return Ok(()) };
                let Some(request) = desktop.pending_execution(1).map_err(|error|error.to_string())?.into_iter().next() else { return Ok(()); };
                let operation = match engine.begin() { Ok(operation)=>operation,Err(_)=>return Ok(()) };
                let mut journal = journal.lock().map_err(|error|error.to_string())?;
                if journal.is_err() {
                    *journal = voldiq_core::cleanup::journal::Journal::open(&engine.operations.directory().join("cleanup.db")).map_err(|error|error.to_string());
                }
                let journal = journal.as_mut().map_err(|error|error.clone())?;
                let invalidated = engine.index.lock().map_err(|error|error.to_string())?.take().map(|loaded|loaded.id);
                *engine.duplicates.lock().map_err(|error|error.to_string())? = None;
                let _ = app.emit("agent-operation",serde_json::json!({"id":operation.id,"index_id":invalidated,"phase":"started"}));
                struct EndOperation { app:tauri::AppHandle, id:u64 }
                impl Drop for EndOperation {
                    fn drop(&mut self) { let _=self.app.emit("agent-operation",serde_json::json!({"id":self.id,"phase":"finished"})); }
                }
                let _event = EndOperation { app:app.clone(), id:operation.id };
                let mut last_progress: Option<(std::time::Instant,voldiq_core::plans::ExecutionProgress)> = None;
                let result = desktop.execute_request(&request.plan_id,&request.manifest_hash,journal,
                    voldiq_core::plans::RecyclingWorker::new(&operation._lease,&operation.cancel,&apartment),|progress| {
                        let boundary = match &progress {
                            voldiq_core::plans::ExecutionProgress::Copy { bytes,total,.. } => bytes=="0" || bytes==total,
                            voldiq_core::plans::ExecutionProgress::Recycling { processed } => *processed==0,
                        };
                        if last_progress.as_ref().is_none_or(|(_,previous)|previous!=&progress)
                            && (boundary || last_progress.as_ref().is_none_or(|(last,_)|last.elapsed()>=std::time::Duration::from_millis(100))) {
                            let _=app.emit("agent-operation",serde_json::json!({"id":operation.id,"phase":"progress","progress":progress}));
                            last_progress=Some((std::time::Instant::now(),progress));
                        }
                    });
                result.map(|_|()).map_err(|error|error.to_string())
            })();
            // Discovery and busy leases may be unavailable temporarily; execution states prevent replay.
            if let Err(error) = result { eprintln!("Agent dispatch unavailable: {error}"); }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }).map_err(|error|error.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn cancel_agent_plan(
    reviews: tauri::State<'_, PlanReviews>,
    id: String,
    hash: String,
) -> Result<View, String> {
    let desktop = reviews.desktop()?;
    tauri::async_runtime::spawn_blocking(move || {
        desktop
            .cancel(&id, &hash)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn get_recovery_vault_quota(
    reviews: tauri::State<'_, PlanReviews>,
) -> Result<voldiq_core::plans::VaultQuota, String> {
    let desktop = reviews.desktop()?;
    tauri::async_runtime::spawn_blocking(move || {
        desktop.vault_quota().map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn set_recovery_vault_quota(
    reviews: tauri::State<'_, PlanReviews>,
    bytes: Option<String>,
) -> Result<voldiq_core::plans::VaultQuota, String> {
    let desktop = reviews.desktop()?;
    tauri::async_runtime::spawn_blocking(move || {
        desktop
            .set_vault_quota(bytes.as_deref())
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn set_recovery_vault_retention(
    reviews: tauri::State<'_, PlanReviews>,
    days: Option<u16>,
) -> Result<(), String> {
    let desktop = reviews.desktop()?;
    tauri::async_runtime::spawn_blocking(move || {
        desktop
            .set_vault_retention(days)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn list_transfer_records(
    reviews: tauri::State<'_, PlanReviews>,
    before: Option<String>,
    limit: usize,
    filter: voldiq_core::plans::TransferFilter,
) -> Result<voldiq_core::plans::TransferPage, String> {
    let desktop = reviews.desktop()?;
    tauri::async_runtime::spawn_blocking(move || {
        desktop
            .transfer_operations(before.as_deref(), limit, &filter)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn get_transfer_record(
    reviews: tauri::State<'_, PlanReviews>,
    id: String,
    after: Option<u32>,
    limit: usize,
    filter: voldiq_core::plans::TransferFilter,
) -> Result<voldiq_core::plans::TransferCatalogue, String> {
    let desktop = reviews.desktop()?;
    tauri::async_runtime::spawn_blocking(move || {
        desktop
            .transfer_catalogue(&id, after.map(voldiq_core::NodeId), limit, &filter)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn list_agent_plans(
    reviews: tauri::State<'_, PlanReviews>,
    offset: usize,
    limit: usize,
) -> Result<ReviewPage, String> {
    let desktop = reviews.desktop()?;
    tauri::async_runtime::spawn_blocking(move || {
        desktop
            .list(offset, limit)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn get_agent_plan(
    reviews: tauri::State<'_, PlanReviews>,
    id: String,
) -> Result<Review, String> {
    let desktop = reviews.desktop()?;
    tauri::async_runtime::spawn_blocking(move || {
        desktop.review(&id).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn approve_agent_plan(
    reviews: tauri::State<'_, PlanReviews>,
    id: String,
    hash: String,
) -> Result<Option<View>, String> {
    let desktop = reviews.desktop()?;
    tauri::async_runtime::spawn_blocking(move || {
        let review = desktop.review(&id).map_err(|error| error.to_string())?;
        if review.plan.hash != hash || review.plan.state != State::AwaitingApproval || !review.plan.execution_available {
            return Err("This plan is no longer available for approval. Refresh its review.".into());
        }
        let manifest = &review.plan.manifest;
        let destinations = manifest.action.approval_destinations();
        let destination = match destinations {
            [] => String::new(),
            [one] => format!("\nDestination: {}", one.destination_path().display()),
            [first, rest @ ..] => format!("\nDestinations: {} and {} more reviewed folders", first.destination_path().display(), rest.len()),
        };
        let message = format!(
            "Approve this exact plan?\n\nConnection: {} (#{})\nSource: {}{}\nFiles: {}\nLogical bytes: {}\n\n{}\n\nManifest hash:\n{}\n\nReview the full selection in Voldiq before approving. This grants one attempt within two minutes. The connection must request execution separately.",
            review.connection, manifest.connection_id, manifest.root, destination, manifest.entries.len(),
            manifest.logical_bytes, manifest.action.approval_description(), hash,
        );
        let confirmed = rfd::MessageDialog::new().set_title("Voldiq · Approve exact plan").set_description(&message).set_buttons(rfd::MessageButtons::YesNo).set_level(rfd::MessageLevel::Warning).show();
        if confirmed != rfd::MessageDialogResult::Yes { return Ok(None); }
        desktop.approve(&id,&hash).map(Some).map_err(|error| error.to_string())
    }).await.map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn reject_agent_plan(
    reviews: tauri::State<'_, PlanReviews>,
    id: String,
    hash: String,
) -> Result<View, String> {
    let desktop = reviews.desktop()?;
    tauri::async_runtime::spawn_blocking(move || {
        desktop
            .reject(&id, &hash)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}
