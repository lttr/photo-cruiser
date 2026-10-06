//! Encoding rendered pixels to files.

use crate::{Error, Result};
use image::RgbaImage;
use image::codecs::jpeg::JpegEncoder;
use std::io::BufWriter;
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

pub fn save(img: &RgbaImage, path: &Path, format: OutFormat) -> Result<()> {
    let file = std::fs::File::create(path).map_err(|e| Error::io(path, e))?;
    let mut out = BufWriter::new(file);
    let res = match format {
        // JPEG has no alpha.
        OutFormat::Jpeg { quality } => image::DynamicImage::ImageRgba8(img.clone())
            .into_rgb8()
            .write_with_encoder(JpegEncoder::new_with_quality(&mut out, quality)),
        OutFormat::Png => img.write_to(&mut out, image::ImageFormat::Png),
    };
    res.map_err(|e| Error::encode(path, e))
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
}
