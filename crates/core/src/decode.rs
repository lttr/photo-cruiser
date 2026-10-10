//! Decoding: the `image` crate for JPEG/PNG/GIF/WebP/BMP (EXIF orientation applied),
//! system libheif for AVIF/HEIC/HEIF (libheif applies irot/imir itself).

use crate::catalog::extension;
use crate::{Error, Result};
use image::{ImageDecoder, ImageReader, RgbaImage};
use serde::Serialize;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Jpeg,
    Png,
    Gif,
    Webp,
    Bmp,
    Avif,
    Heic,
}

impl Format {
    /// Lower-case name, as in JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            Format::Jpeg => "jpeg",
            Format::Png => "png",
            Format::Gif => "gif",
            Format::Webp => "webp",
            Format::Bmp => "bmp",
            Format::Avif => "avif",
            Format::Heic => "heic",
        }
    }

    /// Formats decoded through libheif are detected by extension; the rest by content.
    fn heif_from_path(path: &Path) -> Option<Format> {
        match extension(&path.file_name()?.to_string_lossy())?.as_str() {
            "avif" => Some(Format::Avif),
            "heic" | "heif" => Some(Format::Heic),
            _ => None,
        }
    }

    fn from_image(f: image::ImageFormat) -> Option<Format> {
        use image::ImageFormat as F;
        Some(match f {
            F::Jpeg => Format::Jpeg,
            F::Png => Format::Png,
            F::Gif => Format::Gif,
            F::WebP => Format::Webp,
            F::Bmp => Format::Bmp,
            _ => return None,
        })
    }
}

impl std::fmt::Display for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Header information, without decoding pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Probe {
    pub format: Format,
    /// Displayed size, i.e. after orientation.
    pub width: u32,
    pub height: u32,
    /// EXIF orientation 1–8 (1 = none). HEIF transforms are applied by libheif and reported as 1.
    pub orientation: u8,
}

/// Decoded, oriented RGBA pixels. `image` may be downscaled (see [`decode_max_side`]);
/// `source_width`/`source_height` always describe the full-resolution photo, so
/// coordinates (crop rectangles) stay in source pixels.
#[derive(Clone, Debug)]
pub struct Decoded {
    pub image: RgbaImage,
    pub format: Format,
    pub source_width: u32,
    pub source_height: u32,
}

impl Decoded {
    pub fn source_size(&self) -> (u32, u32) {
        (self.source_width, self.source_height)
    }

    /// Source pixels per pixel of `image` (1.0 when not downscaled).
    pub fn source_scale(&self) -> f32 {
        self.source_width as f32 / self.image.width().max(1) as f32
    }

    /// Downscale so that the longer side is at most `max_side`. No-op if already smaller.
    pub fn downscale(mut self, max_side: u32) -> Self {
        let (w, h) = self.image.dimensions();
        if w.max(h) > max_side && max_side > 0 {
            let k = max_side as f64 / w.max(h) as f64;
            let (nw, nh) = (((w as f64 * k).round() as u32).max(1), ((h as f64 * k).round() as u32).max(1));
            self.image = image::imageops::thumbnail(&self.image, nw, nh);
        }
        self
    }
}

pub fn probe(path: &Path) -> Result<Probe> {
    if let Some(format) = Format::heif_from_path(path) {
        let (ctx, handle) = heif_open(path)?;
        let _keep_alive = ctx;
        return Ok(Probe { format, width: handle.width(), height: handle.height(), orientation: 1 });
    }
    let (format, mut decoder) = image_decoder(path)?;
    let (w, h) = decoder.dimensions();
    let orientation = decoder.orientation().map_err(|e| Error::decode(path, e))?.to_exif();
    let (width, height) = if (5..=8).contains(&orientation) { (h, w) } else { (w, h) };
    Ok(Probe { format, width, height, orientation })
}

/// Decode at full resolution.
pub fn decode(path: &Path) -> Result<Decoded> {
    if let Some(format) = Format::heif_from_path(path) {
        return decode_heif(path, format);
    }
    let (format, mut decoder) = image_decoder(path)?;
    let orientation = decoder.orientation().map_err(|e| Error::decode(path, e))?;
    let mut img = image::DynamicImage::from_decoder(decoder).map_err(|e| Error::decode(path, e))?;
    img.apply_orientation(orientation);
    let image = img.into_rgba8();
    let (source_width, source_height) = image.dimensions();
    Ok(Decoded { image, format, source_width, source_height })
}

/// Decode, then downscale for display (e.g. to the GPU's max texture side).
pub fn decode_max_side(path: &Path, max_side: Option<u32>) -> Result<Decoded> {
    let d = decode(path)?;
    Ok(match max_side {
        Some(m) => d.downscale(m),
        None => d,
    })
}

