---
status: done
verified: [behaviour]
blocked_by: [01, 03]
references:
  - "Research: ../research.md"
---

# 05 — Shadows & highlights

**What to build:** Lifts shadows and pulls back highlights, like darktable's tone equalizer "compress shadows/highlights" preset. Opens at 0.45 ("medium"), slider 0–1. Uses the smoothing filter from Local contrast.

## Acceptance criteria

- [x] Mid-tones barely change; no halos around high-contrast edges
- [x] Opening the tool applies the opening value as one undoable step; Esc reverts it; double-clicking the slider resets it to the opening value
- [x] Available from the command palette; works the same through `pcr` (edits JSON, render, save)
- [x] Unit test in core for the pixel math; a render golden image
- [x] `just verify` passes; headless shot shows the effect

## Verification (behaviour)

- `just verify` passed on e4dc4cc: `verify ok`, including `verify-shadows-highlights.png` (state: 0.45 = opening after Auto).
- Headless real window, palette query "shadows": one row, "Shadows & highlights" with key `T`. Activating it opens the tool at 0.45, and the state is dirty with no errors.
- Measured in the shots before and after (`adjust/window.jpg`): the wall at (160,100) goes from sRGB ~32 to ~60, and the wall next to the window (195,100) is ~58. That's within 3 levels of the far wall, so no halo. The sky goes from (214,231,247) to (190,205,220).
- Undo, Esc, Auto and No change: `update_shadows_highlights_opens_at_medium`. `T` key: `tool_keys`. No halos: `tones::lifts_shadows_without_halos` and the golden test `shadows_highlights_on_a_backlit_room`.

## Notes

- Local contrast (03) isn't built yet, so this ticket built the smoothing filter: `core::smooth`, a self-guided filter (He et al.). Planes with a longer side over 1024 px are filtered on a shrunk copy (fast guided filter) and applied at full size.
- The filter smooths log luminance with a radius of 2.5 % of the longer side (darktable's 5 % diameter) and keeps edges over about 0.5 EV. Across a strong step, under 0.4 EV bleeds into the filtered luminance, over about 1 % of the width. In the fixture, the wall next to the window ends up within 3 levels of the wall far from it.
- Curve: the change in EV for an area `d` EV from middle grey is `−s·d³/(d² + 1)`, capped at ±2 EV. Highlights get half: there's less room above middle grey than below. The darktable preset's real numbers weren't checked; 0.45 is just its name for "medium".
- Key `T`. JSON name `shadows_highlights`; `pcr` also accepts `tones`.
- Speed (release, loaded machine): about 0.4 s at 12 MP and 75 ms at preview size.
