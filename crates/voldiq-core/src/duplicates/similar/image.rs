use crate::{Error, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::path::Path;
use windows::{
    Win32::{
        Foundation::RPC_E_CHANGED_MODE,
        Graphics::Imaging::*,
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
            CoUninitialize,
            StructuredStorage::{PROPVARIANT, PropVariantClear, PropVariantToUInt16},
        },
    },
    core::{Interface, w},
};

const PIXEL_LIMIT: u64 = 25_000_000;
const THUMBNAIL_EDGE: u32 = 768;

#[derive(Clone, Copy)]
pub(super) enum Format {
    Jpeg,
    Png,
}

impl Format {
    pub(super) fn of(name: &str) -> Option<Self> {
        let ext = Path::new(name).extension()?.to_str()?;
        if ext.eq_ignore_ascii_case("jpg") || ext.eq_ignore_ascii_case("jpeg") {
            Some(Self::Jpeg)
        } else if ext.eq_ignore_ascii_case("png") {
            Some(Self::Png)
        } else {
            None
        }
    }
}

pub(super) struct Image {
    pub hash: u64,
    pub width: u32,
    pub height: u32,
    pub png: String,
}

struct Apartment(bool);

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

pub(super) struct Decoder {
    factory: IWICImagingFactory,
    _apartment: Apartment,
}

impl Decoder {
    pub(super) fn new() -> Result<Self> {
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let apartment = Apartment(result.is_ok());
        if result != RPC_E_CHANGED_MODE {
            result.ok().map_err(failure)?;
        }
        let factory =
            unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
                .map_err(failure)?;
        Ok(Self {
            factory,
            _apartment: apartment,
        })
    }

