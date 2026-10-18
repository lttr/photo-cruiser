//! One-slider adjustment tools. Each tool is one [`Op::Adjust`] step in the edit stack
//! with a single value. [`render`](crate::render::render) applies the adjustments after
//! the geometry (crop, rotation), in [`Tool::ALL`] order, whatever their order in the
//! stack. So a tool that measures the image (auto exposure, auto levels) sees the cropped
//! photo.
//!
//! A tool opens at a value chosen for the photo ([`Tool::opening`]), measured on the
//! image the tool will be applied to (geometry done, earlier tools not). Its neutral
//! value changes nothing, and a stack never stores it ([`crate::EditStack::set_adjust`]).

use crate::edit::{EditStack, Op};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::ops::RangeInclusive;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    /// Auto levels at a strength, 0–2: per channel, the darkest and brightest pixels
    /// become black and white. For photos of documents.
    Levels,
    /// Brightness in EV (stops): linear light × 2^value.
    Exposure,
}

/// Linear-light luminance of the middle grey that L* = 50 is.
pub const MIDDLE_GREY: f32 = 0.1842;

impl Tool {
    /// Every tool, in the order they are applied (the enum's order). Levels comes before
    /// exposure: it stretches each channel to the full range, so it would undo an exposure
    /// done before it.
    pub const ALL: [Tool; 2] = [Tool::Levels, Tool::Exposure];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Levels => "Auto levels",
            Tool::Exposure => "Exposure",
        }
    }

    /// The slider's range; values outside it are clamped.
    pub fn range(self) -> RangeInclusive<f32> {
        match self {
            Tool::Levels => 0.0..=2.0,
            Tool::Exposure => -2.0..=2.0,
        }
    }

    /// The value that changes nothing.
    pub fn neutral(self) -> f32 {
        match self {
            Tool::Levels | Tool::Exposure => 0.0,
        }
    }

    /// The value as the panel shows it.
    pub fn format(self, value: f32) -> String {
        match self {
            Tool::Levels => format!("{value:.2}"),
            Tool::Exposure => format!("{value:+.2} EV"),
        }
    }

    pub fn clamp(self, value: f32) -> f32 {
        let r = self.range();
        if value.is_nan() { self.neutral() } else { value.clamp(*r.start(), *r.end()) }
    }

    /// `value` moved by `steps` keyboard steps (negative: down), within the range and
    /// rounded to 0.01. A step is 0.1 (EV or strength), a `fine` one 0.01.
    pub fn step(self, value: f32, steps: i32, fine: bool) -> f32 {
        let size = if fine { 0.01 } else { 0.1 };
        // `+ 0.0`: no "-0.00".
        (self.clamp(value + steps as f32 * size) * 100.0).round() / 100.0 + 0.0
    }

    /// The value the tool opens at for `img`: auto exposure brings the average brightness
    /// to middle grey; levels opens at strength 1 (it measures the image as it applies).
    /// Rounded to 0.01.
    pub fn opening(self, img: &RgbaImage) -> f32 {
        let v = match self {
            Tool::Levels => 1.0,
            Tool::Exposure => auto_exposure(img),
        };
        // `+ 0.0`: no "-0.00".
        (self.clamp(v) * 100.0).round() / 100.0 + 0.0
    }

    /// Apply the tool at `value` (clamped to its range) to `img` in place.
    pub fn apply(self, img: &mut RgbaImage, value: f32) {
        let value = self.clamp(value);
        if value == self.neutral() {
            return;
        }
        match self {
            Tool::Levels => {
                let luts = levels_luts(img, value);
                map_rgb(img, &luts)
            }
            Tool::Exposure => map_rgb(img, &[exposure_lut(value); 3]),
        }
    }
}

impl std::str::FromStr for Tool {
    type Err = String;

