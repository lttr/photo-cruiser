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
    /// Turn clockwise by `quarters` × 90° (1–3), after the crop.
    Rotate { quarters: u8 },
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

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// The crop, if any. A stack holds at most one.
    pub fn crop(&self) -> Option<Rect> {
        self.ops.iter().find_map(|op| match op {
            Op::Crop(r) => Some(*r),
            _ => None,
        })
    }

    /// Clockwise quarter turns, 0–3. A stack holds at most one rotation, after the crop.
    pub fn rotation(&self) -> u8 {
        self.ops
            .iter()
            .find_map(|op| match op {
                Op::Rotate { quarters } => Some(quarters % 4),
                _ => None,
            })
            .unwrap_or(0)
    }

    /// Turn by `quarters` × 90° more (negative: counter-clockwise). A full turn removes
    /// the rotation.
    pub fn rotate(&mut self, quarters: i32) {
        let q = (self.rotation() as i32 + quarters).rem_euclid(4) as u8;
        self.ops.retain(|op| !matches!(op, Op::Rotate { .. }));
        if q != 0 {
            self.ops.push(Op::Rotate { quarters: q });
        }
    }

    /// Replace the crop (`None` removes it). Crops never nest: the new one is in source
    /// pixels of the original, like the one it replaces.
    pub fn set_crop(&mut self, crop: Option<Rect>) {
        let at = self.ops.iter().position(|op| matches!(op, Op::Crop(_)));
        match (at, crop) {
            (Some(i), Some(r)) => self.ops[i] = Op::Crop(r),
            (Some(i), None) => {
                self.ops.remove(i);
            }
            (None, Some(r)) => self.ops.insert(0, Op::Crop(r)),
            (None, None) => {}
        }
    }
}

/// Undo / redo of one photo's stack: the stacks before and after the current one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct History {
    past: Vec<EditStack>,
    future: Vec<EditStack>,
}

impl History {
    /// Undo steps kept.
    pub const MAX: usize = 100;

    /// Replace `stack` with `new`, remembering the old one. A new change clears the redo
    /// steps. Returns false (and records nothing) if nothing changed.
    pub fn change(&mut self, stack: &mut EditStack, new: EditStack) -> bool {
        if *stack == new {
            return false;
        }
        self.past.push(std::mem::replace(stack, new));
        if self.past.len() > Self::MAX {
            self.past.remove(0);
        }
        self.future.clear();
        true
    }

    /// Step back. Returns false if there is nothing to undo.
    pub fn undo(&mut self, stack: &mut EditStack) -> bool {
        let Some(prev) = self.past.pop() else { return false };
        self.future.push(std::mem::replace(stack, prev));
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// Step forward again. Returns false if there is nothing to redo.
    pub fn redo(&mut self, stack: &mut EditStack) -> bool {
        let Some(next) = self.future.pop() else { return false };
        self.past.push(std::mem::replace(stack, next));
        true
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
    fn one_crop_replaced_or_removed() {
        let mut s = EditStack::default();
        assert_eq!(s.crop(), None);
        s.set_crop(Some(Rect::new(1.0, 1.0, 5.0, 5.0)));
        s.set_crop(Some(Rect::new(2.0, 2.0, 8.0, 8.0)));
        assert_eq!(s.ops, [Op::Crop(Rect::new(2.0, 2.0, 8.0, 8.0))]);
        s.set_crop(None);
        assert!(s.is_empty());
    }

    #[test]
    fn rotation_after_the_crop_and_normalised() {
        let mut s = EditStack::default();
        s.rotate(-1);
        assert_eq!(s.ops, [Op::Rotate { quarters: 3 }]);
        assert!(s.to_json().contains(r#""op": "rotate""#), "{}", s.to_json());
        s.set_crop(Some(Rect::new(1.0, 1.0, 5.0, 5.0)));
        s.rotate(2);
        assert_eq!(s.ops, [Op::Crop(Rect::new(1.0, 1.0, 5.0, 5.0)), Op::Rotate { quarters: 1 }]);
        s.rotate(-1);
        assert_eq!((s.rotation(), s.ops.len()), (0, 1), "a full turn is no rotation");
    }

    #[test]
    fn history_undo_redo() {
        let crop = |x| EditStack::default().with(Op::Crop(Rect::new(x, 0.0, 10.0, 10.0)));
        let (mut h, mut s) = (History::default(), EditStack::default());
        assert!(h.change(&mut s, crop(1.0)));
        assert!(!h.change(&mut s, crop(1.0)), "no change, no step");
        assert!(h.change(&mut s, crop(2.0)));
        assert!(h.undo(&mut s) && s == crop(1.0));
        assert!(h.undo(&mut s) && s.is_empty());
        assert!(!h.can_undo() && h.can_redo());
        assert!(!h.undo(&mut s));
        assert!(h.redo(&mut s) && s == crop(1.0));
        // A new change drops the redo steps.
        assert!(h.change(&mut s, crop(3.0)));
        assert!(h.can_undo() && !h.can_redo());
        assert!(!h.redo(&mut s));
        for i in 0..150 {
            h.change(&mut s, crop(10.0 + i as f32));
        }
        let mut undos = 0;
        while h.undo(&mut s) {
            undos += 1;
        }
        assert_eq!(undos, History::MAX);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(EditStack::from_json(r#"{"version": 99, "ops": []}"#).is_err());
        assert!(EditStack::from_json(r#"{"version": 1, "ops": [{"op": "blur"}]}"#).is_err());
        assert_eq!(EditStack::from_json(r#"{"version": 1}"#).unwrap(), EditStack::default());
    }
}
