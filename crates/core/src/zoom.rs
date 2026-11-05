//! Zooming into one photo: how much bigger than fit-to-view it is drawn, and which part of
//! it is at the view's centre. Independent of the image's resolution (a downscaled preview
//! and the full image zoom alike), so only the image's aspect and, for "actual pixels",
//! its source size matter.

use crate::geom::{Point, Rect, Size, ViewMap, fit_rect};
use serde::{Deserialize, Serialize};

/// The largest zoom shows one source pixel this many view units wide (at least 4× fit).
pub const MAX_PIXEL: f32 = 8.0;
/// One zoom in / out step.
pub const STEP: f32 = std::f32::consts::SQRT_2;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Zoom {
    /// Multiple of the fit size; 1 shows the whole image.
    pub scale: f32,
    /// The image point at the view's centre, as a fraction of its width and height.
    pub x: f32,
    pub y: f32,
}

impl Default for Zoom {
    fn default() -> Self {
        Zoom::FIT
    }
}

impl Zoom {
    pub const FIT: Zoom = Zoom { scale: 1.0, x: 0.5, y: 0.5 };

    pub fn is_fit(&self) -> bool {
        self.scale <= 1.0
    }

    /// The scale that shows one source pixel per view unit (at least fit).
    pub fn actual(bounds: Rect, source: Size) -> f32 {
        let fit = fit_rect(bounds, source);
        if fit.w <= 0.0 { 1.0 } else { (source.width / fit.w).max(1.0) }
    }

    pub fn max_scale(bounds: Rect, source: Size) -> f32 {
        let fit = fit_rect(bounds, source);
        if fit.w <= 0.0 { 1.0 } else { (source.width / fit.w * MAX_PIXEL).max(4.0) }
    }

    /// Where an image of `img`'s aspect is drawn in `bounds`. On an axis where it is bigger
    /// than the view, it never leaves a gap at an edge; on one where it is smaller, it is
    /// centred.
    pub fn rect(&self, bounds: Rect, img: Size) -> Rect {
        let fit = fit_rect(bounds, img);
        let (w, h) = (fit.w * self.scale, fit.h * self.scale);
        let place = |start: f32, len: f32, size: f32, at: f32| {
            if size <= len {
                start + (len - size) / 2.0
            } else {
                (start + len / 2.0 - at * size).clamp(start + len - size, start)
            }
        };
        Rect::new(place(bounds.x, bounds.w, w, self.x), place(bounds.y, bounds.h, h, self.y), w, h)
    }

    /// View ↔ image pixels (of size `img`) for this zoom.
    pub fn map(&self, bounds: Rect, img: Size) -> ViewMap {
        let r = self.rect(bounds, img);
        ViewMap { fit: r, scale: if img.width > 0.0 { r.w / img.width } else { 1.0 } }
    }

    /// The same view, with the centre moved to what is really shown (no gaps) and the scale
    /// in `1..=max`.
    pub fn clamped(&self, bounds: Rect, source: Size) -> Zoom {
        let scale = self.scale.clamp(1.0, Zoom::max_scale(bounds, source));
        if scale <= 1.0 {
            return Zoom::FIT;
        }
        let z = Zoom { scale, ..*self };
        let r = z.rect(bounds, source);
        let c = bounds.center();
        Zoom { scale, x: (c.x - r.x) / r.w, y: (c.y - r.y) / r.h }
    }

    /// Zoom to `scale`, keeping the image point under the view point `at` where it is.
    pub fn zoom_at(&self, bounds: Rect, source: Size, scale: f32, at: Point) -> Zoom {
        let scale = scale.clamp(1.0, Zoom::max_scale(bounds, source));
        let p = self.map(bounds, source).to_source(at);
        let fit = fit_rect(bounds, source);
        let s = fit.w / source.width * scale;
        let c = bounds.center();
        let centre = Point::new(p.x - (at.x - c.x) / s, p.y - (at.y - c.y) / s);
        Zoom { scale, x: centre.x / source.width, y: centre.y / source.height }.clamped(bounds, source)
    }

    /// Multiply the scale by `factor`, keeping the point under `at` in place.
    pub fn by(&self, bounds: Rect, source: Size, factor: f32, at: Point) -> Zoom {
        self.zoom_at(bounds, source, self.scale * factor, at)
    }

