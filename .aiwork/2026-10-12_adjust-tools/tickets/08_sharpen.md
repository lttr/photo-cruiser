---
status: ready
blocked_by: [01]
references:
  - "Research: ../research.md"
---

# 08 — Sharpen

**What to build:** Unsharp mask. Opens at amount 0.5, slider 0–1; radius 2 and threshold 0.5 fixed, as in darktable.

## Acceptance criteria

- [ ] Radius scales with render size, so a downscaled preview looks like the full-size save
- [ ] Opening the tool applies the opening value as one undoable step; Esc reverts it; double-clicking the slider resets it to the opening value
- [ ] Available from the command palette; works the same through `pcr` (edits JSON, render, save)
- [ ] Unit test in core for the pixel math; a render golden image
- [ ] `just verify` passes; headless shot shows the effect