    /// The JSON name: `exposure`.
    fn from_str(s: &str) -> Result<Self, String> {
        serde_json::from_value(serde_json::Value::String(s.into())).map_err(|_| {
            let names: Vec<String> = Tool::ALL.iter().map(|t| serde_json::to_string(t).unwrap()).collect();
            format!("unknown tool {s:?} (one of {})", names.join(", ").replace('"', ""))
        })
    }
}

/// `tool`'s opening value for `src` with `stack`'s geometry, as the app measures it.
pub fn opening(src: &crate::Decoded, stack: &EditStack, tool: Tool) -> crate::Result<f32> {
    let img = crate::render::render(src, &stack.geometry(), Default::default())?;
    Ok(tool.opening(&img))
}

/// Apply every adjustment of `stack` to `img`, in [`Tool::ALL`] order.
pub fn apply_all(img: &mut RgbaImage, stack: &EditStack) {
    for tool in Tool::ALL {
        if let Some(v) = stack.adjust(tool) {
            tool.apply(img, v);
        }
    }
}

/// sRGB-encoded byte → linear light, 0–1.
fn linear_lut() -> &'static [f32; 256] {
    static LUT: OnceLock<[f32; 256]> = OnceLock::new();
    LUT.get_or_init(|| std::array::from_fn(|i| srgb_to_linear(i as f32 / 255.0)))
}

pub fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

pub fn linear_to_srgb(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

fn to_byte(v: f32) -> u8 {
    (v * 255.0).round().clamp(0.0, 255.0) as u8
}

/// An 8-bit curve each on R, G and B; alpha stays.
fn map_rgb(img: &mut RgbaImage, luts: &[[u8; 256]; 3]) {
    for px in img.pixels_mut() {
        for (c, lut) in px.0[..3].iter_mut().zip(luts) {
            *c = lut[*c as usize];
        }
    }
}

/// Exposure: linear light × 2^ev, clipped at white.
fn exposure_lut(ev: f32) -> [u8; 256] {
    let k = ev.exp2();
    let lin = linear_lut();
    std::array::from_fn(|i| to_byte(linear_to_srgb(lin[i] * k)))
}

/// Auto levels at `strength` (0–2) for `img`, one curve per channel. Per channel, the
/// darkest and the brightest 0.5% of the pixels (alpha-weighted) become black and white,
/// linear in between, on the sRGB-encoded values: so the grey point is midway as you see
/// it, like classic levels. Strength 0–1 blends from the original to that; 1–2 raises the
/// clipping at each end from 0.5% to 5%. A channel with a single value stays.
fn levels_luts(img: &RgbaImage, strength: f32) -> [[u8; 256]; 3] {
    let mut hist = [[0f64; 256]; 3];
    let mut weight = 0f64;
    for px in img.pixels() {
        let a = f64::from(px.0[3]) / 255.0;
        weight += a;
        for c in 0..3 {
            hist[c][px.0[c] as usize] += a;
        }
    }
    let clip = weight * f64::from(0.005 + 0.045 * (strength - 1.0).clamp(0.0, 1.0));
    let blend = strength.clamp(0.0, 1.0);
    // The first value, walking `bins`, with more than `clip` of the weight behind it.
    fn point<'a>(mut bins: impl Iterator<Item = (usize, &'a f64)>, clip: f64) -> Option<usize> {
        let mut sum = 0.0;
        bins.find(|(_, n)| {
            sum += **n;
            sum > clip
        })
        .map(|(i, _)| i)
    }
    std::array::from_fn(|c| {
        let h = &hist[c];
        let black = point(h.iter().enumerate(), clip);
        let white = point(h.iter().enumerate().rev(), clip);
        std::array::from_fn(|i| match (black, white) {
            (Some(b), Some(w)) if w > b => {
                let v = i as f32;
                let stretched = ((v - b as f32) / (w - b) as f32 * 255.0).clamp(0.0, 255.0);
                (v + (stretched - v) * blend).round() as u8
            }
            _ => i as u8,
        })
    })
}

