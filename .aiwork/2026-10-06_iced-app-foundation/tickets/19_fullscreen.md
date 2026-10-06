---
status: done
blocked_by: []
verified: [behaviour]
references:
  - "Spec: ../spec.md"
---

# 19 — Full screen

**What to build:** A full-screen view for looking at photos. `F11` or `F` toggle it, and `Esc` leaves it (in crop mode, `Esc` leaves crop mode first). In full screen the toolbar and status line are hidden, so the photo gets the whole monitor on a black background. Arrows, crop and everything else keep working.

Full screen is an `Action` (`toggle_fullscreen`, `exit_fullscreen`), so scripts and tests can drive it. `State` owns the flag, and the runtime sets the window mode from it. If the window manager changes the mode on its own (a compositor shortcut, say), a resize re-reads the real window mode, so the flag and the hidden bars stay in sync.

## Acceptance criteria

- [x] `F11` and `F` toggle full screen; `Esc` leaves it, but only after leaving crop mode
- [x] Toolbar and status line are hidden in full screen, and the image area fills the window
- [x] `--dump-state` reports `fullscreen`; a script-mode shot with `toggle_fullscreen` shows the window at monitor size
- [x] Crop coordinates stay correct in full screen (source pixels)
- [x] `just verify` green

## Verification (behaviour, 2026-10-06)

- `just verify`: ok (all tests, including `fullscreen_hides_bars_and_keys` and `update_fullscreen_toggle_and_exit`).
- Script `[next, toggle_fullscreen, toggle_crop, set_crop 40,30 200×150]`, real window on COSMIC: the window is 2560×1440 (monitor size) and state shows `fullscreen: true`, the crop is still 200×150 @ 40,30, and there are no errors. The shot has black pillarboxes; the photo reaches the top and bottom edges, so no bars are drawn.
- Script `[toggle_fullscreen, exit_fullscreen]`: the window goes back to 1100×850, `fullscreen: false`, and the toolbar and status bars are back (rgb 27,27,27 at the top and bottom).
- Keys are covered by `iced_test`, because script mode can't send input.
