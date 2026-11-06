use crate::{Engine, analysis_status::IndexId};
use serde::Serialize;
use std::sync::Arc;
#[cfg(windows)]
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use tauri::{State, ipc::Channel};
#[cfg(windows)]
use voldiq_core::scanner::SharedIndex;
use voldiq_core::{NodeId, ScanConcurrency};

#[derive(Default)]
pub(crate) struct Sessions {
    #[cfg(windows)]
    run: Mutex<Option<Run>>,
    #[cfg(windows)]
    navigation: Arc<AtomicBool>,
}

#[cfg(windows)]
struct NavigationPermit(Arc<AtomicBool>);
#[cfg(windows)]
impl Drop for NavigationPermit {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[cfg(windows)]
struct Run {
    id: u64,
    control: voldiq_core::live::Control,
    acknowledgements: std::sync::mpsc::SyncSender<IndexId>,
    pending: Option<Publication>,
}

#[cfg(windows)]
struct Publication {
    before_id: IndexId,
    before: SharedIndex,
    after_id: IndexId,
    after: SharedIndex,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Event {
    Started {
        id: u64,
    },
    Observed {
        id: u64,
        before_index: IndexId,
        index_id: IndexId,
        entries: usize,
        report: voldiq_core::live::Report,
    },
    Ended {
        id: u64,
        error: Option<String>,
    },
}

impl Sessions {
    pub(crate) fn stop(&self, id: Option<u64>) -> Result<(), String> {
        #[cfg(windows)]
        {
            let run = {
                let mut slot = self.run.lock().map_err(|error| error.to_string())?;
                if slot
                    .as_ref()
                    .is_some_and(|run| id.is_none_or(|id| run.id == id))
                {
                    slot.take()
                } else {
                    None
                }
            };
            if let Some(run) = run {
                // Dropping the only sender also wakes a worker waiting for frontend acknowledgement.
                run.control.stop().map_err(|error| error.to_string())?;
            }
        }
        #[cfg(not(windows))]
        let _ = id;
        Ok(())
    }
}

#[tauri::command]
pub(crate) fn start_live(
    index_id: IndexId,
    concurrency: ScanConcurrency,
    profile: voldiq_core::ScanProfile,
    progress: Channel<Event>,
    engine: State<'_, Arc<Engine>>,
) -> Result<u64, String> {
    #[cfg(windows)]
    {
        let operation = engine.begin()?;
        let index = engine.index_for(index_id)?;
        let control = voldiq_core::live::Control::new().map_err(|error| error.to_string())?;
        let id = operation.id;
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        *engine
            .duplicates
            .lock()
            .map_err(|error| error.to_string())? = None;
        *engine.live.run.lock().map_err(|error| error.to_string())? = Some(Run {
            id,
            control: control.clone(),
            acknowledgements: send,
            pending: None,
        });
        let engine = engine.inner().clone();
        tauri::async_runtime::spawn_blocking(move || {
            let result = run_session(
                &engine,
                id,
                index_id,
                index,
                concurrency,
                profile,
                &control,
                &receive,
                &progress,
            );
            let cleanup = engine.live.stop(Some(id));
            drop(operation);
            let _ = progress.send(Event::Ended {
                id,
                error: result.and(cleanup).err(),
            });
        });
        Ok(id)
    }
    #[cfg(not(windows))]
    {
        let _ = (index_id, concurrency, profile, progress, engine);
        Err("Live notifications are available on Windows only.".into())
    }
}

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
fn run_session(
    engine: &Arc<Engine>,
    id: u64,
    mut expected: IndexId,
    baseline: SharedIndex,
    concurrency: ScanConcurrency,
    profile: voldiq_core::ScanProfile,
    control: &voldiq_core::live::Control,
    acknowledgements: &std::sync::mpsc::Receiver<IndexId>,
    progress: &Channel<Event>,
) -> Result<(), String> {
    let mut session = {
        let index = baseline.read().map_err(|error| error.to_string())?;
        voldiq_core::live::Session::start(&index, concurrency, profile, control.clone())
            .map_err(|error| error.to_string())?
    };
    drop(baseline);
    progress
        .send(Event::Started { id })
        .map_err(|error| error.to_string())?;
    while let Some(update) = session.observe().map_err(|error| error.to_string())? {
        let entries = update.index.entries.len();
        let measured_at = update.report.after.measured_at;
        let next = Arc::new(std::sync::RwLock::new(update.index));
        let next_id = {
            let mut slot = engine.live.run.lock().map_err(|error| error.to_string())?;
            let Some(run) = slot.as_mut().filter(|run| run.id == id) else {
                return Ok(());
            };
            if control.is_stopped() {
                return Ok(());
            }
            let before = engine.index_for(expected)?;
            let next_id = engine.publish(next.clone(), measured_at, false)?;
            run.pending = Some(Publication {
                before_id: expected,
                before,
                after_id: next_id,
                after: next,
            });
            next_id
        };
        // Deliver even if stop raced publication, so the frontend can adopt the final published identity.
        progress
            .send(Event::Observed {
                id,
                before_index: expected,
                index_id: next_id,
                entries,
                report: update.report,
            })
            .map_err(|error| error.to_string())?;
        match acknowledgements.recv() {
            Ok(accepted) if accepted == next_id => expected = next_id,
            _ if control.is_stopped() => return Ok(()),
            _ => return Err("Live observation was not acknowledged. Restart Live.".into()),
        }
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn stop_live(id: u64, engine: State<'_, Arc<Engine>>) -> Result<(), String> {
    engine.live.stop(Some(id))
}

#[tauri::command]
pub(crate) fn resynchronize_live(id: u64, engine: State<'_, Arc<Engine>>) -> Result<(), String> {
    #[cfg(windows)]
    {
        let slot = engine.live.run.lock().map_err(|error| error.to_string())?;
        slot.as_ref()
            .filter(|run| run.id == id)
            .ok_or_else(|| "Live session is no longer active.".to_owned())?
            .control
            .resynchronize()
            .map_err(|error| error.to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = (id, engine);
        Err("Live notifications are unavailable.".into())
    }
}

#[tauri::command]
pub(crate) fn acknowledge_live(
    id: u64,
    index_id: IndexId,
    engine: State<'_, Arc<Engine>>,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        let mut slot = engine.live.run.lock().map_err(|error| error.to_string())?;
        let run = slot
            .as_mut()
            .filter(|run| {
                run.id == id
                    && run
                        .pending
                        .as_ref()
                        .is_some_and(|pending| pending.after_id == index_id)
            })
            .ok_or_else(|| "Live observation changed or was already acknowledged.".to_owned())?;
        run.acknowledgements
            .try_send(index_id)
            .map_err(|error| error.to_string())?;
        run.pending = None;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (id, index_id, engine);
        Err("Live notifications are unavailable.".into())
    }
}

#[tauri::command]
pub(crate) async fn live_directory(
    id: u64,
    before_index: IndexId,
    index_id: IndexId,
    scope: NodeId,
    engine: State<'_, Arc<Engine>>,
) -> Result<NodeId, String> {
    #[cfg(windows)]
    {
        if engine.live.navigation.swap(true, Ordering::AcqRel) {
            return Err("Live folder navigation is already resolving.".into());
        }
        let permit = NavigationPermit(engine.live.navigation.clone());
        let (before, after) = {
            let slot = engine.live.run.lock().map_err(|error| error.to_string())?;
            let pending = slot
                .as_ref()
                .filter(|run| run.id == id)
                .and_then(|run| run.pending.as_ref())
                .filter(|pending| pending.before_id == before_index && pending.after_id == index_id)
                .ok_or_else(|| "Live folder context is no longer available.".to_owned())?;
            (pending.before.clone(), pending.after.clone())
        };
        tauri::async_runtime::spawn_blocking(move || {
            let _permit = permit;
            let before = before.read().map_err(|error| error.to_string())?;
            let after = after.read().map_err(|error| error.to_string())?;
            Ok(voldiq_core::live::corresponding_directory(
                &before, &after, scope,
            ))
        })
        .await
        .map_err(|error| error.to_string())?
    }
    #[cfg(not(windows))]
    {
        let _ = (id, before_index, index_id, scope, engine);
        Err("Live notifications are unavailable.".into())
    }
}
