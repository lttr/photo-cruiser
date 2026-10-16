//! CPU reference pipeline: decoded pixels + edit stack → output pixels. This
//! implementation defines correctness; a GPU preview must match it. The geometry (crop,
//! rotation) comes first, in stack order, then the adjustments ([`crate::adjust`]), then
//! the downscale to `max_side`.

use crate::decode::Decoded;
use crate::edit::{EditStack, Op};
use crate::{Error, Result};
use image::RgbaImage;

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderOpts {
    /// Downscale the result so its longer side is at most this.
    pub max_side: Option<u32>,
}

/// Apply `edits` to `src`. Rectangles are in source pixels and are mapped onto
/// `src.image` if it was decoded downscaled. A crop that is empty after clamping to the
/// image is [`Error::EmptyCrop`]; a partial overlap is clamped.
pub fn render(src: &Decoded, edits: &EditStack, opts: RenderOpts) -> Result<RgbaImage> {
    let k = 1.0 / src.source_scale();
    let mut img = src.image.clone();
    for op in &edits.ops {
        match op {
            Op::Crop(r) => {
                let scaled = crate::Rect::new(r.x * k, r.y * k, r.w * k, r.h * k);
                let p = scaled.to_pixels(img.dimensions()).ok_or(Error::EmptyCrop(*r))?;
                img = image::imageops::crop_imm(&img, p.x, p.y, p.w, p.h).to_image();
            }
            Op::Rotate { quarters } => {
                img = match quarters % 4 {
                    1 => image::imageops::rotate90(&img),
                    2 => image::imageops::rotate180(&img),
                    3 => image::imageops::rotate270(&img),
                    _ => img,
                };
            }
            Op::Adjust { .. } => {}
        }
    }
    crate::adjust::apply_all(&mut img, edits);
    Ok(match opts.max_side {
        Some(m) => {
            Decoded { source_width: img.width(), source_height: img.height(), image: img, format: src.format }
                .downscale(m)
                .image
        }
        None => img,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rect;
    use crate::decode::Format;

    fn gradient(w: u32, h: u32) -> Decoded {
        let image = RgbaImage::from_fn(w, h, |x, y| image::Rgba([x as u8, y as u8, 0, 255]));
        Decoded { image, format: Format::Png, source_width: w, source_height: h }
    }

    #[test]
    fn crop_exact_pixels() {
        let edits = EditStack::default().with(Op::Crop(Rect::new(10.0, 20.0, 30.0, 40.0)));
        let out = render(&gradient(100, 100), &edits, RenderOpts::default()).unwrap();
        assert_eq!(out.dimensions(), (30, 40));
        assert_eq!(out.get_pixel(0, 0).0, [10, 20, 0, 255]);
        assert_eq!(out.get_pixel(29, 39).0, [39, 59, 0, 255]);
    }

    #[test]
    fn rotate_after_crop() {
        let crop = Op::Crop(Rect::new(10.0, 20.0, 30.0, 40.0));
        let turned = |quarters| {
            let edits = EditStack::default().with(crop.clone()).with(Op::Rotate { quarters });
            render(&gradient(100, 100), &edits, RenderOpts::default()).unwrap()
        };
        // Clockwise: the crop's bottom-left corner ends up top-left.
        let cw = turned(1);
        assert_eq!(cw.dimensions(), (40, 30));
        assert_eq!(cw.get_pixel(0, 0).0, [10, 59, 0, 255]);
        let ccw = turned(3);
        assert_eq!(ccw.get_pixel(0, 0).0, [39, 20, 0, 255]);
        let half = turned(2);
        assert_eq!(half.dimensions(), (30, 40));
        assert_eq!(half.get_pixel(0, 0).0, [39, 59, 0, 255]);
    }

    #[test]
    fn adjustments_after_the_geometry() {
        use crate::adjust::Tool;
        // Listed before the crop, still applied after it: same result as crop, then adjust.
        let crop = Op::Crop(Rect::new(10.0, 20.0, 30.0, 40.0));
        let adjust = Op::Adjust { tool: Tool::Exposure, value: 1.0 };
        let edits = EditStack::default().with(adjust).with(crop.clone());
        let out = render(&gradient(100, 100), &edits, RenderOpts::default()).unwrap();
        let mut expected =
            render(&gradient(100, 100), &EditStack::default().with(crop), RenderOpts::default()).unwrap();
        Tool::Exposure.apply(&mut expected, 1.0);
        assert_eq!(out, expected);
        assert_ne!(out.get_pixel(0, 0).0, [10, 20, 0, 255]);
    }

    #[test]
    fn crop_on_downscaled_source() {
        let d = gradient(200, 100).downscale(100);
        assert_eq!(d.image.dimensions(), (100, 50));
        let edits = EditStack::default().with(Op::Crop(Rect::new(100.0, 0.0, 100.0, 100.0)));
        assert_eq!(render(&d, &edits, RenderOpts::default()).unwrap().dimensions(), (50, 50));
    }

    #[test]
    fn max_side_and_partial_crop_clamps() {
        let edits = EditStack::default().with(Op::Crop(Rect::new(-20.0, 40.0, 80.0, 100.0)));
        let out = render(&gradient(100, 60), &edits, RenderOpts { max_side: Some(30) }).unwrap();
        assert_eq!(out.dimensions(), (30, 10)); // clamped to 60x20, then halved
    }

    #[test]
    fn crop_outside_image_is_an_error() {
        for r in [
            Rect::new(500.0, 500.0, 10.0, 10.0),
            Rect::new(-50.0, 0.0, 50.0, 10.0),
            Rect::new(0.0, 0.0, 0.4, 10.0),
        ] {
            let edits = EditStack::default().with(Op::Crop(r));
            let err = render(&gradient(100, 60), &edits, RenderOpts::default()).unwrap_err();
            assert!(matches!(err, Error::EmptyCrop(e) if e == r), "{err:?}");
            assert_eq!(err.kind(), "empty_crop");
        }
    }
}
