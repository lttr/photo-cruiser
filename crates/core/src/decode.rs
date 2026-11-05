//! Decoding: the `image` crate for JPEG/PNG/GIF/WebP/BMP (EXIF orientation applied),
//! system libheif for AVIF/HEIC/HEIF (libheif applies irot/imir itself).

use crate::catalog::extension;
use crate::source::one_at_a_time;
use crate::{Error, Result};
use image::{ImageDecoder, ImageReader, RgbaImage};
use serde::Serialize;
use std::io::{BufRead, Cursor, Seek};
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
    retry_io(|| {
        let _one = one_at_a_time(path);
        probe_once(path)
    })
}

fn probe_once(path: &Path) -> Result<Probe> {
    if let Some(format) = Format::heif_from_path(path) {
        let (ctx, handle) = heif_open(path)?;
        let _keep_alive = ctx;
        return Ok(Probe { format, width: handle.width(), height: handle.height(), orientation: 1 });
    }
    if let Some(head) = read_jpeg_head(path)? {
        let h = head.parsed();
        let orientation = jpeg_orientation(&h).to_exif();
        let (width, height) =
            if (5..=8).contains(&orientation) { (h.height, h.width) } else { (h.width, h.height) };
        return Ok(Probe { format: Format::Jpeg, width, height, orientation });
    }
    let (format, mut decoder) = image_decoder(path)?;
    let (w, h) = decoder.dimensions();
    let orientation = decoder.orientation().map_err(|e| Error::image(path, e))?.to_exif();
    let (width, height) = if (5..=8).contains(&orientation) { (h, w) } else { (w, h) };
    Ok(Probe { format, width, height, orientation })
}

/// Decode at full resolution.
pub fn decode(path: &Path) -> Result<Decoded> {
    retry_io(|| decode_once(path))
}

fn decode_once(path: &Path) -> Result<Decoded> {
    // Read first, decode from memory: the file is open only briefly (see `one_at_a_time`).
    let bytes = crate::source::read(path)?;
    if let Some(format) = Format::heif_from_path(path) {
        return decode_heif(path, format, &bytes);
    }
    let (format, mut decoder) = decoder_from(path, ImageReader::new(Cursor::new(bytes)))?;
    let orientation = decoder.orientation().map_err(|e| Error::image(path, e))?;
    let mut img = image::DynamicImage::from_decoder(decoder).map_err(|e| Error::image(path, e))?;
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
    preview_within(path, 0.03)
}

/// [`embedded_preview`] whose aspect ratio may differ from the photo's by `tolerance`
/// (relative).
fn preview_within(path: &Path, tolerance: f64) -> Result<Option<Decoded>> {
    retry_io(|| {
        let _one = one_at_a_time(path);
        preview_within_once(path, tolerance)
    })
}

fn preview_within_once(path: &Path, tolerance: f64) -> Result<Option<Decoded>> {
    let decoded = if let Some(format) = Format::heif_from_path(path) {
        let (_ctx, handle) = heif_open(path)?;
        let mut ids = [0; 1];
        if handle.thumbnail_ids(&mut ids) == 0 {
            return Ok(None);
        }
        let thumb = handle.thumbnail(ids[0]).map_err(|e| Error::decode(path, e))?;
        let image = heif_rgba(path, &thumb)?;
        Decoded { image, format, source_width: handle.width(), source_height: handle.height() }
    } else if let Some(head) = read_jpeg_head(path)? {
        let h = head.parsed();
        let Some(jpeg) = h.exif.and_then(crate::meta::exif_thumbnail) else { return Ok(None) };
        let Ok(mut img) = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg) else {
            return Ok(None);
        };
        let orientation = jpeg_orientation(&h);
        let (source_width, source_height) = match orientation.to_exif() {
            5..=8 => (h.height, h.width),
            _ => (h.width, h.height),
        };
        img.apply_orientation(orientation);
        Decoded { image: img.into_rgba8(), format: Format::Jpeg, source_width, source_height }
    } else {
        let (format, mut decoder) = image_decoder(path)?;
        if format != Format::Jpeg {
            return Ok(None);
        }
        let (w, h) = decoder.dimensions();
        let exif = decoder.exif_metadata().map_err(|e| Error::image(path, e))?;
        let Some(jpeg) = exif.as_deref().and_then(crate::meta::exif_thumbnail) else { return Ok(None) };
        let Ok(mut img) = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg) else {
            return Ok(None);
        };
        let orientation = decoder.orientation().map_err(|e| Error::image(path, e))?;
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
    Ok((mismatch < tolerance).then_some(decoded))
}

