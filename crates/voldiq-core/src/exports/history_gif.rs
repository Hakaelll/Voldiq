use super::{StreamOutcome, output::LimitedOutput};
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
pub struct HistoryGifRequest {
    pub clip: ExportClipRequest,
    pub fingerprint: String,
    pub width: u16,
    pub height: u16,
    pub delay_centiseconds: u16,
    pub looped: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GifStage {
    Preparing,
    Encoding,
    Revalidating,
    Finalizing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct GifProgress {
    pub stage: GifStage,
    pub completed_frames: usize,
    pub total_frames: usize,
}

fn invalid(message: impl std::fmt::Display) -> Error {
    Error::Invalid(format!("History GIF export failed: {message}"))
}

#[derive(Serialize)]
struct Metadata<'a> {
    format: &'static str,
    generated_at: u64,
    clip: &'a ExportClip,
    view: View,
    pixel_authority: &'static str,
    encoding: &'static str,
}
#[derive(Serialize)]
struct View {
    width: u16,
    height: u16,
    delay_centiseconds: u16,
    looped: bool,
}

/// The frame supplier returns PNG bytes in clip order, then None. It never receives live paths.
/// Output prefixes must stay private until Complete and destination publication succeed.
pub fn stream_history_gif(
    store: &Store,
    request: &HistoryGifRequest,
    writer: &mut impl Write,
    cancelled: &AtomicBool,
    mut progress: impl FnMut(GifProgress),
    mut frames: impl FnMut() -> Result<Option<Vec<u8>>>,
) -> Result<StreamOutcome<GifProgress>> {
    let mut state = GifProgress {
        stage: GifStage::Preparing,
        completed_frames: 0,
        total_frames: request.clip.snapshot_ids.len(),
    };
    if cancelled.load(Ordering::Relaxed) {
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
    {
        return Err(invalid(
            "invalid dimensions, playback speed or source fingerprint",
        ));
    }
    let result = (|| -> Result<Option<()>> {
        let Some(clip) = store.export_clip(&request.clip, cancelled, |value| {
            state.completed_frames = value.prepared_frames;
            state.total_frames = value.total_frames;
            progress(state);
        })?
        else {
            return Ok(None);
        };
        if clip.fingerprint != request.fingerprint {
            return Err(invalid("recorded source changed; prepare the clip again"));
        }
        if cancelled.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let metadata = serde_json::to_vec(&Metadata {
            format: "gif",
            generated_at: SystemTime::now().duration_since(UNIX_EPOCH).map_err(invalid)?.as_secs(),
            clip: &clip,
            view: View { width: request.width, height: request.height, delay_centiseconds: request.delay_centiseconds, looped: request.looped },
            pixel_authority: "Client-rendered illustration; pixels, captions and geometry are not attested by core. Recorded clip context grants no file-operation authority.",
            encoding: "Opaque sRGB PNG inputs; at most 256 quantized colors per GIF frame. Playback delay is illustrative, not real elapsed time.",
        }).map_err(invalid)?;
        drop(clip);
        if metadata.len() > 9 * 1024 * 1024 {
            return Err(invalid("metadata exceeds its byte limit"));
        }
        let output = LimitedOutput {
            writer,
            written: 0,
            limit: 64 * 1024 * 1024,
            cancelled,
        };
        let mut encoder =
            gif::Encoder::new(output, request.width, request.height, &[]).map_err(invalid)?;
        if request.looped {
            encoder.set_repeat(gif::Repeat::Infinite).map_err(invalid)?;
        }
        encoder
            .write_raw_extension(
                gif::Extension::Comment.into(),
                &[b"VOLDIQ-HISTORY-1\n", &metadata],
            )
            .map_err(invalid)?;
        drop(metadata);
        state.stage = GifStage::Encoding;
        state.completed_frames = 0;
        progress(state);
        let mut input_bytes = 0_usize;
        for _ in 0..state.total_frames {
            if cancelled.load(Ordering::Relaxed) {
                return Ok(None);
            }
            let image = frames()?.ok_or_else(|| invalid("missing PNG frame"))?;
            input_bytes = input_bytes
                .checked_add(image.len())
                .ok_or_else(|| invalid("input length overflow"))?;
            if input_bytes > 32 * 1024 * 1024 {
                return Err(invalid("PNG frames exceed the 32 MiB clip input limit"));
            }
            let mut reader = super::png_input::read(
                &image,
                u32::from(request.width),
                u32::from(request.height),
            )?;
            let rgba = reader.info().color_type == png::ColorType::Rgba;
            let mut rgb =
                Vec::with_capacity(usize::from(request.width) * usize::from(request.height) * 3);
            let row_bytes = usize::from(request.width) * if rgba { 4 } else { 3 };
            for _ in 0..request.height {
                if cancelled.load(Ordering::Relaxed) {
                    return Ok(None);
                }
                let row = reader
                    .next_row()
                    .map_err(invalid)?
                    .ok_or_else(|| invalid("missing PNG row"))?;
                if row.data().len() != row_bytes {
                    return Err(invalid("invalid PNG row length"));
                }
                if rgba {
                    for pixel in row.data().chunks_exact(4) {
                        if pixel[3] != 255 {
                            return Err(invalid("PNG frames must be opaque"));
                        }
                        rgb.extend_from_slice(&pixel[..3]);
                    }
                } else {
                    rgb.extend_from_slice(row.data());
                }
            }
            if reader.next_row().map_err(invalid)?.is_some() {
                return Err(invalid("unexpected extra PNG row"));
            }
            reader.finish().map_err(invalid)?;
            drop(reader);
            drop(image);
            if cancelled.load(Ordering::Relaxed) {
                return Ok(None);
            }
            let mut frame = gif::Frame::from_rgb_speed(request.width, request.height, &rgb, 10);
            drop(rgb);
            frame.delay = request.delay_centiseconds;
            frame.dispose = gif::DisposalMethod::Keep;
            if cancelled.load(Ordering::Relaxed) {
                return Ok(None);
            }
            encoder.write_frame(&frame).map_err(invalid)?;
            state.completed_frames += 1;
            progress(state);
        }
        if cancelled.load(Ordering::Relaxed) {
            return Ok(None);
        }
        if frames()?.is_some() {
            return Err(invalid("unexpected extra PNG frame"));
        }
        state.stage = GifStage::Revalidating;
        state.completed_frames = 0;
        progress(state);
        let Some(current) = store.export_clip(&request.clip, cancelled, |value| {
            state.completed_frames = value.prepared_frames;
            progress(state);
        })?
        else {
            return Ok(None);
        };
        if current.fingerprint != request.fingerprint {
            return Err(invalid("recorded source changed during encoding"));
        }
        if cancelled.load(Ordering::Relaxed) {
            return Ok(None);
        }
        encoder.into_inner().map_err(invalid)?;
        state.stage = GifStage::Finalizing;
        progress(state);
        Ok(Some(()))
    })();
    if cancelled.load(Ordering::Relaxed) {
        return Ok(StreamOutcome::Cancelled(state));
    }
    Ok(match result? {
        Some(()) => StreamOutcome::Complete(state),
        None => StreamOutcome::Cancelled(state),
    })
}
