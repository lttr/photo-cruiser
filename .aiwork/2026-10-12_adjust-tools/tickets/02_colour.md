---
status: ready
blocked_by: [01]
references:
  - "Research: ../research.md"
---

# 02 — Colour

**What to build:** Opens at 1.0× darktable's "basic colorfulness: standard" preset (saturation 0.2, shadows +0.25, highlights −0.25, vibrance-style weighting towards dull colours); the slider scales it from 0 to 2×.

## Acceptance criteria

- [ ] Skin tones don't go orange at 1.0× on a portrait test photo
- [ ] Opening the tool applies the opening value as one undoable step; Esc reverts it; double-clicking the slider resets it to the opening value
- [ ] Available from the command palette; works the same through `pcr` (edits JSON, render, save)
- [ ] Unit test in core for the pixel math; a render golden image
- [ ] `just verify` passes; headless shot shows the effect
