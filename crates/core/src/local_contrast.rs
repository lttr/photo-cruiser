//! The Local contrast tool's pixel math: brings out detail, like darktable's local
//! contrast at its "detail" slider (100 % changes nothing, opens at 125 %, 200 % for an
//! HDR look). Its other controls stay at darktable's defaults (highlights and shadows
//! 50 %, midtone range 0.5), so they aren't modelled.
//!
//! The lightness (luminance, sRGB-encoded: perceptual, like darktable's Lab L) is split
//! into a base, smoothed by an edge-aware filter ([`crate::smooth::guided_fit`]), and the
//! detail on top of it. In lightness, fine texture is small and an edge is big whether
//! it's in the shadows or the highlights; in log luminance, dark texture is as big as an
//! edge.
//!
//! The detail is scaled by the slider, and by `1 − a`, where `a` is the filter's slope:
//! near 1 at an edge, which the filter keeps, near 0 in texture, which it averages. So
//! edges (a window frame, a tree against the sky) get no halo, while texture gets the
//! full boost.

use crate::adjust::{lightness, relight};
use crate::smooth::{Plane, guided_fit};
use image::RgbaImage;

/// The filter's radius as a share of the longer side: detail up to about this size is
/// boosted, larger shapes are left to the other tools.
const RADIUS: f32 = 0.01;
/// Variations in lightness (0–1) under about this count as detail; stronger ones as edges.
const EDGE: f32 = 0.05;
/// No pixel's lightness changes by more than this.
const MAX_CHANGE: f32 = 0.25;

/// The change in lightness of a pixel `detail` off its base, where the filter's slope is
/// `a`, at `amount` (1 changes nothing, 2 doubles texture).
pub fn change(detail: f32, a: f32, amount: f32) -> f32 {
    ((amount - 1.0) * (1.0 - a).clamp(0.0, 1.0) * detail).clamp(-MAX_CHANGE, MAX_CHANGE)
}

/// Apply the tool at `amount` to `img` in place; alpha stays.
pub fn apply(img: &mut RgbaImage, amount: f32) {
    if amount == 1.0 || img.width() == 0 || img.height() == 0 {
        return;
    }
    let (w, h) = (img.width() as usize, img.height() as usize);
    let (luma, light) = lightness(img);
    let r = ((w.max(h) as f32 * RADIUS).round() as usize).max(2);
    let plane = Plane::new(w, h, light);
    let (a, b) = guided_fit(&plane, r, EDGE * EDGE);
    relight(img, &luma, |i| {
        let (l, a) = (plane.data[i], a.data[i]);
        l + change(l - (a * l + b.data[i]), a, amount)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn change_by_amount_and_edge() {
        assert_eq!(change(0.2, 0.0, 1.0), 0.0, "100 % changes nothing");
        assert!((change(0.04, 0.0, 2.0) - 0.04).abs() < 1e-6, "200 % doubles texture");
        assert!((change(-0.04, 0.0, 1.25) + 0.01).abs() < 1e-6);
        assert!(change(0.04, 0.95, 2.0).abs() < 0.0021, "an edge barely changes");
        assert_eq!(change(9.0, 0.0, 2.0), MAX_CHANGE);
    }

    /// A dark side and a bright side at sRGB `dark` and `bright` (the edge at x 150),
    /// each with a fine texture of +6.
    fn step(dark: u8, bright: u8) -> RgbaImage {
        RgbaImage::from_fn(300, 100, |x, y| {
            let tex = if (x / 2 + y / 2) % 2 == 0 { 6 } else { 0 };
            let v = if x < 150 { dark + tex } else { bright + tex };
            Rgba([v, v, v, 255])
        })
    }

    #[test]
    fn boosts_texture_without_halos() {
        // A strong edge (about 3 EV), a medium one (about 1 EV) and a soft one (0.5 EV).
        for (d, b) in [(34, 184), (110, 184), (140, 184)] {
            let orig = step(d, b);
            let mut img = orig.clone();
            apply(&mut img, 2.0);
            let at = |x: u32| i32::from(img.get_pixel(x, 50).0[0]);
            let was = |x: u32| i32::from(orig.get_pixel(x, 50).0[0]);
            // Texture stronger on both sides, far from the edge.
            for x in [40, 250] {
                let (now, before) = ((at(x) - at(x + 2)).abs(), (was(x) - was(x + 2)).abs());
                assert!(now >= before + 3, "{d}/{b}, column {x}: texture {now} vs {before}");
            }
            // A halo is an overshoot: up to the edge, no pixel goes beyond the darkest and
            // brightest texture far from it, on its own side (1 level for rounding).
            let range = |xs: std::ops::Range<u32>| {
                xs.clone().map(at).min().unwrap() - 1..=xs.map(at).max().unwrap() + 1
            };
            let (dark, bright) = (range(20..60), range(240..280));
            for x in 100..200 {
                let side = if x < 150 { &dark } else { &bright };
                assert!(side.contains(&at(x)), "{d}/{b}, column {x}: {} outside {side:?}", at(x));
            }
            assert_eq!(img.get_pixel(0, 0).0[3], 255);
        }
    }

    #[test]
    fn hundred_percent_and_flat_are_identity() {
        let mut img = step(34, 184);
        apply(&mut img, 1.0);
        assert_eq!(img, step(34, 184));
        let mut flat = RgbaImage::from_pixel(32, 32, Rgba([90, 120, 150, 200]));
        apply(&mut flat, 2.0);
        assert_eq!(flat.get_pixel(5, 5).0, [90, 120, 150, 200]);
        apply(&mut RgbaImage::new(0, 0), 2.0);
    }
}
