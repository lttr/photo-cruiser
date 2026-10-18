//! The Colour tool's pixel math: darktable's color balance rgb preset "basic colorfulness:
//! standard" (perceptual saturation in darktable UCS 2022: global +0.2, shadows +0.25,
//! mid-tones 0, highlights −0.25), with every amount scaled by a strength.
//!
//! Ported from darktable's `src/iop/colorbalancergb.c` and
//! `src/common/colorspaces_inline_conversions.h` (© Aurélien Pierre, GPL). Differences:
//! the photo is sRGB (darktable works in linear Rec. 2020), so XYZ comes from linear sRGB
//! and the gamut boundary is sRGB's; the luminance masks use CIE Y (darktable: Filmlight
//! Yrg's Y, nearly the same). Both soft clips (the saturation factor, the gamut boundary)
//! only hold back a change, never reach below where the pixel started, so strength 0 is
//! the identity and small strengths change little.
//!
//! Applying it per pixel is slow, so [`lut`] samples it on a grid of sRGB values, and
//! [`apply`] interpolates.

use image::RgbaImage;
use std::sync::OnceLock;

/// The preset's saturation: global, then per luminance range.
const GLOBAL: f32 = 0.2;
const SHADOWS: f32 = 0.25;
const HIGHLIGHTS: f32 = -0.25;
// Mid-tones: 0, so their mask isn't computed.

/// Linear sRGB → CIE XYZ (D65) and back.
const RGB_TO_XYZ: [[f32; 3]; 3] = [
    [0.412_456_4, 0.357_576_1, 0.180_437_5],
    [0.212_672_9, 0.715_152_2, 0.072_175],
    [0.019_333_9, 0.119_192, 0.950_304_1],
];
const XYZ_TO_RGB: [[f32; 3]; 3] = [
    [3.240_454_2, -1.537_138_5, -0.498_531_4],
    [-0.969_266, 1.876_010_8, 0.041_556],
    [0.055_643_4, -0.204_025_9, 1.057_225_2],
];
const D65: [f32; 2] = [0.312_71, 0.329_02];
const SRGB_PRIMARIES: [[f32; 2]; 3] = [[0.64, 0.33], [0.30, 0.60], [0.15, 0.06]];

/// Mask middle grey (darktable's `mask_grey_fulcrum`, 0.1845) and the exponent that maps
/// it to about 50%.
const MASK_FULCRUM: f32 = 0.1845;
const MASK_GAMMA: f32 = 0.410_120_6;

const L_STAR_RANGE: f32 = 2.098_883_8;
const L_STAR_UPPER: f32 = 2.098_85;
/// Brightness from lightness and chroma (Helmholtz–Kohlrausch).
const HK: f32 = 1.336_542_2;

fn mul(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| m[r][0] * v[0] + m[r][1] * v[1] + m[r][2] * v[2])
}

/// darktable's `soft_clip`: `x` above `soft` approaches `hard` exponentially.
fn soft_clip(x: f32, soft: f32, hard: f32) -> f32 {
    let norm = hard - soft;
    if x <= soft {
        x
    } else if norm <= 0.0 {
        soft
    } else {
        soft + (1.0 - (-(x - soft) / norm).exp()) * norm
    }
}

fn y_to_l_star(y: f32) -> f32 {
    let y_hat = y.powf(0.631_651_35);
    L_STAR_RANGE * y_hat / (y_hat + 1.124_267_7)
}

fn l_star_to_y(l: f32) -> f32 {
    (1.124_267_7 * l / (L_STAR_RANGE - l)).powf(1.583_151_9)
}

/// Lightness of white (Y = 1).
fn l_white() -> f32 {
    y_to_l_star(1.0)
}

fn xy_to_uv_star_prime(xy: [f32; 2]) -> [f32; 2] {
    let u = -0.783_941 * xy[0] + 0.277_513 * xy[1] + 0.153_836_58;
    let v = 0.745_273_54 * xy[0] - 0.205_375_87 * xy[1] - 0.165_478_38;
    let d = 0.318_707_3 * xy[0] + 2.167_436_9 * xy[1] + 0.291_320_55;
    let d = if d >= 0.0 { d.max(f32::MIN_POSITIVE) } else { d.min(-f32::MIN_POSITIVE) };
    let (u, v) = (u / d, v / d);
    let us = 1.396_562_3 * u / (u.abs() + 1.492_173_5);
    let vs = 1.451_395_4 * v / (v.abs() + 1.524_886_4);
    [-1.124_983_9 * us - 0.980_483_7 * vs, 1.863_233_2 * us + 1.971_853_1 * vs]
}

/// Chroma from lightness `l_star` and squared colorfulness `m2`.
fn chroma(l_star: f32, m2: f32, l_white: f32) -> f32 {
    15.932_994 * l_star.powf(0.652_399_8) * m2.powf(0.600_755_7) / l_white
}

