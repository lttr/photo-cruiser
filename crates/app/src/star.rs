//! The star of a photo to keep: an image drawn once here, antialiased (the app turns the
//! renderer's antialiasing off) and independent of the font's glyphs.

use crate::message::Message;
use iced::widget::image::Handle;
use iced::{ContentFit, Element};
use std::sync::OnceLock;

/// Pixels of the drawn star; shown smaller, so it stays sharp on HiDPI.
const SIDE: u32 = 96;
/// Gold, with a dark rim so it shows on any photo.
const FILL: [u8; 3] = [250, 199, 46];
const RIM: [u8; 3] = [20, 20, 20];

/// A five-pointed star centred in a square of side 1, `grow` outwards.
fn inside(x: f32, y: f32, grow: f32) -> bool {
    let (outer, inner) = (0.47 + grow, 0.47 * 0.45 + grow);
    let pts: Vec<(f32, f32)> = (0..10)
        .map(|k| {
            let r = if k % 2 == 0 { outer } else { inner };
            let a = std::f32::consts::PI * (k as f32 / 5.0 - 0.5);
            (0.5 + r * a.cos(), 0.53 + r * a.sin())
        })
        .collect();
    // Even-odd ray casting.
    let mut odd = false;
    for i in 0..pts.len() {
        let ((x1, y1), (x2, y2)) = (pts[i], pts[(i + 1) % pts.len()]);
        if (y1 > y) != (y2 > y) && x < x1 + (y - y1) / (y2 - y1) * (x2 - x1) {
            odd = !odd;
        }
    }
    odd
}

fn handle() -> Handle {
    static STAR: OnceLock<Handle> = OnceLock::new();
    STAR.get_or_init(|| {
        const SS: u32 = 4;
        let rim = 0.035;
        let mut px = Vec::with_capacity((SIDE * SIDE * 4) as usize);
        for y in 0..SIDE {
            for x in 0..SIDE {
                let (mut fill, mut edge) = (0u32, 0u32);
                for sy in 0..SS {
                    for sx in 0..SS {
                        let fx = (x as f32 + (sx as f32 + 0.5) / SS as f32) / SIDE as f32;
                        let fy = (y as f32 + (sy as f32 + 0.5) / SS as f32) / SIDE as f32;
                        if inside(fx, fy, -rim) {
                            fill += 1;
                        } else if inside(fx, fy, 0.0) {
                            edge += 1;
                        }
                    }
                }
                let n = (SS * SS) as f32;
                let cover = (fill + edge) as f32 / n;
                let t = if fill + edge == 0 { 0.0 } else { fill as f32 / (fill + edge) as f32 };
                let mix = |c: usize| (FILL[c] as f32 * t + RIM[c] as f32 * (1.0 - t)).round() as u8;
                px.extend([mix(0), mix(1), mix(2), (cover * 255.0).round() as u8]);
            }
        }
        Handle::from_rgba(SIDE, SIDE, px)
    })
    .clone()
}

/// A star `side` units wide and high.
pub fn star<'a>(side: u16) -> Element<'a, Message> {
    let side = f32::from(side);
    iced::widget::image(handle()).width(side).height(side).content_fit(ContentFit::Contain).into()
}
