use crate::{Engine, analysis_status::IndexId};
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tauri::State;
use voldiq_core::exports::{ReportFormat, ReportSelection, StreamOutcome, StreamProgress};

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Owner {
    Index { id: IndexId },
    History { fingerprint: String },
}
#[derive(Clone, Copy, Serialize)]
#[serde(untagged)]
enum Progress {
    Records(StreamProgress),
    Frames(voldiq_core::exports::GifProgress),
    Video(voldiq_core::exports::WebmProgress),
}

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
    owner: Owner,
    phase: Phase,
}
#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Phase {
    Choosing,
    Running { id: u64, progress: Progress },
}
#[derive(Serialize)]
pub(crate) struct Receipt {
    request_id: String,
    owner: Owner,
    outcome: Outcome,
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Outcome {
    DialogCancelled,
    Cancelled {
        progress: Progress,
    },
    Complete {
        progress: Progress,
        path: std::path::PathBuf,
    },
}

// The command retains one latest progress value, not an unbounded event channel.
struct Session {
    engine: Arc<Engine>,
    request_id: String,
    owner: Owner,
}
impl Session {
    fn begin(engine: Arc<Engine>, request_id: String, owner: Owner) -> Result<Self, String> {
        if let Owner::History { fingerprint } = &owner
            && (fingerprint.len() != 64
                || !fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err("Invalid history export identity.".into());
        }
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
            return Err("Another report export is active.".into());
        }
        *slot = Some(Run {
            event: Event {
                request_id: request_id.clone(),
                owner: owner.clone(),
                phase: Phase::Choosing,
            },
            cancel: Arc::new(AtomicBool::new(false)),
        });
        drop(slot);
        Ok(Self {
            engine,
            request_id,
            owner,
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
            .filter(|run| run.event.request_id == self.request_id && run.event.owner == self.owner)
            .is_none_or(|run| run.cancel.load(Ordering::Relaxed)))
    }
    fn running(&self, id: u64, cancel: Arc<AtomicBool>, progress: Progress) -> Result<(), String> {
        let mut slot = self
            .engine
            .exports
            .run
            .lock()
            .map_err(|error| error.to_string())?;
        let run = slot
            .as_mut()
            .filter(|run| run.event.request_id == self.request_id && run.event.owner == self.owner)
            .ok_or("report export session ended.")?;
        if run.cancel.load(Ordering::Relaxed) {
            cancel.store(true, Ordering::Relaxed);
        }
        run.cancel = cancel;
        run.event.phase = Phase::Running { id, progress };
        Ok(())
    }
    fn progress(&self, progress: Progress) {
        if let Ok(mut slot) = self.engine.exports.run.lock()
            && let Some(run) = slot.as_mut()
            && run.event.request_id == self.request_id
            && run.event.owner == self.owner
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
            owner: self.owner.clone(),
            outcome,
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.engine.exports.run.lock()
            && slot.as_ref().is_some_and(|run| {
                run.event.request_id == self.request_id && run.event.owner == self.owner
            })
        {
            *slot = None;
        }
    }
}

#[tauri::command]
pub(crate) async fn save_report(
    index_id: IndexId,
    request_id: String,
    request: ReportSelection,
    format: ReportFormat,
    engine: State<'_, Arc<Engine>>,
) -> Result<Receipt, String> {
    #[cfg(windows)]
    {
        save_file(
            index_id,
            request_id,
            FileRequest::Records {
                selection: request,
                format,
            },
            engine,
        )
        .await
    }
    #[cfg(not(windows))]
    {
        let _ = (index_id, request_id, request, format, engine);
        Err("Report file export is available on Windows only.".into())
    }
}

#[tauri::command]
pub(crate) async fn save_svg(
    index_id: IndexId,
    request_id: String,
    request: voldiq_core::exports::SvgRequest,
    engine: State<'_, Arc<Engine>>,
) -> Result<Receipt, String> {
    #[cfg(windows)]
    {
        save_file(index_id, request_id, FileRequest::Svg(request), engine).await
    }
    #[cfg(not(windows))]
    {
        let _ = (index_id, request_id, request, engine);
        Err("Report file export is available on Windows only.".into())
    }
}

#[tauri::command]
pub(crate) async fn save_png(
    request: tauri::ipc::Request<'_>,
    engine: State<'_, Arc<Engine>>,
) -> Result<Receipt, String> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields, rename_all = "camelCase")]
    struct Metadata {
        index_id: IndexId,
        request_id: String,
        context: voldiq_core::exports::GraphicContext,
        scale: u8,
    }
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("PNG export requires a binary packet.".into());
    };
    let header: [u8; 4] = bytes
        .get(..4)
        .ok_or("Missing PNG packet header.")?
        .try_into()
        .map_err(|_| "Invalid PNG packet header.")?;
    let length = u32::from_le_bytes(header) as usize;
    if length > 64 * 1024 || bytes.len() < 4 + length || bytes.len() - 4 - length > 16 * 1024 * 1024
    {
        return Err("PNG packet exceeds its metadata or image limit.".into());
    }
    let metadata: Metadata = serde_json::from_slice(&bytes[4..4 + length])
        .map_err(|_| "Invalid PNG packet metadata.")?;
    let image = bytes[4 + length..].to_vec();
    #[cfg(windows)]
    {
        save_file(
            metadata.index_id,
            metadata.request_id,
            FileRequest::Png(voldiq_core::exports::PngRequest {
                context: metadata.context,
                scale: metadata.scale,
                image,
            }),
            engine,
        )
        .await
    }
    #[cfg(not(windows))]
    {
        let _ = (
            metadata.index_id,
            metadata.request_id,
            metadata.context,
            metadata.scale,
            image,
            engine,
        );
        Err("Report file export is available on Windows only.".into())
    }
}

