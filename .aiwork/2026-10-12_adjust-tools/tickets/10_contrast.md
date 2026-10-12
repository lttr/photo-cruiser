---
status: ready
blocked_by: [01]
references:
  - "Research: ../research.md"
---

# 10 — Contrast

**What to build:** Sigmoid contrast curve like darktable's sigmoid, without its other controls. Opens at about 1.2, slider 1.0–2.0; 1.0 means no change.

## Acceptance criteria

- [ ] The opening value is checked on real photos (`PC_PHOTOS`) and adjusted if 1.2 is too strong or too weak; the decision goes in implementation notes
- [ ] Opening the tool applies the opening value as one undoable step; Esc reverts it; double-clicking the slider resets it to the opening value
- [ ] Available from the command palette; works the same through `pcr` (edits JSON, render, save)
- [ ] Unit test in core for the pixel math; a render golden image
- [ ] `just verify` passes; headless shot shows the effect
