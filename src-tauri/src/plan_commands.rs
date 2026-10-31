use std::{
    path::Path,
    sync::{Arc, Mutex},
};
use voldiq_core::plans::{Desktop, Review, ReviewPage, State, View};

pub struct PlanReviews {
    desktop: Mutex<Result<Arc<Desktop>, String>>,
}

impl PlanReviews {
    pub fn open(directory: &Path) -> Self {
        Self {
            desktop: Mutex::new(
                Desktop::open(directory)
                    .map(Arc::new)
                    .map_err(|error| error.to_string()),
            ),
        }
    }
    fn desktop(&self) -> Result<Arc<Desktop>, String> {
        self.desktop
            .lock()
            .map_err(|_| "Human review state unavailable.".to_owned())?
            .clone()
    }
    pub fn close(&self) {
        if let Ok(mut state) = self.desktop.lock() {
            let previous =
                std::mem::replace(&mut *state, Err("Human review session ended.".into()));
            if let Ok(desktop) = previous {
                desktop.close();
            }
        }
    }
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
