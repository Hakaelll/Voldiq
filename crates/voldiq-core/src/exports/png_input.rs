use crate::{Error, Result};
use std::io::Cursor;

pub(super) fn read(image: &[u8], width: u32, height: u32) -> Result<png::Reader<Cursor<&[u8]>>> {
    let invalid = |message: &str| Error::Invalid(format!("Invalid exported PNG image: {message}"));
    if image.len() > 16 * 1024 * 1024
        || width == 0
        || height == 0
        || width > 8192
        || height > 8192
        || u64::from(width) * u64::from(height) > 16_777_216
    {
        return Err(invalid("input length or resolution exceeds its limit"));
    }
    let mut decoder = png::Decoder::new(Cursor::new(image));
    decoder.set_limits(png::Limits {
        bytes: 64 * 1024 * 1024,
    });
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    let reader = decoder
        .read_info()
        .map_err(|_| invalid("unreadable header"))?;
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
            "expected a matching static, non-interlaced 8-bit RGB(A) image",
        ));
    }
    Ok(reader)
}
