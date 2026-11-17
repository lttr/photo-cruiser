//! One-slider adjustment tools. Each tool is one [`Op::Adjust`] step in the edit stack
//! with a single value. [`render`](crate::render::render) applies the adjustments after
//! the geometry (crop, rotation), in [`Tool::ALL`] order, whatever their order in the
//! stack. So a tool that measures the image (auto exposure, auto levels) sees the cropped
//! photo. Straighten is the exception: it is part of the geometry, done before the crop
//! ([`Tool::is_geometry`]).
//!
//! A tool opens at a value chosen for the photo ([`Tool::opening`]), measured on the
//! image the tool will be applied to (geometry done, earlier tools not). Its neutral
//! value changes nothing, and a stack never stores it ([`crate::EditStack::set_adjust`]).

use crate::edit::{EditStack, Op};
use crate::warmth::{encode, linear_lut};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::ops::RangeInclusive;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    /// Turns the photo by a small angle, −45…45° (clockwise positive), and cuts off the
    /// empty corners ([`crate::straighten`]). Geometry: [`render`](crate::render::render)
    /// does it before the crop, not [`apply_all`].
    Straighten,
    /// Auto levels at a strength, 0–2: per channel, the darkest and brightest pixels
    /// become black and white. For photos of documents.
    Levels,
    /// White balance along the colour temperature: + warmer (amber), − cooler (blue), in
    /// units of 100 mired, −2…2 ([`crate::warmth`]).
    Warmth,
    /// Removes haze, 0–1, like darktable's haze removal ([`crate::dehaze`]).
    #[serde(alias = "haze_removal")]
    Dehaze,
    /// Brightness in EV (stops): linear light × 2^value.
    Exposure,
    /// Lifts shadows and pulls back highlights by area, 0–1, like darktable's tone
    /// equalizer "compress shadows/highlights" ([`crate::tones`]).
    #[serde(alias = "tones")]
    ShadowsHighlights,
    /// Brings out detail, 1–2 (100–200 %), like darktable's local contrast "detail"
    /// ([`crate::local_contrast`]). 1 changes nothing.
    #[serde(alias = "clarity")]
    LocalContrast,
    /// An S-curve, 1–2, like darktable's sigmoid contrast ([`crate::contrast`]). 1 changes
    /// nothing.
    Contrast,
    /// Colourfulness: darktable's "basic colorfulness: standard" preset × the value, 0–2.
    /// More in the shadows than in the highlights ([`crate::colour`]).
    #[serde(alias = "color")]
    Colour,
    /// Unsharp mask at an amount, 0–1, like darktable's sharpen (radius 2, threshold 0.5)
    /// ([`crate::sharpen`]).
    Sharpen,
    /// Darker (−) or brighter (+) corners of the (cropped) frame, −1…1, like darktable's
    /// vignette at its default shape ([`crate::vignette`]).
    Vignette,
}

/// Linear-light luminance of the middle grey that L* = 50 is.
pub const MIDDLE_GREY: f32 = 0.1842;

