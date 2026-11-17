//! The Warmth tool's pixel math: a white balance along the colour temperature of light.
//!
//! Warmth `w` adapts the photo, in linear light, as if its light were `w × 100` mired
//! warmer (positive) or cooler (negative) than daylight: von Kries scaling of Bradford cone
//! responses, by the ratio between the white of a black body at 6504 K and 100 mired
//! warmer (3941 K), raised to the power `w`. So −w undoes +w, and 0 is the identity. White
//! keeps its luminance; channels that go above white clip.
//!
//! The tool opens at grey-world white balance ([`grey_world`]): the warmth that brings the
//! photo's average colour closest to neutral grey. A green or magenta cast stays, since
//! one slider moves only between amber and blue.

use crate::adjust::{linear_to_srgb, srgb_to_linear};
use image::RgbaImage;
use std::sync::OnceLock;

/// Mired (10⁶ / kelvin) per unit of warmth.
pub const MIRED_PER_UNIT: f32 = 100.0;
/// The colour temperature of warmth 0: D65's correlated colour temperature.
const NEUTRAL_K: f32 = 6504.0;

/// Linear sRGB → CIE XYZ (D65).
const RGB_TO_XYZ: [[f32; 3]; 3] = [
    [0.412_456_4, 0.357_576_1, 0.180_437_5],
    [0.212_672_9, 0.715_152_2, 0.072_175],
    [0.019_333_9, 0.119_192, 0.950_304_1],
];
/// CIE XYZ → Bradford cone responses.
const BRADFORD: [[f32; 3]; 3] =
    [[0.8951, 0.2664, -0.1614], [-0.7502, 1.7135, 0.0367], [0.0389, -0.0685, 1.0296]];

type Mat = [[f32; 3]; 3];

fn mul(m: &Mat, v: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| m[r][0] * v[0] + m[r][1] * v[1] + m[r][2] * v[2])
}

fn matmul(a: &Mat, b: &Mat) -> Mat {
    std::array::from_fn(|r| std::array::from_fn(|c| (0..3).map(|k| a[r][k] * b[k][c]).sum()))
}

fn inverse(m: &Mat) -> Mat {
    let cof = |r: usize, c: usize| {
        let (r1, r2) = ((r + 1) % 3, (r + 2) % 3);
        let (c1, c2) = ((c + 1) % 3, (c + 2) % 3);
        m[r1][c1] * m[r2][c2] - m[r1][c2] * m[r2][c1]
    };
    let det: f32 = (0..3).map(|c| m[0][c] * cof(0, c)).sum();
    // The adjugate is the transposed cofactor matrix.
    std::array::from_fn(|r| std::array::from_fn(|c| cof(c, r) / det))
}

/// The chromaticity of a black body at `k` kelvin, 1667–25000 K (Kim et al. 2002).
fn planckian_xy(k: f32) -> [f32; 2] {
    let (k, k2, k3) = (k as f64, (k as f64).powi(2), (k as f64).powi(3));
    let x = if k <= 4000.0 {
        -0.266_123_9e9 / k3 - 0.234_358_9e6 / k2 + 0.877_695_6e3 / k + 0.179_910
    } else {
        -3.025_846_9e9 / k3 + 2.107_037_9e6 / k2 + 0.222_634_7e3 / k + 0.240_390
    };
    let (x2, x3) = (x * x, x * x * x);
    let y = if k <= 2222.0 {
        -1.106_381_4 * x3 - 1.348_110_20 * x2 + 2.185_558_32 * x - 0.202_196_83
    } else if k <= 4000.0 {
        -0.954_947_6 * x3 - 1.374_185_93 * x2 + 2.091_370_15 * x - 0.167_488_67
    } else {
        3.081_758_0 * x3 - 5.873_386_70 * x2 + 3.751_129_97 * x - 0.370_014_83
    };
    [x as f32, y as f32]
}

/// Bradford cone responses of the colour `xy` at Y = 1.
fn cones(xy: [f32; 2]) -> [f32; 3] {
    let [x, y] = xy;
    mul(&BRADFORD, [x / y, 1.0, (1.0 - x - y) / y])
}

/// log₂ of the cone gains for warmth 1: the white of 100 mired warmer than neutral over
/// the neutral white.
fn log_gains() -> &'static [f32; 3] {
    static G: OnceLock<[f32; 3]> = OnceLock::new();
    G.get_or_init(|| {
        let warm = 1e6 / (1e6 / NEUTRAL_K + MIRED_PER_UNIT);
        let (n, w) = (cones(planckian_xy(NEUTRAL_K)), cones(planckian_xy(warm)));
        std::array::from_fn(|i| (w[i] / n[i]).log2())
    })
}

fn luminance(rgb: [f32; 3]) -> f32 {
    (0..3).map(|i| RGB_TO_XYZ[1][i] * rgb[i]).sum()
}

