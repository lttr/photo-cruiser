---
status: done
verified: [checks, behaviour]
blocked_by: [01]
references:
  - "Research: ../research.md"
---

# 08 — Sharpen

**What to build:** Unsharp mask. Opens at amount 0.5, slider 0–1; radius 2 and threshold 0.5 fixed, as in darktable.

## Acceptance criteria

- [x] Radius scales with render size, so a downscaled preview looks like the full-size save
- [x] Opening the tool applies the opening value as one undoable step; Esc reverts it; double-clicking the slider resets it to the opening value
- [x] Available from the command palette; works the same through `pcr` (edits JSON, render, save)
- [x] Unit test in core for the pixel math; a render golden image
- [x] `just verify` passes; headless shot shows the effect

## Verification (checks, behaviour)

- `just check` green; `just verify` printed `verify ok`, including `verify-sharpen.png`: the tool opens at 0.5 and is back at 0.5 after a change plus Auto, on top of the shadows & highlights and local contrast edits the earlier steps saved; the footer shows "Sharpen 0.50" and the window frame and bar get a crisp light rim against the dark wall.
- Headless real window, fresh copy of `fixtures/adjust`, palette query "sharp": one row, "Sharpen" with key `P`. Activating it opens the tool at 0.5 (`edits` = sharpen 0.5), dirty, no errors.
- Radius scales with render size: core test `sharpen::radius_scales_with_render_size` (a half-size render sharpened at scale 0.5 is within 3 levels of the full-size result halved; at the full-size radius it's clearly off), and the golden test `sharpen_on_a_backlit_room` (a 160 px decode vs a full-size render downscaled to 160). In the real window, a 4512×3000 photo at Sharpen 1, against `pcr` renders resized to the same 628×941 area: full-size sharpened then downscaled differs by 4.47 levels on average, unsharpened by 4.48 (GPU resampling), sharpening the small image at the full-size radius by 5.85. At that downscale a 2 px radius is close to invisible, as in the save.
- Undo, Esc, Auto and No change: `update_sharpen_opens_at_half`. `P` key: `tool_keys`. pcr: `sharpen_render_and_save` (render, edits JSON, save). Pixel math: `sharpen::tests` (threshold, kernel, edges crisper, flat and 0 unchanged). Golden image: `window-sharpen.png`, checked by eye before blessing (the dark bar darker, the sky next to it brighter; nothing else changed).
- Real photos: 3 from the run's sample set (copied to `/tmp/photo-cruiser/08/real`, not committed), rendered by `pcr` at 0.5 and compared at 1:1: hair, leaves, fabric seams and a speckled surface get crisper, no visible halos.

## Notes

- Math: darktable's sharpen. Lightness (sRGB-encoded luminance, like Lab L) minus its Gaussian blur (σ = 2 px of the full-size photo, cut at 2.5 σ, as darktable's radius 2 works out); the part beyond the threshold (0.005, darktable's 0.5 in L 0–100) × the amount is added back, as a gain on linear RGB so the hue stays.
- Scale: `Tool::apply_at(img, value, scale)` and `adjust::apply_all(img, stack, scale)` take the render's scale of the full-size photo; `render` passes the decode's scale, the app the preview's width over the photo's full-size width. `Tool::apply` is `apply_at` at 1.
- Order: last, after colour, on the finished tones.
- Key `P`. JSON name `sharpen`.