#[cfg(windows)]
enum FileRequest {
    Records {
        selection: ReportSelection,
        format: ReportFormat,
    },
    Svg(voldiq_core::exports::SvgRequest),
    Png(voldiq_core::exports::PngRequest),
}
#[cfg(windows)]
impl FileRequest {
    fn extension(&self) -> &'static str {
        match self {
            Self::Records { format, .. } => format.extension(),
            Self::Svg(_) => "svg",
            Self::Png(_) => "png",
        }
    }
}

#[cfg(windows)]
async fn save_file(
    index_id: IndexId,
    request_id: String,
    request: FileRequest,
    engine: State<'_, Arc<Engine>>,
) -> Result<Receipt, String> {
    #[cfg(windows)]
    {
        let engine = engine.inner().clone();
        let session = Session::begin(engine.clone(), request_id, Owner::Index { id: index_id })?;
        engine.index_for(index_id)?;
        let extension = request.extension();
        let path = choose_file(extension).await?;
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
            Progress::Records(StreamProgress {
                inspected: 0,
                rows: 0,
                total_entries,
            }),
        )?;
        // The blocking worker owns both the session and operation until every file handle is closed.
        tauri::async_runtime::spawn_blocking(move || {
            let index = index.read().map_err(|error| error.to_string())?;
            let result = match &request {
                FileRequest::Records { selection, format } => voldiq_core::exports::save_report(
                    &index,
                    *format,
                    selection,
                    &path,
                    &operation._lease,
                    &operation.cancel,
                    |value| session.progress(Progress::Records(value)),
                ),
                FileRequest::Png(selection) => voldiq_core::exports::save_png(
                    &index,
                    selection,
                    &path,
                    &operation._lease,
                    &operation.cancel,
                    |value| session.progress(Progress::Records(value)),
                ),
                FileRequest::Svg(selection) => voldiq_core::exports::save_svg(
                    &index,
                    selection,
                    &path,
                    &operation._lease,
                    &operation.cancel,
                    |value| session.progress(Progress::Records(value)),
                ),
            }
            .map_err(|error| error.to_string())?;
            let outcome = match result {
                StreamOutcome::Complete(progress) => Outcome::Complete {
                    progress: Progress::Records(progress),
                    path,
                },
                StreamOutcome::Cancelled(progress) => Outcome::Cancelled {
                    progress: Progress::Records(progress),
                },
            };
            Ok(session.receipt(outcome))
        })
        .await
        .map_err(|error| error.to_string())?
    }
    #[cfg(not(windows))]
    {
        let _ = (index_id, request_id, request, engine);
        Err("Report file export is available on Windows only.".into())
    }
}

