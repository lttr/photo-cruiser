---
name: verify-app
description: Verify a change to Photo Cruiser with evidence (tests, CLI output, real-window screenshot + state JSON) before calling it done. Use after implementing or fixing anything in core, cli or app, when asked to verify/check/test the work, or before committing.
---

# Verify Photo Cruiser work

"Done" means the evidence below exists and you looked at it. Report what you checked and how. If something failed, quote the failing output.

## 1. Baseline (always)

```bash
just verify
```

This runs `just check` (fmt, clippy `-D warnings`, all tests), then a CLI smoke test, then the real-window smoke script (`scripts/smoke.json` on `fixtures/`, headless through `scripts/headless.sh`), and asserts the state JSON (index 4, crop mode, crop 200×150 @ 40,30, no errors, not loading). It must print `verify ok`. Then **Read `/tmp/photo-cruiser/verify.png`**: img5.HEIC (checkerboard) should be dimmed to about 50% outside a sharp-edged selection, with the status line `img5.HEIC — 5 / 9 — crop 200×150 @ 40,30`.

## 2. Change-specific evidence

Pick whatever the change touches:

- **core logic** (geom, crop, loader, edit, render, decode): add or extend a unit test in the module, or in `crates/core/tests/fixtures.rs` for anything that touches real files. Add the fixture to `scripts/gen_fixtures.py` if a new one is needed, then run `just fixtures`.
- **CLI**: add a test in `crates/cli/tests/cli.rs` (`assert_cmd`) covering the exit code and the JSON shape. Run the command once by hand and show its output.
- **GUI behaviour or rendering**: write a script in `/tmp/photo-cruiser/<name>.json` that reaches the state, run it headless (`just shot`, or `scripts/headless.sh`; see the `run-app` skill), then:
  - check the state JSON fields with `jq -e`;
  - **Read the PNG**, and describe what you see and whether it matches what you expected;
  - for mouse or keyboard behaviour, add an `iced_test` case in `crates/app/src/tests.rs`. Script mode can't send input events.
- **Pixel-level claims** (dim strength, colours): measure them, don't eyeball them. For example, compare mean RGB of a region between two shots with a short `uv run --with pillow` snippet. The test `crop_draw_move_resize_dim_and_esc` shows the method. The real window and `iced_test` render differently (MSAA is off in the app on purpose), so check both when it matters.
- **Real photos**: optionally run against `~/sandbox/photo-cruiser/test-formats`. Never commit those files or copy them into the repo.
  - CLI: `target/debug/pcr list ~/sandbox/photo-cruiser/test-formats --json` lists 7 entries.
  - GUI: next×6, toggle_crop, set_crop. The state should show `7 / 7` and the crop.

## 3. Report

- The commands you ran and their results (tests passed, `verify ok`).
- The evidence you looked at: PNG paths and what they show, plus the relevant JSON fields.
- Anything not verified, and why.

## Never

- Use OS input injection (`wtype`, `xdotool`) or take desktop screenshots.
- Open a visible window for verification. Verification is headless; visible windows are only for when the user asks to see the app.
- Leave windows open.
- Claim GUI behaviour works on the strength of unit tests alone, when a shot or an `iced_test` case could show it.