/// xyY → darktable UCS JCH (lightness, chroma, hue).
fn xyy_to_jch(xy: [f32; 2], y: f32, l_white: f32) -> [f32; 3] {
    let uv = xy_to_uv_star_prime(xy);
    let l_star = y_to_l_star(y);
    [l_star / l_white, chroma(l_star, uv[0] * uv[0] + uv[1] * uv[1], l_white), uv[1].atan2(uv[0])]
}

/// darktable UCS JCH → xy and Y.
fn jch_to_xyy(jch: [f32; 3], l_white: f32) -> ([f32; 2], f32) {
    let l_star = (jch[0] * l_white).clamp(0.0, L_STAR_UPPER);
    let m = if l_star == 0.0 {
        0.0
    } else {
        (jch[1] * l_white / (15.932_994 * l_star.powf(0.652_399_8))).powf(0.832_285_1)
    };
    let (up, vp) = (m * jch[2].cos(), m * jch[2].sin());
    let us = -5.037_522_4 * up - 2.504_856_3 * vp;
    let vs = 4.760_029_4 * up + 2.874_013 * vp;
    let u = -1.492_173_5 * us / (us.abs() - 1.396_562_3);
    let v = -1.524_886_4 * vs / (vs.abs() - 1.451_395_4);
    let x = 0.167_171_47 * u + 0.141_299_8 * v - 0.008_015_313;
    let y = -0.150_959_09 * u - 0.155_185_06 * v - 0.008_433_124;
    let d = 0.940_254_75 * u + v - 0.025_632_597;
    let d = if d >= 0.0 { d.max(f32::MIN_POSITIVE) } else { d.min(-f32::MIN_POSITIVE) };
    ([x / d, y / d], l_star_to_y(l_star))
}

const GAMUT_LEN: usize = 512;

/// darktable's `dt_UCS_22_build_gamut_LUT` for sRGB: the squared colorfulness of the
/// gamut boundary per hue, from −π to π.
fn gamut() -> &'static [f32; GAMUT_LEN] {
    static LUT: OnceLock<[f32; GAMUT_LEN]> = OnceLock::new();
    LUT.get_or_init(|| {
        let tau = std::f32::consts::TAU;
        let pi = std::f32::consts::PI;
        let delta = |a: f32, b: f32| {
            let d = a - b;
            if d < -pi {
                d + tau
            } else if d > pi {
                d - tau
            } else {
                d
            }
        };
        let [r, g, b] = SRGB_PRIMARIES;
        let hue = |p: [f32; 2]| (p[1] - D65[1]).atan2(p[0] - D65[0]);
        let (hr, hg, hb) = (hue(r), hue(g), hue(b));
        // Where the ray from white at `tan` meets the edge from `p` to `q`.
        let edge = |p: [f32; 2], q: [f32; 2], tan: f32| {
            let t = (D65[1] - p[1] + tan * (p[0] - D65[0])) / (q[1] - p[1] + tan * (p[0] - q[0]));
            [p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1])]
        };
        let mut sum = [0f32; GAMUT_LEN];
        let mut count = [0f32; GAMUT_LEN];
        let steps = 50 * GAMUT_LEN;
        for i in 0..steps {
            let angle = -pi + i as f32 / steps as f32 * tau;
            let tan = angle.tan();
            let within = |t: f32| (0.0..=1.0).contains(&t);
            let xy = if within(delta(angle, hb) / delta(hr, hb)) {
                edge(b, r, tan)
            } else if within(delta(angle, hr) / delta(hg, hr)) {
                edge(r, g, tan)
            } else if within(delta(angle, hg) / delta(hb, hg)) {
                edge(g, b, tan)
            } else {
                [0.0, 0.0]
            };
            let uv = xy_to_uv_star_prime(xy);
            let h = uv[1].atan2(uv[0]);
            let k = ((GAMUT_LEN - 1) as f32 * (h + pi) / tau).round() as i64;
            let k = k.rem_euclid(GAMUT_LEN as i64) as usize;
            sum[k] += uv[0] * uv[0] + uv[1] * uv[1];
            count[k] += 1.0;
        }
        std::array::from_fn(|k| sum[k] / count[k].max(1.0))
    })
}

fn lookup_gamut(hue: f32) -> f32 {
    let lut = gamut();
    let x = GAMUT_LEN as f32 * (hue + std::f32::consts::PI) / std::f32::consts::TAU;
    let (lo, hi) = (x.floor(), x.ceil());
    let (i, j) = (
        (lo as i64).rem_euclid(GAMUT_LEN as i64) as usize,
        (hi as i64).rem_euclid(GAMUT_LEN as i64) as usize,
    );
    lut[i] + if i == j { 0.0 } else { (x - lo) * (lut[j] - lut[i]) }
}

