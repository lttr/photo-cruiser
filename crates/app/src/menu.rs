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
    Star,
    StarredOnly,
    Crop,
    Save,
    ResetEdits,
    Revert,
    Fullscreen,
    Names,
    InfoPanel,
    EditsPanel,
    Layout,
    Shortcuts,
    About,
}

/// Top to bottom.
pub const ITEMS: [Item; 15] = [
    Item::OpenFolder,
    Item::Switch,
    Item::Crop,
    Item::Save,
    Item::ResetEdits,
    Item::Revert,
    Item::Star,
    Item::StarredOnly,
    Item::Fullscreen,
    Item::Names,
    Item::InfoPanel,
    Item::EditsPanel,
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
            Item::Star if s.star() == Some(true) => "Remove star",
            Item::Star => "Star",
            Item::StarredOnly if s.starred_only => "Show all photos",
            Item::StarredOnly => "Show starred only",
            Item::Crop if s.mode == Mode::Crop => "Stop cropping",
            Item::Crop => "Crop",
            Item::Save => "Save",
            Item::ResetEdits => "Reset edits",
            Item::Revert => "Revert to original",
            Item::Fullscreen => "Full screen",
            Item::Names if s.names => "Hide file names",
            Item::Names => "Show file names",
            Item::InfoPanel if s.info_panel => "Hide info",
            Item::InfoPanel => "Show info",
            Item::EditsPanel if s.edits_panel => "Hide edits",
            Item::EditsPanel => "Show edits",
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
            Item::Star => "1",
            Item::StarredOnly => "Ctrl+1",
            Item::Crop => "C",
            Item::Save => "Ctrl+S",
            Item::ResetEdits | Item::Revert => "",
            Item::Fullscreen => "F",
            Item::Names => "N",
            Item::InfoPanel => "I",
            Item::EditsPanel => "Shift+E",
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
            Item::Star => Action::ToggleStar.into(),
            Item::StarredOnly => Action::ToggleStarredOnly.into(),
            Item::Crop => Action::ToggleCrop.into(),
            Item::Save => Action::Save.into(),
            Item::ResetEdits => Action::ResetEdits.into(),
            Item::Revert => Action::Revert.into(),
            Item::Fullscreen => Action::ToggleFullscreen.into(),
            Item::Names => Action::ToggleNames.into(),
            Item::InfoPanel => Action::ToggleInfoPanel.into(),
            Item::EditsPanel => Action::ToggleEditsPanel.into(),
            Item::Layout => Action::NextLayout.into(),
            Item::Shortcuts => Action::ToggleHelp.into(),
            Item::About => Action::ToggleAbout.into(),
        }
    }
}
