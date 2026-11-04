use super::{
    GraphicContext, StreamOutcome, StreamProgress,
    graphics::{PreparedGraphic, prepare},
};
use crate::{Error, Result, index::Index};
use std::{
    io::{self, Cursor, Write},
    sync::atomic::{AtomicBool, Ordering},
};

pub struct PngRequest {
    pub context: GraphicContext,
    pub scale: u8,
    pub image: Vec<u8>,
}

fn invalid(message: impl std::fmt::Display) -> Error {
    Error::Invalid(format!("Invalid storage-map PNG: {message}"))
}

struct Output<'a, W> {
    writer: &'a mut W,
    written: usize,
    cancelled: &'a AtomicBool,
}
impl<W: Write> Write for Output<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(io::Error::other("PNG export cancelled."));
        }
        if bytes.len() > (64 * 1024 * 1024_usize).saturating_sub(self.written) {
            return Err(io::Error::other("PNG exceeds the 64 MiB output limit."));
        }
        let count = self.writer.write(bytes)?;
        self.written += count;
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

pub fn stream_png(
    index: &Index,
    request: &PngRequest,
    writer: &mut impl Write,
    cancelled: &AtomicBool,
    progress: impl FnMut(StreamProgress),
) -> Result<StreamOutcome> {
    let (last, mut context) = match prepare(index, &request.context, "png", cancelled, progress)? {
        PreparedGraphic::Cancelled(last) => return Ok(StreamOutcome::Cancelled(last)),
        PreparedGraphic::Complete { progress, context } => (progress, context),
    };
    if ![1, 2].contains(&request.scale) || request.image.len() > 16 * 1024 * 1024 {
        return Err(invalid("unsupported scale or input length"));
    }
    let width = (request.context.view.width * f64::from(request.scale)).ceil() as u32;
    let height = (request.context.view.height * f64::from(request.scale)).ceil() as u32;
    if width > 8192 || height > 8192 || u64::from(width) * u64::from(height) > 16_777_216 {
        return Err(invalid("resolution exceeds 8192 per side or 16 megapixels"));
    }
    let mut decoder = png::Decoder::new(Cursor::new(&request.image));
    decoder.set_limits(png::Limits {
        bytes: 64 * 1024 * 1024,
    });
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    let mut reader = decoder.read_info().map_err(invalid)?;
    let info = reader.info();
    if info.width != width
        || info.height != height
        || info.bit_depth != png::BitDepth::Eight
        || !matches!(info.color_type, png::ColorType::Rgb | png::ColorType::Rgba)
        || info.interlaced
        || info.animation_control.is_some()
        || info.frame_control.is_some()
    {
        return Err(invalid(
            "expected a static, non-interlaced 8-bit RGB(A) image matching the captured viewport",
        ));
    }
    let color = info.color_type;
    context["pixel_width"] = width.into();
    context["pixel_height"] = height.into();
    context["pixel_ratio"] = request.scale.into();
    context["pixel_encoding"] =
        "8-bit sRGB; labels are rasterized. Client ancillary metadata and profiles are discarded."
            .into();
    let result = (|| -> Result<()> {
        let output = Output {
            writer,
            written: 0,
            cancelled,
        };
        let mut encoder = png::Encoder::new(output, width, height);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
        encoder
            .add_itxt_chunk(
                "Voldiq".into(),
                serde_json::to_string(&context).map_err(invalid)?,
            )
            .map_err(invalid)?;
        let mut encoded = encoder.write_header().map_err(invalid)?;
        {
            let mut stream = encoded
                .stream_writer_with_size(64 * 1024)
                .map_err(invalid)?;
            let row_bytes = width as usize * if color == png::ColorType::Rgb { 3 } else { 4 };
            for _ in 0..height {
                if cancelled.load(Ordering::Relaxed) {
                    return Err(invalid("cancelled"));
                }
                let row = reader
                    .next_row()
                    .map_err(invalid)?
                    .ok_or_else(|| invalid("missing image row"))?;
                if row.data().len() != row_bytes {
                    return Err(invalid("unexpected image row length"));
                }
                stream.write_all(row.data())?;
            }
            if reader.next_row().map_err(invalid)?.is_some() {
                return Err(invalid("unexpected extra image row"));
            }
            reader.finish().map_err(invalid)?;
            stream.finish().map_err(invalid)?;
        }
        encoded.finish().map_err(invalid)?;
        Ok(())
    })();
    if cancelled.load(Ordering::Relaxed) {
        return Ok(StreamOutcome::Cancelled(last));
    }
    result?;
    Ok(StreamOutcome::Complete(last))
}
