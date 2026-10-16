//! Transparent canvas stacked over the image area. It draws the crop dimming and is the
//! app's input layer: keys and mouse events become [`Message`]s here (a widget, not an
//! `event::listen` subscription, so `iced_test` can drive it). The crop state machine
//! itself is `photo_cruiser_core::crop::CropTool`, in source pixels. It also reports the
//! area's size, which the grid layout needs ([`Message::Body`]).

use crate::action::Action;
use crate::menu;
use crate::message::Message;
use iced::keyboard::{self, Key, key::Named};
use iced::mouse::{self, Button, Cursor, Interaction};
use iced::widget::canvas::{self, Event, Frame, Geometry, Stroke};
use iced::{Color, Rectangle, Renderer, Theme};
use photo_cruiser_core::crop::{Hit, MouseKind, hit};
use photo_cruiser_core::geom::ViewMap;
use photo_cruiser_core::{Point, Rect, Size, Tool};

/// Grab distance around the selection edges, in logical window pixels.
pub const EDGE: f32 = 8.0;

/// What decides the meaning of a key.
#[derive(Clone, Copy, Debug, Default)]
pub struct Modes {
    /// The menu, the shortcuts or the About panel is open (`Esc` closes it first).
    pub menu: bool,
    pub help: bool,
    pub about: bool,
    /// The custom aspect field has the keyboard (only `Esc` is ours).
    pub input: bool,
    /// The command palette shows: its own layer takes the keys ([`crate::palette::Keys`]).
    pub palette: bool,
    pub crop: bool,
    /// A one-slider tool is open (`Enter` closes it, `Esc` cancels it).
    pub tool: bool,
    pub fullscreen: bool,
    /// The grid shows (not a single photo).
    pub grid: bool,
}

/// What the overlay needs to know; rebuilt on every `view`.
pub struct Overlay {
    pub modes: Modes,
    /// Selection in source pixels.
    pub selection: Option<Rect>,
    /// Source size of the shown image, if any.
    pub image: Option<Size>,
    /// The area's size as the state knows it; a different one is reported.
    pub body: Option<iced::Size>,
}

fn rect(r: Rectangle) -> Rect {
    Rect::new(r.x, r.y, r.width, r.height)
}