#[cfg(windows)]
async fn choose_file(extension: &'static str) -> Result<Option<std::path::PathBuf>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        rfd::FileDialog::new()
            .set_title("Save export · existing files are preserved")
            .add_filter("Export", &[extension])
            .set_file_name(if matches!(extension, "gif" | "webm") {
                format!("voldiq-history.{extension}")
            } else {
                format!("voldiq-report.{extension}")
            })
            .save_file()
    })
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn save_history_gif(
    request: tauri::ipc::Request<'_>,
    engine: State<'_, Arc<Engine>>,
    database: State<'_, crate::snapshot_commands::SnapshotDatabase>,
) -> Result<Receipt, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields, rename_all = "camelCase")]
    struct Metadata {
        request_id: String,
        request: voldiq_core::exports::HistoryGifRequest,
        lengths: Vec<usize>,
    }
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("GIF export requires a binary packet.".into());
    };
    let header: [u8; 4] = bytes
        .get(..4)
        .ok_or("Missing GIF packet header.")?
        .try_into()
        .map_err(|_| "Invalid GIF packet header.")?;
    let length = u32::from_le_bytes(header) as usize;
    if length > 64 * 1024 || bytes.len() < 4 + length || bytes.len() - 4 - length > 32 * 1024 * 1024
    {
        return Err("GIF packet exceeds its metadata or image limit.".into());
    }
    let metadata: Metadata = serde_json::from_slice(&bytes[4..4 + length])
        .map_err(|_| "Invalid GIF packet metadata.")?;
    if !(2..=60).contains(&metadata.lengths.len())
        || metadata.lengths.len() != metadata.request.clip.snapshot_ids.len()
    {
        return Err("GIF frame count does not match the requested clip.".into());
    }
    let mut offset = 4 + length;
    let mut images = Vec::with_capacity(metadata.lengths.len());
    for count in metadata.lengths {
        if count == 0 || count > 16 * 1024 * 1024 || count > bytes.len().saturating_sub(offset) {
            return Err("Invalid GIF frame length.".into());
        }
        images.push(bytes[offset..offset + count].to_vec());
        offset += count;
    }
    if offset != bytes.len() {
        return Err("Unexpected trailing GIF packet bytes.".into());
    }
    #[cfg(windows)]
    {
        let engine = engine.inner().clone();
        let session = Session::begin(
            engine.clone(),
            metadata.request_id,
            Owner::History {
                fingerprint: metadata.request.fingerprint.clone(),
            },
        )?;
        let path = choose_file("gif").await?;
        if session.cancelled()? {
            return Ok(session.receipt(Outcome::DialogCancelled));
        }
        let Some(path) = path else {
            return Ok(session.receipt(Outcome::DialogCancelled));
        };
        let operation = engine.begin()?;
        session.running(
            operation.id,
            operation.cancel.clone(),
            Progress::Frames(voldiq_core::exports::GifProgress {
                stage: voldiq_core::exports::GifStage::Preparing,
                completed_frames: 0,
                total_frames: images.len(),
            }),
        )?;
        let database = database.0.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let store = database.lock().map_err(|error| error.to_string())?;
            let mut images = images.into_iter();
            let result = voldiq_core::exports::save_history_gif(
                &store,
                &metadata.request,
                &path,
                &operation._lease,
                &operation.cancel,
                |value| session.progress(Progress::Frames(value)),
                || Ok(images.next()),
            )
            .map_err(|error| error.to_string())?;
            let outcome = match result {
                StreamOutcome::Complete(progress) => Outcome::Complete {
                    progress: Progress::Frames(progress),
                    path,
                },
                StreamOutcome::Cancelled(progress) => Outcome::Cancelled {
                    progress: Progress::Frames(progress),
                },
            };
            Ok(session.receipt(outcome))
        })
        .await
        .map_err(|error| error.to_string())?
    }
    #[cfg(not(windows))]
    {
        let _ = (
            metadata.request_id,
            metadata.request,
            images,
            engine,
            database,
        );
        Err("History file export is available on Windows only.".into())
    }
}

