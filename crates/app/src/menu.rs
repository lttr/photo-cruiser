//! The footer menu's items, shared by the view (labels, clicks) and the keyboard
//! (`↑`/`↓` move the focus, `Enter` picks it).

use crate::action::Action;
use crate::message::Message;
use crate::state::{Mode, State, View};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    OpenFolder,
    /// Grid ↔ photo.
    Switch,
    Crop,
    Fullscreen,
    Names,
    Layout,
    Shortcuts,
    About,
}

/// Top to bottom.
pub const ITEMS: [Item; 8] = [
    Item::OpenFolder,
    Item::Switch,
    Item::Crop,
    Item::Fullscreen,
    Item::Names,
    Item::Layout,
    Item::Shortcuts,
    Item::About,
];

impl Item {
    pub fn label(self, s: &State) -> &'static str {
        match self {
            Item::OpenFolder => "Open folder…",
            Item::Switch if s.view == View::Grid => "Open photo",
            Item::Switch => "All photos",
            Item::Crop if s.mode == Mode::Crop => "Stop cropping",
            Item::Crop => "Crop",
            Item::Fullscreen => "Full screen",
            Item::Names if s.names => "Hide file names",
            Item::Names => "Show file names",
            Item::Layout => s.layout.next().label(),
            Item::Shortcuts => "Keyboard shortcuts",
            Item::About => "About Photo Cruiser",
        }
    }

    pub fn keys(self, s: &State) -> &'static str {
        match self {
            Item::OpenFolder => "Ctrl+O",
            Item::Switch if s.view == View::Grid => "Enter",
            Item::Switch => "Esc",
            Item::Crop => "C",
            Item::Fullscreen => "F",
            Item::Names => "N",
            Item::Layout => "Z",
            Item::Shortcuts => "?",
            Item::About => "",
        }
    }

    pub fn message(self, s: &State) -> Message {
        match self {
            Item::OpenFolder => Message::OpenDialog,
            Item::Switch if s.view == View::Grid => Action::ShowPhoto { index: None }.into(),
            Item::Switch => Action::ShowGrid.into(),
            Item::Crop => Action::ToggleCrop.into(),
            Item::Fullscreen => Action::ToggleFullscreen.into(),
            Item::Names => Action::ToggleNames.into(),
            Item::Layout => Action::NextLayout.into(),
            Item::Shortcuts => Action::ToggleHelp.into(),
            Item::About => Action::ToggleAbout.into(),
        }
    }
}