/// The saturation change for linear luminance `y`, at strength 1: global plus the shadows
/// and highlights amounts weighted by darktable's luminance masks (fall-off 1).
fn boost(y: f32) -> f32 {
    let fulcrum = MASK_FULCRUM.powf(MASK_GAMMA);
    let x = (y.max(0.0).powf(MASK_GAMMA) - fulcrum) / fulcrum;
    let shadows = 1.0 / (1.0 + x.exp());
    let highlights = 1.0 / (1.0 + (-x).exp());
    GLOBAL + shadows * SHADOWS + highlights * HIGHLIGHTS
}

/// The preset at `strength` on one linear sRGB pixel; out-of-gamut results are clipped.
pub fn saturate(rgb: [f32; 3], strength: f32) -> [f32; 3] {
    let xyz = mul(&RGB_TO_XYZ, rgb.map(|c| c.max(0.0)));
    let sum = xyz[0] + xyz[1] + xyz[2];
    if sum <= 0.0 {
        return rgb;
    }
    let lw = l_white();
    let jch = xyy_to_jch([xyz[0] / sum, xyz[1] / sum], xyz[1], lw);
    let hue = jch[2];
    // Chroma and brightness (HCB), rotated so the saturation moves along a line of
    // constant hue, as in darktable.
    let (c, b) = (jch[1], jch[0] * (jch[1].powf(HK) + 1.0));
    let radius = c.hypot(b);
    if radius <= 0.0 {
        return rgb;
    }
    let (sin_t, cos_t) = (c / radius, b / radius);
    let p = c.max(f32::MIN_POSITIVE);
    let w = sin_t * c + cos_t * b;
    let a = (1.0 + strength * boost(xyz[1])).max(0.0);
    let max_a = p.hypot(w) / p;
    let a = soft_clip(a, (0.5 * max_a).max(1.0), max_a);
    let p2 = (a - 1.0) * p;
    let w2 = (p * p * (1.0 - a * a) + w * w).max(0.0).sqrt();
    let c2 = (cos_t * p2 + sin_t * w2).max(0.0);
    let b2 = (-sin_t * p2 + cos_t * w2).max(0.0);
    // Gamut: soft-clip the saturation at constant brightness, towards sRGB's boundary.
    let j2 = b2 / (c2.powf(HK) + 1.0);
    let edge = chroma(j2 * lw, lookup_gamut(hue), lw);
    let edge_b = j2 * (edge.powf(HK) + 1.0);
    let edge_s = if edge_b > 0.0 { edge / edge_b } else { 0.0 };
    let s0 = if b > 0.0 { c / b } else { 0.0 };
    let s = if b2 > 0.0 { c2 / b2 } else { 0.0 };
    let s = soft_clip(s, (0.8 * edge_s).max(s0), edge_s.max(s0));
    let c3 = s * b2;
    let j3 = b2 / (c3.powf(HK) + 1.0);
    let (xy, y) = jch_to_xyy([j3, c3, hue], lw);
    if xy[1] <= 0.0 {
        return rgb;
    }
    let xyz = [xy[0] * y / xy[1], y, (1.0 - xy[0] - xy[1]) * y / xy[1]];
    mul(&XYZ_TO_RGB, xyz).map(|c| c.clamp(0.0, 1.0))
}

/// Grid points per channel of the sampled curve.
pub const GRID: usize = 33;

/// [`saturate`] at `strength` sampled on a `GRID³` grid of sRGB-encoded values, as
/// sRGB-encoded 0–255 values, red slowest.
pub fn lut(strength: f32) -> Vec<[f32; 3]> {
    let enc = |i: usize| i as f32 / (GRID - 1) as f32;
    let mut out = Vec::with_capacity(GRID * GRID * GRID);
    for r in 0..GRID {
        for g in 0..GRID {
            for b in 0..GRID {
                let lin = [r, g, b].map(|i| crate::adjust::srgb_to_linear(enc(i)));
                out.push(saturate(lin, strength).map(|c| crate::adjust::linear_to_srgb(c) * 255.0));
            }
        }
    }
    out
}

/// One pixel through the sampled curve (trilinear).
fn interpolate(lut: &[[f32; 3]], px: [u8; 3]) -> [u8; 3] {
    let scale = (GRID - 1) as f32 / 255.0;
    let pos = px.map(|c| f32::from(c) * scale);
    let i0 = pos.map(|p| (p as usize).min(GRID - 2));
    let f: [f32; 3] = std::array::from_fn(|c| pos[c] - i0[c] as f32);
    let at = |r: usize, g: usize, b: usize| &lut[((i0[0] + r) * GRID + i0[1] + g) * GRID + i0[2] + b];
    std::array::from_fn(|c| {
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let along_b = |r, g| lerp(at(r, g, 0)[c], at(r, g, 1)[c], f[2]);
        let v =
            lerp(lerp(along_b(0, 0), along_b(0, 1), f[1]), lerp(along_b(1, 0), along_b(1, 1), f[1]), f[0]);
        v.round().clamp(0.0, 255.0) as u8
    })
}

