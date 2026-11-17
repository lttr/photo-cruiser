---
status: done
blocked_by: [01]
references:
  - "Research: ../research.md"
---

# 06 — Warmth

**What to build:** Opens at grey-world auto white balance; the slider shifts colour temperature warmer or cooler.

## Acceptance criteria

- [x] Auto removes a synthetic colour cast on a fixture
- [x] Opening the tool applies the opening value as one undoable step; Esc reverts it; double-clicking the slider resets it to the opening value
- [x] Available from the command palette; works the same through `pcr` (edits JSON, render, save)
- [x] Unit test in core for the pixel math; a render golden image
- [x] `just verify` passes; headless shot shows the effect

## Notes

- One unit is 100 mired: von Kries scaling of Bradford cone responses by the ratio of the black-body whites at 6504 K and 3941 K, raised to the value (−2…2). White keeps its luminance. Key `W`.
- Auto is grey world on near-grey pixels: a first estimate on every pixel, then up to four rounds over the pixels with saturation below 0.3 once balanced. Plain grey world turned photos of warm rooms blue. Warm scenes like a cream tent or a sunset still come out cooler; the slider takes it back.
- One slider moves between amber and blue only, so a green or magenta cast stays.