#[tauri::command]
pub(crate) async fn save_history_webm(
    request: tauri::ipc::Request<'_>,
    engine: State<'_, Arc<Engine>>,
    database: State<'_, crate::snapshot_commands::SnapshotDatabase>,
) -> Result<Receipt, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields, rename_all = "camelCase")]
    struct Metadata {
        request_id: String,
        request: voldiq_core::exports::HistoryWebmRequest,
    }
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("WebM export requires a binary packet.".into());
    };
    let header: [u8; 4] = bytes
        .get(..4)
        .ok_or("Missing WebM packet header.")?
        .try_into()
        .map_err(|_| "Invalid WebM packet header.")?;
    let length = u32::from_le_bytes(header) as usize;
    if length > 64 * 1024
        || bytes.len() <= 4 + length
        || bytes.len() - 4 - length > 32 * 1024 * 1024
    {
        return Err("WebM packet exceeds its metadata or video limit.".into());
    }
    let metadata: Metadata = serde_json::from_slice(&bytes[4..4 + length])
        .map_err(|_| "Invalid WebM packet metadata.")?;
    if !(2..=60).contains(&metadata.request.clip.snapshot_ids.len()) {
        return Err("Choose 2-60 recorded captures.".into());
    }
    let image = bytes[4 + length..].to_vec();
    #[cfg(windows)]
    {
        let engine = engine.inner().clone();
        let session = Session::begin(
            engine.clone(),
            metadata.request_id,
            Owner::History {
                fingerprint: metadata.request.fingerprint.clone(),
            },
        )?;
        let path = choose_file("webm").await?;
        if session.cancelled()? {
            return Ok(session.receipt(Outcome::DialogCancelled));
        }
        let Some(path) = path else {
            return Ok(session.receipt(Outcome::DialogCancelled));
        };
        let operation = engine.begin()?;
        session.running(
            operation.id,
            operation.cancel.clone(),
            Progress::Video(voldiq_core::exports::WebmProgress::Preparing {
                completed_frames: 0,
                total_frames: metadata.request.clip.snapshot_ids.len(),
            }),
        )?;
        let database = database.0.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let store = database.lock().map_err(|error| error.to_string())?;
            let result = voldiq_core::exports::save_history_webm(
                &store,
                &metadata.request,
                &image,
                &path,
                &operation._lease,
                &operation.cancel,
                |value| session.progress(Progress::Video(value)),
            )
            .map_err(|error| error.to_string())?;
            let outcome = match result {
                StreamOutcome::Complete(progress) => Outcome::Complete {
                    progress: Progress::Video(progress),
                    path,
                },
                StreamOutcome::Cancelled(progress) => Outcome::Cancelled {
                    progress: Progress::Video(progress),
                },
            };
            Ok(session.receipt(outcome))
        })
        .await
        .map_err(|error| error.to_string())?
    }
    #[cfg(not(windows))]
    {
        let _ = (
            metadata.request_id,
            metadata.request,
            image,
            engine,
            database,
        );
        Err("History file export is available on Windows only.".into())
    }
}

#[tauri::command]
pub(crate) fn report_progress(
    owner: Owner,
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
        .filter(|run| run.event.owner == owner && run.event.request_id == request_id)
        .map(|run| run.event.clone()))
}

#[tauri::command]
pub(crate) fn cancel_report(
    owner: Owner,
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
        .filter(|run| run.event.owner == owner && run.event.request_id == request_id)
    {
        run.cancel.store(true, Ordering::Relaxed);
        Ok(true)
    } else {
        Ok(false)
    }
}
