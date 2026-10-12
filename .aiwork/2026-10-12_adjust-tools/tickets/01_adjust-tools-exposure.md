---
status: ready
blocked_by: []
references:
  - "Research: ../research.md"
---

# 01 — Adjustment tools + Exposure

**What to build:** The shared machinery for one-slider tools, with Exposure as the first tool. Adjustments are steps in the edit stack; they are rendered on the CPU, saved like crop (photo overwritten, original kept in the backup), and shown live while dragging. The app shows a one-slider panel for the open tool. Exposure opens at auto exposure (average brightness to middle grey), slider ±2 EV.

## Acceptance criteria

- [ ] Adjustment steps serialise in the edit stack (version bumped if needed) and combine correctly with crop
- [ ] Dragging the slider gives a live preview without noticeable lag on a full-size photo
- [ ] Leaving the photo saves it with no extra step
- [ ] Opening the tool applies the opening value as one undoable step; Esc reverts it; double-clicking the slider resets it to the opening value
- [ ] Available from the command palette; works the same through `pcr` (edits JSON, render, save)
- [ ] Unit test in core for the pixel math; a render golden image
- [ ] `just verify` passes; headless shot shows the effect