    pub(super) fn decode(&self, input: &[u8], format: Format, preview: bool) -> Result<Image> {
        // All COM interfaces that borrow the input are released before this function returns.
        let stream = unsafe { self.factory.CreateStream() }.map_err(failure)?;
        unsafe { stream.InitializeFromMemory(input) }.map_err(failure)?;
        let class = match format {
            Format::Jpeg => CLSID_WICJpegDecoder,
            Format::Png => CLSID_WICPngDecoder,
        };
        let decoder: IWICBitmapDecoder =
            unsafe { CoCreateInstance(&class, None, CLSCTX_INPROC_SERVER) }.map_err(failure)?;
        unsafe { decoder.Initialize(&stream, WICDecodeMetadataCacheOnDemand) }.map_err(failure)?;
        if unsafe { decoder.GetFrameCount() }.map_err(failure)? != 1 {
            return Err(Error::Invalid(
                "Only single-frame images can be compared.".into(),
            ));
        }
        let frame = unsafe { decoder.GetFrame(0) }.map_err(failure)?;
        let (mut width, mut height) = (0, 0);
        unsafe { frame.GetSize(&mut width, &mut height) }.map_err(failure)?;
        if !(1..=16_384).contains(&width)
            || !(1..=16_384).contains(&height)
            || u64::from(width) * u64::from(height) > PIXEL_LIMIT
        {
            return Err(Error::Invalid(
                "The image exceeds the decoding limits.".into(),
            ));
        }
        let converter = unsafe { self.factory.CreateFormatConverter() }.map_err(failure)?;
        unsafe {
            converter.Initialize(
                &frame,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
        }
        .map_err(failure)?;
        let mut source: IWICBitmapSource = converter.cast().map_err(failure)?;
        if matches!(format, Format::Jpeg) {
            let (transform, swap) = orientation(&frame);
            if transform != WICBitmapTransformRotate0 {
                let rotate = unsafe { self.factory.CreateBitmapFlipRotator() }.map_err(failure)?;
                unsafe { rotate.Initialize(&source, transform) }.map_err(failure)?;
                source = rotate.cast().map_err(failure)?;
            }
            if swap {
                std::mem::swap(&mut width, &mut height);
            }
        }
        let sample = self.pixels(&source, 9, 8)?;
        let mut hash = 0;
        for row in sample.chunks_exact(9 * 4) {
            for offset in (0..8 * 4).step_by(4) {
                hash = (hash << 1)
                    | u64::from(
                        luminance(&row[offset..offset + 4])
                            > luminance(&row[offset + 4..offset + 8]),
                    );
            }
        }
        let png = if preview {
            let edge = width.max(height).max(THUMBNAIL_EDGE);
            let w = (width * THUMBNAIL_EDGE / edge).max(1);
            let h = (height * THUMBNAIL_EDGE / edge).max(1);
            let mut rgba = self.pixels(&source, w, h)?;
            for pixel in rgba.chunks_exact_mut(4) {
                pixel.swap(0, 2);
                let alpha = u32::from(pixel[3]);
                for channel in &mut pixel[..3] {
                    *channel = (u32::from(*channel) * 255 + alpha / 2)
                        .checked_div(alpha)
                        .unwrap_or(0)
                        .min(255) as u8;
                }
            }
            encode(w, h, &rgba)?
        } else {
            String::new()
        };
        Ok(Image {
            hash,
            width,
            height,
            png,
        })
    }

    fn pixels(&self, source: &IWICBitmapSource, width: u32, height: u32) -> Result<Vec<u8>> {
        let scaler = unsafe { self.factory.CreateBitmapScaler() }.map_err(failure)?;
        unsafe { scaler.Initialize(source, width, height, WICBitmapInterpolationModeFant) }
            .map_err(failure)?;
        let stride = width * 4;
        let mut pixels = vec![0; (stride * height) as usize];
        unsafe { scaler.CopyPixels(std::ptr::null(), stride, &mut pixels) }.map_err(failure)?;
        Ok(pixels)
    }
}

fn luminance(pixel: &[u8]) -> u32 {
    29 * u32::from(pixel[0])
        + 150 * u32::from(pixel[1])
        + 77 * u32::from(pixel[2])
        + 128 * (255 - u32::from(pixel[3]))
}

fn orientation(frame: &IWICBitmapFrameDecode) -> (WICBitmapTransformOptions, bool) {
    let mut value = PROPVARIANT::default();
    let orientation = unsafe { frame.GetMetadataQueryReader() }
        .ok()
        .and_then(|reader| {
            unsafe { reader.GetMetadataByName(w!("/app1/ifd/{ushort=274}"), &mut value) }.ok()?;
            unsafe { PropVariantToUInt16(&value) }.ok()
        });
    let _ = unsafe { PropVariantClear(&mut value) };
    match orientation {
        Some(2) => (WICBitmapTransformFlipHorizontal, false),
        Some(3) => (WICBitmapTransformRotate180, false),
        Some(4) => (WICBitmapTransformFlipVertical, false),
        Some(5) => (
            WICBitmapTransformOptions(
                WICBitmapTransformRotate90.0 | WICBitmapTransformFlipHorizontal.0,
            ),
            true,
        ),
        Some(6) => (WICBitmapTransformRotate90, true),
        Some(7) => (
            WICBitmapTransformOptions(
                WICBitmapTransformRotate270.0 | WICBitmapTransformFlipHorizontal.0,
            ),
            true,
        ),
        Some(8) => (WICBitmapTransformRotate270, true),
        _ => (WICBitmapTransformRotate0, false),
    }
}

fn encode(width: u32, height: u32, rgba: &[u8]) -> Result<String> {
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let map = |error: png::EncodingError| {
            Error::Invalid(format!("Image preview encoding failed: {error}"))
        };
        let mut writer = encoder.write_header().map_err(map)?;
        writer.write_image_data(rgba).map_err(map)?;
        writer.finish().map_err(map)?;
    }
    Ok(STANDARD.encode(png))
}

fn failure(error: windows::core::Error) -> Error {
    Error::Invalid(format!("Windows could not decode this image: {error}"))
}
