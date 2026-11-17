---
status: done
verified: [behaviour]
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

## Verification (behaviour)

- `just verify` passed on 2fa143f/e4dc4cc: `verify ok`, including `verify-warmth.png` (state: warmth step < −0.5, value == opening after Auto).
- Headless real window, palette (`toggle_palette`, query "warmth", activate) on `adjust/warm-cast.jpg`: the state shows the warmth step at −0.60 = opening, dirty, no errors. In the shot, the wall at fixture (40,10) measured (110,112,112), from (122,111,93) in the original: neutral.
- The app (script and palette), a direct run and `pcr render --adjust warmth` all open at −0.60.
- Undo, Esc, Auto and No change: `update_warmth_opens_at_grey_world` (app state test). `W` key: `tool_keys`.

## Notes

- One unit is 100 mired: von Kries scaling of Bradford cone responses by the ratio of the black-body whites at 6504 K and 3941 K, raised to the value (−2…2). White keeps its luminance. Key `W`.
- Auto is grey world on near-grey pixels: a first estimate on every pixel, then up to four rounds over the pixels with saturation below 0.3 once balanced. Plain grey world turned photos of warm rooms blue. Warm scenes like a cream tent or a sunset still come out cooler; the slider takes it back.
- One slider moves between amber and blue only, so a green or magenta cast stays.
