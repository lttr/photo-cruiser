---
status: done
verified: [checks, behaviour]
blocked_by: [01]
references:
  - "Research: ../research.md"
---

# 10 — Contrast

**What to build:** Sigmoid contrast curve like darktable's sigmoid, without its other controls. Opens at about 1.2, slider 1.0–2.0; 1.0 means no change.

## Acceptance criteria

- [x] The opening value is checked on real photos (`PC_PHOTOS`) and adjusted if 1.2 is too strong or too weak; the decision goes in implementation notes
- [x] Opening the tool applies the opening value as one undoable step; Esc reverts it; double-clicking the slider resets it to the opening value
- [x] Available from the command palette; works the same through `pcr` (edits JSON, render, save)
- [x] Unit test in core for the pixel math; a render golden image
- [x] `just verify` passes; headless shot shows the effect

## Verification (checks, behaviour)

- `just verify` passed: `verify ok`, including `verify-contrast.png`. The tool opens at 1.2 and is back at 1.2 after a change plus Auto; the footer shows "Contrast 1.20". Against the shot before it (same stack without Contrast), the wall goes from about 59 to 50 (red, screen pixels), the sky from 196 to 208.
- Headless real window, fresh copy of `fixtures/adjust`, palette query "contrast": two rows, "Contrast" (`B`) first, "Local contrast" (`D`). Activating it opens the tool at 1.2 (`edits` = contrast 1.2), dirty, no errors.
- Tests: `contrast::tests` (black, grey and white pinned; 1 changes nothing; darker shadows and brighter highlights, stronger at 2, monotonic, no clipping; the slope at middle grey is the value). Golden image `window-contrast.png`, checked by eye first (room darker, sky brighter, tones in order). Undo, Esc, Auto and No change: `update_contrast_opens_at_1_2`. `B` key: `tool_keys`. pcr: `contrast_render_and_save` (render, edits JSON, save).
- Real photos: 12 samples from the user's library (10 camera and phone JPEGs, 1 HEIC; not committed), rendered by `pcr` at 1.0, 1.2, 1.4 and 1.6. At 1.2 the spread of lightness (standard deviation) rises by 8–13 %, the mean moves by at most 5 levels, and the share of near-white pixels barely changes. Flat camera JPEGs (haze, overcast snow) gain the most; already contrasty phone shots look a bit punchier but not harsh. From 1.4 skies and snow start to wash out and forest shadows block up. So 1.2 stays.

## Notes

- Curve: log-logistic (sigmoid's family) on linear light, `odds' = grey_odds · (odds / grey_odds)^value` with odds = `x / (1 − x)`. Black, middle grey (0.1842) and white stay put, so the photo's range isn't compressed (the photo is already display-ready); the log–log slope at middle grey is the value. sigmoid's target black/white and skew aren't modelled.
- Per channel (sigmoid's "per channel" mode, without its hue preservation), so colours get a little stronger with the contrast. An 8-bit LUT, so it's as fast as Exposure.
- Order: after local contrast, before colour. Key `B`, JSON name `contrast`. Panel shows `1.20`.