impl Tool {
    /// Every tool, in the order they are applied (the enum's order). Straighten is part of
    /// the geometry, so it comes first. Levels comes before
    /// exposure: it stretches each channel to the full range, so it would undo an exposure
    /// done before it. Warmth comes after levels, which would neutralise it per channel.
    /// Dehaze comes before exposure: it measures the haze against the brightest haze.
    /// Shadows & highlights comes after exposure: it goes by the distance from middle grey.
    /// Local contrast comes after it: it boosts the detail on top of the areas it has set.
    /// Contrast comes after both, as darktable's sigmoid does: it shapes the finished tones.
    /// Colour comes after them: its masks go by brightness. Sharpen comes next, on the
    /// finished tones. Vignette comes last, on the finished picture.
    pub const ALL: [Tool; 11] = [
        Tool::Straighten,
        Tool::Levels,
        Tool::Warmth,
        Tool::Dehaze,
        Tool::Exposure,
        Tool::ShadowsHighlights,
        Tool::LocalContrast,
        Tool::Contrast,
        Tool::Colour,
        Tool::Sharpen,
        Tool::Vignette,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Straighten => "Straighten",
            Tool::Levels => "Auto levels",
            Tool::Warmth => "Warmth",
            Tool::Dehaze => "Dehaze",
            Tool::Exposure => "Exposure",
            Tool::ShadowsHighlights => "Shadows & highlights",
            Tool::LocalContrast => "Local contrast",
            Tool::Contrast => "Contrast",
            Tool::Colour => "Colour",
            Tool::Sharpen => "Sharpen",
            Tool::Vignette => "Vignette",
        }
    }

    /// The slider's range; values outside it are clamped.
    pub fn range(self) -> RangeInclusive<f32> {
        match self {
            Tool::Levels | Tool::Colour => 0.0..=2.0,
            Tool::ShadowsHighlights | Tool::Dehaze | Tool::Sharpen => 0.0..=1.0,
            Tool::LocalContrast | Tool::Contrast => 1.0..=2.0,
            Tool::Vignette => -1.0..=1.0,
            Tool::Straighten => -45.0..=45.0,
            Tool::Exposure | Tool::Warmth => -2.0..=2.0,
        }
    }

    /// The value that changes nothing.
    pub fn neutral(self) -> f32 {
        match self {
            Tool::Straighten
            | Tool::Levels
            | Tool::Warmth
            | Tool::Dehaze
            | Tool::Exposure
            | Tool::ShadowsHighlights
            | Tool::Colour
            | Tool::Sharpen
            | Tool::Vignette => 0.0,
            Tool::LocalContrast | Tool::Contrast => 1.0,
        }
    }

    /// The value as the panel shows it.
    pub fn format(self, value: f32) -> String {
        match self {
            Tool::Levels | Tool::ShadowsHighlights | Tool::Dehaze | Tool::Contrast | Tool::Sharpen => {
                format!("{value:.2}")
            }
            Tool::Exposure => format!("{value:+.2} EV"),
            Tool::Warmth | Tool::Vignette => format!("{value:+.2}"),
            Tool::Colour => format!("{value:.2}×"),
            Tool::LocalContrast => format!("{:.0}%", value * 100.0),
            Tool::Straighten => format!("{value:+.2}°"),
        }
    }

    /// Whether the tool is part of the geometry (it changes the photo's shape):
    /// [`render`](crate::render::render) does it before the crop, not [`apply_all`].
    /// Only Straighten.
    pub fn is_geometry(self) -> bool {
        self == Tool::Straighten
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
    /// to middle grey; levels opens at strength 1 (it measures the image as it applies);
    /// warmth at grey-world white balance; shadows & highlights at 0.45, darktable's
    /// "medium"; dehaze at 0.2, darktable's default; local contrast at 1.25, darktable's
    /// default detail; contrast at 1.2, near sigmoid's "neutral gray" preset (1.22); colour
    /// at 1, the preset as darktable has it. Sharpen at 0.5, darktable's default amount;
    /// vignette at −0.5, darktable's default; straighten at the angle that levels the
    /// photo's lines.
    /// Rounded to 0.01.
    pub fn opening(self, img: &RgbaImage) -> f32 {
        self.fixed_opening().unwrap_or_else(|| {
            self.rounded(match self {
                Tool::Exposure => auto_exposure(img),
                Tool::Warmth => crate::warmth::grey_world(img),
                Tool::Straighten => crate::straighten::detect(img),
                _ => unreachable!("{self:?} has a fixed opening"),
            })
        })
    }

    /// [`Tool::opening`] for the tools whose opening doesn't depend on the image; `None`
    /// for those that measure it (Exposure, Warmth, Straighten).
    pub fn fixed_opening(self) -> Option<f32> {
        let v = match self {
            Tool::Levels | Tool::Colour => 1.0,
            Tool::ShadowsHighlights => 0.45,
            Tool::Dehaze => 0.2,
            Tool::LocalContrast => 1.25,
            Tool::Contrast => 1.2,
            Tool::Sharpen => 0.5,
            Tool::Vignette => -0.5,
            Tool::Exposure | Tool::Warmth | Tool::Straighten => return None,
        };
        Some(self.rounded(v))
    }

    /// `v` clamped and rounded to 0.01.
    fn rounded(self, v: f32) -> f32 {
        // `+ 0.0`: no "-0.00".
        (self.clamp(v) * 100.0).round() / 100.0 + 0.0
    }

    /// Apply the tool at `value` (clamped to its range) to `img`, a full-size render, in place.
    /// Straighten replaces it with the straightened image, which is smaller.
    pub fn apply(self, img: &mut RgbaImage, value: f32) {
        self.apply_at(img, value, 1.0)
    }

    /// [`Tool::apply`] to `img` rendered at `scale` of the full-size photo (a downscaled
    /// preview: below 1). Tools that work on a radius in pixels scale it by that.
    pub fn apply_at(self, img: &mut RgbaImage, value: f32, scale: f32) {
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
            Tool::Warmth => crate::warmth::apply(img, value),
            Tool::Dehaze => crate::dehaze::apply(img, value),
            Tool::ShadowsHighlights => crate::tones::apply(img, value),
            Tool::LocalContrast => crate::local_contrast::apply(img, value),
            Tool::Contrast => map_rgb(img, &[crate::contrast::lut(value); 3]),
            Tool::Colour => crate::colour::apply(img, value),
            Tool::Sharpen => crate::sharpen::apply(img, value, scale),
            Tool::Vignette => crate::vignette::apply(img, value),
            Tool::Straighten => *img = crate::straighten::apply(img, value),
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

/// `tool`'s opening value for `src` with `stack`'s geometry, as the app measures it (on
/// [`EditStack::geometry_for`]). Only a tool that measures the image renders it.
pub fn opening(src: &crate::Decoded, stack: &EditStack, tool: Tool) -> crate::Result<f32> {
    if let Some(v) = tool.fixed_opening() {
        return Ok(v);
    }
    let img = crate::render::render(src, &stack.geometry_for(tool), Default::default())?;
    Ok(tool.opening(&img))
}

/// `stack` applied to `base`, the photo with [`EditStack::geometry_for`] `tool`, rendered
/// at `scale` of the full-size photo: what the geometry left out (the straightening, for
/// Straighten), then the adjustments.
pub fn preview(base: &RgbaImage, stack: &EditStack, tool: Tool, scale: f32) -> RgbaImage {
    // A geometry tool (Straighten, the only one) is left out of `base`: apply it, as
    // `Tool::apply` would, without copying `base` first.
    let left_out = stack.adjust(tool).filter(|_| tool.is_geometry()).map(|v| tool.clamp(v));
    let mut img = match left_out {
        Some(v) if v != tool.neutral() => crate::straighten::apply(base, v),
        _ => base.clone(),
    };
    apply_all(&mut img, stack, scale);
    img
}

/// Apply every adjustment of `stack` to `img`, rendered at `scale` of the full-size photo
/// (1: full size), in [`Tool::ALL`] order. Straighten isn't one: it's done with the
/// geometry.
pub fn apply_all(img: &mut RgbaImage, stack: &EditStack, scale: f32) {
    for tool in Tool::ALL.into_iter().filter(|t| !t.is_geometry()) {
        if let Some(v) = stack.adjust(tool) {
            tool.apply_at(img, v, scale);
        }
    }
}

pub fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

pub fn linear_to_srgb(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

/// sRGB-encoded 0–1 → byte.
pub(crate) fn to_byte(v: f32) -> u8 {
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
pub(crate) const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// Each pixel's luminance (linear) and lightness (the luminance sRGB-encoded: perceptual,
/// like Lab L), for tools that change the lightness ([`relight`]).
pub(crate) fn lightness(img: &RgbaImage) -> (Vec<f32>, Vec<f32>) {
    let lin = linear_lut();
    let luma: Vec<f32> =
        img.pixels().map(|px| (0..3).map(|c| LUMA[c] * lin[px.0[c] as usize]).sum()).collect();
    let light = luma.iter().map(|y| linear_to_srgb(*y)).collect();
    (luma, light)
}

/// Give each pixel `i` of `img` (luminance `luma[i]`) the lightness `new(i)`, clamped to
/// 0–1, as a gain on linear RGB so the hue stays; alpha stays.
pub(crate) fn relight(img: &mut RgbaImage, luma: &[f32], new: impl Fn(usize) -> f32) {
    let lin = linear_lut();
    for (i, px) in img.pixels_mut().enumerate() {
        let new = new(i).clamp(0.0, 1.0);
        let k = if luma[i] > 1e-5 { srgb_to_linear(new) / luma[i] } else { 1.0 };
        for c in &mut px.0[..3] {
            *c = encode(lin[*c as usize] * k);
        }
    }
}

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

    /// Whether the stack has an adjustment that changes the tones (any but the geometry).
    pub fn has_tones(&self) -> bool {
        self.adjustments().any(|(t, _)| !t.is_geometry())
    }

    /// The adjustment steps, in the stack's order.
    pub fn adjustments(&self) -> impl Iterator<Item = (Tool, f32)> + '_ {
        self.ops.iter().filter_map(|op| match op {
            Op::Adjust { tool, value } => Some((*tool, *value)),
            _ => None,
        })
    }

    /// The stack without its adjustments, except Straighten: what decides the photo's
    /// shape.
    pub fn geometry(&self) -> EditStack {
        let shape = |op: &&Op| !matches!(op, Op::Adjust { tool, .. } if !tool.is_geometry());
        let ops = self.ops.iter().filter(shape).cloned().collect();
        EditStack { ops, ..self.clone() }
    }

    /// The geometry `tool` opens on, measures its opening value on and previews from:
    /// [`EditStack::geometry`], but for Straighten without the straightening and the crop
    /// (it shows the whole photo, levelled as it turns).
    pub fn geometry_for(&self, tool: Tool) -> EditStack {
        self.shown_with(tool).geometry()
    }

    /// The stack as the photo shows while `tool` is open: all of it, but for a geometry
    /// tool (Straighten) without the crop and the tool's own step: the whole photo, which
    /// the preview turns.
    pub fn shown_with(&self, tool: Tool) -> EditStack {
        let mut s = self.clone();
        if tool.is_geometry() {
            s.set_crop(None);
            s.set_adjust(tool, None);
        }
        s
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
        assert!("blur".parse::<Tool>().unwrap_err().contains(
            "one of straighten, levels, warmth, dehaze, exposure, shadows_highlights, local_contrast, contrast, colour, sharpen, vignette"
        ));
        assert_eq!("color".parse::<Tool>(), Ok(Tool::Colour));
        // Levels goes before exposure, whatever order they're set in.
        s.set_adjust(Tool::Exposure, Some(0.5));
        s.set_adjust(Tool::Levels, Some(1.0));
        assert_eq!(s.adjustments().collect::<Vec<_>>(), [(Tool::Levels, 1.0), (Tool::Exposure, 0.5)]);
    }
}