/// The linear-sRGB matrix that scales the cones by `2^(g × w)`.
fn unscaled(g: &[f32; 3], w: f32) -> Mat {
    let (to, from) = to_cones();
    let scale: Mat =
        std::array::from_fn(|r| std::array::from_fn(|c| if r == c { (g[r] * w).exp2() } else { 0.0 }));
    matmul(from, &matmul(&scale, to))
}

/// Linear sRGB → Bradford cones, and back.
fn to_cones() -> &'static (Mat, Mat) {
    static M: OnceLock<(Mat, Mat)> = OnceLock::new();
    M.get_or_init(|| {
        let m = matmul(&BRADFORD, &RGB_TO_XYZ);
        (m, inverse(&m))
    })
}

/// The linear-sRGB matrix for warmth `w`, scaled so white keeps its luminance. So
/// `matrix(-w)` is its inverse up to a brightness factor near 1.
pub fn matrix(w: f32) -> Mat {
    let m = unscaled(log_gains(), w);
    let y = luminance(mul(&m, [1.0; 3]));
    m.map(|row| row.map(|v| v / y))
}

/// sRGB-encoded byte → linear light.
pub(crate) fn linear_lut() -> &'static [f32; 256] {
    static LUT: OnceLock<[f32; 256]> = OnceLock::new();
    LUT.get_or_init(|| std::array::from_fn(|i| srgb_to_linear(i as f32 / 255.0)))
}

pub(crate) const ENCODE_LEN: usize = 8192;

/// Linear light at `i / (ENCODE_LEN − 1)` → sRGB-encoded byte.
pub(crate) fn encode_lut() -> &'static [u8] {
    static LUT: OnceLock<Vec<u8>> = OnceLock::new();
    LUT.get_or_init(|| {
        (0..ENCODE_LEN)
            .map(|i| (linear_to_srgb(i as f32 / (ENCODE_LEN - 1) as f32) * 255.0).round() as u8)
            .collect()
    })
}

/// Apply warmth `w` to `img` in place; alpha stays.
pub fn apply(img: &mut RgbaImage, w: f32) {
    if w == 0.0 {
        return;
    }
    let m = matrix(w);
    let (lin, enc) = (linear_lut(), encode_lut());
    let top = (ENCODE_LEN - 1) as f32;
    for px in img.pixels_mut() {
        let rgb = [lin[px.0[0] as usize], lin[px.0[1] as usize], lin[px.0[2] as usize]];
        let out = mul(&m, rgb);
        for (c, v) in px.0[..3].iter_mut().zip(out) {
            *c = enc[(v.clamp(0.0, 1.0) * top).round() as usize];
        }
    }
}

/// Saturation, (max − min) / max of linear R, G, B, below which a pixel counts as near
/// grey once balanced.
const NEAR_GREY: f32 = 0.3;

/// Grey-world white balance, on the near-grey pixels: the warmth that brings the mean
/// linear colour closest to neutral (least squares on log cone responses, ignoring overall
/// brightness). Plain grey world turns a photo of mostly warm things (wood, skin, a red
/// dress) blue, so after a first estimate on every pixel, only those near grey under the
/// estimate count, a few rounds. Pixels count by alpha; clipped (a channel at 250 or more)
/// and near-black (all below 8) ones don't count, as they hold no colour of the light.
/// Unclamped; 0 without colour to go by.
pub fn grey_world(img: &RgbaImage) -> f32 {
    let lin = linear_lut();
    // At most about 2¹⁶ samples: plenty for an average.
    let stride = (img.len() / 4 / 65_536).max(1);
    let samples: Vec<([f32; 3], f32)> = img
        .pixels()
        .step_by(stride)
        .filter_map(|px| {
            let [r, g, b, a] = px.0;
            let max = r.max(g).max(b);
            (a > 0 && (8..250).contains(&max))
                .then(|| ([lin[r as usize], lin[g as usize], lin[b as usize]], f32::from(a) / 255.0))
        })
        .collect();
    let total: f32 = samples.iter().map(|(_, a)| a).sum();
    let Some(mut w) = balance(samples.iter().copied()) else { return 0.0 };
    for _ in 0..4 {
        let m = matrix(w);
        let grey = samples.iter().copied().filter(|(rgb, _)| {
            let [r, g, b] = mul(&m, *rgb);
            let max = r.max(g).max(b);
            max > 0.0 && (max - r.min(g).min(b)) / max < NEAR_GREY
        });
        // Too little near grey to go by: keep the estimate.
        if grey.clone().map(|(_, a)| a).sum::<f32>() < 0.05 * total {
            break;
        }
        match balance(grey) {
            Some(next) if (next - w).abs() > 0.005 => w = next,
            Some(next) => return next,
            None => break,
        }
    }
    w
}

