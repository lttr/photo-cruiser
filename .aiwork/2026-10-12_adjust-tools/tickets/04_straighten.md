---
status: done
verified: [checks, behaviour]
blocked_by: [01]
references:
  - "Research: ../research.md"
---

# 04 — Straighten

**What to build:** Opens at an angle detected from lines in the photo, slider ±45°. Empty corners are cropped automatically. Works together with the existing crop.

## Acceptance criteria

- [x] Auto angle is within 0.5° on a tilted synthetic fixture
- [x] A crop made before or after straightening stays inside the image and stays correct
- [x] Opening the tool applies the opening value as one undoable step; Esc reverts it; double-clicking the slider resets it to the opening value
- [x] Available from the command palette; works the same through `pcr` (edits JSON, render, save)
- [x] Unit test in core for the pixel math; a render golden image
- [x] `just verify` passes; headless shot shows the effect

## Verification (checks, behaviour)

- `just check` and `just verify` passed (`verify ok`, including `verify-straighten.png`). The new verify step opens Straighten on `fixtures/straighten/horizon.jpg` (turned 3.4° clockwise): it opens at −3.39°, and after a change plus Auto it's back at −3.39°. The shot shows the horizon level, the pole and walls upright, no empty corners, and the footer "Straighten −3.39°".
- Auto angle: −3.39° on the fixture (−3.4° expected); `straighten_levels_a_tilted_horizon` (golden.rs) also measures the horizon in the render: 3.4° before, under 0.5° after. On synthetic lines at 0, 0.3, 2.5, −4.2, 7.3 and 9.5° it's within 0.1°. Lines at 20° (a roof), speckle and flat images give 0.
- Crop, headless real window, each on a fresh copy: a crop made first (the house) and then straightened comes out straightened, with no empty corners (`b-crop-before`). After straightening, crop mode shows the whole straightened photo, and the strip selected there (`0,100 320×140`) is exactly what the applied crop shows (`b-crop-after`, `b-crop-after2`). Core: `straighten_before_the_crop`, `a_crop_is_part_of_the_straightened_frame` (crops at the frame's corner have no empty pixels); app: `update_straighten_opens_level_and_keeps_the_crop`.
- Undo, Esc, Auto: in the window, opening and then Undo leaves no edits; set 20° and then Esc leaves no edits. Auto is in the verify step. Palette query "straight" gives one row, "Straighten"; activating it opens the tool at −3.39°.
- pcr: `render --adjust straighten` → −3.39°, 297×223 (from 320×240); with `--crop 170,10,130,150` → 121×139, same angle; `save` with the edits JSON → saved 297×223, `edits` not dirty. Test: `straighten_render_and_save`.
- Goldens: `horizon-straighten.png`, `horizon-straighten-crop.png` (checked by eye before blessing).
- Real photos: the 12 samples in `/tmp/photo-cruiser/contrast-samples/` (people outdoors, mostly no straight lines). 10 open at 0°. The wooden tower opens at −1.73° (its posts end up parallel to the frame) and a kitchen photo with a window at +1.86°. Turned 3° clockwise by `pcr` itself, those two then open at −4.60° and −1.24°, so the change is −2.87° and −3.10° (−3° expected). The other 10 stay at 0: without clear lines, the tool doesn't turn.

## Notes

- Model: Straighten is geometry. It's done before the crop, and the crop is a rectangle of the straightened photo in the same coordinates as before (the straightened photo, the largest rectangle of the photo's shape inside the turned one, scaled up to the photo's size). So at 0° nothing changes, and a crop never reaches outside the photo at any angle. It's rendered at the photo's own resolution: the output is `fit_scale` × the crop (e.g. 320×240 at −3.39° → 297×223). The status line and crop sizes stay in the photo's frame.
- In the stack it's an `adjust` op (`tool: straighten`, degrees, clockwise positive), so the tool panel, palette, pcr `--adjust` and undo work as for the other tools. `EditStack::geometry()` includes it; `adjust::apply_all` skips it. `EditStack::geometry_for(Straighten)` (no crop, no straightening) is what the tool opens on and measures, so the app shows the whole photo while Straighten is open.
- Auto angle: a projection profile over edge pixels (each must roughly agree with the angle by its own gradient), the highest peak within ±10° that is 1.5× the score 3° to either side. Steeper lines (roofs, slopes) and texture give no peak, and then it opens at 0. The slider still goes to ±45°.
- Key Q. The old test `q_does_not_quit` now checks that Q opens Straighten.