/// The photo's embedded preview, if it has one: the EXIF thumbnail of a JPEG or the
/// thumbnail item of a HEIF/AVIF, oriented like the photo. Reads only headers and the
/// small thumbnail, so it takes a few ms even when the full decode takes hundreds.
/// `Ok(None)` when there is none, or when its aspect ratio doesn't match the photo
/// (letterboxed thumbnails would jump when the full image replaces them).
pub fn embedded_preview(path: &Path) -> Result<Option<Decoded>> {
    let decoded = if let Some(format) = Format::heif_from_path(path) {
        let (_ctx, handle) = heif_open(path)?;
        let mut ids = [0; 1];
        if handle.thumbnail_ids(&mut ids) == 0 {
            return Ok(None);
        }
        let thumb = handle.thumbnail(ids[0]).map_err(|e| Error::decode(path, e))?;
        let image = heif_rgba(path, &thumb)?;
        Decoded { image, format, source_width: handle.width(), source_height: handle.height() }
    } else {
        let (format, mut decoder) = image_decoder(path)?;
        if format != Format::Jpeg {
            return Ok(None);
        }
        let (w, h) = decoder.dimensions();
        let exif = decoder.exif_metadata().map_err(|e| Error::decode(path, e))?;
        let Some(jpeg) = exif.as_deref().and_then(exif_thumbnail) else { return Ok(None) };
        let Ok(mut img) = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg) else {
            return Ok(None);
        };
        let orientation = decoder.orientation().map_err(|e| Error::decode(path, e))?;
        let (source_width, source_height) = match orientation.to_exif() {
            5..=8 => (h, w),
            _ => (w, h),
        };
        img.apply_orientation(orientation);
        Decoded { image: img.into_rgba8(), format, source_width, source_height }
    };
    let (tw, th) = decoded.image.dimensions();
    let aspect = |w: u32, h: u32| w as f64 / h.max(1) as f64;
    let mismatch = (aspect(tw, th) / aspect(decoded.source_width, decoded.source_height) - 1.0).abs();
    Ok((mismatch < 0.03).then_some(decoded))
}

/// A small image for a grid of photos, longest side at most `max_side`. Uses the
/// embedded preview when it is at least half that size (a few ms), else decodes the
/// whole photo and downscales it.
pub fn thumbnail(path: &Path, max_side: u32) -> Result<Decoded> {
    if let Some(p) = embedded_preview(path)? {
        let (w, h) = p.image.dimensions();
        if w.max(h) * 2 >= max_side {
            return Ok(p.downscale(max_side));
        }
    }
    decode_max_side(path, Some(max_side))
}

/// The JPEG thumbnail referenced by IFD1 of an EXIF block (a TIFF structure).
fn exif_thumbnail(exif: &[u8]) -> Option<&[u8]> {
    let le = match exif.get(..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let u16_at = |o: usize| {
        let b: [u8; 2] = exif.get(o..o + 2)?.try_into().ok()?;
        Some(if le { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) })
    };
    let u32_at = |o: usize| {
        let b: [u8; 4] = exif.get(o..o + 4)?.try_into().ok()?;
        Some(if le { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) })
    };
    let ifd0 = u32_at(4)? as usize;
    let ifd1 = u32_at(ifd0 + 2 + 12 * u16_at(ifd0)? as usize)? as usize;
    if ifd1 == 0 {
        return None;
    }
    let (mut offset, mut length) = (None, None);
    for i in 0..u16_at(ifd1)? as usize {
        let entry = ifd1 + 2 + 12 * i;
        // SHORT (3) values sit in the first two bytes of the value field.
        let value = match u16_at(entry + 2)? {
            3 => u16_at(entry + 8)? as usize,
            _ => u32_at(entry + 8)? as usize,
        };
        match u16_at(entry)? {
            0x0201 => offset = Some(value),
            0x0202 => length = Some(value),
            _ => {}
        }
    }
    let (offset, length) = (offset?, length?);
    exif.get(offset..offset.checked_add(length)?)
}

fn image_decoder(path: &Path) -> Result<(Format, impl ImageDecoder)> {
    let reader = ImageReader::open(path)
        .map_err(|e| Error::io(path, e))?
        .with_guessed_format()
        .map_err(|e| Error::io(path, e))?;
    let format = reader.format().and_then(Format::from_image).ok_or_else(|| {
        // A known photo extension with unrecognisable content is a broken file.
        if crate::catalog::is_photo_name(&path.to_string_lossy()) {
            Error::decode(path, "unrecognised or corrupt image data")
        } else {
            Error::Unsupported { path: path.to_path_buf() }
        }
    })?;
    let decoder = reader.into_decoder().map_err(|e| Error::decode(path, e))?;
    Ok((format, decoder))
}

fn heif_open(path: &Path) -> Result<(libheif_rs::HeifContext<'static>, libheif_rs::ImageHandle)> {
    let ctx = libheif_rs::HeifContext::read_from_file(&path.to_string_lossy()).map_err(|e| {
        if path.exists() {
            Error::decode(path, e)
        } else {
            Error::io(path, std::io::ErrorKind::NotFound.into())
        }
    })?;
    let handle = ctx.primary_image_handle().map_err(|e| Error::decode(path, e))?;
    Ok((ctx, handle))
}

fn decode_heif(path: &Path, format: Format) -> Result<Decoded> {
    let (_ctx, handle) = heif_open(path)?;
    let image = heif_rgba(path, &handle)?;
    let (w, h) = image.dimensions();
    Ok(Decoded { image, format, source_width: w, source_height: h })
}

fn heif_rgba(path: &Path, handle: &libheif_rs::ImageHandle) -> Result<RgbaImage> {
    use libheif_rs::{ColorSpace, LibHeif, RgbChroma};
    let img = LibHeif::new()
        .decode(handle, ColorSpace::Rgb(RgbChroma::Rgba), None)
        .map_err(|e| Error::decode(path, e))?;
    let plane = img.planes().interleaved.ok_or_else(|| Error::decode(path, "no interleaved RGBA plane"))?;
    let (w, h, row) = (plane.width, plane.height, plane.width as usize * 4);
    // Row copies, not a per-byte iterator: 48 MB per 12 MP photo is slow unoptimised (dev builds).
    let mut data = Vec::with_capacity(row * h as usize);
    for r in plane.data.chunks(plane.stride).take(h as usize) {
        data.extend_from_slice(&r[..row]);
    }
    RgbaImage::from_raw(w, h, data).ok_or_else(|| Error::decode(path, "short RGBA plane"))
}
