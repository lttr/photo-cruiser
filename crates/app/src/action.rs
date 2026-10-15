//! The single entry point for behaviour. Keyboard, menu items, scripts and tests
//! all produce [`Action`]s; [`crate::state::State::apply`] interprets them.
//!
//! JSON form (internally tagged): `{"action": "next"}`, `{"action": "goto", "index": 3}`,
//! `{"action": "set_crop", "x": 40, "y": 30, "w": 200, "h": 150}`,
//! `{"action": "open", "path": "fixtures"}`, `{"action": "show_photo", "index": 2}`,
//! `{"action": "set_aspect", "aspect": "16:9"}`, `{"action": "palette_query", "query": "crop"}`.

use crate::grid::Layout;
use photo_cruiser_core::Rect;
use photo_cruiser_core::crop::Aspect;
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
    /// Enter/leave crop mode (`C`). Entering shows the whole original with the applied crop
    /// selected; leaving applies the selection. From the grid: crops the current photo.
    ToggleCrop,
    /// Set the selection (a draft) in source pixels (clamped to the image); turns crop mode on.
    SetCrop(Rect),
    /// Drop the selection, stay in crop mode.
    ClearCrop,
    /// Leave crop mode, dropping the draft (`Esc`). The applied crop stays.
    ExitCrop,
    /// Apply the selection as the photo's crop and leave crop mode (`Enter`, footer
    /// "Apply"). Leaving the photo with a selection applies it too.
    ApplyCrop,
    /// Step through the current photo's edits (`Ctrl+Z`; `Ctrl+Shift+Z`, `Ctrl+Y`).
    Undo,
    Redo,
    /// Turn the current photo 90° clockwise (`Shift+R`) / counter-clockwise (`Shift+L`). In the grid
    /// it is saved at once.
    RotateRight,
    RotateLeft,
    /// Remove all edits of the current photo (menu; undoable). Saved when you leave it.
    ResetEdits,
    /// Back to the original file now: `reset_edits`, then `save` (menu).
    Revert,
    /// Save the current photo now (`Ctrl+S`, menu). Leaving a photo saves it anyway.
    Save,
    /// The shape the crop keeps: `"original"` (the photo's, the default), `"free"`, `"W:H"`
    /// or a number (footer buttons). A selection is reshaped to it.
    SetAspect {
        aspect: Aspect,
    },
    /// The next preset aspect (`A`).
    NextAspect,
    /// Turn the crop aspect portrait ↔ landscape (`X`, footer).
    SwapAspect,
    /// Show/hide the field for a custom aspect ratio (footer "Custom…"). Any other action
    /// closes it.
    ToggleCustomAspect,
    /// Full screen on/off (`F11`, `F`): hides the footer (status line and menu).
    ToggleFullscreen,
    /// Leave full screen (`Esc` outside crop mode).
    ExitFullscreen,
    /// Show/hide the menu (`F10`, footer button). Any other action closes it.
    ToggleMenu,
    /// Hide the menu (`Esc`, click outside it).
    CloseMenu,
    /// Focus this menu item (`Home`/`End`, mouse over it), opening the menu if needed
    /// (`F10`, `Menu` key: the first item).
    MenuFocus {
        index: usize,
    },
    /// Focus the next / previous menu item, wrapping around (`↓`/`↑`, `j`/`k`, `Tab`/`Shift+Tab`).
    /// Ignored while the menu is closed.
    MenuNext,
    MenuPrev,
    /// Pick the focused menu item (`Enter`, `Space`).
    MenuActivate,
    /// Show/hide the command palette (`Ctrl+K`): every action that does something now,
    /// sorted by name. Any other action closes it.
    TogglePalette,
    /// Hide the palette (`Esc`, click outside it).
    ClosePalette,
    /// The palette's search text (typing): shows only the actions whose name has every
    /// word of it, focuses the first.
    PaletteQuery {
        query: String,
    },
    /// Focus this palette row (mouse over it).
    PaletteFocus {
        index: usize,
    },
    /// Focus the next / previous palette row, wrapping around (`↓`/`↑`, `Tab`/`Shift+Tab`).
    PaletteNext,
    PalettePrev,
    /// Do the focused palette row's action (`Enter`).
    PaletteActivate,
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
    /// Show/hide the About panel (menu). Any other action closes it.
    ToggleAbout,
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
                {"action":"next_layout"},{"action":"set_layout","layout":"justified"},{"action":"toggle_about"},
                {"action":"menu_focus","index":2},{"action":"menu_next"},{"action":"menu_prev"},{"action":"menu_activate"},
                {"action":"set_aspect","aspect":"16:9"},{"action":"set_aspect","aspect":"free"},{"action":"next_aspect"},
                {"action":"swap_aspect"},{"action":"toggle_custom_aspect"},{"action":"apply_crop"},
                {"action":"undo"},{"action":"redo"},{"action":"reset_edits"},{"action":"revert"},{"action":"save"},
                {"action":"toggle_palette"},{"action":"palette_query","query":"crop"},{"action":"palette_focus","index":1},
                {"action":"palette_next"},{"action":"palette_prev"},{"action":"palette_activate"},{"action":"close_palette"},
                {"action":"rotate_right"},{"action":"rotate_left"}]"#,
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
        assert_eq!(steps[24], Action::ToggleAbout);
        assert_eq!(steps[25], Action::MenuFocus { index: 2 });
        assert_eq!(steps[28], Action::MenuActivate);
        assert_eq!(steps[29], Action::SetAspect { aspect: Aspect::Ratio { w: 16.0, h: 9.0 } });
        assert_eq!(steps[30], Action::SetAspect { aspect: Aspect::Free });
        assert_eq!(steps[33], Action::ToggleCustomAspect);
        assert_eq!(steps[34], Action::ApplyCrop);
        assert_eq!(steps[38], Action::Revert);
        assert_eq!(steps[41], Action::PaletteQuery { query: "crop".into() });
        assert_eq!(steps[45], Action::PaletteActivate);
        assert_eq!(steps[47], Action::RotateRight);
        assert_eq!(steps.len(), 49);
        let json = serde_json::to_string(&Action::SetCrop(Rect::new(1.0, 2.0, 3.0, 4.0))).unwrap();
        assert_eq!(json, r#"{"action":"set_crop","x":1.0,"y":2.0,"w":3.0,"h":4.0}"#);
        assert!(parse_script(r#"[{"action":"jump"}]"#).is_err());
        assert!(parse_script(r#"[{"action":"set_aspect","aspect":"wide"}]"#).is_err());
    }
}
