//! The Vignette tool's pixel math: darker (−) or brighter (+) corners, −1…1, like
//! darktable's vignette with its shape fixed at the defaults: fall-off start 80 %,
//! fall-off radius 50 %, shape 1 (a circle), width/height ratio 1, centred.
//!
//! Distances are measured from the image's centre in units of half its longer side, so
//! the vignette fits whatever image it gets: after the crop, the cropped frame. Inside
//! 0.8 nothing changes; from there the weight rises smoothly (half a cosine) to 1 at 1.3.
//! A darker vignette scales linear light by `1 + weight·value`, a brighter one adds
//! `weight·value`, as darktable does.

use crate::warmth::{ENCODE_LEN, encode_lut, linear_lut};
use image::RgbaImage;

/// Where the fall-off starts, in half the longer side.
const START: f32 = 0.8;
/// How far the fall-off runs, in half the longer side.
const RADIUS: f32 = 0.5;

/// The vignette's weight, 0–1, at `d` half-longer-sides from the centre.
pub fn weight(d: f32) -> f32 {
    let t = ((d - START) / RADIUS).clamp(0.0, 1.0);
    0.5 - (std::f32::consts::PI * t).cos() / 2.0
}

/// Apply the tool at `value` (−1…1) to `img` in place; alpha stays.
pub fn apply(img: &mut RgbaImage, value: f32) {
    let (w, h) = img.dimensions();
    if value == 0.0 || w == 0 || h == 0 {
        return;
    }
    let lin = linear_lut();
    let enc = encode_lut();
    let top = (ENCODE_LEN - 1) as f32;
    let scale = 2.0 / w.max(h) as f32;
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let d = ((x as f32 + 0.5 - cx) * scale).hypot((y as f32 + 0.5 - cy) * scale);
        let k = weight(d);
        if k == 0.0 {
            continue;
        }
        for c in &mut px.0[..3] {
            let v = lin[*c as usize];
            let v = if value < 0.0 { v * (1.0 + k * value) } else { v + k * value };
            *c = enc[(v.clamp(0.0, 1.0) * top).round() as usize];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn grey(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba([150, 150, 150, 200]))
    }

    #[test]
    fn weight_falls_off_smoothly() {
        assert_eq!(weight(0.0), 0.0);
        assert_eq!(weight(0.8), 0.0);
        assert!((weight(1.05) - 0.5).abs() < 1e-6);
        assert_eq!(weight(1.3), 1.0);
        assert_eq!(weight(2.0), 1.0);
        assert!(weight(0.9) < weight(1.0) && weight(1.0) < weight(1.2));
    }

    #[test]
    fn darker_corners_centre_untouched() {
        let mut img = grey(300, 200);
        apply(&mut img, -0.5);
        let at = |x, y| img.get_pixel(x, y).0;
        assert_eq!(at(150, 100), [150, 150, 150, 200], "centre");
        // Top and bottom edge at the centre: 0.67 of half the longer side, inside the start.
        assert_eq!(at(150, 0), [150, 150, 150, 200]);
        // Corner: d = 1.2, weight 0.9, linear × 0.55.
        let corner = at(0, 0);
        let expected = (crate::adjust::linear_to_srgb(crate::adjust::srgb_to_linear(150.0 / 255.0) * 0.55)
            * 255.0)
            .round() as u8;
        assert!(corner[0].abs_diff(expected) <= 1, "{corner:?} vs {expected}");
        assert_eq!(corner[3], 200, "alpha stays");
        // Symmetric, and darker towards the corner.
        assert_eq!(at(0, 0), at(299, 199));
        assert!(at(0, 100)[0] > corner[0] && at(0, 100)[0] < 150, "{:?}", at(0, 100));
        // Stronger is darker.
        let mut strong = grey(300, 200);
        apply(&mut strong, -1.0);
        assert!(strong.get_pixel(0, 0).0[0] < corner[0]);
    }

    #[test]
    fn brighter_corners() {
        let mut img = grey(200, 200);
        apply(&mut img, 1.0);
        assert_eq!(img.get_pixel(0, 0).0, [255, 255, 255, 200], "corner +1: white");
        assert_eq!(img.get_pixel(100, 100).0[0], 150);
    }

    #[test]
    fn fits_the_frame_it_gets() {
        // The same vignette on a small and a large image: relative to the frame.
        let (mut small, mut large) = (grey(60, 40), grey(600, 400));
        apply(&mut small, -0.5);
        apply(&mut large, -0.5);
        assert!(small.get_pixel(0, 0).0[0].abs_diff(large.get_pixel(0, 0).0[0]) <= 2);
        assert!(small.get_pixel(5, 20).0[0].abs_diff(large.get_pixel(55, 200).0[0]) <= 2);
    }

    #[test]
    fn zero_is_identity() {
        let mut img = grey(30, 20);
        apply(&mut img, 0.0);
        assert_eq!(img, grey(30, 20));
        apply(&mut RgbaImage::new(0, 0), -0.5);
    }
}