impl Overlay {
    fn active(&self) -> bool {
        self.modes.crop && !self.modes.grid
    }

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

/// Every shortcut, for the shortcuts panel. Keep in sync with [`key_message`].
pub const SHORTCUTS: &[(&str, &str)] = &[
    ("→  ←   l  h", "Next / previous photo"),
    ("↓  ↑   j  k", "Grid: row down / up; photo: next / previous"),
    ("Home  End   g  G", "First / last photo"),
    ("Enter", "Grid: open the photo; crop: apply; tool: done"),
    ("Ctrl+K", "Find an action by name"),
    ("Ctrl+O", "Open folder"),
    ("C", "Crop on / off"),
    ("E", "Exposure (opens at auto)"),
    ("A", "Auto levels, for documents (opens at strength 1; 2: whiter paper)"),
    ("Shift+A", "Tool: back to auto (or double-click the slider)"),
    ("0", "Tool: no change (the photo as it was)"),
    ("Shift+R  Shift+L", "Turn right / left"),
    ("Ctrl+Z  u", "Undo"),
    ("Ctrl+Shift+Z  Ctrl+Y", "Redo"),
    ("Ctrl+S", "Save now (leaving a photo saves it)"),
    ("R", "Crop: next aspect ratio (original, free, 1:1, 3:2, …)"),
    ("X", "Crop: portrait / landscape"),
    ("F  F11", "Full screen on / off"),
    ("N", "File names on / off"),
    ("Z", "Grid: small / big thumbnails / justified"),
    ("F10", "Menu: ↑ ↓ Home End to move, Enter to pick"),
    ("?  F1", "Keyboard shortcuts"),
    ("Esc", "Close menu, cancel the tool, drop the crop selection, leave full screen, back to the grid"),
];

/// `key` ignores Shift, `modified` doesn't (Shift+/ is `/` and `?`): symbols like `?` are
/// matched on `modified`. `Esc` closes the menu or panel, then leaves crop mode, then full
/// screen, then goes back from the photo to the grid.
pub fn key_message(key: &Key, modified: &Key, mods: keyboard::Modifiers, m: Modes) -> Option<Message> {
    let plain = !mods.control() && !mods.alt() && !mods.logo();
    let (down, up) = if m.grid { (Action::Down, Action::Up) } else { (Action::Next, Action::Prev) };
    if m.palette {
        return None;
    }
    if m.input {
        return matches!(key, Key::Named(Named::Escape)).then_some(Action::ToggleCustomAspect.into());
    }
    if m.menu && plain {
        let action = match key {
            Key::Named(Named::ArrowDown) => Some(Action::MenuNext),
            Key::Named(Named::ArrowUp) => Some(Action::MenuPrev),
            Key::Named(Named::Tab) if mods.shift() => Some(Action::MenuPrev),
            Key::Named(Named::Tab) => Some(Action::MenuNext),
            Key::Named(Named::Home) => Some(Action::MenuFocus { index: 0 }),
            Key::Named(Named::End) => Some(Action::MenuFocus { index: menu::ITEMS.len() - 1 }),
            Key::Named(Named::Enter | Named::Space) => Some(Action::MenuActivate),
            Key::Character(c) if c == "j" => Some(Action::MenuNext),
            Key::Character(c) if c == "k" => Some(Action::MenuPrev),
            _ => None,
        };
        if let Some(action) = action {
            return Some(action.into());
        }
    }
    // Symbols and vim keys (g vs G) by what was typed.
    if let (true, Key::Character(c)) = (plain, modified) {
        let action = match c.as_str() {
            "?" => Some(Action::ToggleHelp),
            "l" => Some(Action::Next),
            "h" => Some(Action::Prev),
            "j" => Some(down.clone()),
            "k" => Some(up.clone()),
            "g" => Some(Action::First),
            "G" => Some(Action::Last),
            "u" => Some(Action::Undo),
            "R" => Some(Action::RotateRight),
            "L" => Some(Action::RotateLeft),
            "A" if m.tool => Some(Action::AutoAdjust),
            _ => None,
        };
        if let Some(action) = action {
            return Some(action.into());
        }
    }
    Some(match key {
        Key::Named(Named::ArrowRight) => Action::Next.into(),
        Key::Named(Named::ArrowLeft) => Action::Prev.into(),
        Key::Named(Named::ArrowDown) => down.into(),
        Key::Named(Named::ArrowUp) => up.into(),
        Key::Named(Named::Home) => Action::First.into(),
        Key::Named(Named::End) => Action::Last.into(),
        Key::Named(Named::Enter) if m.grid => Action::ShowPhoto { index: None }.into(),
        Key::Named(Named::Enter) if m.crop => Action::ApplyCrop.into(),
        Key::Named(Named::Enter) if m.tool => Action::CloseTool.into(),
        Key::Named(Named::Escape) if m.menu => Action::CloseMenu.into(),
        Key::Named(Named::Escape) if m.help => Action::ToggleHelp.into(),
        Key::Named(Named::Escape) if m.about => Action::ToggleAbout.into(),
        Key::Named(Named::Escape) if m.tool => Action::CancelTool.into(),
        Key::Named(Named::Escape) if m.crop => Action::ExitCrop.into(),
        Key::Named(Named::Escape) if m.fullscreen || m.grid => Action::ExitFullscreen.into(),
        Key::Named(Named::Escape) => Action::ShowGrid.into(),
        Key::Named(Named::F11) => Action::ToggleFullscreen.into(),
        Key::Named(Named::F10 | Named::ContextMenu) if !m.menu => Action::MenuFocus { index: 0 }.into(),
        Key::Named(Named::F10 | Named::ContextMenu) => Action::ToggleMenu.into(),
        Key::Named(Named::F1) => Action::ToggleHelp.into(),
        Key::Character(c) if mods.control() && c.eq_ignore_ascii_case("k") => Action::TogglePalette.into(),
        Key::Character(c) if mods.control() && c.eq_ignore_ascii_case("o") => Message::OpenDialog,
        Key::Character(c) if mods.control() && c.eq_ignore_ascii_case("s") => Action::Save.into(),
        Key::Character(c) if mods.control() && mods.shift() && c.eq_ignore_ascii_case("z") => {
            Action::Redo.into()
        }
        Key::Character(c) if mods.control() && c.eq_ignore_ascii_case("z") => Action::Undo.into(),
        Key::Character(c) if mods.control() && c.eq_ignore_ascii_case("y") => Action::Redo.into(),
        Key::Character(c) if plain && c.eq_ignore_ascii_case("c") => Action::ToggleCrop.into(),
        Key::Character(c) if plain && c.eq_ignore_ascii_case("e") => {
            Action::OpenTool { tool: Tool::Exposure }.into()
        }
        // Shift+R (turn right) is matched above, on the typed `R`.
        Key::Character(c) if plain && m.crop && c.eq_ignore_ascii_case("r") => Action::NextAspect.into(),
        Key::Character(c) if plain && c.eq_ignore_ascii_case("a") => {
            Action::OpenTool { tool: Tool::Levels }.into()
        }
        // `0` typed or the key under it (Czech layout: `é`, Shift gives `0`).
        Key::Character(c) if plain && m.tool && (c == "0" || modified.as_ref() == Key::Character("0")) => {
            Action::NeutralAdjust.into()
        }
        Key::Character(c) if plain && m.crop && c.eq_ignore_ascii_case("x") => Action::SwapAspect.into(),
        Key::Character(c) if plain && c.eq_ignore_ascii_case("f") => Action::ToggleFullscreen.into(),
        Key::Character(c) if plain && c.eq_ignore_ascii_case("n") => Action::ToggleNames.into(),
        Key::Character(c) if plain && c.eq_ignore_ascii_case("z") => Action::NextLayout.into(),
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
            Event::Keyboard(keyboard::Event::KeyPressed { key, modified_key, modifiers, .. }) => {
                return key_message(key, modified_key, *modifiers, self.modes).map(canvas::Action::publish);
            }
            Event::Window(iced::window::Event::RedrawRequested(_)) if self.body != Some(bounds.size()) => {
                return Some(canvas::Action::publish(Message::Body(bounds.size())));
            }
            _ if !self.active() => {
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
        let (Some(sel), Some(img), true) = (self.selection, self.image, self.active()) else { return vec![] };
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
        let Some((p, tol)) = cursor
            .position_over(bounds)
            .filter(|_| self.active())
            .and_then(|pos| self.to_source(bounds, pos))
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
