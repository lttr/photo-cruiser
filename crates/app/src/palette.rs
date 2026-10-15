//! The command palette (`Ctrl+K`): every action that does something right now, sorted by
//! name and filtered by what was typed. Shared by the view (rows, clicks), the state
//! (moving the focus) and the app (`Enter` picks the focused row), like [`crate::menu`].
//! While it shows, [`Keys`] (stacked over it) takes the keys the text field doesn't.

use crate::action::Action;
use crate::grid::Layout;
use crate::message::Message;
use crate::state::{Mode, State, View};
use iced::keyboard::{self, Key, key::Named};
use iced::mouse::Cursor;
use iced::widget::canvas::{self, Event, Geometry};
use iced::{Rectangle, Renderer, Theme};
use photo_cruiser_core::crop::Aspect;
use serde::Serialize;

/// Rows shown at once; the list scrolls to keep the focus in view.
pub const VISIBLE: usize = 12;

/// The palette while it shows.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Palette {
    /// What was typed.
    pub query: String,
    /// The focused row, an index into [`filtered`].
    pub focus: usize,
    /// The first row shown.
    pub top: usize,
}

impl Palette {
    /// Focus row `i` (clamped to `n` rows), scrolling it into view.
    pub fn focus(&mut self, i: usize, n: usize) {
        self.focus = i.min(n.saturating_sub(1));
        if self.focus < self.top {
            self.top = self.focus;
        } else if self.focus >= self.top + VISIBLE {
            self.top = self.focus + 1 - VISIBLE;
        }
    }
}

pub struct Command {
    pub label: String,
    /// Its shortcut, or `""`.
    pub keys: &'static str,
    pub message: Message,
}

fn cmd(label: impl Into<String>, keys: &'static str, message: impl Into<Message>) -> Command {
    Command { label: label.into(), keys, message: message.into() }
}

/// Everything that can be done now, sorted by label ([`sort_key`]). Actions that would change nothing
/// (undo with no history, the layout already in use) are left out.
pub fn commands(s: &State) -> Vec<Command> {
    let mut c = vec![
        cmd("About Photo Cruiser", "", Action::ToggleAbout),
        cmd("Keyboard shortcuts", "?", Action::ToggleHelp),
        cmd("Open folder…", "Ctrl+O", Message::OpenDialog),
        cmd("Quit", "", Action::Quit),
        cmd(if s.names { "Hide file names" } else { "Show file names" }, "N", Action::ToggleNames),
        match s.fullscreen {
            true => cmd("Leave full screen", "F", Action::ExitFullscreen),
            false => cmd("Full screen", "F", Action::ToggleFullscreen),
        },
    ];
    let n = s.len();
    if n > 0 {
        let (first, last) = (s.index == 0, s.index + 1 == n);
        if !first {
            c.push(cmd("First photo", "Home", Action::First));
            c.push(cmd("Previous photo", "←", Action::Prev));
        }
        if !last {
            c.push(cmd("Last photo", "End", Action::Last));
            c.push(cmd("Next photo", "→", Action::Next));
        }
        match s.view {
            View::Grid => {
                c.push(cmd("Open photo", "Enter", Action::ShowPhoto { index: None }));
                for layout in [Layout::Small, Layout::Big, Layout::Justified] {
                    if layout != s.layout {
                        let keys = if layout == s.layout.next() { "Z" } else { "" };
                        c.push(cmd(layout.label(), keys, Action::SetLayout { layout }));
                    }
                }
            }
            View::Photo => c.push(cmd("All photos", "Esc", Action::ShowGrid)),
        }
        edits(s, &mut c);
    }
    c.sort_by_cached_key(|c| sort_key(&c.label));
    c
}

/// Alphabetical, ignoring case, with numbers by value (1:1 before 16:9).
fn sort_key(label: &str) -> Vec<(String, u64)> {
    let mut key = Vec::new();
    let mut chars = label.chars().peekable();
    while chars.peek().is_some() {
        let text: String = std::iter::from_fn(|| chars.next_if(|c| !c.is_ascii_digit())).collect();
        let digits: String = std::iter::from_fn(|| chars.next_if(char::is_ascii_digit)).collect();
        key.push((text.to_lowercase(), digits.parse().unwrap_or(0)));
    }
    key
}

