use crate::{Engine, Operation, cleanup_commands::CleanupJournal};
use serde::Serialize;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::State;
use voldiq_core::cleanup::recycle::RestorationReview;

const REVIEW_LIFETIME: Duration = Duration::from_secs(120);

struct Session {
    id: u64,
    expires: Instant,
    review: RestorationReview,
}

#[derive(Default)]
pub struct RestorationReviews(Arc<Mutex<Option<Session>>>);

#[derive(Serialize)]
pub struct RestorePreview {
    id: u64,
    path: String,
    size: u64,
}

#[derive(Serialize)]
pub struct RestoreResult {
    path: String,
    recorded: bool,
    journal_error: Option<String>,
}

#[tauri::command]
pub async fn preview_restoration(
    receipt: i64,
    engine: State<'_, Arc<Engine>>,
    reviews: State<'_, RestorationReviews>,
    journal: State<'_, CleanupJournal>,
) -> Result<RestorePreview, String> {
    let engine = engine.inner().clone();
    let (id, _) = engine.begin()?;
    let operation = Operation { engine, id };
    let reviews = reviews.0.clone();
    let journal = journal.0.clone();
    let preview = tauri::async_runtime::spawn_blocking({
        let reviews = reviews.clone();
        move || {
            let _operation = operation;
            *reviews.lock().map_err(|error| error.to_string())? = None;
            let journal = journal.lock().map_err(|error| error.to_string())?;
            let journal = journal.as_ref().map_err(|error| error.clone())?;
            let review =
                RestorationReview::prepare(journal, receipt).map_err(|error| error.to_string())?;
            let preview = RestorePreview {
                id,
                path: review
                    .destination()
                    .to_str()
                    .ok_or("Inexact restoration destination.")?
                    .to_owned(),
                size: review.size(),
            };
            *reviews.lock().map_err(|error| error.to_string())? = Some(Session {
                id,
                expires: Instant::now() + REVIEW_LIFETIME,
                review,
            });
            Ok::<_, String>(preview)
        }
    })
    .await
    .map_err(|error| error.to_string())??;
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(REVIEW_LIFETIME).await;
        if let Ok(mut slot) = reviews.lock()
            && slot.as_ref().is_some_and(|session| session.id == id)
        {
            *slot = None;
        }
    });
    Ok(preview)
}

fn consume_review(
    reviews: &Mutex<Option<Session>>,
    id: u64,
    confirmation: &str,
) -> Result<RestorationReview, String> {
    if confirmation != "RESTORE" {
        return Err("Confirm restoration before continuing.".into());
    }
    let mut slot = reviews.lock().map_err(|error| error.to_string())?;
    let session = slot
        .as_ref()
        .ok_or("Restoration review expired. Review again.")?;
    if session.id != id {
        return Err("Restoration belongs to another review.".into());
    }
    if Instant::now() >= session.expires {
        *slot = None;
        return Err("Restoration review expired. Review again.".into());
    }
    slot.take()
        .map(|session| session.review)
        .ok_or_else(|| "Restoration review expired. Review again.".into())
}

#[tauri::command]
pub fn discard_restoration(id: u64, reviews: State<'_, RestorationReviews>) -> Result<(), String> {
    let mut slot = reviews.0.lock().map_err(|error| error.to_string())?;
    if slot.as_ref().is_some_and(|session| session.id == id) {
        *slot = None;
    }
    Ok(())
}

#[tauri::command]
pub async fn execute_restoration(
    id: u64,
    confirmation: String,
    engine: State<'_, Arc<Engine>>,
    reviews: State<'_, RestorationReviews>,
    journal: State<'_, CleanupJournal>,
) -> Result<RestoreResult, String> {
    let engine = engine.inner().clone();
    let (operation_id, _) = engine.begin()?;
    let operation = Operation {
        engine: engine.clone(),
        id: operation_id,
    };
    let review = consume_review(&reviews.0, id, &confirmation)?;
    let journal = journal.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        let mut journal = journal.lock().map_err(|error| error.to_string())?;
        let journal = journal.as_mut().map_err(|error| error.clone())?;
        *engine.index.lock().map_err(|error| error.to_string())? = None;
        *engine
            .duplicates
            .lock()
            .map_err(|error| error.to_string())? = None;
        let restored = review.restore(journal).map_err(|error| error.to_string())?;
        Ok(RestoreResult {
            path: restored
                .path()
                .to_str()
                .ok_or("Inexact restored destination.")?
                .to_owned(),
            recorded: restored.recorded(),
            journal_error: restored.journal_error().map(str::to_owned),
        })
    })
    .await
    .map_err(|error| error.to_string())?
}
