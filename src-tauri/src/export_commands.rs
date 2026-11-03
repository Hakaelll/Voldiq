use crate::{Engine, analysis_status::IndexId};
use serde::Serialize;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tauri::State;
use voldiq_core::exports::{CsvStreamRequest, StreamOutcome, StreamProgress};

#[derive(Default)]
pub(crate) struct Sessions {
    run: Mutex<Option<Run>>,
}
struct Run {
    event: Event,
    cancel: Arc<AtomicBool>,
}
#[derive(Clone, Serialize)]
pub(crate) struct Event {
    request_id: String,
    index_id: IndexId,
    phase: Phase,
}
#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Phase {
    Choosing,
    Running { id: u64, progress: StreamProgress },
}
#[derive(Serialize)]
pub(crate) struct Receipt {
    request_id: String,
    index_id: IndexId,
    outcome: Outcome,
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Outcome {
    DialogCancelled,
    Cancelled {
        progress: StreamProgress,
    },
    Complete {
        progress: StreamProgress,
        path: std::path::PathBuf,
    },
}

// The command retains one latest progress value, not an unbounded event channel.
struct Session {
    engine: Arc<Engine>,
    request_id: String,
    index_id: IndexId,
}
impl Session {
    fn begin(engine: Arc<Engine>, request_id: String, index_id: IndexId) -> Result<Self, String> {
        if request_id.len() != 36
            || !request_id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
        {
            return Err("Invalid export request identity.".into());
        }
        let mut slot = engine
            .exports
            .run
            .lock()
            .map_err(|error| error.to_string())?;
        if slot.is_some() {
            return Err("Another CSV export is active.".into());
        }
        *slot = Some(Run {
            event: Event {
                request_id: request_id.clone(),
                index_id,
                phase: Phase::Choosing,
            },
            cancel: Arc::new(AtomicBool::new(false)),
        });
        drop(slot);
        Ok(Self {
            engine,
            request_id,
            index_id,
        })
    }
    fn cancelled(&self) -> Result<bool, String> {
        let slot = self
            .engine
            .exports
            .run
            .lock()
            .map_err(|error| error.to_string())?;
        Ok(slot
            .as_ref()
            .is_none_or(|run| run.cancel.load(Ordering::Relaxed)))
    }
    fn running(
        &self,
        id: u64,
        cancel: Arc<AtomicBool>,
        progress: StreamProgress,
    ) -> Result<(), String> {
        let mut slot = self
            .engine
            .exports
            .run
            .lock()
            .map_err(|error| error.to_string())?;
        let run = slot.as_mut().ok_or("CSV export session ended.")?;
        if run.cancel.load(Ordering::Relaxed) {
            cancel.store(true, Ordering::Relaxed);
        }
        run.cancel = cancel;
        run.event.phase = Phase::Running { id, progress };
        Ok(())
    }
    fn progress(&self, progress: StreamProgress) {
        if let Ok(mut slot) = self.engine.exports.run.lock()
            && let Some(run) = slot.as_mut()
            && run.event.request_id == self.request_id
            && let Phase::Running {
                progress: latest, ..
            } = &mut run.event.phase
        {
            *latest = progress;
        }
    }
    fn receipt(&self, outcome: Outcome) -> Receipt {
        Receipt {
            request_id: self.request_id.clone(),
            index_id: self.index_id,
            outcome,
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.engine.exports.run.lock()
            && slot.as_ref().is_some_and(|run| {
                run.event.request_id == self.request_id && run.event.index_id == self.index_id
            })
        {
            *slot = None;
        }
    }
}

#[tauri::command]
pub(crate) async fn save_csv_report(
    index_id: IndexId,
    request_id: String,
    request: CsvStreamRequest,
    engine: State<'_, Arc<Engine>>,
) -> Result<Receipt, String> {
    #[cfg(windows)]
    {
        let engine = engine.inner().clone();
        let session = Session::begin(engine.clone(), request_id, index_id)?;
        engine.index_for(index_id)?;
        let path = tauri::async_runtime::spawn_blocking(|| {
            rfd::FileDialog::new()
                .set_title("Save CSV report · existing files are preserved")
                .add_filter("CSV report", &["csv"])
                .set_file_name("voldiq-report.csv")
                .save_file()
        })
        .await
        .map_err(|error| error.to_string())?;
        if session.cancelled()? {
            return Ok(session.receipt(Outcome::DialogCancelled));
        }
        let Some(path) = path else {
            return Ok(session.receipt(Outcome::DialogCancelled));
        };
        let operation = engine.begin()?;
        let index = engine.index_for(index_id)?;
        let total_entries = index
            .read()
            .map_err(|error| error.to_string())?
            .entries
            .len()
            .saturating_sub(1);
        session.running(
            operation.id,
            operation.cancel.clone(),
            StreamProgress {
                inspected: 0,
                rows: 0,
                total_entries,
            },
        )?;
        // The blocking worker owns both the session and operation until every file handle is closed.
        tauri::async_runtime::spawn_blocking(move || {
            let index = index.read().map_err(|error| error.to_string())?;
            let result = voldiq_core::exports::save_csv(
                &index,
                &request,
                &path,
                &operation._lease,
                &operation.cancel,
                |value| session.progress(value),
            )
            .map_err(|error| error.to_string())?;
            let outcome = match result {
                StreamOutcome::Complete(progress) => Outcome::Complete { progress, path },
                StreamOutcome::Cancelled(progress) => Outcome::Cancelled { progress },
            };
            Ok(session.receipt(outcome))
        })
        .await
        .map_err(|error| error.to_string())?
    }
    #[cfg(not(windows))]
    {
        let _ = (index_id, request_id, request, engine);
        Err("CSV file export is available on Windows only.".into())
    }
}

#[tauri::command]
pub(crate) fn csv_report_progress(
    index_id: IndexId,
    request_id: String,
    engine: State<'_, Arc<Engine>>,
) -> Result<Option<Event>, String> {
    let slot = engine
        .exports
        .run
        .lock()
        .map_err(|error| error.to_string())?;
    Ok(slot
        .as_ref()
        .filter(|run| run.event.index_id == index_id && run.event.request_id == request_id)
        .map(|run| run.event.clone()))
}

#[tauri::command]
pub(crate) fn cancel_csv_report(
    index_id: IndexId,
    request_id: String,
    engine: State<'_, Arc<Engine>>,
) -> Result<bool, String> {
    let slot = engine
        .exports
        .run
        .lock()
        .map_err(|error| error.to_string())?;
    if let Some(run) = slot
        .as_ref()
        .filter(|run| run.event.index_id == index_id && run.event.request_id == request_id)
    {
        run.cancel.store(true, Ordering::Relaxed);
        Ok(true)
    } else {
        Ok(false)
    }
}
