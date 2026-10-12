---
status: ready
blocked_by: [01]
references:
  - "Research: ../research.md"
---

# 07 — Vignette

**What to build:** Opens at −0.5 (darker corners), slider −1…+1. Shape fixed at darktable's defaults (fall-off start 80%, radius 50%, shape 1). Applied relative to the cropped frame.

## Acceptance criteria

- [ ] The vignette follows the crop, not the original frame
- [ ] Opening the tool applies the opening value as one undoable step; Esc reverts it; double-clicking the slider resets it to the opening value
- [ ] Available from the command palette; works the same through `pcr` (edits JSON, render, save)
- [ ] Unit test in core for the pixel math; a render golden image
- [ ] `just verify` passes; headless shot shows the effect
