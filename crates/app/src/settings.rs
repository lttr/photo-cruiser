//! Choices remembered between runs, in `~/.config/photo-cruiser/settings.json`
//! (`$XDG_CONFIG_HOME` respected). Only the interactive app uses the file: script mode
//! and tests start from the defaults and never write it.
//!
//! A missing file means defaults; an unreadable one too (with a warning), and it is
//! replaced on the next change. Unknown fields are ignored, missing ones default.

use crate::grid::Layout;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The grid layout (`Z`).
    pub layout: Layout,
    /// File names shown (`N`).
    pub names: bool,
    /// The edits panel shown (`I`).
    pub edits_panel: bool,
}

/// `$XDG_CONFIG_HOME`, else `~/.config`.
pub fn config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
}

/// Where the interactive app keeps its settings.
pub fn default_path() -> Option<PathBuf> {
    Some(config_dir()?.join("photo-cruiser").join("settings.json"))
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Settings::default(),
            Err(e) => {
                eprintln!("photo-cruiser: cannot read {}: {e}", path.display());
                return Settings::default();
            }
        };
        serde_json::from_str(&text).unwrap_or_else(|e| {
            eprintln!("photo-cruiser: ignoring {}: {e}", path.display());
            Settings::default()
        })
    }

    /// Write to a temporary file, then rename: a crash never leaves half a file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(&tmp, json + "\n")?;
        std::fs::rename(&tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_bad_and_saved_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("settings.json");
        assert_eq!(Settings::load(&path), Settings::default(), "missing");
        let s = Settings { layout: Layout::Justified, names: true, edits_panel: true };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);
        assert!(std::fs::read_to_string(&path).unwrap().contains(r#""layout": "justified""#));
        std::fs::write(&path, r#"{"layout": "big", "future": 1}"#).unwrap();
        let partial = Settings { layout: Layout::Big, ..Settings::default() };
        assert_eq!(Settings::load(&path), partial, "unknown fields ignored, missing ones default");
        std::fs::write(&path, "{nope").unwrap();
        assert_eq!(Settings::load(&path), Settings::default(), "broken");
    }
}