/// Rec. 709 luminance weights of linear R, G, B.
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// The EV that brings the mean linear luminance (after clipping at white) to
/// [`MIDDLE_GREY`], within the tool's range. Opaque pixels only count fully; transparent
/// ones not at all. A blank image gets 0.
fn auto_exposure(img: &RgbaImage) -> f32 {
    // Alpha-weighted histograms per channel: the mean for any exposure is a sum over them.
    let mut hist = [[0f64; 256]; 3];
    let mut weight = 0f64;
    for px in img.pixels() {
        let a = f64::from(px.0[3]) / 255.0;
        if a > 0.0 {
            weight += a;
            for c in 0..3 {
                hist[c][px.0[c] as usize] += a;
            }
        }
    }
    if weight == 0.0 {
        return 0.0;
    }
    let lin = linear_lut();
    let mean = |ev: f32| -> f32 {
        let k = f64::from(ev.exp2());
        let sum: f64 = (0..3)
            .map(|c| {
                let s: f64 = hist[c].iter().zip(lin).map(|(n, l)| n * (f64::from(*l) * k).min(1.0)).sum();
                f64::from(LUMA[c]) * s
            })
            .sum();
        (sum / weight) as f32
    };
    let range = Tool::Exposure.range();
    let (mut lo, mut hi) = (*range.start(), *range.end());
    if mean(lo) >= MIDDLE_GREY {
        return lo;
    }
    if mean(hi) <= MIDDLE_GREY {
        return hi;
    }
    // The mean grows with the exposure: bisect.
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if mean(mid) < MIDDLE_GREY { lo = mid } else { hi = mid }
    }
    (lo + hi) / 2.0
}

impl EditStack {
    /// The value of `tool`'s step, if the stack has one.
    pub fn adjust(&self, tool: Tool) -> Option<f32> {
        self.ops.iter().find_map(|op| match op {
            Op::Adjust { tool: t, value } if *t == tool => Some(*value),
            _ => None,
        })
    }

    /// Set `tool`'s step (one per tool), clamped to its range; `None` or the neutral value
    /// removes it. Steps are kept after the geometry, in [`Tool::ALL`] order.
    pub fn set_adjust(&mut self, tool: Tool, value: Option<f32>) {
        self.ops.retain(|op| !matches!(op, Op::Adjust { tool: t, .. } if *t == tool));
        let Some(value) = value.map(|v| tool.clamp(v)).filter(|v| *v != tool.neutral()) else { return };
        let at = self
            .ops
            .iter()
            .position(|op| matches!(op, Op::Adjust { tool: t, .. } if *t > tool))
            .unwrap_or(self.ops.len());
        self.ops.insert(at, Op::Adjust { tool, value });
    }

