//! The Straighten tool: turns the photo by a small angle about its centre and cuts off
//! the empty corners, like darktable's rotate & perspective with "fit". It opens at the
//! angle that levels the photo's lines ([`detect`]).
//!
//! The value is in degrees, clockwise positive (like the quarter turns), −45…45. The
//! straightened photo is the largest upright rectangle of the photo's shape inside the
//! turned photo, so it has no empty corners. It keeps the photo's frame: a crop is a
//! rectangle of it in the same coordinates as on the photo before straightening (scaled
//! by [`fit_scale`]). So a crop stays inside the photo whatever the angle, and at 0° a
//! crop is what it always was. It is rendered at the photo's own resolution, so the
//! output is [`fit_scale`] × the crop's size.

use crate::geom::{Rect, Size};
use image::{Rgba, RgbaImage};

/// [`detect`] looks for lines tilted by at most this many degrees: a photo is rarely
/// taken more askew, and steeper lines (a roof, stairs) aren't meant to be level.
pub const MAX_AUTO: f32 = 10.0;

/// The largest upright rectangle of a `w` × `h` image's shape, centred inside the image
/// turned by `degrees`, as a share of the image's size.
pub fn fit_scale(w: f32, h: f32, degrees: f32) -> f32 {
    let t = degrees.to_radians();
    let (a, b) = (t.cos().abs(), t.sin().abs());
    (w / (w * a + h * b)).min(h / (w * b + h * a))
}

/// `img` turned `degrees` clockwise with the empty corners cut off ([`render`] of the
/// whole frame).
pub fn apply(img: &RgbaImage, degrees: f32) -> RgbaImage {
    render(img, degrees, Rect::with_size(img.dimensions().into()))
}

/// The `area` of `img` straightened by `degrees`. `area` is in `img`'s pixels, a
/// rectangle of the straightened frame scaled up to `img`'s size; it is clamped to it.
pub fn render(img: &RgbaImage, degrees: f32, area: Rect) -> RgbaImage {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let area = area.clamp_to(Size::new(w, h));
    let s = fit_scale(w, h, degrees);
    let out_w = ((area.w * s).round() as u32).max(1);
    let out_h = ((area.h * s).round() as u32).max(1);
    if img.width() == 0 || img.height() == 0 {
        return RgbaImage::new(0, 0);
    }
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (cx, cy) = (w / 2.0, h / 2.0);
    // Straightened frame: output pixel → the frame's point (u) → the source point, turned
    // back counter-clockwise about the centre.
    let (kx, ky) = (area.w * s / out_w as f32, area.h * s / out_h as f32);
    let (ox, oy) = (cx + s * (area.x - cx), cy + s * (area.y - cy));
    RgbaImage::from_fn(out_w, out_h, |i, j| {
        let (dx, dy) = (ox + (i as f32 + 0.5) * kx - cx, oy + (j as f32 + 0.5) * ky - cy);
        let (px, py) = (cx + dx * cos + dy * sin, cy - dx * sin + dy * cos);
        bilinear(img, px - 0.5, py - 0.5)
    })
}

/// `img` at (`x`, `y`) between pixel centres, clamped to the edge.
fn bilinear(img: &RgbaImage, x: f32, y: f32) -> Rgba<u8> {
    let (mx, my) = ((img.width() - 1) as f32, (img.height() - 1) as f32);
    let (x, y) = (x.clamp(0.0, mx), y.clamp(0.0, my));
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (x0, y0) = (x0 as u32, y0 as u32);
    let (x1, y1) = ((x0 + 1).min(img.width() - 1), (y0 + 1).min(img.height() - 1));
    let [a, b, c, d] = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)].map(|(x, y)| img.get_pixel(x, y).0);
    Rgba(std::array::from_fn(|k| {
        let top = f32::from(a[k]) + (f32::from(b[k]) - f32::from(a[k])) * fx;
        let bottom = f32::from(c[k]) + (f32::from(d[k]) - f32::from(c[k])) * fx;
        (top + (bottom - top) * fy).round() as u8
    }))
}

