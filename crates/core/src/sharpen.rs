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

use crate::adjust::{linear_to_srgb, srgb_to_linear};
use crate::warmth::{ENCODE_LEN, encode_lut, linear_lut};
use image::RgbaImage;

/// The blur's σ, in pixels of the full-size photo (darktable's radius).
const RADIUS: f32 = 2.0;
/// Detail under this (lightness 0–1; darktable's 0.5 in L 0–100) is left alone, so flat
/// areas and noise don't get grainier.
const THRESHOLD: f32 = 0.005;

/// Rec. 709 luminance weights of linear R, G, B.
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// The change in lightness of a pixel `detail` off its blur, at `amount`.
pub fn change(detail: f32, amount: f32) -> f32 {
    detail.signum() * (detail.abs() - THRESHOLD).max(0.0) * amount
}

/// A normalised Gaussian kernel of σ `sigma`, cut at 2.5 σ; `[1.0]` for σ 0.
fn kernel(sigma: f32) -> Vec<f32> {
    let r = (2.5 * sigma).ceil() as i32;
    let k: Vec<f32> = (-r..=r).map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp()).collect();
    let sum: f32 = k.iter().sum();
    if sum.is_finite() && sum > 0.0 { k.iter().map(|v| v / sum).collect() } else { vec![1.0] }
}

/// `data` (`w` × `h`) blurred by the separable `kernel`, the edges clamped.
fn blur(data: &[f32], w: usize, h: usize, kernel: &[f32]) -> Vec<f32> {
    let r = (kernel.len() / 2) as isize;
    let at = |i: isize, len: usize| i.clamp(0, len as isize - 1) as usize;
    let mut rows = vec![0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            rows[y * w + x] = kernel
                .iter()
                .enumerate()
                .map(|(j, k)| k * data[y * w + at(x as isize + j as isize - r, w)])
                .sum();
        }
    }
    let mut out = vec![0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            out[y * w + x] = kernel
                .iter()
                .enumerate()
                .map(|(j, k)| k * rows[at(y as isize + j as isize - r, h) * w + x])
                .sum();
        }
    }
    out
}

/// Apply the tool at `amount` to `img`, rendered at `scale` of the full-size photo, in
/// place; alpha stays.
pub fn apply(img: &mut RgbaImage, amount: f32, scale: f32) {
    let sigma = RADIUS * scale;
    if amount == 0.0 || sigma.is_nan() || sigma <= 0.0 || img.width() == 0 || img.height() == 0 {
        return;
    }
    let (w, h) = (img.width() as usize, img.height() as usize);
    let lin = linear_lut();
    let luma: Vec<f32> =
        img.pixels().map(|px| (0..3).map(|c| LUMA[c] * lin[px.0[c] as usize]).sum()).collect();
    let light: Vec<f32> = luma.iter().map(|y| linear_to_srgb(*y)).collect();
    let blurred = blur(&light, w, h, &kernel(sigma));
    let enc = encode_lut();
    let top = (ENCODE_LEN - 1) as f32;
    for (i, px) in img.pixels_mut().enumerate() {
        let l = light[i];
        let new = (l + change(l - blurred[i], amount)).clamp(0.0, 1.0);
        let k = if luma[i] > 1e-5 { srgb_to_linear(new) / luma[i] } else { 1.0 };
        for c in &mut px.0[..3] {
            *c = enc[((lin[*c as usize] * k).clamp(0.0, 1.0) * top).round() as usize];
        }
    }
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

    #[test]
    fn kernel_is_normalised_and_cut_at_two_and_a_half_sigma() {
        let k = kernel(2.0);
        assert_eq!(k.len(), 11);
        assert!((k.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        assert!(k[5] > k[4] && (k[4] - k[6]).abs() < 1e-7);
        assert_eq!(kernel(0.0), vec![1.0]);
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
