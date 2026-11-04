use super::{StreamOutcome, output::LimitedOutput, webm_container};
use crate::{
    Error, Result,
    snapshots::{ExportClip, ExportClipRequest, Store},
};
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    sync::atomic::{AtomicBool, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryWebmRequest {
    pub clip: ExportClipRequest,
    pub fingerprint: String,
    pub width: u16,
    pub height: u16,
    pub delay_centiseconds: u16,
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum WebmProgress {
    Preparing {
        completed_frames: usize,
        total_frames: usize,
    },
    Validating {
        inspected_bytes: usize,
        total_bytes: usize,
    },
    Revalidating {
        completed_frames: usize,
        total_frames: usize,
    },
    Finalizing {
        completed_frames: usize,
        total_frames: usize,
    },
}
#[derive(Serialize)]
struct Metadata<'a> {
    format: &'static str,
    generated_at: u64,
    clip: &'a ExportClip,
    width: u16,
    height: u16,
    delay_centiseconds: u16,
    codec: &'a str,
    video_blocks: usize,
    first_timestamp_ns: String,
    last_timestamp_ns: String,
    authority: &'static str,
    limitations: &'static str,
}
fn invalid(message: &str) -> Error {
    Error::Invalid(format!("History WebM export failed: {message}"))
}

/// Validates and remuxes a bounded recorder profile; encoded pixel contents remain client supplied.
pub fn stream_history_webm(
    store: &Store,
    request: &HistoryWebmRequest,
    image: &[u8],
    writer: &mut impl Write,
    cancel: &AtomicBool,
    mut progress: impl FnMut(WebmProgress),
) -> Result<StreamOutcome<WebmProgress>> {
    let total = request.clip.snapshot_ids.len();
    let mut state = WebmProgress::Preparing {
        completed_frames: 0,
        total_frames: total,
    };
    if cancel.load(Ordering::Relaxed) {
        return Ok(StreamOutcome::Cancelled(state));
    }
    if request.width == 0
        || request.height == 0
        || request.width > 1280
        || request.height > 720
        || ![50, 100, 200].contains(&request.delay_centiseconds)
        || request.fingerprint.len() != 64
        || !request
            .fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || image.len() > 32 * 1024 * 1024
    {
        return Err(invalid("invalid source, dimensions, speed or input size"));
    }
    let result = (|| -> Result<Option<()>> {
        let Some(clip) = store.export_clip(&request.clip, cancel, |p| {
            state = WebmProgress::Preparing {
                completed_frames: p.prepared_frames,
                total_frames: p.total_frames,
            };
            progress(state);
        })?
        else {
            return Ok(None);
        };
        if clip.fingerprint != request.fingerprint {
            return Err(invalid("recorded source changed; prepare the clip again"));
        }
        state = WebmProgress::Validating {
            inspected_bytes: 0,
            total_bytes: image.len(),
        };
        progress(state);
        let container = webm_container::validate(image, request.width, request.height, cancel)?;
        state = WebmProgress::Validating {
            inspected_bytes: image.len(),
            total_bytes: image.len(),
        };
        progress(state);
        let metadata = serde_json::to_vec(&Metadata {
            format: "webm",
            generated_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| invalid("clock unavailable"))?
                .as_secs(),
            clip: &clip,
            width: request.width,
            height: request.height,
            delay_centiseconds: request.delay_centiseconds,
            codec: container.codec,
            video_blocks: container.blocks,
            first_timestamp_ns: container.first_timestamp_ns.to_string(),
            last_timestamp_ns: container.last_timestamp_ns.to_string(),
            authority: "Native recorded clip context; client-encoded pixels and their correspondence to snapshots are not attested. No file-operation authority.",
            limitations: "One VP8/VP9 video track, no audio. Source timing is retained and may differ from requested illustrative cadence. No interpolation of measured values. Input text metadata, external references, cues and seek tables are discarded. Output supports sequential playback; player seeking may be limited. Codec bitstreams are bounded but not decoded or attested by core.",
        })
        .map_err(|_| invalid("metadata encoding failed"))?;
        if metadata.len() > 9 * 1024 * 1024 {
            return Err(invalid("metadata exceeds its byte limit"));
        }
        drop(clip);
        let mut output = LimitedOutput {
            writer,
            written: 0,
            limit: 64 * 1024 * 1024,
            cancelled: cancel,
        };
        container.write(&mut output, &metadata)?;
        state = WebmProgress::Revalidating {
            completed_frames: 0,
            total_frames: total,
        };
        progress(state);
        let Some(current) = store.export_clip(&request.clip, cancel, |p| {
            state = WebmProgress::Revalidating {
                completed_frames: p.prepared_frames,
                total_frames: p.total_frames,
            };
            progress(state);
        })?
        else {
            return Ok(None);
        };
        if current.fingerprint != request.fingerprint {
            return Err(invalid("recorded source changed during export"));
        }
        state = WebmProgress::Finalizing {
            completed_frames: total,
            total_frames: total,
        };
        progress(state);
        Ok(Some(()))
    })();
    if cancel.load(Ordering::Relaxed) {
        return Ok(StreamOutcome::Cancelled(state));
    }
    Ok(match result? {
        Some(()) => StreamOutcome::Complete(state),
        None => StreamOutcome::Cancelled(state),
    })
}
