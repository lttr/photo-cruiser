//! Interactive crop selection: draw, move and resize with the mouse, clamped to the image.
//! All coordinates are source pixels; the caller maps from view coordinates (see
//! [`crate::geom::ViewMap`]) and passes the grab tolerance converted to source pixels.
//!
//! The selection keeps an [`Aspect`] ratio while drawn and resized: the photo's own by
//! default. Corner drags keep the opposite corner, edge drags the opposite edge (the
//! other axis grows from the middle).

use crate::geom::{Point, Rect, Size};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// The shape a crop keeps. JSON (and [`FromStr`]): `"original"`, `"free"`, `"16:9"`, `"2.35"`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Aspect {
    /// The photo's own.
    #[default]
    Original,
    /// Any shape.
    Free,
    /// `w:h`, turned to the photo's orientation (3:2 is 2:3 on a portrait photo).
    Ratio { w: f32, h: f32 },
}

/// Ratios outside `1/MAX_RATIO..MAX_RATIO` are rejected.
pub const MAX_RATIO: f32 = 20.0;

impl Aspect {
    /// The choices offered (the footer, `A` cycles through them).
    pub const PRESETS: [Aspect; 7] = [
        Aspect::Original,
        Aspect::Free,
        Aspect::Ratio { w: 1.0, h: 1.0 },
        Aspect::Ratio { w: 3.0, h: 2.0 },
        Aspect::Ratio { w: 4.0, h: 3.0 },
        Aspect::Ratio { w: 5.0, h: 4.0 },
        Aspect::Ratio { w: 16.0, h: 9.0 },
    ];

    /// The next preset (a custom ratio goes back to the first).
    pub fn next(self) -> Aspect {
        let i = Self::PRESETS.iter().position(|a| *a == self);
        Self::PRESETS[i.map_or(0, |i| (i + 1) % Self::PRESETS.len())]
    }

    pub fn is_preset(self) -> bool {
        Self::PRESETS.contains(&self)
    }

    /// Width / height of the crop on an image of size `img`, `None` for free. `swapped`
    /// turns it the other way (portrait ↔ landscape).
    pub fn ratio(self, img: Size, swapped: bool) -> Option<f32> {
        let photo = img.width / img.height.max(1.0);
        let r = match self {
            Aspect::Free => return None,
            Aspect::Original => photo,
            Aspect::Ratio { w, h } if (w >= h) == (photo >= 1.0) => w / h,
            Aspect::Ratio { w, h } => h / w,
        };
        Some(if swapped { 1.0 / r } else { r })
    }
}

impl fmt::Display for Aspect {
    /// `original`, `free`, `16:9`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Aspect::Original => f.write_str("original"),
            Aspect::Free => f.write_str("free"),
            Aspect::Ratio { w, h } => write!(f, "{w}:{h}"),
        }
    }
}

impl FromStr for Aspect {
    type Err = String;

    /// `original`, `free`, `W:H` (also `WxH`, `W/H`) or a single ratio like `2.35`.
    fn from_str(s: &str) -> Result<Self, String> {
        let s = s.trim();
        match s.to_ascii_lowercase().as_str() {
            "original" => return Ok(Aspect::Original),
            "free" => return Ok(Aspect::Free),
            _ => {}
        }
        let bad = || format!("bad aspect ratio {s:?}: expected original, free, W:H or a number");
        let num = |t: &str| t.trim().parse::<f32>().ok().filter(|n| n.is_finite() && *n > 0.0);
        let (w, h) = match s.split_once([':', 'x', 'X', '/', '×']) {
            Some((w, h)) => (num(w).ok_or_else(bad)?, num(h).ok_or_else(bad)?),
            None => (num(s).ok_or_else(bad)?, 1.0),
        };
        if !(1.0 / MAX_RATIO..=MAX_RATIO).contains(&(w / h)) {
            return Err(format!("aspect ratio {s:?} out of range (1:{MAX_RATIO} to {MAX_RATIO}:1)"));
        }
        Ok(Aspect::Ratio { w, h })
    }
}

impl TryFrom<String> for Aspect {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        s.parse()
    }
}

