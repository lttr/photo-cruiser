//! The Contrast tool's pixel math: an S-curve like darktable's sigmoid, without its other
//! controls. The value is the curve's contrast, 1–2; 1 changes nothing.
//!
//! The curve is a log-logistic one, the family sigmoid is built on, pinned so that black
//! stays black, white stays white and middle grey stays middle grey (the photo is already
//! display-ready, so there's no scene range to compress). In linear light, a pixel's odds
//! `x / (1 − x)` are raised to the power of the contrast around middle grey's odds. So the
//! slope at middle grey (in log–log) is the contrast, shadows go darker and highlights
//! brighter, and both roll off smoothly into black and white without clipping.
//!
//! It's applied per channel (darktable's "per channel" mode), so colours get a little
//! stronger with the contrast, as on film.

use crate::adjust::{MIDDLE_GREY, linear_to_srgb, to_byte};
use crate::warmth::linear_lut;

/// Linear light `x` (0–1) through the curve at `contrast` (1 changes nothing).
pub fn curve(x: f32, contrast: f32) -> f32 {
    if x <= 0.0 || x >= 1.0 {
        return x.clamp(0.0, 1.0);
    }
    let grey = MIDDLE_GREY / (1.0 - MIDDLE_GREY);
    let odds = grey * (x / (1.0 - x) / grey).powf(contrast);
    odds / (1.0 + odds)
}

/// The curve as an 8-bit table on sRGB-encoded values.
pub fn lut(contrast: f32) -> [u8; 256] {
    let lin = linear_lut();
    std::array::from_fn(|i| to_byte(linear_to_srgb(curve(lin[i], contrast))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_black_grey_and_white() {
        for c in [1.0, 1.2, 2.0] {
            assert_eq!(curve(0.0, c), 0.0);
            assert_eq!(curve(1.0, c), 1.0);
            assert!((curve(MIDDLE_GREY, c) - MIDDLE_GREY).abs() < 1e-6, "{c}");
        }
    }

    #[test]
    fn one_changes_nothing() {
        let lut = lut(1.0);
        assert!(lut.iter().enumerate().all(|(i, v)| usize::from(*v) == i));
    }

    #[test]
    fn darker_shadows_brighter_highlights_by_contrast() {
        let (soft, hard) = (lut(1.2), lut(2.0));
        // sRGB 118 is middle grey.
        for i in (5..110).step_by(5) {
            assert!(hard[i] < soft[i] && soft[i] < i as u8, "shadow {i}: {} {}", soft[i], hard[i]);
        }
        for i in (125..250).step_by(5) {
            assert!(hard[i] > soft[i] && soft[i] > i as u8, "highlight {i}: {} {}", soft[i], hard[i]);
        }
        // Monotonic, so no tones swap places.
        assert!(hard.windows(2).all(|w| w[0] <= w[1]));
        // No clipping: near-black and near-white pixels keep apart from the ends.
        assert!(hard[30] > 0 && hard[240] < 255, "{} {}", hard[30], hard[240]);
    }

    #[test]
    fn slope_at_middle_grey_is_the_contrast() {
        let d = 1e-3;
        for c in [1.2, 1.5, 2.0] {
            let slope =
                (curve(MIDDLE_GREY * (1.0 + d), c).ln() - curve(MIDDLE_GREY, c).ln()) / (1.0 + d).ln();
            // Log–log slope of the odds is c; of the value itself, c·(1 − y)/(1 − x) = c at grey.
            assert!((slope - c).abs() < 0.01, "{c}: {slope}");
        }
    }
}