/// The warmth that makes the alpha-weighted mean of `samples` (linear sRGB) neutral, if
/// there is one.
fn balance(samples: impl Iterator<Item = ([f32; 3], f32)>) -> Option<f32> {
    let (mut sum, mut weight) = ([0f64; 3], 0f64);
    for (rgb, a) in samples {
        weight += f64::from(a);
        for (s, c) in sum.iter_mut().zip(rgb) {
            *s += f64::from(a * c);
        }
    }
    if weight == 0.0 {
        return None;
    }
    let (to, _) = to_cones();
    let mean = mul(to, sum.map(|s| (s / weight) as f32));
    let white = mul(to, [1.0; 3]);
    if mean.iter().any(|v| *v <= 1e-6) {
        return None;
    }
    // Remove the common part (brightness) from the cast and the gains, then project.
    let centre = |v: [f32; 3]| {
        let m = (v[0] + v[1] + v[2]) / 3.0;
        v.map(|x| x - m)
    };
    let cast = centre(std::array::from_fn(|i| (mean[i] / white[i]).log2()));
    let gains = centre(*log_gains());
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    Some(-dot(cast, gains) / dot(gains, gains))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn flat(px: [u8; 3]) -> RgbaImage {
        RgbaImage::from_pixel(8, 8, Rgba([px[0], px[1], px[2], 255]))
    }

    #[test]
    fn identity_at_zero_and_inverse() {
        // Inverse up to brightness: a multiple of the identity, near 1.
        let m = matmul(&matrix(0.7), &matrix(-0.7));
        for (r, row) in m.iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                let want = if r == c { m[0][0] } else { 0.0 };
                assert!((v - want).abs() < 1e-4, "{m:?}");
            }
        }
        assert!((m[0][0] - 1.0).abs() < 0.02, "{m:?}");
        let mut img = flat([10, 120, 240]);
        apply(&mut img, 0.0);
        assert_eq!(img, flat([10, 120, 240]));
    }

    #[test]
    fn warmer_is_amber_cooler_is_blue() {
        let grey = |w: f32| {
            let mut img = flat([119, 119, 119]);
            apply(&mut img, w);
            img.get_pixel(0, 0).0
        };
        let [r, g, b, a] = grey(1.0);
        assert!(r > g && g > b && a == 255, "warm: {:?}", grey(1.0));
        let [r, g, b, _] = grey(-1.0);
        assert!(b > g && g > r, "cool: {:?}", grey(-1.0));
        // Luminance about kept.
        let luma = |p: [u8; 4]| {
            let l: Vec<f32> = p[..3].iter().map(|c| srgb_to_linear(f32::from(*c) / 255.0)).collect();
            0.2126 * l[0] + 0.7152 * l[1] + 0.0722 * l[2]
        };
        for w in [-2.0, -1.0, 1.0, 2.0] {
            assert!((luma(grey(w)) - luma([119; 4])).abs() < 0.01, "{w}: {:?}", grey(w));
        }
        // 100 mired warmer than daylight is about 3940 K: sRGB ~ (255, 206, 158) scaled.
        let [r, g, b, _] = grey(1.0);
        let (rg, rb) = (f32::from(g) / f32::from(r), f32::from(b) / f32::from(r));
        assert!((0.75..0.9).contains(&rg) && (0.5..0.7).contains(&rb), "{:?}", grey(1.0));
    }

    #[test]
    fn grey_world_removes_a_cast() {
        assert_eq!(grey_world(&flat([119, 119, 119])), 0.0);
        // A grey scene under light 1 unit warmer, and cooler: the opening undoes it.
        for cast in [1.3, -0.8] {
            // Mid-tones, so that no channel clips under the cast.
            let mut scene = RgbaImage::from_fn(64, 16, |x, y| {
                let v = (50 + x * 2) as u8;
                if y < 8 { Rgba([v, v, v, 255]) } else { Rgba([v / 2 + 50, v, 150 - v / 4, 255]) }
            });
            let original = scene.clone();
            let before = grey_world(&scene);
            apply(&mut scene, cast);
            let w = grey_world(&scene);
            assert!((w - (before - cast)).abs() < 0.05, "cast {cast}: {w} vs {}", before - cast);
            apply(&mut scene, w - before);
            let back = scene.get_pixel(10, 0).0;
            let orig = original.get_pixel(10, 0).0;
            assert!(back.iter().zip(orig).all(|(a, b)| a.abs_diff(b) <= 3), "{back:?} vs {orig:?}");
        }
        // Nothing to go by: blank, clipped white, black, transparent.
        assert_eq!(grey_world(&RgbaImage::new(0, 0)), 0.0);
        assert_eq!(grey_world(&flat([255, 255, 255])), 0.0);
        assert_eq!(grey_world(&flat([0, 0, 0])), 0.0);
        assert_eq!(grey_world(&RgbaImage::from_pixel(4, 4, Rgba([200, 100, 50, 0]))), 0.0);
    }
}
