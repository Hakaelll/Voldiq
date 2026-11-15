use crate::{Engine, IndexId};
use serde::Serialize;
use std::sync::{Arc, atomic::Ordering};
use tauri::{State, ipc::Channel};
use voldiq_core::{
    NodeId,
    inspection::{VideoPhase, VideoRequest, VideoResult, compress_video},
};

#[derive(Serialize)]
pub(crate) struct Progress {
    id: u64,
    phase: Phase,
}

#[derive(Serialize)]
#[serde(untagged)]
enum Phase {
    Choosing(&'static str),
    Running(VideoPhase),
}

#[derive(Serialize)]
pub(crate) struct Receipt {
    result: VideoResult,
    path: Option<String>,
}

#[tauri::command]
pub(crate) async fn compress_local_video(
    index_id: IndexId,
    id: NodeId,
    progress: Channel<Progress>,
    engine: State<'_, Arc<Engine>>,
) -> Result<Option<Receipt>, String> {
    let engine = engine.inner().clone();
    let index = engine.index_for(index_id)?;
    let operation = engine.begin()?;
    tauri::async_runtime::spawn_blocking(move || {
        let report = |phase| {
            let _ = progress.send(Progress {
                id: operation.id,
                phase,
            });
        };
        report(Phase::Choosing("choosing"));
        let Some(encoder) = rfd::FileDialog::new()
            .set_title("Choose ffmpeg.exe from a trusted installation")
            .add_filter("FFmpeg executable", &["exe"])
            .pick_file()
        else {
            return Ok(None);
        };
        if operation.cancel.load(Ordering::Acquire) {
            return Ok(None);
        }
        let Some(output) = rfd::FileDialog::new()
            .set_title("Save a separate MP4 copy")
            .add_filter("MP4 video", &["mp4"])
            .set_file_name("voldiq-compressed.mp4")
            .save_file()
        else {
            return Ok(None);
        };
        let path = output
            .to_str()
            .ok_or("Choose a filename that Voldiq can display exactly.")?
            .to_owned();
        if !engine.index_is_current(index_id, &index)? {
            return Err("Analysis changed. Select the video again.".into());
        }
        let index = index.read().map_err(|error| error.to_string())?;
        let result = compress_video(
            VideoRequest {
                index: &index,
                id,
                encoder_path: &encoder,
                destination: &output,
            },
            &operation._lease,
            &operation.cancel,
            |phase| report(Phase::Running(phase)),
        )
        .map_err(|error| error.to_string())?;
        Ok(Some(Receipt {
            path: matches!(result, VideoResult::Published { .. }).then_some(path),
            result,
        }))
    })
    .await
    .map_err(|error| error.to_string())?
}
