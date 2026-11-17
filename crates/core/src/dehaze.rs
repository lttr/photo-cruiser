//! The Dehaze tool's pixel math: removes haze, like darktable's haze removal at a strength
//! 0–1 (opens at 0.2), with its distance fixed at darktable's default 0.2.
//!
//! Haze is light from the air in front of the scene: in linear light, a pixel is
//! `scene · t + airlight · (1 − t)`, where the transmission `t` falls with the distance.
//! The dark channel prior (He, Sun and Tang, 2009) estimates it: a haze-free scene has a
//! dark colour channel nearly everywhere, so how bright a pixel's darkest channel is
//! (relative to the airlight) measures the haze in front of it. Like darktable:
//!
//! - The airlight is the average colour of the brightest 5 % (by R + G + B) of the 5 %
//!   haziest pixels (by dark channel).
//! - `t = 1 − strength · dark / airlight`, smoothed edge-aware ([`crate::smooth::guided`]),
//!   so texture is dehazed by its area while a ridge against the sky stays sharp.
//! - Distance limits `t` from below, so far haze is not all removed: `t_min =
//!   level^(1.125 · distance)`, where `level` is the haziest 5 %'s dark channel.
//! - The scene is `(pixel − airlight) / max(t, t_min) + airlight`.
//!
//! Unlike darktable, the dark channel is taken per pixel instead of as the minimum over a
//! window: darktable's window spreads a dark object's low haze a few pixels into the sky
//! beside it, a haze halo that its colour-guided filter hides and the self-guided one here
//! wouldn't.

use crate::smooth::{Plane, guided};
use crate::warmth::{encode, linear_lut};
use image::RgbaImage;

/// darktable's distance, fixed: how far into the scene haze is removed (1: all of it).
const DISTANCE: f32 = 0.2;
/// The haziest pixels, and the brightest of them, are the top 5 %.
const QUANTILE: f32 = 0.95;
/// The filter's radius as a share of the longer side: about darktable's 9 pixels on a
/// 4000-pixel photo.
const RADIUS: f32 = 0.0025;
/// Variations in the dark channel (0–1 of the airlight) under about this are averaged,
/// stronger ones kept as edges.
const EDGE: f32 = 0.1;

/// Pixel `i` of `img` in linear light.
fn linear(img: &RgbaImage, i: usize) -> [f32; 3] {
    let (lut, px) = (linear_lut(), &img.as_raw()[4 * i..4 * i + 3]);
    std::array::from_fn(|c| lut[px[c] as usize])
}

/// The airlight (linear RGB) of `img`, whose pixels' darkest channels (linear) are
/// `dark`, and the haziest 5 %'s dark channel.
pub fn airlight(img: &RgbaImage, dark: &[f32]) -> ([f32; 3], f32) {
    let at = |v: &mut Vec<f32>| {
        let i = ((v.len() as f32 * QUANTILE) as usize).min(v.len() - 1);
        *v.select_nth_unstable_by(i, f32::total_cmp).1
    };
    let level = at(&mut dark.to_vec());
    let hazy: Vec<usize> = (0..dark.len()).filter(|i| dark[*i] >= level).collect();
    let sum = |i: &usize| linear(img, *i).iter().sum::<f32>();
    let bright = at(&mut hazy.iter().map(sum).collect());
    let (mut total, mut n) = ([0f64; 3], 0u32);
    for i in hazy.iter().filter(|i| sum(i) >= bright) {
        linear(img, *i).iter().zip(&mut total).for_each(|(v, t)| *t += f64::from(*v));
        n += 1;
    }
    (total.map(|t| (t / f64::from(n)) as f32), level)
}

/// The scene behind `pixel` (linear), seen through transmission `t` of `air`.
pub fn clear(pixel: f32, air: f32, t: f32) -> f32 {
    ((pixel - air) / t + air).clamp(0.0, 1.0)
}