/// The Colour tool at `strength` on `img` in place; alpha stays.
pub fn apply(img: &mut RgbaImage, strength: f32) {
    let lut = lut(strength);
    for px in img.pixels_mut() {
        let [r, g, b] = interpolate(&lut, [px.0[0], px.0[1], px.0[2]]);
        px.0[..3].copy_from_slice(&[r, g, b]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adjust::{linear_to_srgb, srgb_to_linear};

    fn exact(px: [u8; 3], strength: f32) -> [u8; 3] {
        let lin = px.map(|c| srgb_to_linear(f32::from(c) / 255.0));
        saturate(lin, strength).map(|c| (linear_to_srgb(c) * 255.0).round() as u8)
    }

    fn chroma_of(px: [u8; 3]) -> f32 {
        let lin = px.map(|c| srgb_to_linear(f32::from(c) / 255.0));
        let xyz = mul(&RGB_TO_XYZ, lin);
        let s = xyz.iter().sum::<f32>();
        xyy_to_jch([xyz[0] / s, xyz[1] / s], xyz[1], l_white())[1]
    }

    #[test]
    fn ucs_round_trip() {
        let lw = l_white();
        for xyy in [([0.3127, 0.329], 0.18), ([0.45, 0.4], 0.5), ([0.2, 0.15], 0.05), ([0.3, 0.55], 0.9)] {
            let (xy, y) = jch_to_xyy(xyy_to_jch(xyy.0, xyy.1, lw), lw);
            assert!((xy[0] - xyy.0[0]).abs() < 1e-3 && (xy[1] - xyy.0[1]).abs() < 1e-3, "{xyy:?} → {xy:?}");
            assert!((y - xyy.1).abs() < 1e-3, "{y}");
        }
    }

    #[test]
    fn identity_at_zero_and_greys_stay() {
        for px in [[200, 120, 90], [30, 60, 200], [255, 0, 0], [12, 200, 40], [250, 250, 245]] {
            let out = exact(px, 0.0);
            assert!(px.iter().zip(out).all(|(a, b)| a.abs_diff(b) <= 1), "{px:?} → {out:?}");
        }
        for v in [0u8, 40, 119, 200, 255] {
            let out = exact([v; 3], 2.0);
            assert!(out.iter().all(|c| c.abs_diff(v) <= 1), "grey {v} → {out:?}");
        }
    }

    #[test]
    fn more_colour_in_shadows_than_highlights() {
        // Dull colours gain chroma, keeping their hue and roughly their lightness.
        let dark = [70, 55, 45];
        let light = [230, 215, 205];
        let gain = |px| chroma_of(exact(px, 1.0)) / chroma_of(px);
        assert!(gain(dark) > 1.2, "shadows: {}", gain(dark));
        assert!(gain(light) < gain(dark), "highlights gain less: {} vs {}", gain(light), gain(dark));
        assert!(gain(dark) > 1.0 && chroma_of(exact(dark, 2.0)) > chroma_of(exact(dark, 1.0)), "2× is more");
        // Hue: the channels keep their order.
        let out = exact([150, 110, 90], 1.0);
        assert!(out[0] > out[1] && out[1] > out[2], "{out:?}");
    }

    #[test]
    fn saturated_colours_keep_their_hue() {
        // Already at the sRGB edge: they can't gain chroma. Perceptual saturation darkens
        // them a little (as in darktable); the other channels stay near 0, no hue shift.
        for (px, c) in [([255, 0, 0], 0), ([0, 0, 255], 2), ([255, 140, 0], 0), ([0, 200, 0], 1)] {
            let out = exact(px, 2.0);
            assert!(out[c] <= px[c] && out[c] >= px[c] - 64, "{px:?} → {out:?}");
            assert!((0..3).filter(|i| *i != c).all(|i| out[i].abs_diff(px[i]) <= 12), "{px:?} → {out:?}");
        }
    }

    #[test]
    fn sampled_curve_is_close_to_exact() {
        let lut = lut(1.0);
        let mut worst = 0;
        for r in (0..=255).step_by(7) {
            for g in (0..=255).step_by(11) {
                for b in (0..=255).step_by(13) {
                    let px = [r as u8, g as u8, b as u8];
                    let (a, e) = (interpolate(&lut, px), exact(px, 1.0));
                    worst = worst.max(a.iter().zip(e).map(|(a, e)| a.abs_diff(e)).max().unwrap());
                }
            }
        }
        assert!(worst <= 4, "max diff {worst}");
    }
}