impl From<Aspect> for String {
    fn from(a: Aspect) -> String {
        a.to_string()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MouseKind {
    Press,
    Move,
    Release,
}

/// What a press at a point would do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    New,
    Move,
    /// Which edges follow the cursor.
    Resize {
        left: bool,
        right: bool,
        top: bool,
        bottom: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Drag {
    hit: Hit,
    /// Press position (unclamped).
    start: Point,
    orig: Option<Rect>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CropTool {
    pub selection: Option<Rect>,
    drag: Option<Drag>,
    /// The shape drawing and resizing keep.
    pub aspect: Aspect,
    /// The aspect turned the other way (portrait ↔ landscape).
    pub swapped: bool,
}

impl CropTool {
    /// Drop the selection (the aspect stays).
    pub fn clear(&mut self) {
        self.selection = None;
        self.drag = None;
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Use `aspect` from now on; a selection is reshaped to it (same area and centre, inside
    /// the image).
    pub fn set_aspect(&mut self, aspect: Aspect, img: Size) {
        self.aspect = aspect;
        self.reshape(img);
    }

    /// Turn the aspect the other way (portrait ↔ landscape), reshaping the selection.
    pub fn swap(&mut self, img: Size) {
        self.swapped = !self.swapped;
        self.reshape(img);
    }

    fn reshape(&mut self, img: Size) {
        self.drag = None;
        let (Some(s), Some(r)) = (self.selection, self.aspect.ratio(img, self.swapped)) else { return };
        let w = (s.w * s.h * r).sqrt();
        let k = (img.width / w).min(img.height * r / w).min(1.0);
        let (w, h) = (w * k, w * k / r);
        let c = s.center();
        let x = (c.x - w / 2.0).clamp(0.0, img.width - w);
        let y = (c.y - h / 2.0).clamp(0.0, img.height - h);
        self.selection = Some(Rect::new(x, y, w, h));
    }

    /// Set the selection directly (scripts, tests), as given (the aspect isn't applied);
    /// clamped, empty → none.
    pub fn set(&mut self, rect: Rect, img: Size) {
        self.drag = None;
        let r = rect.clamp_to(img);
        self.selection = (r.w >= 1.0 && r.h >= 1.0).then_some(r);
    }

    /// Handle a mouse event at `p` on an image of size `img`, with edge grab tolerance `tol`.
    pub fn mouse(&mut self, kind: MouseKind, p: Point, tol: f32, img: Size) {
        let clamp = |p: Point| Point::new(p.x.clamp(0.0, img.width), p.y.clamp(0.0, img.height));
        match kind {
            MouseKind::Press => {
                let hit = hit(self.selection, p, tol);
                if hit == Hit::New {
                    if !Rect::with_size(img).expand(tol).contains(p) {
                        return;
                    }
                    self.selection = None;
                }
                self.drag = Some(Drag { hit, start: p, orig: self.selection });
            }
            MouseKind::Move | MouseKind::Release => {
                let Some(drag) = self.drag else { return };
                let (start, pos) = (clamp(drag.start), clamp(p));
                let ratio = self.aspect.ratio(img, self.swapped);
                self.selection = Some(match (drag.hit, drag.orig, ratio) {
                    (Hit::Move, Some(o), _) => Rect {
                        x: (o.x + p.x - drag.start.x).clamp(0.0, img.width - o.w),
                        y: (o.y + p.y - drag.start.y).clamp(0.0, img.height - o.h),
                        ..o
                    },
                    (Hit::New, _, Some(r)) | (_, None, Some(r)) => anchored(start, pos, r, img),
                    (Hit::Resize { left, right, top, bottom }, Some(o), Some(r)) => {
                        match (left || right, top || bottom) {
                            (true, true) => {
                                let anchor = Point::new(
                                    if left { o.right() } else { o.x },
                                    if top { o.bottom() } else { o.y },
                                );
                                anchored(anchor, pos, r, img)
                            }
                            (true, false) => edge(o, left, pos, r, img),
                            // A top/bottom drag is a left/right one with x and y swapped.
                            _ => {
                                transpose(edge(transpose(o), top, pos.transpose(), 1.0 / r, img.transpose()))
                            }
                        }
                    }
                    (Hit::New, _, None) | (_, None, None) => Rect::from_corners(start, pos),
                    (Hit::Resize { left, right, top, bottom }, Some(o), None) => {
                        let (mut x0, mut y0, mut x1, mut y1) = (o.x, o.y, o.right(), o.bottom());
                        if left {
                            x0 = pos.x;
                        }
                        if right {
                            x1 = pos.x;
                        }
                        if top {
                            y0 = pos.y;
                        }
                        if bottom {
                            y1 = pos.y;
                        }
                        Rect::from_corners(Point::new(x0, y0), Point::new(x1, y1))
                    }
                });
                if kind == MouseKind::Release {
                    self.drag = None;
                    // A click without a drag leaves no selection.
                    if self.selection.is_some_and(|s| s.w < 1.0 || s.h < 1.0) {
                        self.selection = None;
                    }
                }
            }
        }
    }
}

/// The largest rectangle of ratio `r` with a corner at `anchor`, reaching towards `to`
/// (covering it where the image allows), inside `img`.
fn anchored(anchor: Point, to: Point, r: f32, img: Size) -> Rect {
    let (dx, dy) = (to.x - anchor.x, to.y - anchor.y);
    let room_w = if dx >= 0.0 { img.width - anchor.x } else { anchor.x };
    let room_h = if dy >= 0.0 { img.height - anchor.y } else { anchor.y };
    let w = dx.abs().max(dy.abs() * r).min(room_w).min(room_h * r);
    let h = w / r;
    let x = if dx >= 0.0 { anchor.x } else { anchor.x - w };
    let y = if dy >= 0.0 { anchor.y } else { anchor.y - h };
    Rect::new(x, y, w, h)
}

/// The left (`left`) or right edge of `o` dragged to `to.x`: the opposite edge stays, the
/// height follows ratio `r` around `o`'s middle, inside `img`.
fn edge(o: Rect, left: bool, to: Point, r: f32, img: Size) -> Rect {
    let ax = if left { o.right() } else { o.x };
    let dx = to.x - ax;
    let cy = o.center().y;
    let room_w = if dx >= 0.0 { img.width - ax } else { ax };
    let w = dx.abs().min(room_w).min(2.0 * cy.min(img.height - cy) * r);
    let h = w / r;
    Rect::new(if dx >= 0.0 { ax } else { ax - w }, cy - h / 2.0, w, h)
}

fn transpose(r: Rect) -> Rect {
    Rect::new(r.y, r.x, r.h, r.w)
}

/// Hit-test `p` against `selection` with grab tolerance `tol`.
pub fn hit(selection: Option<Rect>, p: Point, tol: f32) -> Hit {
    let Some(r) = selection else { return Hit::New };
    if !r.expand(tol).contains(p) {
        return Hit::New;
    }
    let left = (p.x - r.x).abs() <= tol;
    let right = !left && (p.x - r.right()).abs() <= tol;
    let top = (p.y - r.y).abs() <= tol;
    let bottom = !top && (p.y - r.bottom()).abs() <= tol;
    if left || right || top || bottom { Hit::Resize { left, right, top, bottom } } else { Hit::Move }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMG: Size = Size::new(1000.0, 500.0);

    fn drag(t: &mut CropTool, a: (f32, f32), b: (f32, f32)) {
        t.mouse(MouseKind::Press, Point::new(a.0, a.1), 4.0, IMG);
        t.mouse(MouseKind::Move, Point::new((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0), 4.0, IMG);
        t.mouse(MouseKind::Release, Point::new(b.0, b.1), 4.0, IMG);
    }

    fn free() -> CropTool {
        CropTool { aspect: Aspect::Free, ..CropTool::default() }
    }

    #[test]
    fn draw_move_resize() {
        let mut t = free();
        drag(&mut t, (600.0, 400.0), (100.0, 100.0));
        assert_eq!(t.selection, Some(Rect::new(100.0, 100.0, 500.0, 300.0)));
        drag(&mut t, (300.0, 200.0), (350.0, 250.0));
        assert_eq!(t.selection, Some(Rect::new(150.0, 150.0, 500.0, 300.0)));
        drag(&mut t, (650.0, 450.0), (700.0, 480.0)); // bottom-right corner
        assert_eq!(t.selection, Some(Rect::new(150.0, 150.0, 550.0, 330.0)));
        assert!(!t.is_dragging());
    }

    #[test]
    fn clamps_to_image() {
        let mut t = free();
        drag(&mut t, (500.0, 250.0), (5000.0, -300.0));
        assert_eq!(t.selection, Some(Rect::new(500.0, 0.0, 500.0, 250.0)));
        drag(&mut t, (700.0, 100.0), (2000.0, 2000.0)); // move far away: stays inside
        assert_eq!(t.selection, Some(Rect::new(500.0, 250.0, 500.0, 250.0)));
    }

    #[test]
    fn click_clears_and_outside_press_ignored() {
        let mut t = free();
        drag(&mut t, (10.0, 10.0), (100.0, 100.0));
        drag(&mut t, (500.0, 300.0), (500.0, 300.0));
        assert_eq!(t.selection, None);
        drag(&mut t, (-100.0, -100.0), (100.0, 100.0));
        assert_eq!(t.selection, None);
    }

    #[test]
    fn set_clamps() {
        let mut t = CropTool::default();
        t.set(Rect::new(900.0, 400.0, 500.0, 500.0), IMG);
        assert_eq!(t.selection, Some(Rect::new(900.0, 400.0, 100.0, 100.0)));
        t.set(Rect::new(2000.0, 0.0, 10.0, 10.0), IMG);
        assert_eq!(t.selection, None);
    }

    #[test]
    fn keeps_the_photo_aspect_by_default() {
        let mut t = CropTool::default(); // IMG is 2:1
        drag(&mut t, (100.0, 100.0), (300.0, 300.0)); // the larger side wins
        assert_eq!(t.selection, Some(Rect::new(100.0, 100.0, 400.0, 200.0)));
        drag(&mut t, (500.0, 300.0), (520.0, 250.0)); // bottom-right corner: top-left stays
        assert_eq!(t.selection, Some(Rect::new(100.0, 100.0, 420.0, 210.0)));
        drag(&mut t, (100.0, 200.0), (300.0, 200.0)); // left edge: grows from the middle
        assert_eq!(t.selection, Some(Rect::new(300.0, 150.0, 220.0, 110.0)));
        drag(&mut t, (400.0, 150.0), (400.0, 50.0)); // top edge
        assert_eq!(t.selection, Some(Rect::new(200.0, 50.0, 420.0, 210.0)));
        drag(&mut t, (400.0, 400.0), (0.0, 0.0)); // up-left, stopped by the left edge
        assert_eq!(t.selection, Some(Rect::new(0.0, 200.0, 400.0, 200.0)));
        drag(&mut t, (900.0, 100.0), (2000.0, 120.0)); // right edge of the image stops it
        assert_eq!(t.selection, Some(Rect::new(900.0, 100.0, 100.0, 50.0)));
        drag(&mut t, (5.0, 250.0), (500.0, 250.0)); // edge drag limited by the height around the middle
        let s = t.selection.unwrap();
        assert!((s.w / s.h - 2.0).abs() < 1e-4 && s.y >= 0.0 && s.bottom() <= 500.0, "{s:?}");
    }

    #[test]
    fn ratio_follows_orientation_and_swaps() {
        let land = Size::new(600.0, 400.0);
        let port = land.transpose();
        let r32 = Aspect::Ratio { w: 3.0, h: 2.0 };
        assert_eq!(r32.ratio(land, false), Some(1.5));
        assert_eq!(r32.ratio(port, false), Some(2.0 / 3.0));
        assert_eq!(r32.ratio(land, true), Some(2.0 / 3.0));
        assert_eq!(Aspect::Ratio { w: 2.0, h: 3.0 }.ratio(land, false), Some(1.5));
        assert_eq!(Aspect::Original.ratio(port, false), Some(2.0 / 3.0));
        assert_eq!(Aspect::Original.ratio(port, true), Some(1.5));
        assert_eq!(Aspect::Free.ratio(land, true), None);
    }

    #[test]
    fn changing_the_aspect_reshapes_the_selection() {
        let mut t = free();
        t.set(Rect::new(100.0, 100.0, 300.0, 300.0), IMG);
        t.set_aspect(Aspect::Ratio { w: 4.0, h: 1.0 }, IMG); // same area and centre
        assert_eq!(t.selection, Some(Rect::new(0.0, 175.0, 600.0, 150.0)));
        t.swap(IMG); // 1:4 on a 500 px high image: shrinks to fit
        assert_eq!(t.selection, Some(Rect::new(237.5, 0.0, 125.0, 500.0)));
        t.set_aspect(Aspect::Free, IMG);
        assert_eq!(t.selection, Some(Rect::new(237.5, 0.0, 125.0, 500.0)));
        t.clear();
        assert_eq!((t.aspect, t.swapped), (Aspect::Free, true), "clear keeps the aspect");
    }

    #[test]
    fn aspect_parse_and_json() {
        assert_eq!("original".parse(), Ok(Aspect::Original));
        assert_eq!(" Free ".parse(), Ok(Aspect::Free));
        assert_eq!("16:9".parse(), Ok(Aspect::Ratio { w: 16.0, h: 9.0 }));
        assert_eq!("4x5".parse(), Ok(Aspect::Ratio { w: 4.0, h: 5.0 }));
        assert_eq!("2.35".parse(), Ok(Aspect::Ratio { w: 2.35, h: 1.0 }));
        for bad in ["", "0:1", "1:", "a:b", "-1:2", "100:1", "inf"] {
            assert!(bad.parse::<Aspect>().is_err(), "{bad:?}");
        }
        assert_eq!(serde_json::to_string(&Aspect::Ratio { w: 2.35, h: 1.0 }).unwrap(), r#""2.35:1""#);
        assert_eq!(serde_json::from_str::<Aspect>(r#""original""#).unwrap(), Aspect::Original);
        assert!(serde_json::from_str::<Aspect>(r#""wide""#).is_err());
        assert_eq!(Aspect::Original.next(), Aspect::Free);
        assert_eq!(Aspect::PRESETS[6].next(), Aspect::Original);
        assert_eq!(Aspect::Ratio { w: 7.0, h: 5.0 }.next(), Aspect::Original);
    }
}