    /// The adjustment steps, in the stack's order.
    pub fn adjustments(&self) -> impl Iterator<Item = (Tool, f32)> + '_ {
        self.ops.iter().filter_map(|op| match op {
            Op::Adjust { tool, value } => Some((*tool, *value)),
            _ => None,
        })
    }

    /// The stack without its adjustments: what decides the photo's shape.
    pub fn geometry(&self) -> EditStack {
        let ops = self.ops.iter().filter(|op| !matches!(op, Op::Adjust { .. })).cloned().collect();
        EditStack { ops, ..self.clone() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rect;
    use image::Rgba;

    fn flat(v: u8) -> RgbaImage {
        RgbaImage::from_pixel(8, 8, Rgba([v, v, v, 255]))
    }

    #[test]
    fn keyboard_steps() {
        assert_eq!(Tool::Exposure.step(0.37, 1, false), 0.47);
        assert_eq!(Tool::Exposure.step(0.37, -3, true), 0.34);
        assert_eq!(Tool::Exposure.step(1.95, 1, false), 2.0, "clamped to the range");
        assert_eq!(Tool::Levels.step(0.05, -1, false), 0.0);
        assert_eq!(Tool::Exposure.step(0.1, -1, false).to_string(), "0", "no -0");
        // Ten fine steps make one, without drift.
        let v = (0..10).fold(0.0, |v, _| Tool::Exposure.step(v, 1, true));
        assert_eq!(v, Tool::Exposure.step(0.0, 1, false));
    }

    #[test]
    fn srgb_round_trip() {
        for i in 0..=255u8 {
            assert_eq!(to_byte(linear_to_srgb(srgb_to_linear(f32::from(i) / 255.0))), i);
        }
    }

    #[test]
    fn exposure_pixel_math() {
        // +1 EV doubles linear light: sRGB 100 (0.127 linear) → 0.255 linear → 138.
        let mut img = RgbaImage::from_pixel(2, 1, Rgba([100, 0, 255, 128]));
        Tool::Exposure.apply(&mut img, 1.0);
        assert_eq!(img.get_pixel(0, 0).0, [138, 0, 255, 128], "black stays, white clips, alpha kept");
        Tool::Exposure.apply(&mut img, -1.0);
        assert_eq!(img.get_pixel(0, 0).0[0], 100, "−1 EV undoes +1 EV below white");
        // Middle grey (L* 50, sRGB 119) −2 EV is a quarter of the light.
        let mut grey = flat(119);
        Tool::Exposure.apply(&mut grey, -2.0);
        let lin = srgb_to_linear(f32::from(grey.get_pixel(0, 0).0[0]) / 255.0);
        assert!((lin - MIDDLE_GREY / 4.0).abs() < 0.002, "{lin}");
        // Out of range is clamped; 0 changes nothing.
        let (mut a, mut b) = (flat(60), flat(60));
        Tool::Exposure.apply(&mut a, 7.0);
        Tool::Exposure.apply(&mut b, 2.0);
        assert_eq!(a, b);
        let mut c = flat(60);
        Tool::Exposure.apply(&mut c, 0.0);
        assert_eq!(c, flat(60));
    }

    #[test]
    fn auto_exposure_reaches_middle_grey() {
        assert_eq!(Tool::Exposure.opening(&flat(119)), 0.0, "already middle grey");
        // A flat dark image: one stop under middle grey needs +1 EV (±8-bit rounding).
        let dark = to_byte(linear_to_srgb(MIDDLE_GREY / 2.0));
        assert!((Tool::Exposure.opening(&flat(dark)) - 1.0).abs() <= 0.01);
        assert_eq!(Tool::Exposure.opening(&flat(2)), 2.0, "clamped to the range");
        assert_eq!(Tool::Exposure.opening(&flat(255)), -2.0);
        // Half white, half black: clipped white can't get brighter, so the black half
        // can't help either; the mean is 0.5 → needs −1.44 EV.
        let mut split = flat(0);
        for y in 0..4 {
            for x in 0..8 {
                split.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        let ev = Tool::Exposure.opening(&split);
        assert!((ev - (MIDDLE_GREY / 0.5).log2()).abs() <= 0.01, "{ev}");
        // Transparent pixels don't count.
        let mut holes = flat(119);
        holes.put_pixel(0, 0, Rgba([0, 0, 0, 0]));
        assert_eq!(Tool::Exposure.opening(&holes), 0.0);
        assert_eq!(Tool::Exposure.opening(&RgbaImage::new(0, 0)), 0.0);
    }

    /// 100 columns, 10 pixels each, sRGB 50, 51.5, … 198.5 (rounded) from left to right.
    fn ramp() -> RgbaImage {
        RgbaImage::from_fn(100, 10, |x, _| {
            let v = (50.0 + 1.5 * x as f32).round() as u8;
            Rgba([v, v, v, 255])
        })
    }

    #[test]
    fn levels_pixel_math() {
        let at = |strength: f32, x: u32| {
            let mut img = ramp();
            Tool::Levels.apply(&mut img, strength);
            img.get_pixel(x, 0).0[0]
        };
        // 0.5% of 1000 pixels is 5: inside the first and last column (50 and 199).
        assert_eq!((at(1.0, 0), at(1.0, 99)), (0, 255), "black and white points");
        // Linear in between: 125 is midway between 50 and 199 → 127.
        assert_eq!(at(1.0, 50), ((125.0 - 50.0) / 149.0 * 255.0f32).round() as u8);
        // Half strength: half way from the original.
        assert_eq!((at(0.5, 0), at(0.5, 99)), (25, 227));
        // Strength 2 clips 5% (50 pixels, 5 columns) at each end: the next columns, 58 and
        // 191, are the black and white points.
        assert_eq!((at(2.0, 5), at(2.0, 6), at(2.0, 93), at(2.0, 94)), (0, 2, 253, 255));
        assert!(at(1.5, 3) < at(1.0, 3) && at(1.5, 3) > 0, "between");
        // Strength 0, and out of range, change nothing / clamp.
        let mut img = ramp();
        Tool::Levels.apply(&mut img, 0.0);
        assert_eq!(img, ramp());
        assert_eq!((at(-1.0, 0), at(9.0, 4)), (50, 0));
    }

    #[test]
    fn levels_whiten_tinted_paper_per_channel() {
        // Yellowish paper with dark-brown text: each channel stretches on its own.
        let mut doc = RgbaImage::from_fn(10, 10, |x, _| {
            if x == 0 { Rgba([70, 60, 40, 255]) } else { Rgba([200, 190, 140, 255]) }
        });
        doc.put_pixel(5, 5, Rgba([0, 0, 0, 0]));
        Tool::Levels.apply(&mut doc, 1.0);
        assert_eq!(doc.get_pixel(0, 0).0, [0, 0, 0, 255], "text black");
        assert_eq!(doc.get_pixel(1, 0).0, [255, 255, 255, 255], "paper white");
        // A transparent pixel doesn't count as black.
        assert_eq!(doc.get_pixel(5, 5).0, [0, 0, 0, 0]);
        // A flat image (one value per channel) stays, and so does an empty one.
        let mut paper = RgbaImage::from_pixel(4, 4, Rgba([200, 190, 140, 255]));
        Tool::Levels.apply(&mut paper, 2.0);
        assert_eq!(paper.get_pixel(0, 0).0, [200, 190, 140, 255]);
        Tool::Levels.apply(&mut RgbaImage::new(0, 0), 1.0);
        assert_eq!(Tool::Levels.opening(&paper), 1.0);
    }

    #[test]
    fn one_step_per_tool_after_the_geometry() {
        let mut s = EditStack::default();
        s.set_adjust(Tool::Exposure, Some(0.5));
        s.set_crop(Some(Rect::new(1.0, 1.0, 5.0, 5.0)));
        s.rotate(1);
        s.set_adjust(Tool::Exposure, Some(3.0));
        assert_eq!(
            s.ops,
            [
                Op::Crop(Rect::new(1.0, 1.0, 5.0, 5.0)),
                Op::Rotate { quarters: 1 },
                Op::Adjust { tool: Tool::Exposure, value: 2.0 },
            ]
        );
        assert_eq!(s.adjust(Tool::Exposure), Some(2.0));
        assert_eq!(s.geometry().ops.len(), 2);
        assert!(s.to_json().contains(r#""op": "adjust""#) && s.to_json().contains(r#""tool": "exposure""#));
        assert_eq!(EditStack::from_json(&s.to_json()).unwrap(), s);
        s.set_adjust(Tool::Exposure, Some(0.0));
        assert_eq!((s.adjust(Tool::Exposure), s.ops.len()), (None, 2), "neutral is no step");
        assert_eq!("exposure".parse::<Tool>(), Ok(Tool::Exposure));
        assert!("blur".parse::<Tool>().unwrap_err().contains("one of levels, exposure"));
        // Levels goes before exposure, whatever order they're set in.
        s.set_adjust(Tool::Exposure, Some(0.5));
        s.set_adjust(Tool::Levels, Some(1.0));
        assert_eq!(s.adjustments().collect::<Vec<_>>(), [(Tool::Levels, 1.0), (Tool::Exposure, 0.5)]);
    }
}
