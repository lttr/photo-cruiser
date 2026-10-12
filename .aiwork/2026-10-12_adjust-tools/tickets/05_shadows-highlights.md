---
status: ready
blocked_by: [01, 03]
references:
  - "Research: ../research.md"
---

# 05 — Shadows & highlights

**What to build:** Lifts shadows and pulls back highlights, like darktable's tone equalizer "compress shadows/highlights" preset. Opens at 0.45 ("medium"), slider 0–1. Uses the smoothing filter from Local contrast.

## Acceptance criteria

- [ ] Mid-tones barely change; no halos around high-contrast edges
- [ ] Opening the tool applies the opening value as one undoable step; Esc reverts it; double-clicking the slider resets it to the opening value
- [ ] Available from the command palette; works the same through `pcr` (edits JSON, render, save)
- [ ] Unit test in core for the pixel math; a render golden image
- [ ] `just verify` passes; headless shot shows the effect