/// Longest side the lines are looked for at.
const DETECT_SIDE: u32 = 640;
/// Sobel gradient (on 0–255 lightness) an edge needs: about 10 levels of contrast.
const MIN_EDGE: f32 = 40.0;
/// The coarse search's step (degrees).
const COARSE: f32 = 0.25;
/// In the coarse search, an edge pixel counts for an angle if its own tilt (from its
/// gradient) is this close (degrees). Tight, so a peak needs pixels that agree with it.
const AGREE: f32 = 2.0;
/// In the fine search, the same, looser: pixels along a slightly tilted line read a few
/// degrees off (the pixel grid's staircase), and the projection is what's precise.
const AGREE_FINE: f32 = 5.0;
/// Edge pixels tilted more than this count for no angle: steeper lines (a roof, a slope)
/// aren't meant to be level.
const STEEP: f32 = MAX_AUTO + AGREE_FINE;
/// A peak is compared with the score this far to either side (degrees).
const AROUND: f32 = 3.0;
/// A peak's score must be this much over the score [`AROUND`] to either side: lines pile
/// up at one angle, texture scores about the same at every angle.
const MIN_PEAK: f32 = 1.5;
/// The edge pixels agreeing with the peak must be at least this × the longer side: a
/// few stray pixels make no line.
const MIN_PIXELS: f32 = 0.5;

/// An edge pixel: where it is, how strong, its own tilt from the nearest axis (from its
/// gradient; degrees, clockwise) and whether it is on a level-ish line (else an
/// upright-ish one).
struct Edge {
    x: f32,
    y: f32,
    weight: f32,
    tilt: f32,
    level: bool,
}

