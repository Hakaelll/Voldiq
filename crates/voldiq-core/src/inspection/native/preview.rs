use super::{decode_text, read_prefix};
use crate::{Result, inspection::Inspection};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{fs::File, io::Cursor, path::Path};

const TEXT_LIMIT: usize = 64 * 1024;
const PNG_LIMIT: usize = 8 * 1024 * 1024;
const PIXELS: u64 = 2_097_152;
const THUMBNAIL_SIDE: u32 = 512;

pub(super) fn read(file: &File, path: &Path, logical: u64) -> Result<Inspection> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension == "png" {
        if logical > PNG_LIMIT as u64 {
            return Ok(Inspection::Metadata {
                reason: "PNG preview is limited to files up to 8 MiB.",
            });
        }
        let input = read_prefix(file, PNG_LIMIT)?;
        return Ok(if input.len() > PNG_LIMIT {
            Inspection::Metadata {
                reason: "PNG preview is limited to files up to 8 MiB.",
            }
        } else {
            thumbnail(&input).unwrap_or(Inspection::Metadata {
                reason: "Image preview requires a readable static PNG within the pixel limit.",
            })
        });
    }
    if matches!(
        extension.as_str(),
        "txt"
            | "md"
            | "log"
            | "csv"
            | "tsv"
            | "json"
            | "toml"
            | "ini"
            | "xml"
            | "yaml"
            | "yml"
            | "html"
            | "htm"
            | "svg"
            | "css"
            | "js"
            | "ts"
            | "rs"
            | "py"
    ) {
        let input = read_prefix(file, TEXT_LIMIT)?;
        let truncated = input.len() > TEXT_LIMIT;
        if let Some(text) = decode_text(&input[..input.len().min(TEXT_LIMIT)], truncated) {
            return Ok(Inspection::Text { text, truncated });
        }
        return Ok(Inspection::Metadata {
            reason: "Text preview requires UTF-8 or UTF-16 with a byte-order mark.",
        });
    }
    Ok(Inspection::Metadata {
        reason: "Only static PNG images and supported text formats can be previewed.",
    })
}

fn thumbnail(input: &[u8]) -> Option<Inspection> {
    let mut decoder = png::Decoder::new(Cursor::new(input));
    decoder.set_limits(png::Limits {
        bytes: 16 * 1024 * 1024,
    });
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let info = reader.info();
    let (width, height) = (info.width, info.height);
    if width == 0
        || height == 0
        || width > 4096
        || height > 4096
        || u64::from(width) * u64::from(height) > PIXELS
        || info.animation_control.is_some()
        || info.frame_control.is_some()
    {
        return None;
    }
    let mut pixels = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut pixels).ok()?;
    reader.finish().ok()?;
    let edge = width.max(height).max(THUMBNAIL_SIDE);
    let small_width = (width * THUMBNAIL_SIDE / edge).max(1);
    let small_height = (height * THUMBNAIL_SIDE / edge).max(1);
    let channels = frame.color_type.samples();
    let mut rgba = Vec::with_capacity((small_width * small_height * 4) as usize);
    for y in 0..small_height {
        let row = (y * height / small_height) as usize * frame.line_size;
        for x in 0..small_width {
            let at = row + (x * width / small_width) as usize * channels;
            let pixel = pixels.get(at..at + channels)?;
            match frame.color_type {
                png::ColorType::Grayscale => rgba.extend([pixel[0], pixel[0], pixel[0], 255]),
                png::ColorType::GrayscaleAlpha => {
                    rgba.extend([pixel[0], pixel[0], pixel[0], pixel[1]]);
                }
                png::ColorType::Rgb => rgba.extend([pixel[0], pixel[1], pixel[2], 255]),
                png::ColorType::Rgba => rgba.extend_from_slice(pixel),
                png::ColorType::Indexed => return None,
            }
        }
    }
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, small_width, small_height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&rgba).ok()?;
        writer.finish().ok()?;
    }
    Some(Inspection::Image {
        width,
        height,
        png: STANDARD.encode(png),
    })
}
