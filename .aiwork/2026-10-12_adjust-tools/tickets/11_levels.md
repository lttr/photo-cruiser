---
status: done
verified: [checks, behaviour, review]
reviewed_sha: c0ee5cb
blocked_by: [01]
references:
  - "Research: ../research.md"
  - https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/rgb-levels/
---

# 11 — Auto levels

**What to build:** Like darktable's rgb levels "auto" button, with one strength slider. For each channel, the darkest 0.5% of pixels become black and the brightest 0.5% become white, with the grey point midway. The use case is photos of documents with weak contrast and tinted paper: the paper turns white, the text turns black.

Strength opens at 1, slider 0–2:

- **0 → 1** blends from the original to auto levels (for normal photos, where auto is too strong). Levels are linear per channel, so this is the same as moving the black and white points part of the way.
- **1 → 2** raises the clipping from 0.5% to 5% at each end (for documents with shadows or uneven light: whiter paper, blacker text).

## Acceptance criteria

- [x] A photo of a document with grey or yellowish paper comes out with white paper and dark text at strength 1; strength 2 whitens paper that is still grey at 1
- [x] Strength 0 leaves the photo unchanged
- [x] Black and white points are measured on the cropped area, not the whole photo
- [x] Opening the tool applies strength 1 as one undoable step; Esc reverts it; double-clicking the slider resets it to 1
- [x] Available from the command palette; works the same through `pcr` (edits JSON, render, save)
- [x] Unit test in core for the pixel math; a render golden image
- [x] `just verify` passes; headless shot shows the effect
