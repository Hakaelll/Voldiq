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
                let result = desktop.execute_request(&request.plan_id,&request.manifest_hash,journal,
                    voldiq_core::plans::RecyclingWorker::new(&operation._lease,&operation.cancel,&apartment),|_|{});
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
        if review.plan.hash != hash || review.plan.state != State::AwaitingApproval { return Err("Plan changed or is no longer awaiting approval. Refresh its review.".into()); }
        let manifest = &review.plan.manifest;
        let message = format!("Approve this exact recycling plan?\n\nConnection: {} (#{})\nFolder: {}\nFiles: {}\nLogical bytes: {}\nAction: send to Recycle Bin\nRisk: manual review required\n\nManifest hash:\n{}\n\nReview the full selection in Voldiq before approving. This grants one attempt within at most two minutes. Approval does not start execution or guarantee recovered space.",review.connection,manifest.connection_id,manifest.root,manifest.entries.len(),manifest.logical_bytes,hash);
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_waiting_instance_can_retry_but_a_closed_session_cannot_reopen() {
        let state = tempfile::tempdir().unwrap();
        let first = PlanReviews::open(state.path());
        let first_handle = first.desktop().unwrap();
        let waiting = PlanReviews::open(state.path());
        assert!(waiting.desktop().is_err());
        first.close();
        assert!(waiting.desktop().is_ok());
        assert!(first.desktop().is_err());
        assert!(first_handle.list(0, 25).is_err());
        waiting.close();
        assert!(waiting.desktop().is_err());
    }
}
