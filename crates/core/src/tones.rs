//! The Shadows & highlights tool's pixel math: lifts dark areas and pulls back bright ones
//! towards middle grey, like darktable's tone equalizer preset "compress
//! shadows/highlights", by a strength 0–1 (the presets: soft 0.25, medium 0.45, strong
//! 0.65).
//!
//! Each pixel's exposure changes by how far its *area* is from middle grey, in EV: the
//! luminance smoothed by an edge-aware filter ([`crate::smooth::guided`]). So detail
//! inside an area keeps its contrast, and strong edges (a window, the sky over a
//! mountain) leave no halos. Mid-tones barely change: the change grows with the cube of
//! the distance near middle grey and linearly beyond 1 EV. Highlights get half the
//! shadows' change: there is less room above middle grey (about 2.4 EV) than below.

use crate::adjust::{LUMA, MIDDLE_GREY};
use crate::smooth::{Plane, guided};
use crate::warmth::{encode, linear_lut};
use image::RgbaImage;

/// The filter's radius as a share of the longer side: darktable's default smoothing
/// diameter is 5 %.
const RADIUS: f32 = 0.025;
/// Edges stronger than about this many EV are kept by the filter (its `eps` is the square).
const EDGE_EV: f32 = 0.5;
/// Highlights get this share of the shadows' change.
const HIGHLIGHTS: f32 = 0.5;
/// No pixel changes by more than this many EV.
const MAX_EV: f32 = 2.0;

/// The change in EV for an area `d` EV from middle grey at `strength`.
pub fn curve(d: f32, strength: f32) -> f32 {
    let side = if d > 0.0 { HIGHLIGHTS } else { 1.0 };
    (-strength * side * d * d * d / (d * d + 1.0)).clamp(-MAX_EV, MAX_EV)
}

/// Apply the tool at `strength` to `img` in place; alpha stays.
pub fn apply(img: &mut RgbaImage, strength: f32) {
    if strength == 0.0 || img.width() == 0 || img.height() == 0 {
        return;
    }
    let (w, h) = (img.width() as usize, img.height() as usize);
    let lin = linear_lut();
    let grey = MIDDLE_GREY.log2();
    let ev: Vec<f32> = img
        .pixels()
        .map(|px| {
            let y: f32 = (0..3).map(|c| LUMA[c] * lin[px.0[c] as usize]).sum();
            y.max(1e-4).log2() - grey
        })
        .collect();
    let r = ((w.max(h) as f32 * RADIUS).round() as usize).max(1);
    let area = guided(&Plane::new(w, h, ev), r, EDGE_EV * EDGE_EV);
    for (px, d) in img.pixels_mut().zip(area.data) {
        let k = curve(d, strength).exp2();
        for c in &mut px.0[..3] {
            *c = encode(lin[*c as usize] * k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn curve_spares_mid_tones() {
        assert_eq!(curve(0.0, 0.45), 0.0);
        assert!(curve(0.3, 0.45).abs() < 0.02 && curve(-0.3, 0.45).abs() < 0.04, "mid-tones barely change");
        assert!(curve(-4.0, 0.45) > 1.5, "deep shadows lift: {}", curve(-4.0, 0.45));
        assert!((-0.6..-0.4).contains(&curve(2.4, 0.45)), "white pulls back: {}", curve(2.4, 0.45));
        assert_eq!(curve(-9.0, 1.0), MAX_EV);
        assert!(curve(-2.0, 0.65) > curve(-2.0, 0.45), "stronger");
    }

    /// A dark room (sRGB 25, with texture) with a bright window (sRGB 235) on the right.
    fn room() -> RgbaImage {
        RgbaImage::from_fn(200, 100, |x, y| {
            let tex = if (x / 2 + y / 2) % 2 == 0 { 4 } else { 0 };
            let v = if x < 120 { 25 + tex } else { 231 + tex };
            Rgba([v, v, v, 255])
        })
    }

    #[test]
    fn lifts_shadows_without_halos() {
        let mut img = room();
        apply(&mut img, 0.45);
        let at = |x: u32| img.get_pixel(x, 50).0[0];
        assert!(at(20) > 45, "room lifted: {}", at(20));
        assert!(at(180) < 231 && at(180) > 200, "window pulled back a little: {}", at(180));
        // Every column of the room the same, up to the window: no glow along its edge.
        let far = (at(20), at(22));
        for x in 0..120 {
            assert!(
                at(x).abs_diff(far.0).min(at(x).abs_diff(far.1)) <= 3,
                "column {x}: {} vs {far:?}",
                at(x)
            );
        }
        for x in 122..200 {
            assert!(at(x).abs_diff(at(180)).min(at(x).abs_diff(at(182))) <= 3, "column {x}: {}", at(x));
        }
        // Texture kept: the room's 4-level pattern is still there.
        assert!(at(20).abs_diff(at(22)) >= 4, "{} vs {}", at(20), at(22));
        let a = img.get_pixel(0, 0).0[3];
        assert_eq!(a, 255);
    }

    #[test]
    fn zero_is_identity_and_mid_grey_stays() {
        let mut img = room();
        apply(&mut img, 0.0);
        assert_eq!(img, room());
        let mut grey = RgbaImage::from_pixel(16, 16, Rgba([119, 119, 119, 200]));
        apply(&mut grey, 1.0);
        assert_eq!(grey.get_pixel(3, 3).0, [119, 119, 119, 200]);
        apply(&mut RgbaImage::new(0, 0), 0.5);
    }
}
