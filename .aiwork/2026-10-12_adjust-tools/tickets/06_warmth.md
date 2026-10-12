---
status: ready
blocked_by: [01]
references:
  - "Research: ../research.md"
---

# 06 — Warmth

**What to build:** Opens at grey-world auto white balance; the slider shifts colour temperature warmer or cooler.

## Acceptance criteria

- [ ] Auto removes a synthetic colour cast on a fixture
- [ ] Opening the tool applies the opening value as one undoable step; Esc reverts it; double-clicking the slider resets it to the opening value
- [ ] Available from the command palette; works the same through `pcr` (edits JSON, render, save)
- [ ] Unit test in core for the pixel math; a render golden image
- [ ] `just verify` passes; headless shot shows the effect