/// A small image for a grid of photos, longest side at most `max_side`. Uses the
/// embedded preview when it is at least half that size (a few ms), else decodes the
/// whole photo and downscales it. The preview's aspect ratio may be off by up to 8%
/// (phones round thumbnail sides to multiples of 16, e.g. 128×176 for 3000×4000); it
/// is resized to the photo's aspect ratio.
pub fn thumbnail(path: &Path, max_side: u32) -> Result<Decoded> {
    if let Some(mut p) = preview_within(path, 0.08)? {
        let (w, h) = p.image.dimensions();
        let long = w.max(h);
        if long * 2 >= max_side {
            let (sw, sh) = p.source_size();
            let k = long.min(max_side) as f64 / sw.max(sh) as f64;
            let (nw, nh) = (((sw as f64 * k).round() as u32).max(1), ((sh as f64 * k).round() as u32).max(1));
            if (nw, nh) != (w, h) {
                p.image = image::imageops::thumbnail(&p.image, nw, nh);
            }
            return Ok(p);
        }
    }
    decode_max_side(path, Some(max_side))
}

/// What Save carries over from the original: its ICC profile and EXIF block (a TIFF
/// structure, without the `Exif\0\0` header).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Metadata {
    pub icc: Option<Vec<u8>>,
    pub exif: Option<Vec<u8>>,
}

/// The ICC profile and EXIF of a photo, as far as its format has them. XMP and maker
/// notes outside EXIF are not read (a saved photo loses them).
pub fn metadata(path: &Path) -> Result<Metadata> {
    retry_io(|| {
        let _one = one_at_a_time(path);
        metadata_once(path)
    })
}

fn metadata_once(path: &Path) -> Result<Metadata> {
    if Format::heif_from_path(path).is_some() {
        use libheif_rs::ColorProfile;
        let (_ctx, handle) = heif_open(path)?;
        let icc = handle
            .color_profile_raw()
            .filter(|p| p.profile_type() != libheif_rs::color_profile_types::NCLX)
            .map(|p| p.data);
        let mut ids = [0; 1];
        // A HEIF EXIF item starts with the offset of the TIFF header (after those 4 bytes).
        let exif = (handle.metadata_block_ids(&mut ids, b"Exif") > 0)
            .then(|| handle.metadata(ids[0]).ok())
            .flatten()
            .and_then(|d| {
                let skip = u32::from_be_bytes(d.get(..4)?.try_into().ok()?) as usize;
                d.get(4 + skip..).map(<[u8]>::to_vec)
            });
        return Ok(Metadata { icc, exif });
    }
    let (_, mut decoder) = image_decoder(path)?;
    let icc = decoder.icc_profile().map_err(|e| Error::image(path, e))?;
    let exif = decoder.exif_metadata().map_err(|e| Error::image(path, e))?;
    Ok(Metadata { icc, exif })
}

/// Quantisation tables, sampling and density of a JPEG (see [`crate::meta::jpeg_settings`]).
/// `Ok(None)` for other formats.
pub fn jpeg_settings(path: &Path) -> Result<Option<crate::meta::JpegSettings>> {
    let bytes = retry_io(|| crate::source::read(path))?;
    Ok(crate::meta::jpeg_settings(&bytes))
}

/// Run a read, again after a short pause when it fails with a transient I/O error (see
/// [`Error::is_transient`]): network shares (SMB over gvfs) report EIO now and then for
/// files that read fine a moment later.
fn retry_io<T>(mut read: impl FnMut() -> Result<T>) -> Result<T> {
    for pause_ms in [50, 250] {
        match read() {
            Err(e) if e.is_transient() => std::thread::sleep(std::time::Duration::from_millis(pause_ms)),
            r => return r,
        }
    }
    read()
}