/// Apply the tool at `strength` to `img` in place; alpha stays.
pub fn apply(img: &mut RgbaImage, strength: f32) {
    if strength == 0.0 || img.width() == 0 || img.height() == 0 {
        return;
    }
    let (w, h) = (img.width() as usize, img.height() as usize);
    // Linear light through the table as it's needed: no full-size copy of it.
    let lut = linear_lut();
    let dark: Vec<f32> = img.pixels().map(|px| lut[px.0[0].min(px.0[1]).min(px.0[2]) as usize]).collect();
    let (air, level) = airlight(img, &dark);
    if air.iter().any(|a| *a <= 1e-4) {
        return; // A black photo: no haze to measure.
    }
    drop(dark);
    let haze: Vec<f32> = img
        .pixels()
        .map(|px| (0..3).map(|c| lut[px.0[c] as usize] / air[c]).fold(f32::MAX, f32::min))
        .collect();
    let r = ((w.max(h) as f32 * RADIUS).round() as usize).max(2);
    let haze = guided(&Plane::new(w, h, haze), r, EDGE * EDGE);
    let t_min = level.powf(1.125 * DISTANCE).clamp(1.0 / 1024.0, 1.0);
    for (px, d) in img.pixels_mut().zip(haze.data) {
        let t = (1.0 - strength * d).max(t_min);
        for c in 0..3 {
            px.0[c] = encode(clear(lut[px.0[c] as usize], air[c], t));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adjust::{linear_to_srgb, srgb_to_linear, to_byte};
    use image::Rgba;

    const AIR: [f32; 3] = [0.55, 0.58, 0.63];

    /// Sky (pure airlight) on top; below, the same textured hill seen through transmission
    /// 0.3 (far, x < 100), 0.6 and 0.9 (near, x ≥ 200).
    fn hills() -> RgbaImage {
        RgbaImage::from_fn(300, 200, |x, y| {
            let t = if y < 60 { 0.0 } else { [0.3, 0.6, 0.9][x as usize / 100] };
            let tex = if (x / 2 + y / 2) % 2 == 0 { 0.06 } else { 0.0 };
            let scene = [0.05 + tex, 0.12 + tex, 0.04 + tex];
            let v: [u8; 3] =
                std::array::from_fn(|c| to_byte(linear_to_srgb(scene[c] * t + AIR[c] * (1.0 - t))));
            Rgba([v[0], v[1], v[2], 255])
        })
    }

    #[test]
    fn airlight_is_the_brightest_haze() {
        let img = hills();
        let lin: Vec<[f32; 3]> = img
            .pixels()
            .map(|p| std::array::from_fn(|c| srgb_to_linear(f32::from(p.0[c]) / 255.0)))
            .collect();
        let dark: Vec<f32> = lin.iter().map(|p| p[0].min(p[1]).min(p[2])).collect();
        let (air, level) = airlight(&img, &dark);
        for c in 0..3 {
            assert!((air[c] - AIR[c]).abs() < 0.01, "{air:?}");
        }
        assert!((level - 0.55).abs() < 0.01, "{level}");
    }

    #[test]
    fn clear_inverts_haze() {
        // scene 0.1 through t 0.5 of air 0.8: 0.45.
        assert!((clear(0.45, 0.8, 0.5) - 0.1).abs() < 1e-6);
        assert_eq!(clear(0.8, 0.8, 0.3), 0.8, "the airlight itself stays");
        assert_eq!(clear(0.0, 0.8, 0.5), 0.0, "clamped at black");
    }

    #[test]
    fn lifts_the_veil_more_where_hazier_up_to_the_distance() {
        let orig = hills();
        let at = |s: f32| {
            let mut img = orig.clone();
            apply(&mut img, s);
            img
        };
        let (img, strong) = (at(0.2), at(1.0));
        let g = |img: &RgbaImage, x, y| img.get_pixel(x, y).0[1];
        // How far a pixel is from the airlight, in linear light, against the original:
        // 1 / t where it was seen through t.
        let lift = |img: &RgbaImage, x| {
            let lin = |v: u8| srgb_to_linear(f32::from(v) / 255.0);
            (AIR[1] - lin(g(img, x, 150))) / (AIR[1] - lin(g(&orig, x, 150)))
        };
        let (far, mid, near) = (lift(&img, 50), lift(&img, 150), lift(&img, 250));
        assert!(far > mid && mid > near && near > 1.02, "far {far}, middle {mid}, near {near}");
        // The distance (0.2) caps the far hill's lift at 1 / 0.55^0.225 = 1.14.
        assert!((1.1..1.18).contains(&far), "{far}");
        assert!(g(&strong, 50, 150).abs_diff(g(&img, 50, 150)) <= 1, "far: no more at 1");
        assert!(g(&strong, 250, 150) + 3 < g(&img, 250, 150), "near: more at 1");
        assert!(g(&orig, 150, 20).abs_diff(g(&img, 150, 20)) <= 1, "sky as it was");
        assert_eq!(img.get_pixel(0, 0).0[3], 255);
    }

    #[test]
    fn no_haze_at_the_sky_edge() {
        // Up to the ridge (y 60), the sky stays the airlight.
        let orig = hills();
        let mut img = orig.clone();
        apply(&mut img, 1.0);
        for y in 50..60 {
            let (now, was) = (img.get_pixel(150, y).0, orig.get_pixel(150, y).0);
            assert!(now.iter().zip(was).all(|(a, b)| a.abs_diff(b) <= 2), "y {y}: {now:?} vs {was:?}");
        }
    }

    #[test]
    fn zero_flat_and_black_are_identity() {
        let mut img = hills();
        apply(&mut img, 0.0);
        assert_eq!(img, hills());
        let mut flat = RgbaImage::from_pixel(32, 32, Rgba([90, 120, 150, 200]));
        apply(&mut flat, 1.0);
        assert!(flat.get_pixel(5, 5).0.iter().zip([90, 120, 150, 200]).all(|(a, b)| a.abs_diff(b) <= 1));
        let mut black = RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 255]));
        apply(&mut black, 1.0);
        assert_eq!(black.get_pixel(1, 1).0, [0, 0, 0, 255]);
        apply(&mut RgbaImage::new(0, 0), 1.0);
    }
}
