//! The info panel's lightness histogram: a filled curve, black at the left, white at the
//! right, with guides at the quarters.

use crate::message::Message;
use iced::mouse::Cursor;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::{Color, Point, Rectangle, Renderer, Theme};

/// Bin heights 0–1 ([`photo_cruiser_core::info::Histogram::heights`]); `None` draws only
/// the frame (the image isn't loaded yet).
pub struct Chart(pub Option<[f32; 256]>);

impl canvas::Program<Message> for Chart {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), Color::from_rgba(0.0, 0.0, 0.0, 0.35));
        let guide = Stroke::default().with_width(1.0).with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.08));
        for q in 1..4 {
            let x = (w * q as f32 / 4.0).round() + 0.5;
            frame.stroke(&Path::line(Point::new(x, 0.0), Point::new(x, h)), guide);
        }
        if let Some(heights) = &self.0 {
            // A little room at the top, so a full bin still reads as a peak.
            let top = h * 0.92;
            let curve = Path::new(|p| {
                p.move_to(Point::new(0.0, h));
                for (i, v) in heights.iter().enumerate() {
                    p.line_to(Point::new(i as f32 / 255.0 * w, h - v * top));
                }
                p.line_to(Point::new(w, h));
                p.close();
            });
            frame.fill(&curve, Color::from_rgba(0.85, 0.85, 0.85, 0.75));
        }
        vec![frame.into_geometry()]
    }
}