/// The start of a JPEG, up to and including its frame header: what a probe or the
/// embedded preview needs, without reading the whole file (the `image` crate's JPEG
/// decoder reads it all, which takes as long as a full decode on a network share).
/// `None` if it isn't a JPEG or is broken before the frame header (the caller falls back
/// to the decoder, which reports why). Hold [`one_at_a_time`] while calling.
fn read_jpeg_head(path: &Path) -> Result<Option<JpegHeadBytes>> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).map_err(|e| Error::io(path, e))?;
    let mut b = Vec::new();
    let mut want = 64 * 1024;
    loop {
        let got =
            (&mut f).take((want - b.len()) as u64).read_to_end(&mut b).map_err(|e| Error::io(path, e))?;
        match crate::meta::jpeg_head(&b) {
            crate::meta::Head::Done(_) => return Ok(Some(JpegHeadBytes(b))),
            crate::meta::Head::NotJpeg => return Ok(None),
            crate::meta::Head::More(_) if got == 0 => return Ok(None),
            // Markers before the frame (ICC profiles, MPF) can run past 64 kB.
            crate::meta::Head::More(n) => want = n.max(b.len() + 64 * 1024),
        }
    }
}

/// Bytes [`crate::meta::jpeg_head`] parses to `Done`.
struct JpegHeadBytes(Vec<u8>);

impl JpegHeadBytes {
    fn parsed(&self) -> crate::meta::JpegHead<'_> {
        match crate::meta::jpeg_head(&self.0) {
            crate::meta::Head::Done(h) => h,
            _ => unreachable!("checked by read_jpeg_head"),
        }
    }
}

/// A JPEG's EXIF orientation (none if absent or out of range).
fn jpeg_orientation(h: &crate::meta::JpegHead) -> image::metadata::Orientation {
    let o = h.exif.and_then(crate::meta::exif_orientation).unwrap_or(1);
    image::metadata::Orientation::from_exif(o.min(255) as u8)
        .unwrap_or(image::metadata::Orientation::NoTransforms)
}

/// A decoder reading `path` itself. On gvfs, hold [`one_at_a_time`] while it's in use.
pub(crate) fn image_decoder(path: &Path) -> Result<(Format, impl ImageDecoder)> {
    decoder_from(path, ImageReader::open(path).map_err(|e| Error::io(path, e))?)
}

fn decoder_from<R: BufRead + Seek + 'static>(
    path: &Path,
    reader: ImageReader<R>,
) -> Result<(Format, impl ImageDecoder + use<R>)> {
    let reader = reader.with_guessed_format().map_err(|e| Error::io(path, e))?;
    let format = reader.format().and_then(Format::from_image).ok_or_else(|| {
        // A known photo extension with unrecognisable content is a broken file.
        if crate::catalog::is_photo_name(&path.to_string_lossy()) {
            Error::decode(path, "unrecognised or corrupt image data")
        } else {
            Error::Unsupported { path: path.to_path_buf() }
        }
    })?;
    let decoder = reader.into_decoder().map_err(|e| Error::image(path, e))?;
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

fn decode_heif(path: &Path, format: Format, bytes: &[u8]) -> Result<Decoded> {
    let ctx = libheif_rs::HeifContext::read_from_bytes(bytes).map_err(|e| Error::decode(path, e))?;
    let handle = ctx.primary_image_handle().map_err(|e| Error::decode(path, e))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn eio() -> Error {
        Error::io("x.jpg", std::io::Error::from_raw_os_error(5))
    }

    #[test]
    fn retry_io_retries_transient_errors_only() {
        let mut calls = 0;
        let r = retry_io(|| {
            calls += 1;
            if calls < 3 { Err(eio()) } else { Ok(calls) }
        });
        assert_eq!(r.unwrap(), 3);

        let mut calls = 0;
        let r: Result<()> = retry_io(|| {
            calls += 1;
            Err(Error::io("x.jpg", std::io::ErrorKind::NotFound.into()))
        });
        assert!(r.is_err());
        assert_eq!(calls, 1);

        let mut calls = 0;
        let r: Result<()> = retry_io(|| {
            calls += 1;
            Err(eio())
        });
        assert!(r.unwrap_err().is_transient());
        assert_eq!(calls, 3);
    }

    #[test]
    fn image_io_errors_are_io() {
        let e = Error::image("x.jpg", image::ImageError::IoError(std::io::Error::from_raw_os_error(5)));
        assert_eq!(e.kind(), "io");
        assert!(e.is_transient());
    }
}
