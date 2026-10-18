---
status: done
verified: [checks, behaviour]
blocked_by: [01]
references:
  - "Research: ../research.md"
---

# 02 — Colour

**What to build:** Opens at 1.0× darktable's "basic colorfulness: standard" preset (saturation 0.2, shadows +0.25, highlights −0.25, vibrance-style weighting towards dull colours); the slider scales it from 0 to 2×.

## Acceptance criteria

- [x] Skin tones don't go orange at 1.0× on a portrait test photo
- [x] Opening the tool applies the opening value as one undoable step; Esc reverts it; double-clicking the slider resets it to the opening value
- [x] Available from the command palette; works the same through `pcr` (edits JSON, render, save)
- [x] Unit test in core for the pixel math; a render golden image
- [x] `just verify` passes; headless shot shows the effect

## Notes

- Port of color balance rgb's darktable UCS saturation path in `crates/core/src/colour.rs`. The preset has no vibrance (it's 0); the "towards dull colours" part comes from the soft clips: colours near the sRGB edge can't gain much.
- Differences from darktable: sRGB in and out (darktable works in Rec. 2020), CIE Y for the luminance masks, and both soft clips never push a pixel below where it started, so strength 0 is the identity and small strengths change little.
- Saturated colours already at the sRGB edge come out slightly darker (pure blue 255 → 223 at 1×): perceptual saturation trades lightness for chroma, as in darktable.
- Applied through a 33³ grid of sRGB values, trilinear: at most 4 levels off the exact math, about 10 ms to build per value (release).
- Skin tones checked on personal portraits (kalendar_0006, 0024, 0033, 0048): slightly warmer at 1×, not orange; 2× is warmer still.
- Key: S.
