---
status: ready
blocked_by: [01]
references:
  - "Research: ../research.md"
---

# 03 — Local contrast

**What to build:** darktable's local contrast. Opens at detail 125%, slider 100–200%; highlights 50%, shadows 50% and midtone range 0.5 fixed. Builds the edge-aware smoothing filter that Shadows & highlights and Dehaze reuse.

## Acceptance criteria

- [ ] No halos along strong edges at 200%
- [ ] The smoothing filter lives in core, reusable by other tools
- [ ] Opening the tool applies the opening value as one undoable step; Esc reverts it; double-clicking the slider resets it to the opening value
- [ ] Available from the command palette; works the same through `pcr` (edits JSON, render, save)
- [ ] Unit test in core for the pixel math; a render golden image
- [ ] `just verify` passes; headless shot shows the effect