/// The angle (degrees) that levels `img`'s lines: the horizon, walls, window frames.
/// Lines tilted by more than [`MAX_AUTO`] don't count. 0 if there are no clear lines.
///
/// The edge pixels are projected across the lines at each candidate angle (a projection
/// profile, as for deskewing scans). At the lines' angle the pixels of each line fall
/// into one 1 px band, so the score ([`line_score`]) peaks there. In 0.25° steps, the
/// highest peak within [`MAX_AUTO`] that stands out ([`MIN_PEAK`]) and has enough
/// pixels ([`MIN_PIXELS`]) wins; then it is refined in 0.02° steps within 1°.
pub fn detect(img: &RgbaImage) -> f32 {
    let (edges, side) = edges(img);
    if edges.is_empty() {
        return 0.0;
    }
    let steps = |from: f32, to: f32, step: f32| {
        (0..=((to - from) / step).round() as i32).map(move |i| from + i as f32 * step)
    };
    // Past the range by AROUND, to see whether a peak near its end stands out.
    let reach = MAX_AUTO + AROUND;
    let coarse: Vec<(f32, f32)> =
        steps(-reach, reach, COARSE).map(|t| (t, line_score(&edges, t, side, AGREE))).collect();
    let Some(t) = best_peak(&coarse) else { return 0.0 };
    if (edges.iter().filter(|e| (e.tilt - t).abs() <= AGREE).count() as f32) < MIN_PIXELS * side {
        return 0.0;
    }
    let fine = steps(t - 1.0, t + 1.0, 0.02).filter(|t| t.abs() <= MAX_AUTO);
    let (tilt, _) = fine
        .map(|t| (t, line_score(&edges, t, side, AGREE_FINE)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();
    // Turning by −tilt levels a line tilted by `tilt`. Rounded to 0.01; `+ 0.0`: no "-0".
    (-tilt * 100.0).round() / 100.0 + 0.0
}

/// The angle of the highest peak of `scores` (angle, score; in [`COARSE`] steps) within
/// [`MAX_AUTO`] that is at least [`MIN_PEAK`] × the score [`AROUND`] to each side.
fn best_peak(scores: &[(f32, f32)]) -> Option<f32> {
    let k = (AROUND / COARSE).round() as usize;
    (k..scores.len().saturating_sub(k))
        .filter(|&i| scores[i].0.abs() <= MAX_AUTO)
        .filter(|&i| scores[i].1 >= scores[i - 1].1 && scores[i].1 >= scores[i + 1].1)
        .filter(|&i| scores[i].1 >= MIN_PEAK * scores[i - k].1.max(scores[i + k].1))
        .max_by(|&a, &b| scores[a].1.total_cmp(&scores[b].1))
        .map(|i| scores[i].0)
}

/// The edge pixels of `img` that may be on level or upright lines, and the longer side
/// of the size they were found at (`img` shrunk to [`DETECT_SIDE`]).
fn edges(img: &RgbaImage) -> (Vec<Edge>, f32) {
    let small;
    let img = match img.width().max(img.height()) > DETECT_SIDE {
        true => {
            let k = DETECT_SIDE as f32 / img.width().max(img.height()) as f32;
            let (w, h) = ((img.width() as f32 * k).round() as u32, (img.height() as f32 * k).round() as u32);
            small = image::imageops::thumbnail(img, w.max(1), h.max(1));
            &small
        }
        false => img,
    };
    let (w, h) = (img.width() as usize, img.height() as usize);
    let light: Vec<f32> = img
        .pixels()
        .map(|p| 0.299 * f32::from(p.0[0]) + 0.587 * f32::from(p.0[1]) + 0.114 * f32::from(p.0[2]))
        .collect();
    let at = |x: usize, y: usize| light[y * w + x];
    let mut out = Vec::new();
    for y in 1..h.saturating_sub(1) {
        for x in 1..w.saturating_sub(1) {
            let gx = at(x + 1, y - 1) + 2.0 * at(x + 1, y) + at(x + 1, y + 1)
                - at(x - 1, y - 1)
                - 2.0 * at(x - 1, y)
                - at(x - 1, y + 1);
            let gy = at(x - 1, y + 1) + 2.0 * at(x, y + 1) + at(x + 1, y + 1)
                - at(x - 1, y - 1)
                - 2.0 * at(x, y - 1)
                - at(x + 1, y - 1);
            let weight = gx.hypot(gy);
            // The gradient is across the line: ±90° for a level line, 0° or 180° for an
            // upright one. The line's tilt from the nearest axis:
            let tilt = (gy.atan2(gx).to_degrees() - 45.0).rem_euclid(90.0) - 45.0;
            if weight >= MIN_EDGE && tilt.abs() <= STEEP {
                out.push(Edge {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                    weight,
                    tilt,
                    level: gy.abs() > gx.abs(),
                });
            }
        }
    }
    (out, w.max(h) as f32)
}

/// How much the edges tilted about `tilt` (within `agree`) pile up on lines at that
/// angle: the sum of the squared weights per 1 px band across the lines, level and
/// upright ones apart.
fn line_score(edges: &[Edge], tilt: f32, side: f32, agree: f32) -> f32 {
    let (sin, cos) = tilt.to_radians().sin_cos();
    // The distance across runs from about −side·sin to side·(cos + sin) (up to 1.2 side
    // at the steepest tilts), offset by `side`: 3 side bands hold all of it, so no pixels
    // pile up in a clamped end band.
    let bands = (3.0 * side) as usize + 2;
    let (mut level, mut upright) = (vec![0f32; bands], vec![0f32; bands]);
    for e in edges.iter().filter(|e| (e.tilt - tilt).abs() <= agree) {
        // The distance across the line: along its normal.
        let (across, bins) = match e.level {
            true => (-e.x * sin + e.y * cos, &mut level),
            false => (e.x * cos + e.y * sin, &mut upright),
        };
        // Shared between the two nearest bands, so the score is smooth in the angle.
        let at = (across + side).clamp(0.0, (bands - 2) as f32);
        let (i, f) = (at as usize, at.fract());
        bins[i] += e.weight * (1.0 - f);
        bins[i + 1] += e.weight * f;
    }
    level.iter().chain(&upright).map(|v| v * v).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    const RED: Rgba<u8> = Rgba([255, 0, 0, 255]);
    const GREY: Rgba<u8> = Rgba([100, 100, 100, 255]);

    #[test]
    fn fit_scale_of_the_turned_frame() {
        assert_eq!(fit_scale(400.0, 300.0, 0.0), 1.0);
        // A square at 45°: the upright square inside it is 1/√2 its size.
        assert!((fit_scale(100.0, 100.0, 45.0) - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-4);
        // 4:3 at 10°: the height binds, 3 / (4 sin 10° + 3 cos 10°).
        assert!((fit_scale(400.0, 300.0, 10.0) - 0.8222).abs() < 1e-4);
        assert_eq!(fit_scale(400.0, 300.0, -10.0), fit_scale(400.0, 300.0, 10.0));
        assert_eq!(fit_scale(300.0, 400.0, 10.0), fit_scale(400.0, 300.0, 10.0), "portrait alike");
    }

    #[test]
    fn turns_clockwise_about_the_centre() {
        // Grey, opaque, with a red dot 40 px right of the centre.
        let mut img = RgbaImage::from_pixel(201, 151, GREY);
        for y in 74..77 {
            for x in 139..142 {
                img.put_pixel(x, y, RED);
            }
        }
        let out = apply(&img, 30.0);
        let s = fit_scale(201.0, 151.0, 30.0);
        assert_eq!(out.dimensions(), ((201.0 * s).round() as u32, (151.0 * s).round() as u32));
        // Clockwise by 30° (y down): the dot moves down, 40·(cos 30°, sin 30°) off the centre.
        let (cx, cy) = (out.width() as f32 / 2.0, out.height() as f32 / 2.0);
        let (x, y) = (cx + 40.0 * 0.866 - 0.5, cy + 40.0 * 0.5 - 0.5);
        assert_eq!(out.get_pixel(x.round() as u32, y.round() as u32).0, RED.0, "at {x}, {y}");
        // 0° changes nothing.
        assert_eq!(apply(&img, 0.0), img);
    }

    #[test]
    fn a_crop_is_part_of_the_straightened_frame() {
        let img = RgbaImage::from_fn(200, 100, |x, y| Rgba([x as u8, (y * 2) as u8, 50, 255]));
        let whole = apply(&img, 8.0);
        let s = fit_scale(200.0, 100.0, 8.0);
        // The right half: the right half of the straightened photo, at its resolution.
        let half = render(&img, 8.0, Rect::new(100.0, 0.0, 100.0, 100.0));
        assert_eq!(half.dimensions(), ((100.0 * s).round() as u32, (100.0 * s).round() as u32));
        let off = whole.width() - half.width();
        for (x, y) in [(0, 0), (10, 40), (half.width() - 1, half.height() - 1)] {
            let (a, b) = (half.get_pixel(x, y).0, whole.get_pixel(x + off, y).0);
            assert!(a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 2), "{x},{y}: {a:?} vs {b:?}");
        }
        // Clamped to the frame.
        let side = (50.0 * s).round() as u32;
        assert_eq!(render(&img, 8.0, Rect::new(150.0, 50.0, 500.0, 500.0)).dimensions(), (side, side));
    }

    /// Edge pixels far apart across the lines score apart, wherever they are in the frame:
    /// near the bottom right of a landscape photo, an upright edge at a clockwise tilt is
    /// more than `side` across.
    #[test]
    fn line_score_keeps_far_edges_in_their_own_bands() {
        let edge = |x, y| Edge { x, y, weight: 1.0, tilt: 8.0, level: false };
        let score = |e: &[Edge]| line_score(e, 8.0, 400.0, AGREE);
        let corner = [edge(370.0, 290.0), edge(390.0, 290.0)];
        let apart: f32 = corner.iter().map(|e| score(std::slice::from_ref(e))).sum();
        assert!((score(&corner) - apart).abs() < 1e-4, "{} vs {apart}", score(&corner));
    }

    /// Light stripes on dark (level lines every 30 px, and upright ones every 50 px),
    /// drawn turned `tilt`° clockwise, anti-aliased.
    fn lines(w: u32, h: u32, tilt: f32) -> RgbaImage {
        drawn(w, h, tilt, |u, t| t.rem_euclid(30.0) < 3.0 || u.rem_euclid(50.0) < 3.0)
    }

    /// Light on dark where `on(u, t)` in the upright frame (x, y), turned `tilt`°
    /// clockwise, anti-aliased (4×4 samples per pixel) and slightly blurred.
    fn drawn(w: u32, h: u32, tilt: f32, on: impl Fn(f32, f32) -> bool) -> RgbaImage {
        let (sin, cos) = tilt.to_radians().sin_cos();
        let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
        let sharp = RgbaImage::from_fn(w, h, |x, y| {
            let mut v = 40.0;
            for i in 0..16 {
                let (dx, dy) = (x as f32 + (i % 4) as f32 / 4.0 - cx, y as f32 + (i / 4) as f32 / 4.0 - cy);
                if on(cx + dx * cos + dy * sin, cy - dx * sin + dy * cos) {
                    v += 180.0 / 16.0;
                }
            }
            Rgba([v as u8, v as u8, v as u8, 255])
        });
        // A little blur, like a lens.
        image::imageops::blur(&sharp, 0.8)
    }

    #[test]
    fn detects_the_tilt_of_lines() {
        for tilt in [2.5, -4.2, 0.0, 0.3, 7.3, 9.5] {
            let found = detect(&lines(400, 300, tilt));
            assert!((found + tilt).abs() <= 0.1, "tilted {tilt}: found {found}");
        }
        // Large: shrunk first, same angle.
        let found = detect(&lines(1600, 1000, 1.7));
        assert!((found + 1.7).abs() <= 0.1, "large: {found}");
    }

    #[test]
    fn no_lines_no_turn() {
        assert_eq!(detect(&RgbaImage::from_pixel(64, 48, GREY)), 0.0, "flat");
        assert_eq!(detect(&RgbaImage::new(0, 0)), 0.0, "empty");
        // Texture without lines: a fixed pseudo-random speckle.
        let mut seed = 1u32;
        let noise = RgbaImage::from_fn(300, 200, |_, _| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let v = (seed >> 16) as u8;
            Rgba([v, v, v, 255])
        });
        assert_eq!(detect(&noise), 0.0, "speckle");
        // Lines steeper than MAX_AUTO (a roof, a slope): left alone.
        let roof =
            drawn(400, 300, 20.0, |_, t| [40.0, 95.0, 170.0, 230.0].iter().any(|y| (t - y).abs() < 2.0));
        assert_eq!(detect(&roof), 0.0, "20°");
    }
}
