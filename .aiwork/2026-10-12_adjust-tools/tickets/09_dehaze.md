---
status: ready
blocked_by: [01, 03]
references:
  - "Research: ../research.md"
---

# 09 — Dehaze

**What to build:** Like darktable's haze removal. Opens at strength 0.2, slider 0–1; distance fixed at 0.2. Uses the smoothing filter from Local contrast.

## Acceptance criteria

- [ ] A photo without haze changes only slightly at the opening value
- [ ] Opening the tool applies the opening value as one undoable step; Esc reverts it; double-clicking the slider resets it to the opening value
- [ ] Available from the command palette; works the same through `pcr` (edits JSON, render, save)
- [ ] Unit test in core for the pixel math; a render golden image
- [ ] `just verify` passes; headless shot shows the effect
