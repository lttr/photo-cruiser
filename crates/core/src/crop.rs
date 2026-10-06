//! Interactive crop selection: draw, move and resize with the mouse, clamped to the image.
//! All coordinates are source pixels; the caller maps from view coordinates (see
//! [`crate::geom::ViewMap`]) and passes the grab tolerance converted to source pixels.

use crate::geom::{Point, Rect, Size};
use serde::{Deserialize, Serialize};

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
}

impl CropTool {
    pub fn clear(&mut self) {
        self.selection = None;
        self.drag = None;
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Set the selection directly (scripts, tests); clamped, empty → none.
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
                self.selection = Some(match (drag.hit, drag.orig) {
                    (Hit::New, _) | (_, None) => Rect::from_corners(start, pos),
                    (Hit::Move, Some(o)) => Rect {
                        x: (o.x + p.x - drag.start.x).clamp(0.0, img.width - o.w),
                        y: (o.y + p.y - drag.start.y).clamp(0.0, img.height - o.h),
                        ..o
                    },
                    (Hit::Resize { left, right, top, bottom }, Some(o)) => {
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

    #[test]
    fn draw_move_resize() {
        let mut t = CropTool::default();
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
        let mut t = CropTool::default();
        drag(&mut t, (500.0, 250.0), (5000.0, -300.0));
        assert_eq!(t.selection, Some(Rect::new(500.0, 0.0, 500.0, 250.0)));
        drag(&mut t, (700.0, 100.0), (2000.0, 2000.0)); // move far away: stays inside
        assert_eq!(t.selection, Some(Rect::new(500.0, 250.0, 500.0, 250.0)));
    }

    #[test]
    fn click_clears_and_outside_press_ignored() {
        let mut t = CropTool::default();
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
}
