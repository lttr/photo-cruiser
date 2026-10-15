//! Geometry in floating-point pixels. Crop rectangles live in *source-image* pixels
//! (after EXIF/HEIF orientation), so they survive window resizes and downscaled previews.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Point { x, y }
    }

    /// x and y swapped.
    pub const fn transpose(self) -> Self {
        Point::new(self.y, self.x)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const fn new(width: f32, height: f32) -> Self {
        Size { width, height }
    }

    /// Width and height swapped.
    pub const fn transpose(self) -> Self {
        Size::new(self.height, self.width)
    }

    /// The size after `quarters` clockwise quarter turns.
    pub fn rotate(self, quarters: u8) -> Self {
        if quarters % 2 == 1 { self.transpose() } else { self }
    }
}

impl From<(u32, u32)> for Size {
    fn from((w, h): (u32, u32)) -> Self {
        Size::new(w as f32, h as f32)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Integer pixel rectangle, e.g. the effective crop.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Rect { x, y, w, h }
    }

    pub fn with_size(size: Size) -> Self {
        Rect::new(0.0, 0.0, size.width, size.height)
    }

    /// The rectangle spanned by two corners, in any order.
    pub fn from_corners(a: Point, b: Point) -> Self {
        Rect::new(a.x.min(b.x), a.y.min(b.y), (a.x - b.x).abs(), (a.y - b.y).abs())
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn center(&self) -> Point {
        Point::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x <= self.right() && p.y >= self.y && p.y <= self.bottom()
    }

    pub fn expand(&self, by: f32) -> Rect {
        Rect::new(self.x - by, self.y - by, self.w + 2.0 * by, self.h + 2.0 * by)
    }

    /// Intersection with `0..size`. Empty (zero-sized) if fully outside.
    pub fn clamp_to(&self, size: Size) -> Rect {
        let x0 = self.x.clamp(0.0, size.width);
        let y0 = self.y.clamp(0.0, size.height);
        let x1 = self.right().clamp(0.0, size.width);
        let y1 = self.bottom().clamp(0.0, size.height);
        Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
    }

    /// Round the edges to whole pixels and clamp to `size`. `None` if empty.
    pub fn to_pixels(&self, size: (u32, u32)) -> Option<PixelRect> {
        let c = self.clamp_to(size.into());
        let (x0, y0, x1, y1) = (c.x.round(), c.y.round(), c.right().round(), c.bottom().round());
        let (w, h) = (x1 - x0, y1 - y0);
        (w >= 1.0 && h >= 1.0).then_some(PixelRect { x: x0 as u32, y: y0 as u32, w: w as u32, h: h as u32 })
    }
}

impl Rect {
    /// This rectangle (inside an image of `size`) after turning the image clockwise by
    /// `quarters` × 90°. Turning by `4 - quarters` in `size.rotate(quarters)` undoes it.
    pub fn rotate(&self, quarters: u8, size: Size) -> Rect {
        let (sw, sh) = (size.width, size.height);
        match quarters % 4 {
            1 => Rect::new(sh - self.bottom(), self.x, self.h, self.w),
            2 => Rect::new(sw - self.right(), sh - self.bottom(), self.w, self.h),
            3 => Rect::new(self.y, sw - self.right(), self.h, self.w),
            _ => *self,
        }
    }
}

impl From<PixelRect> for Rect {
    fn from(p: PixelRect) -> Self {
        Rect::new(p.x as f32, p.y as f32, p.w as f32, p.h as f32)
    }
}

impl std::fmt::Display for PixelRect {
    /// `W×H @ X,Y`, as shown in the status line.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}×{} @ {},{}", self.w, self.h, self.x, self.y)
    }
}

