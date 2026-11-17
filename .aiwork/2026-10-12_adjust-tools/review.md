---
reviewed_sha: cc1b1fb
verified: [checks, behaviour, review]
---

# Review: adjust tools 04, 07, 08, 09, 10 (e111906..cc1b1fb)

## Passes
- `/simplify` (4 reviewers): 8 cleanups applied in 460cab1, 7d0b624, 765d585. All of them leave output byte-identical; 120 `pcr` renders were compared against the previous build. Cleanups left for later are in `implementation-notes.md`.
- `/code-review xhigh --fix`: 10 findings, 6 fixed in cc1b1fb.
  - The Sharpen preview radius ignored the preview's shrink to screen size.
  - Straighten's line detection overflowed its bands. Test: `line_score_keeps_far_edges_in_their_own_bands`.
  - `render` didn't clamp the Straighten angle.
  - Dehaze kept a full-size linear copy of the photo.
  - Save snapped the crop of a straightened JPEG to even pixels for nothing.
  - The pipeline docs didn't mention Straighten.
- Gate: `just verify` passes at cc1b1fb. All 15 headless shots were produced, and I checked the five new tools' shots by eye.

## Left open
- **Zoom on a straightened photo:** "actual pixels" shows it at about 108%, and the size in the status line is about 7% larger than the saved file. `Picture.source` is the frame size before straightening. Separating frame size from pixel size is a design change.
- **Straighten opening:** detection runs on the UI thread. It's capped at 640 px; on a heavily textured photo it may stall for tens of milliseconds.
- **`pcr --adjust straighten`:** it renders a full-size image only so detection can shrink it.
- **`straighten::edges`:** it repeats `Decoded::downscale`'s shrink logic, because `downscale` takes ownership.
- **Flaky test:** `exposure_slider_drag_and_double_click` failed a few times under load and passed on every rerun.
