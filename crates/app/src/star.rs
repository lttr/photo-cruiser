//! The star of a photo to keep: drawn, so it doesn't depend on the font's glyphs.

use crate::message::Message;
use iced::mouse::Cursor;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::{Color, Element, Point, Rectangle, Renderer, Theme};

/// Gold, with a dark rim so it shows on any photo.
const FILL: Color = Color::from_rgb(0.98, 0.78, 0.18);
const RIM: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.6);

struct Star;

impl canvas::Program<Message> for Star {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let c = frame.center();
        let outer = bounds.width.min(bounds.height) / 2.0 - 1.0;
        let inner = outer * 0.45;
        let star = Path::new(|p| {
            for k in 0..10 {
                let r = if k % 2 == 0 { outer } else { inner };
                let a = std::f32::consts::PI * (k as f32 / 5.0 - 0.5);
                let pt = Point::new(c.x + r * a.cos(), c.y + r * a.sin() + outer * 0.08);
                if k == 0 { p.move_to(pt) } else { p.line_to(pt) }
            }
            p.close();
        });
        frame.fill(&star, FILL);
        frame.stroke(&star, Stroke::default().with_width(1.0).with_color(RIM));
        vec![frame.into_geometry()]
    }
}

/// A star `side` units wide and high.
pub fn star<'a>(side: u16) -> Element<'a, Message> {
    let side = f32::from(side);
    iced::widget::canvas(Star).width(side).height(side).into()
}