/// Where an image of size `img` is drawn inside `bounds`: scaled to fit (up or down),
/// aspect kept, centered. Same as iced's `ContentFit::Contain`.
pub fn fit_rect(bounds: Rect, img: Size) -> Rect {
    if img.width <= 0.0 || img.height <= 0.0 {
        return Rect::new(bounds.center().x, bounds.center().y, 0.0, 0.0);
    }
    let scale = (bounds.w / img.width).min(bounds.h / img.height);
    let (w, h) = (img.width * scale, img.height * scale);
    Rect::new(bounds.center().x - w / 2.0, bounds.center().y - h / 2.0, w, h)
}

/// Mapping between view coordinates and source pixels for a contain-fit image.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewMap {
    /// Where the image is drawn, in view coordinates.
    pub fit: Rect,
    /// View units per source pixel.
    pub scale: f32,
}

impl ViewMap {
    pub fn new(bounds: Rect, img: Size) -> Self {
        let fit = fit_rect(bounds, img);
        ViewMap { fit, scale: if img.width > 0.0 { fit.w / img.width } else { 1.0 } }
    }

    pub fn to_source(&self, p: Point) -> Point {
        Point::new((p.x - self.fit.x) / self.scale, (p.y - self.fit.y) / self.scale)
    }

    pub fn to_view(&self, p: Point) -> Point {
        Point::new(self.fit.x + p.x * self.scale, self.fit.y + p.y * self.scale)
    }

    pub fn rect_to_view(&self, r: Rect) -> Rect {
        let p = self.to_view(Point::new(r.x, r.y));
        Rect::new(p.x, p.y, r.w * self.scale, r.h * self.scale)
    }

    /// A length in view units, in source pixels.
    pub fn len_to_source(&self, len: f32) -> f32 {
        len / self.scale
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotate_rect_and_back() {
        let (r, size) = (Rect::new(10.0, 20.0, 30.0, 40.0), Size::new(100.0, 60.0));
        assert_eq!(r.rotate(1, size), Rect::new(0.0, 10.0, 40.0, 30.0));
        assert_eq!(r.rotate(2, size), Rect::new(60.0, 0.0, 30.0, 40.0));
        assert_eq!(r.rotate(3, size), Rect::new(20.0, 60.0, 40.0, 30.0));
        for q in 0..4 {
            assert_eq!(r.rotate(q, size).rotate(4 - q, size.rotate(q)), r, "{q}");
        }
    }

    #[test]
    fn fit_landscape_in_square() {
        let r = fit_rect(Rect::new(0.0, 0.0, 100.0, 100.0), Size::new(200.0, 100.0));
        assert_eq!(r, Rect::new(0.0, 25.0, 100.0, 50.0));
    }

    #[test]
    fn fit_upscales_and_centers() {
        let r = fit_rect(Rect::new(10.0, 10.0, 400.0, 100.0), Size::new(20.0, 10.0));
        assert_eq!(r, Rect::new(110.0, 10.0, 200.0, 100.0));
    }

    #[test]
    fn view_map_round_trip() {
        let m = ViewMap::new(Rect::new(0.0, 0.0, 1000.0, 750.0), Size::new(4000.0, 2000.0));
        assert_eq!(m.scale, 0.25);
        let p = Point::new(1234.0, 567.0);
        let back = m.to_source(m.to_view(p));
        assert!((back.x - p.x).abs() < 1e-3 && (back.y - p.y).abs() < 1e-3);
        assert_eq!(m.to_view(Point::new(0.0, 0.0)), Point::new(0.0, 125.0));
    }

    #[test]
    fn clamping_and_pixels() {
        let size = (100, 50);
        let r = Rect::new(-10.0, 40.25, 50.0, 30.0);
        assert_eq!(r.clamp_to(size.into()), Rect::new(0.0, 40.25, 40.0, 9.75));
        assert_eq!(r.to_pixels(size), Some(PixelRect { x: 0, y: 40, w: 40, h: 10 }));
        assert_eq!(Rect::new(200.0, 0.0, 10.0, 10.0).to_pixels(size), None);
        assert_eq!(PixelRect { x: 1, y: 2, w: 3, h: 4 }.to_string(), "3×4 @ 1,2");
    }
}