/// The current photo's crop and edits.
fn edits(s: &State, c: &mut Vec<Command>) {
    let selection = s.crop.selection.is_some();
    if s.mode == Mode::Crop {
        if selection {
            c.push(cmd("Apply crop", "Enter", Action::ApplyCrop));
            c.push(cmd("Clear crop selection", "", Action::ClearCrop));
        }
        c.push(cmd("Cancel crop", "Esc", Action::ExitCrop));
        let current = s.crop.aspect;
        for aspect in Aspect::PRESETS.into_iter().filter(|a| *a != current) {
            let name = match aspect {
                Aspect::Original => "original".into(),
                Aspect::Free => "free".into(),
                _ => aspect.to_string(),
            };
            c.push(cmd(format!("Aspect ratio {name}"), "", Action::SetAspect { aspect }));
        }
        c.push(cmd("Aspect ratio custom…", "", Action::ToggleCustomAspect));
        c.push(cmd("Next aspect ratio", "A", Action::NextAspect));
        if let Some(r) = s.crop_ratio().filter(|r| (r - 1.0).abs() > 1e-3) {
            let label = if r > 1.0 { "Crop portrait" } else { "Crop landscape" };
            c.push(cmd(label, "X", Action::SwapAspect));
        }
    } else {
        c.push(cmd("Crop", "C", Action::ToggleCrop));
    }
    c.push(cmd("Turn right", "Shift+R", Action::RotateRight));
    c.push(cmd("Turn left", "Shift+L", Action::RotateLeft));
    let history = s.current_path().and_then(|p| s.edits.get(p)).map(|p| &p.history);
    if history.is_some_and(|h| h.can_undo()) {
        c.push(cmd("Undo", "Ctrl+Z", Action::Undo));
    }
    if history.is_some_and(|h| h.can_redo()) {
        c.push(cmd("Redo", "Ctrl+Shift+Z", Action::Redo));
    }
    let edited = !s.stack().is_empty();
    if s.dirty() || (s.mode == Mode::Crop && selection) {
        c.push(cmd("Save", "Ctrl+S", Action::Save));
    }
    if edited {
        c.push(cmd("Reset edits", "", Action::ResetEdits));
    }
    if edited || s.backup().is_some() {
        c.push(cmd("Revert to original", "", Action::Revert));
    }
}

/// Every word of `query` is in `label` (ignoring case).
pub fn matches(label: &str, query: &str) -> bool {
    let label = label.to_lowercase();
    query.to_lowercase().split_whitespace().all(|w| label.contains(w))
}

/// The rows the palette shows: [`commands`] matching its query (all of them while it's closed).
pub fn filtered(s: &State) -> Vec<Command> {
    let query = s.palette.as_ref().map_or("", |p| p.query.as_str());
    commands(s).into_iter().filter(|c| matches(&c.label, query)).collect()
}

/// Invisible layer over the palette that takes its keys before the text field (which
/// would swallow `Esc`): `Esc` closes it, `↑`/`↓` and `Tab` move the focus, `Enter` picks.
pub struct Keys;

pub fn key_action(key: &Key, mods: keyboard::Modifiers) -> Option<Action> {
    Some(match key {
        Key::Named(Named::Escape) => Action::ClosePalette,
        Key::Named(Named::ArrowDown) => Action::PaletteNext,
        Key::Named(Named::ArrowUp) => Action::PalettePrev,
        Key::Named(Named::Tab) if mods.shift() => Action::PalettePrev,
        Key::Named(Named::Tab) => Action::PaletteNext,
        Key::Named(Named::Enter) => Action::PaletteActivate,
        Key::Character(c) if mods.control() && c.eq_ignore_ascii_case("k") => Action::TogglePalette,
        _ => return None,
    })
}

impl canvas::Program<Message> for Keys {
    type State = ();

    fn update(&self, _: &mut (), event: &Event, _: Rectangle, _: Cursor) -> Option<canvas::Action<Message>> {
        let Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event else { return None };
        key_action(key, *modifiers).map(|a| canvas::Action::publish(a.into()).and_capture())
    }

    fn draw(&self, _: &(), _: &Renderer, _: &Theme, _: Rectangle, _: Cursor) -> Vec<Geometry> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching() {
        assert!(matches("Next photo", ""));
        assert!(matches("Next photo", "PHO nex"));
        assert!(!matches("Next photo", "next grid"));
    }

    #[test]
    fn sorting() {
        let mut labels =
            ["Aspect ratio 16:9", "aspect ratio 1:1", "Aspect ratio 5:4", "About", "Aspect ratio free"];
        labels.sort_by_cached_key(|l| sort_key(l));
        assert_eq!(
            labels,
            ["About", "aspect ratio 1:1", "Aspect ratio 5:4", "Aspect ratio 16:9", "Aspect ratio free"]
        );
    }

    #[test]
    fn focus_scrolls() {
        let mut p = Palette::default();
        p.focus(VISIBLE + 2, 30);
        assert_eq!((p.focus, p.top), (VISIBLE + 2, 3));
        p.focus(1, 30);
        assert_eq!((p.focus, p.top), (1, 1));
        p.focus(50, 5);
        assert_eq!((p.focus, p.top), (4, 1));
        p.focus(0, 0);
        assert_eq!((p.focus, p.top), (0, 0));
    }
}
