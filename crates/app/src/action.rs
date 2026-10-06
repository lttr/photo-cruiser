//! The single entry point for behaviour. Keyboard, toolbar buttons, scripts and tests
//! all produce [`Action`]s; [`crate::state::State::apply`] interprets them.
//!
//! JSON form (internally tagged): `{"action": "next"}`, `{"action": "goto", "index": 3}`,
//! `{"action": "set_crop", "x": 40, "y": 30, "w": 200, "h": 150}`,
//! `{"action": "open", "path": "fixtures"}`.

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
    /// Jump to a 0-based index. Out of range is an error (index unchanged).
    Goto {
        index: usize,
    },
    /// Enter/leave crop mode (`C`). Clears the selection.
    ToggleCrop,
    /// Set the selection in source pixels (clamped to the image); turns crop mode on.
    SetCrop(Rect),
    /// Drop the selection, stay in crop mode.
    ClearCrop,
    /// Leave crop mode (`Esc`).
    ExitCrop,
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
                {"action":"clear_crop"},{"action":"exit_crop"},{"action":"prev"},{"action":"quit"}]"#,
        )
        .unwrap();
        assert_eq!(steps[1], Action::Goto { index: 2 });
        assert_eq!(steps[3], Action::SetCrop(Rect::new(40.0, 30.0, 200.0, 150.0)));
        assert_eq!(steps.len(), 9);
        let json = serde_json::to_string(&Action::SetCrop(Rect::new(1.0, 2.0, 3.0, 4.0))).unwrap();
        assert_eq!(json, r#"{"action":"set_crop","x":1.0,"y":2.0,"w":3.0,"h":4.0}"#);
        assert!(parse_script(r#"[{"action":"jump"}]"#).is_err());
    }
}
