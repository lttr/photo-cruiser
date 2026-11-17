//! Encoding rendered pixels to files. JPEG goes through `jpeg-encoder` (custom tables,
//! sampling, density, metadata), WebP through libwebp (`webp`), PNG through `image`.

use crate::decode::Metadata;
use crate::meta::{self, JpegSettings};
use crate::{Error, Result};
use image::{ImageEncoder, RgbaImage};
use std::io::Write;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutFormat {
    Jpeg { quality: u8 },
    Png,
}

impl OutFormat {
    /// From the output file extension; `quality` applies to JPEG.
    pub fn from_path(path: &Path, quality: u8) -> Result<OutFormat> {
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase());
        match ext.as_deref() {
            Some("jpg" | "jpeg") => Ok(OutFormat::Jpeg { quality: quality.clamp(1, 100) }),
            Some("png") => Ok(OutFormat::Png),
            _ => Err(Error::UnsupportedOutput { path: path.to_path_buf() }),
        }
    }
}

/// How to encode, in full.
#[derive(Clone, Debug, PartialEq)]
pub enum Encoding {
    /// `settings.tables` if set (natural order), else the IJG tables at `quality`.
    Jpeg {
        settings: Box<JpegSettings>,
        quality: u8,
    },
    Png,
    Webp {
        lossless: bool,
        quality: u8,
    },
    /// The archive JPEG of `pcr export-tree` and `pcr export` ([`ARCHIVE_QUALITY`]).
    /// `sampling` as in [`JpegSettings::sampling`]: (1, 1) 4:4:4, (2, 1) 4:2:2, else 4:2:0.
    Archive {
        quality: u8,
        sampling: (u8, u8),
    },
}

/// Default quality of [`Encoding::Archive`]: visually indistinguishable full screen on 4K,
/// fine for A4 prints; 95 is 31 % bigger for a gain visible only at 100 % zoom (see
/// `.aiwork/2026-11-17_batch-export/research.md`).
pub const ARCHIVE_QUALITY: u8 = 92;

impl From<OutFormat> for Encoding {
    fn from(f: OutFormat) -> Self {
        match f {
            OutFormat::Jpeg { quality } => Encoding::Archive { quality, sampling: (2, 2) },
            OutFormat::Png => Encoding::Png,
        }
    }
}

/// Write `img` to `path` without metadata.
pub fn save(img: &RgbaImage, path: &Path, format: OutFormat) -> Result<()> {
    write(img, path, &format.into(), &Metadata::default())
}

/// The ICC profile and EXIF of `source` for a rendering of it of `width`×`height`: EXIF
/// patched as [`meta::patch_exif`] does (upright, new size, no stale thumbnail), and left
/// out if it can't be parsed.
pub fn saved_metadata(source: &Path, width: u32, height: u32) -> Result<Metadata> {
    let meta = crate::decode::metadata(source)?;
    Ok(Metadata { icc: meta.icc, exif: meta.exif.and_then(|e| meta::patch_exif(&e, width, height)) })
}

/// Encode `img` into a new file at `path`, with `meta` (EXIF already patched by the
/// caller). JPEG has no alpha: transparent pixels are flattened onto white.
pub fn write(img: &RgbaImage, path: &Path, encoding: &Encoding, meta: &Metadata) -> Result<()> {
    let bytes = encode_to_vec(img, encoding, meta).map_err(|e| Error::encode(path, e))?;
    write_bytes(path, &bytes)
}

/// Write encoded bytes into a new file at `path`, synced.
pub fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    let _one = crate::source::one_at_a_time(path);
    let mut file = std::fs::File::create(path).map_err(|e| Error::io(path, e))?;
    file.write_all(bytes).and_then(|_| file.sync_all()).map_err(|e| Error::io(path, e))
}

/// The name of a chroma sampling, `4:2:0` and so on (see [`Encoding::Archive`]).
pub fn sampling_name(sampling: (u8, u8)) -> &'static str {
    match sampling {
        (1, 1) => "4:4:4",
        (2, 1) => "4:2:2",
        _ => "4:2:0",
    }
}

/// The archive JPEG: mozjpeg's progressive encoding with trellis quantisation and
/// optimised scans, smallest for its quality; encoding time doesn't matter here (see
/// `.aiwork/2026-11-17_batch-export/research.md` for the benchmark behind the preset).
fn archive_jpeg(
    img: &RgbaImage,
    quality: u8,
    sampling: (u8, u8),
    meta: &Metadata,
) -> std::result::Result<Vec<u8>, String> {
    use mozjpeg_rs::{Encoder, Subsampling};
    let subsampling = match sampling_name(sampling) {
        "4:4:4" => Subsampling::S444,
        "4:2:2" => Subsampling::S422,
        _ => Subsampling::S420,
    };
    let mut enc = Encoder::max_compression().quality(quality.clamp(1, 100)).subsampling(subsampling);
    // Best effort, as for the other encoders: a block too big for its segment is left out.
    if let Some(icc) = &meta.icc {
        enc = enc.icc_profile(icc.clone());
    }
    if let Some(exif) = meta.exif.as_ref().filter(|e| e.len() <= 65535 - 8) {
        enc = enc.exif_data(exif.clone());
    }
    let (w, h) = img.dimensions();
    enc.encode_rgb(&rgb(img), w, h).map_err(|e| e.to_string())
}

