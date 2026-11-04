mod validate;
use super::{
    GraphicContext, GraphicView, StreamOutcome, StreamProgress,
    graphics::{PreparedGraphic, prepare},
};
use crate::{Error, Result, index::Index};
use serde::Deserialize;
use std::{
    io::Write,
    sync::atomic::{AtomicBool, Ordering},
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SvgRequest {
    pub context: GraphicContext,
    pub drawing: String,
}
fn invalid() -> Error {
    Error::Invalid("Unsupported or invalid storage-map SVG drawing.".into())
}
pub fn stream_svg(
    index: &Index,
    request: &SvgRequest,
    writer: &mut impl Write,
    cancelled: &AtomicBool,
    progress: impl FnMut(StreamProgress),
) -> Result<StreamOutcome> {
    if !cancelled.load(Ordering::Relaxed) && request.drawing.len() > 8 * 1024 * 1024 {
        return Err(invalid());
    }
    let (last, mut context) = match prepare(index, &request.context, "svg", cancelled, progress)? {
        PreparedGraphic::Cancelled(last) => return Ok(StreamOutcome::Cancelled(last)),
        PreparedGraphic::Complete { progress, context } => (progress, context),
    };
    context["font_encoding"] = "Requested system fonts; fonts are not embedded.".into();
    let Some(bytes) =
        validate::encode(&request.drawing, &request.context.view, &context, cancelled)?
    else {
        return Ok(StreamOutcome::Cancelled(last));
    };
    for chunk in bytes.chunks(64 * 1024) {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(StreamOutcome::Cancelled(last));
        }
        writer.write_all(chunk)?;
    }
    Ok(if cancelled.load(Ordering::Relaxed) {
        StreamOutcome::Cancelled(last)
    } else {
        StreamOutcome::Complete(last)
    })
}
