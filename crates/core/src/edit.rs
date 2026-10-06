//! Non-destructive edits: an ordered stack of operations, serialised as versioned JSON.

use crate::geom::Rect;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};

pub const EDIT_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// Crop to a rectangle in source pixels (oriented, full resolution).
    Crop(Rect),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EditStack {
    pub version: u32,
    #[serde(default)]
    pub ops: Vec<Op>,
}

impl Default for EditStack {
    fn default() -> Self {
        EditStack { version: EDIT_VERSION, ops: Vec::new() }
    }
}

impl EditStack {
    pub fn with(mut self, op: Op) -> Self {
        self.ops.push(op);
        self
    }

    pub fn from_json(text: &str) -> Result<Self> {
        let stack: EditStack = serde_json::from_str(text).map_err(|e| Error::Edits(e.to_string()))?;
        if stack.version != EDIT_VERSION {
            return Err(Error::Edits(format!(
                "unsupported version {} (expected {EDIT_VERSION})",
                stack.version
            )));
        }
        Ok(stack)
    }

    /// Read an edits JSON file: `Error::Io` if unreadable, `Error::Edits` if invalid.
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
        EditStack::from_json(&text).map_err(|e| Error::Edits(format!("{}: {e}", path.display())))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("edit stack serialises")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip() {
        let s = EditStack::default().with(Op::Crop(Rect::new(1.0, 2.0, 30.0, 40.0)));
        let json = s.to_json();
        assert!(json.contains(r#""op": "crop""#), "{json}");
        assert_eq!(EditStack::from_json(&json).unwrap(), s);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(EditStack::from_json(r#"{"version": 99, "ops": []}"#).is_err());
        assert!(EditStack::from_json(r#"{"version": 1, "ops": [{"op": "blur"}]}"#).is_err());
        assert_eq!(EditStack::from_json(r#"{"version": 1}"#).unwrap(), EditStack::default());
    }
}