    /// Move the image by `dx`, `dy` view units (a drag).
    pub fn pan(&self, bounds: Rect, source: Size, dx: f32, dy: f32) -> Zoom {
        let r = self.rect(bounds, source);
        let z = Zoom { x: self.x - dx / r.w, y: self.y - dy / r.h, ..*self };
        z.clamped(bounds, source)
    }

    /// Fit ↔ actual pixels at `at` (a double-click).
    pub fn toggle_actual(&self, bounds: Rect, source: Size, at: Point) -> Zoom {
        match self.is_fit() {
            true => self.zoom_at(bounds, source, Zoom::actual(bounds, source), at),
            false => Zoom::FIT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: Rect = Rect::new(0.0, 0.0, 1000.0, 500.0);
    const IMG: Size = Size::new(4000.0, 2000.0);

    fn close(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 1e-2 && (a.y - b.y).abs() < 1e-2
    }

    #[test]
    fn fit_is_contain() {
        assert_eq!(Zoom::FIT.rect(VIEW, IMG), fit_rect(VIEW, IMG));
        assert_eq!(Zoom::FIT.map(VIEW, IMG), ViewMap::new(VIEW, IMG));
    }

    #[test]
    fn actual_pixels_and_limits() {
        assert_eq!(Zoom::actual(VIEW, IMG), 4.0);
        assert_eq!(Zoom::max_scale(VIEW, IMG), 32.0);
        // Smaller than the view: fit already shows it bigger than 1:1.
        assert_eq!(Zoom::actual(VIEW, Size::new(100.0, 50.0)), 1.0);
        assert_eq!(Zoom::max_scale(VIEW, Size::new(100.0, 50.0)), 4.0);
        let z = Zoom { scale: 100.0, x: 0.5, y: 0.5 }.clamped(VIEW, IMG);
        assert_eq!(z.scale, 32.0);
        assert_eq!(Zoom { scale: 0.5, x: 0.1, y: 0.1 }.clamped(VIEW, IMG), Zoom::FIT);
    }

    #[test]
    fn zoom_keeps_the_point_under_the_cursor() {
        let at = Point::new(800.0, 100.0);
        let before = Zoom::FIT.map(VIEW, IMG).to_source(at);
        let z = Zoom::FIT.by(VIEW, IMG, 2.0, at);
        assert_eq!(z.scale, 2.0);
        assert!(close(z.map(VIEW, IMG).to_source(at), before));
        let z2 = z.by(VIEW, IMG, 1.5, at);
        assert!(close(z2.map(VIEW, IMG).to_source(at), before));
    }

    #[test]
    fn no_gaps_at_the_edges() {
        // Zooming at the very corner: the image's corner stays in the view's corner.
        let z = Zoom::FIT.by(VIEW, IMG, 3.0, Point::new(0.0, 0.0));
        let r = z.rect(VIEW, IMG);
        assert_eq!((r.x, r.y, r.w, r.h), (0.0, 0.0, 3000.0, 1500.0));
        // Panning past the edge stops there.
        let p = z.pan(VIEW, IMG, 500.0, 500.0);
        assert_eq!(p, z);
        let p = z.pan(VIEW, IMG, -10_000.0, 0.0).rect(VIEW, IMG);
        assert_eq!(p.right(), VIEW.right());
    }

    #[test]
    fn smaller_axis_stays_centred() {
        // A tall image in a wide view: at 1.5× it is still narrower than the view.
        let tall = Size::new(1000.0, 2000.0);
        let z = Zoom::FIT.by(VIEW, tall, 1.5, Point::new(900.0, 250.0));
        let r = z.rect(VIEW, tall);
        assert_eq!(r.center().x, VIEW.center().x);
        assert_eq!(r.h, 750.0);
    }

    #[test]
    fn toggle_actual_and_back() {
        let z = Zoom::FIT.toggle_actual(VIEW, IMG, Point::new(500.0, 250.0));
        assert_eq!(z, Zoom { scale: 4.0, x: 0.5, y: 0.5 });
        assert_eq!(z.toggle_actual(VIEW, IMG, Point::new(1.0, 1.0)), Zoom::FIT);
    }
}
