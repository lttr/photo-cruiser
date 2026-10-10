//! The single entry point for behaviour. Keyboard, menu items, scripts and tests
//! all produce [`Action`]s; [`crate::state::State::apply`] interprets them.
//!
//! JSON form (internally tagged): `{"action": "next"}`, `{"action": "goto", "index": 3}`,
//! `{"action": "set_crop", "x": 40, "y": 30, "w": 200, "h": 150}`,
//! `{"action": "open", "path": "fixtures"}`, `{"action": "show_photo", "index": 2}`.

use crate::grid::Layout;
use photo_cruiser_core::Rect;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    /// Open a folder (or a file: its folder, positioned on that file).
    Open {
        path: PathBuf,
    },
    Next,
    Prev,
    First,
    Last,
    /// Jump to a 0-based index. Out of range is an error (index unchanged).
    Goto {
        index: usize,
    },
    /// Grid: the photo one row up / down (`↑`/`↓`, `k`/`j`). Ignored in the photo view.
    Up,
    Down,
    /// Show one photo (`Enter` or a click in the grid): `index`, or the current one.
    ShowPhoto {
        #[serde(default)]
        index: Option<usize>,
    },
    /// Back to the grid of all photos (`Esc` in the photo view). Leaves crop mode.
    ShowGrid,
    /// Enter/leave crop mode (`C`). Clears the selection. From the grid: crops the current photo.
    ToggleCrop,
    /// Set the selection in source pixels (clamped to the image); turns crop mode on.
    SetCrop(Rect),
    /// Drop the selection, stay in crop mode.
    ClearCrop,
    /// Leave crop mode (`Esc`).
    ExitCrop,
    /// Full screen on/off (`F11`, `F`): hides the footer (status line and menu).
    ToggleFullscreen,
    /// Leave full screen (`Esc` outside crop mode).
    ExitFullscreen,
    /// Show/hide the menu (`F10`, footer button). Any other action closes it.
    ToggleMenu,
    /// Hide the menu (`Esc`, click outside it).
    CloseMenu,
    /// Show/hide file names in the status line and the grid (`N`, menu). Off at first start, then remembered.
    ToggleNames,
    /// Grid: the next layout, small → big thumbnails → justified (`Z`, menu). Small at first start, then remembered.
    NextLayout,
    /// Grid: this layout (`"small"`, `"big"`, `"justified"`).
    SetLayout {
        layout: Layout,
    },
    /// Show/hide the keyboard shortcuts (`?`, `F1`, menu). Any other action closes it.
    ToggleHelp,
    Quit,
}

/// Parse a script: a JSON array of actions.
pub fn parse_script(json: &str) -> Result<Vec<Action>, serde_json::Error> {
    serde_json::from_str(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_format() {
        let steps = parse_script(
            r#"[{"action":"next"},{"action":"goto","index":2},{"action":"toggle_crop"},
                {"action":"set_crop","x":40,"y":30,"w":200,"h":150},{"action":"open","path":"/tmp"},
                {"action":"clear_crop"},{"action":"exit_crop"},{"action":"toggle_fullscreen"},
                {"action":"exit_fullscreen"},{"action":"prev"},{"action":"quit"},
                {"action":"toggle_menu"},{"action":"close_menu"},{"action":"toggle_help"},
                {"action":"first"},{"action":"last"},{"action":"up"},{"action":"down"},
                {"action":"show_photo"},{"action":"show_photo","index":3},{"action":"show_grid"},{"action":"toggle_names"},
                {"action":"next_layout"},{"action":"set_layout","layout":"justified"}]"#,
        )
        .unwrap();
        assert_eq!(steps[1], Action::Goto { index: 2 });
        assert_eq!(steps[3], Action::SetCrop(Rect::new(40.0, 30.0, 200.0, 150.0)));
        assert_eq!(steps[7], Action::ToggleFullscreen);
        assert_eq!(steps[11], Action::ToggleMenu);
        assert_eq!(steps[14], Action::First);
        assert_eq!(steps[18], Action::ShowPhoto { index: None });
        assert_eq!(steps[19], Action::ShowPhoto { index: Some(3) });
        assert_eq!(steps[21], Action::ToggleNames);
        assert_eq!(steps[22], Action::NextLayout);
        assert_eq!(steps[23], Action::SetLayout { layout: Layout::Justified });
        assert_eq!(steps.len(), 24);
        let json = serde_json::to_string(&Action::SetCrop(Rect::new(1.0, 2.0, 3.0, 4.0))).unwrap();
        assert_eq!(json, r#"{"action":"set_crop","x":1.0,"y":2.0,"w":3.0,"h":4.0}"#);
        assert!(parse_script(r#"[{"action":"jump"}]"#).is_err());
    }
}
