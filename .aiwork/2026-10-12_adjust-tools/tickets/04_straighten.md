---
status: ready
blocked_by: [01]
references:
  - "Research: ../research.md"
---

# 04 — Straighten

**What to build:** Opens at an angle detected from lines in the photo, slider ±45°. Empty corners are cropped automatically. Works together with the existing crop.

## Acceptance criteria

- [ ] Auto angle is within 0.5° on a tilted synthetic fixture
- [ ] A crop made before or after straightening stays inside the image and stays correct
- [ ] Opening the tool applies the opening value as one undoable step; Esc reverts it; double-clicking the slider resets it to the opening value
- [ ] Available from the command palette; works the same through `pcr` (edits JSON, render, save)
- [ ] Unit test in core for the pixel math; a render golden image
- [ ] `just verify` passes; headless shot shows the effect