fn opaque(img: &RgbaImage) -> bool {
    img.pixels().all(|p| p.0[3] == 255)
}

fn rgb(img: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::with_capacity(img.width() as usize * img.height() as usize * 3);
    for p in img.pixels() {
        let [r, g, b, a] = p.0;
        let over_white = |c: u8| ((c as u32 * a as u32 + 255 * (255 - a as u32) + 127) / 255) as u8;
        out.extend_from_slice(&[over_white(r), over_white(g), over_white(b)]);
    }
    out
}

/// `img` encoded with `encoding` and `meta`, in memory.
pub fn encode_to_vec(
    img: &RgbaImage,
    encoding: &Encoding,
    meta: &Metadata,
) -> std::result::Result<Vec<u8>, String> {
    let (w, h) = img.dimensions();
    let mut out = Vec::new();
    match encoding {
        Encoding::Jpeg { settings, quality } => {
            use jpeg_encoder::{ColorType, Encoder, PixelDensity, PixelDensityUnit, QuantizationTableType};
            let (Ok(w16), Ok(h16)) = (u16::try_from(w), u16::try_from(h)) else {
                return Err(format!("{w}×{h} is too big for JPEG"));
            };
            let mut enc = Encoder::new(&mut out, *quality);
            if let Some([luma, chroma]) = settings.tables {
                let custom = |t: [u16; 64]| QuantizationTableType::Custom(Box::new(t));
                enc.set_quantization_tables(custom(luma), custom(chroma));
            }
            let (sh, sv) = settings.sampling;
            enc.set_sampling_factor(
                jpeg_encoder::SamplingFactor::from_factors(sh, sv)
                    .unwrap_or(jpeg_encoder::SamplingFactor::F_1_1),
            );
            if let Some((unit, x, y)) = settings.density {
                let unit = match unit {
                    1 => PixelDensityUnit::Inches,
                    2 => PixelDensityUnit::Centimeters,
                    _ => PixelDensityUnit::PixelAspectRatio,
                };
                enc.set_density(PixelDensity { density: (x, y), unit });
            }
            // Optimized Huffman tables (smaller, no loss) make jpeg-encoder write one scan per
            // component, which zune-jpeg (the `image` decoder) misreads when chroma is
            // subsampled. Subsampled photos keep the standard tables (interleaved scan).
            enc.set_optimized_huffman_tables(settings.sampling == (1, 1));
            // Best effort: a profile or EXIF block too big for its segments is left out.
            if let Some(icc) = &meta.icc {
                let _ = enc.add_icc_profile(icc);
            }
            if let Some(exif) = &meta.exif {
                let _ = enc.add_exif_metadata(exif);
            }
            enc.encode(&rgb(img), w16, h16, ColorType::Rgb).map_err(|e| e.to_string())?;
        }
        Encoding::Archive { quality, sampling } => out = archive_jpeg(img, *quality, *sampling, meta)?,
        Encoding::Png => {
            let mut enc = image::codecs::png::PngEncoder::new(&mut out);
            if let Some(icc) = &meta.icc {
                let _ = enc.set_icc_profile(icc.clone());
            }
            if let Some(exif) = &meta.exif {
                let _ = enc.set_exif_metadata(exif.clone());
            }
            let res = match opaque(img) {
                true => enc.write_image(&rgb(img), w, h, image::ExtendedColorType::Rgb8),
                false => enc.write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgba8),
            };
            res.map_err(|e| e.to_string())?;
        }
        Encoding::Webp { lossless, quality } => {
            let alpha = !opaque(img);
            let pixels = if alpha { img.as_raw().clone() } else { rgb(img) };
            let enc = match alpha {
                true => webp::Encoder::from_rgba(&pixels, w, h),
                false => webp::Encoder::from_rgb(&pixels, w, h),
            };
            let encoded = match lossless {
                true => enc.encode_lossless(),
                false => enc.encode(*quality as f32),
            };
            out =
                meta::webp_with_metadata(&encoded, meta.icc.as_deref(), meta.exif.as_deref(), alpha, (w, h))
                    .ok_or("libwebp wrote an unreadable file")?;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_png_and_jpeg() {
        let dir = tempfile::tempdir().unwrap();
        let img = RgbaImage::from_pixel(8, 4, image::Rgba([200, 100, 50, 255]));
        for name in ["a.png", "b.JPG"] {
            let p = dir.path().join(name);
            save(&img, &p, OutFormat::from_path(&p, 90).unwrap()).unwrap();
            let back = image::open(&p).unwrap().into_rgba8();
            assert_eq!(back.dimensions(), (8, 4));
            let px = back.get_pixel(3, 2).0;
            assert!(px.iter().zip([200, 100, 50]).all(|(a, b)| a.abs_diff(b) <= 3), "{px:?}");
        }
        assert!(OutFormat::from_path(Path::new("x.tiff"), 90).is_err());
    }

    #[test]
    fn jpeg_flattens_alpha_onto_white() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.jpg");
        let img = RgbaImage::from_pixel(8, 8, image::Rgba([0, 0, 0, 0]));
        save(&img, &p, OutFormat::Jpeg { quality: 95 }).unwrap();
        let px = image::open(&p).unwrap().into_rgb8().get_pixel(4, 4).0;
        assert!(px.iter().all(|c| *c >= 250), "{px:?}");
    }
}
