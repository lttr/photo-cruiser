//! The open tool's slider in the footer: a canvas, so it can tell a drag (live preview,
//! [`Action::DragAdjust`]) from its end ([`Action::SetAdjust`], one undo step) and see a
//! double-click ([`Action::AutoAdjust`]), which iced's slider can't.

use crate::action::Action;
use crate::message::Message;
use iced::mouse::{self, Button, Cursor, Interaction};
use iced::widget::canvas::{self, Event, Frame, Geometry, Path};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};
use photo_cruiser_core::Tool;
use std::time::{Duration, Instant};

/// Room for the handle at both ends of the track.
const PAD: f32 = 8.0;

/// The tool's shortcut, or `""`.
pub fn key(tool: Tool) -> &'static str {
    match tool {
        Tool::Levels => "A",
        Tool::Exposure => "E",
    }
}

pub struct Slider {
    pub tool: Tool,
    pub value: f32,
    /// Marked on the track; a double-click goes back to it.
    pub opening: Option<f32>,
}

#[derive(Default)]
pub struct SliderState {
    dragging: bool,
    value: f32,
    /// The last press, to see a double-click.
    last_press: Option<(Instant, Point)>,
}

/// Two presses closer than this in time and space are a double-click.
const DOUBLE: Duration = Duration::from_millis(400);
const DOUBLE_DIST: f32 = 6.0;

impl Slider {
    /// The value at window x, rounded to 0.01.
    fn locate(&self, bounds: Rectangle, x: f32) -> f32 {
        let r = self.tool.range();
        let t = ((x - bounds.x - PAD) / (bounds.width - 2.0 * PAD).max(1.0)).clamp(0.0, 1.0);
        ((r.start() + t * (r.end() - r.start())) * 100.0).round() / 100.0
    }

    /// x of `value` within the bounds.
    fn x(&self, bounds: Size, value: f32) -> f32 {
        let r = self.tool.range();
        let t = ((value - r.start()) / (r.end() - r.start())).clamp(0.0, 1.0);
        PAD + t * (bounds.width - 2.0 * PAD)
    }
}

impl canvas::Program<Message> for Slider {
    type State = SliderState;

    fn update(
        &self,
        state: &mut SliderState,
        event: &Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<canvas::Action<Message>> {
        let action = match event {
            Event::Mouse(mouse::Event::ButtonPressed(Button::Left)) => {
                let pos = cursor.position_over(bounds)?;
                let double = state
                    .last_press
                    .is_some_and(|(t, p)| t.elapsed() < DOUBLE && p.distance(pos) < DOUBLE_DIST);
                // A third press starts over.
                state.last_press = if double { None } else { Some((Instant::now(), pos)) };
                if double {
                    state.dragging = false;
                    Action::AutoAdjust
                } else {
                    state.dragging = true;
                    state.value = self.locate(bounds, pos.x);
                    Action::DragAdjust { value: state.value }
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) if state.dragging => {
                state.value = self.locate(bounds, position.x);
                Action::DragAdjust { value: state.value }
            }
            Event::Mouse(mouse::Event::ButtonReleased(Button::Left)) if state.dragging => {
                state.dragging = false;
                Action::SetAdjust { value: state.value }
            }
            _ => return None,
        };
        Some(canvas::Action::publish(action.into()).and_capture())
    }

    fn draw(
        &self,
        _: &SliderState,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let palette = theme.palette();
        let mid = bounds.height / 2.0;
        let (left, right) = (PAD, bounds.width - PAD);
        let track = Color { a: 0.3, ..palette.text };
        frame.fill_rectangle(Point::new(left, mid - 2.0), Size::new(right - left, 4.0), track);
        // The change from neutral, in the accent colour.
        let (n, v) = (self.x(bounds.size(), self.tool.neutral()), self.x(bounds.size(), self.value));
        frame.fill_rectangle(Point::new(n.min(v), mid - 2.0), Size::new((v - n).abs(), 4.0), palette.primary);
        if let Some(o) = self.opening {
            let x = self.x(bounds.size(), o);
            let tick = Color { a: 0.7, ..palette.text };
            frame.fill_rectangle(Point::new(x - 1.0, mid - 7.0), Size::new(2.0, 14.0), tick);
        }
        frame.fill(&Path::circle(Point::new(v, mid), 7.0), palette.primary);
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, state: &SliderState, bounds: Rectangle, cursor: Cursor) -> Interaction {
        match state.dragging || cursor.is_over(bounds) {
            true => Interaction::Pointer,
            false => Interaction::default(),
        }
    }
}

/// Transparent layer over the footer's Original label: pressed, it shows the original
/// ([`Action::ShowOriginal`]) until the button is let go, wherever the pointer is then.
/// (iced's `mouse_area` only sees a release over itself.)
pub struct Hold;

#[derive(Default)]
pub struct HoldState {
    pressed: bool,
}

impl canvas::Program<Message> for Hold {
    type State = HoldState;

    fn update(
        &self,
        state: &mut HoldState,
        event: &Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<canvas::Action<Message>> {
        let on = match event {
            Event::Mouse(mouse::Event::ButtonPressed(Button::Left)) if cursor.is_over(bounds) => true,
            Event::Mouse(mouse::Event::ButtonReleased(Button::Left)) if state.pressed => false,
            _ => return None,
        };
        state.pressed = on;
        Some(canvas::Action::publish(Action::ShowOriginal { on }.into()).and_capture())
    }

    fn draw(&self, _: &HoldState, _: &Renderer, _: &Theme, _: Rectangle, _: Cursor) -> Vec<Geometry> {
        vec![]
    }

    fn mouse_interaction(&self, state: &HoldState, bounds: Rectangle, cursor: Cursor) -> Interaction {
        match state.pressed || cursor.is_over(bounds) {
            true => Interaction::Pointer,
            false => Interaction::default(),
        }
    }
}
