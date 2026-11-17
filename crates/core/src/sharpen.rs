//! The Sharpen tool's pixel math: an unsharp mask, like darktable's sharpen at its
//! defaults (radius 2, threshold 0.5), with the amount on the slider (0–1, opens at 0.5).
//!
//! The lightness (luminance, sRGB-encoded: perceptual, like darktable's Lab L) is blurred
//! by a Gaussian of σ = 2 source pixels, cut at 2.5 σ, as darktable does. The detail
//! (lightness − blur) beyond the threshold is added back × the amount, as a gain on linear
//! RGB so the hue stays.
//!
//! The radius is in pixels of the full-size photo: an image rendered at a `scale` of it
//! (a downscaled preview) is blurred by `2 × scale` pixels, so it sharpens what a
//! downscaled full-size save would show.

use crate::adjust::{lightness, relight};
use crate::smooth::{Plane, gaussian};
use image::RgbaImage;

/// The blur's σ, in pixels of the full-size photo (darktable's radius).
const RADIUS: f32 = 2.0;
/// Detail under this (lightness 0–1; darktable's 0.5 in L 0–100) is left alone, so flat
/// areas and noise don't get grainier.
const THRESHOLD: f32 = 0.005;

/// The change in lightness of a pixel `detail` off its blur, at `amount`.
pub fn change(detail: f32, amount: f32) -> f32 {
    detail.signum() * (detail.abs() - THRESHOLD).max(0.0) * amount
}

/// Apply the tool at `amount` to `img`, rendered at `scale` of the full-size photo, in
/// place; alpha stays.
pub fn apply(img: &mut RgbaImage, amount: f32, scale: f32) {
    let sigma = RADIUS * scale;
    if amount == 0.0 || sigma.is_nan() || sigma <= 0.0 || img.width() == 0 || img.height() == 0 {
        return;
    }
    let (w, h) = (img.width() as usize, img.height() as usize);
    let (luma, light) = lightness(img);
    let light = Plane::new(w, h, light);
    let blurred = gaussian(&light, sigma);
    relight(img, &luma, |i| {
        let l = light.data[i];
        l + change(l - blurred.data[i], amount)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn change_above_the_threshold() {
        assert_eq!(change(0.004, 1.0), 0.0, "under the threshold");
        assert_eq!(change(-0.005, 1.0), 0.0);
        assert!((change(0.105, 0.5) - 0.05).abs() < 1e-6);
        assert!((change(-0.105, 1.0) + 0.1).abs() < 1e-6);
        assert_eq!(change(0.3, 0.0), 0.0, "amount 0 changes nothing");
    }

    /// A soft edge from `dark` to `bright`, `ramp` pixels wide, in the middle of `w`.
    fn edge(w: u32, ramp: u32, dark: u8, bright: u8) -> RgbaImage {
        RgbaImage::from_fn(w, 20, |x, _| {
            let t = ((x as f32 - (w - ramp) as f32 / 2.0) / ramp as f32).clamp(0.0, 1.0);
            let v = (dark as f32 + t * (bright as f32 - dark as f32)).round() as u8;
            Rgba([v, v, v, 255])
        })
    }

    #[test]
    fn edges_get_crisper_flat_stays() {
        let orig = edge(100, 6, 60, 180);
        let mut img = orig.clone();
        apply(&mut img, 1.0, 1.0);
        let at = |img: &RgbaImage, x: u32| i32::from(img.get_pixel(x, 10).0[0]);
        // The dark side of the edge darker, the bright side brighter.
        assert!(at(&img, 46) < at(&orig, 46) - 3, "{} vs {}", at(&img, 46), at(&orig, 46));
        assert!(at(&img, 53) > at(&orig, 53) + 3, "{} vs {}", at(&img, 53), at(&orig, 53));
        // Far from it, nothing changes.
        assert_eq!((at(&img, 5), at(&img, 95)), (60, 180));
        assert_eq!(img.get_pixel(0, 0).0[3], 255);
    }

    #[test]
    fn zero_and_flat_are_identity() {
        let orig = edge(100, 6, 60, 180);
        let mut img = orig.clone();
        apply(&mut img, 0.0, 1.0);
        assert_eq!(img, orig);
        let mut flat = RgbaImage::from_pixel(32, 32, Rgba([90, 120, 150, 200]));
        apply(&mut flat, 1.0, 1.0);
        assert_eq!(flat.get_pixel(5, 5).0, [90, 120, 150, 200]);
        apply(&mut RgbaImage::new(0, 0), 1.0, 1.0);
    }

    /// Sharpening a preview at half size looks like the full-size result, halved: the
    /// radius halves with it. At the full-size radius, the preview would be sharpened
    /// far wider than the save.
    #[test]
    fn radius_scales_with_render_size() {
        let full = edge(400, 8, 60, 180);
        let half = |img: &RgbaImage| image::imageops::thumbnail(img, img.width() / 2, img.height() / 2);
        let mut saved = full.clone();
        apply(&mut saved, 1.0, 1.0);
        let saved = half(&saved);
        let diff = |scale: f32| {
            let mut preview = half(&full);
            apply(&mut preview, 1.0, scale);
            (0..preview.width())
                .map(|x| preview.get_pixel(x, 5).0[0].abs_diff(saved.get_pixel(x, 5).0[0]))
                .max()
                .unwrap()
        };
        assert!(diff(0.5) <= 3, "scaled radius: {}", diff(0.5));
        assert!(diff(1.0) > diff(0.5) + 3, "full radius: {} vs {}", diff(1.0), diff(0.5));
    }
}
