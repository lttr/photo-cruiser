//! The "opens in the browser" mark next to an info field with a link: a box with an arrow
//! out of its top-right corner, drawn so it doesn't depend on the font's glyphs.

use crate::message::Message;
use iced::mouse::Cursor;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::{Color, Point, Rectangle, Renderer, Theme};

pub struct LinkIcon(pub Color);

impl canvas::Program<Message> for LinkIcon {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let s = bounds.width.min(bounds.height);
        let at = |x: f32, y: f32| Point::new(x * s, y * s);
        let stroke = Stroke::default().with_width((s / 9.0).max(1.2)).with_color(self.0);
        // The box, open at the top right where the arrow leaves it.
        let frame_path = Path::new(|p| {
            p.move_to(at(0.45, 0.15));
            p.line_to(at(0.15, 0.15));
            p.line_to(at(0.15, 0.85));
            p.line_to(at(0.85, 0.85));
            p.line_to(at(0.85, 0.55));
        });
        let arrow = Path::new(|p| {
            p.move_to(at(0.45, 0.55));
            p.line_to(at(0.9, 0.1));
            p.move_to(at(0.58, 0.1));
            p.line_to(at(0.9, 0.1));
            p.line_to(at(0.9, 0.42));
        });
        frame.stroke(&frame_path, stroke);
        frame.stroke(&arrow, stroke);
        vec![frame.into_geometry()]
    }
}
