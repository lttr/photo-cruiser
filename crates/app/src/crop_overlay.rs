//! Transparent canvas stacked over the image area. It draws the crop dimming and is the
//! app's input layer: keys and mouse events become [`Message`]s here (a widget, not an
//! `event::listen` subscription, so `iced_test` can drive it). The crop state machine
//! itself is `photo_cruiser_core::crop::CropTool`, in source pixels.

use crate::action::Action;
use crate::message::Message;
use iced::keyboard::{self, Key, key::Named};
use iced::mouse::{self, Button, Cursor, Interaction};
use iced::widget::canvas::{self, Event, Frame, Geometry, Stroke};
use iced::{Color, Rectangle, Renderer, Theme};
use photo_cruiser_core::crop::{Hit, MouseKind, hit};
use photo_cruiser_core::geom::ViewMap;
use photo_cruiser_core::{Point, Rect, Size};

/// Grab distance around the selection edges, in logical window pixels.
pub const EDGE: f32 = 8.0;

/// What the overlay needs to know; rebuilt on every `view`.
pub struct Overlay {
    pub active: bool,
    /// Selection in source pixels.
    pub selection: Option<Rect>,
    /// Source size of the shown image, if any.
    pub image: Option<Size>,
}

fn rect(r: Rectangle) -> Rect {
    Rect::new(r.x, r.y, r.width, r.height)
}

impl Overlay {
    fn map(&self, bounds: Rectangle) -> Option<ViewMap> {
        Some(ViewMap::new(rect(bounds), self.image?))
    }

    /// Window position → (source pixel, grab tolerance in source pixels).
    fn to_source(&self, bounds: Rectangle, pos: iced::Point) -> Option<(Point, f32)> {
        let m = self.map(bounds)?;
        Some((m.to_source(Point::new(pos.x, pos.y)), m.len_to_source(EDGE)))
    }
}

#[derive(Default)]
pub struct OverlayState {
    pressed: bool,
    last: iced::Point,
}

pub fn key_message(key: &Key, mods: keyboard::Modifiers) -> Option<Message> {
    Some(match key {
        Key::Named(Named::ArrowRight) => Action::Next.into(),
        Key::Named(Named::ArrowLeft) => Action::Prev.into(),
        Key::Named(Named::Escape) => Action::ExitCrop.into(),
        Key::Character(c) if mods.control() && c.eq_ignore_ascii_case("o") => Message::OpenDialog,
        Key::Character(c)
            if !mods.control() && !mods.alt() && !mods.logo() && c.eq_ignore_ascii_case("c") =>
        {
            Action::ToggleCrop.into()
        }
        _ => return None,
    })
}

impl canvas::Program<Message> for Overlay {
    type State = OverlayState;

    fn update(
        &self,
        state: &mut OverlayState,
        event: &Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<canvas::Action<Message>> {
        let (kind, pos) = match event {
            Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                return key_message(key, *modifiers).map(canvas::Action::publish);
            }
            _ if !self.active => {
                state.pressed = false;
                return None;
            }
            Event::Mouse(mouse::Event::ButtonPressed(Button::Left)) => {
                (MouseKind::Press, cursor.position_over(bounds)?)
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) if state.pressed => {
                (MouseKind::Move, *position)
            }
            Event::Mouse(mouse::Event::ButtonReleased(Button::Left)) if state.pressed => {
                (MouseKind::Release, cursor.position().unwrap_or(state.last))
            }
            _ => return None,
        };
        state.pressed = kind != MouseKind::Release;
        state.last = pos;
        let (p, tol) = self.to_source(bounds, pos)?;
        Some(canvas::Action::publish(Message::Crop(kind, p, tol)).and_capture())
    }

    fn draw(
        &self,
        _state: &OverlayState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let (Some(sel), Some(img), true) = (self.selection, self.image, self.active) else { return vec![] };
        let mut frame = Frame::new(renderer, bounds.size());
        let m = ViewMap::new(Rect::with_size(Size::new(bounds.width, bounds.height)), img);
        let (fit, s) = (m.fit, m.rect_to_view(sel));
        // 50% black in sRGB space (needs iced's `web-colors` feature).
        let dim = Color::from_rgba(0.0, 0.0, 0.0, 0.5);
        for (x, y, w, h) in [
            (fit.x, fit.y, fit.w, s.y - fit.y),                    // above
            (fit.x, s.bottom(), fit.w, fit.bottom() - s.bottom()), // below
            (fit.x, s.y, s.x - fit.x, s.h),                        // left
            (s.right(), s.y, fit.right() - s.right(), s.h),        // right
        ] {
            if w > 0.0 && h > 0.0 {
                frame.fill_rectangle(iced::Point::new(x, y), iced::Size::new(w, h), dim);
            }
        }
        let border = Stroke::default().with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.85)).with_width(1.0);
        frame.stroke_rectangle(iced::Point::new(s.x, s.y), iced::Size::new(s.w, s.h), border);
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, _state: &OverlayState, bounds: Rectangle, cursor: Cursor) -> Interaction {
        let Some((p, tol)) =
            cursor.position_over(bounds).filter(|_| self.active).and_then(|pos| self.to_source(bounds, pos))
        else {
            return Interaction::default();
        };
        let inside = self.image.is_some_and(|img| Rect::with_size(img).contains(p));
        match hit(self.selection, p, tol) {
            Hit::New if inside => Interaction::Crosshair,
            Hit::New => Interaction::default(),
            Hit::Move => Interaction::Grab,
            Hit::Resize { left, right, top, bottom } => match (left || right, top || bottom) {
                (true, false) => Interaction::ResizingHorizontally,
                (false, true) => Interaction::ResizingVertically,
                _ if (left && top) || (right && bottom) => Interaction::ResizingDiagonallyDown,
                _ => Interaction::ResizingDiagonallyUp,
            },
        }
    }
}
