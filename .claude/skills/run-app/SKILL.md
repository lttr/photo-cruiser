---
name: run-app
description: Launch and drive Photo Cruiser (the iced GUI `photo-cruiser` and the `pcr` CLI) to see a change working in the real app. Use when asked to run, start, drive, screenshot or try the app, to reproduce a GUI bug, or to see what a given folder, photo or crop looks like.
---

# Run and drive Photo Cruiser

`.cargo/config.toml` sets `PKG_CONFIG=/usr/bin/pkg-config` (needed for libheif), so plain `cargo` works.

## Hard rules

- **Never** use `wtype`, `xdotool`, `ydotool` or any other OS input injection, and **never** take a desktop screenshot. Drive the GUI only through script mode, below.
- Your own runs are headless: wrap every GUI command in `scripts/headless.sh` (`just shot` and `just verify` already do). A visible window is only for when the user asks to see the app; see "Interactive run".
- Script mode exits on its own. Kill any window you start without a script (`just run`) as soon as you've finished with it, unless the user is using it.
- Personal photos (`~/sandbox/photo-cruiser/test-formats`, `test-photos`) are read-only inputs. Never copy them into the repo.

## CLI (fastest; no window)

```bash
cargo build --workspace                     # target/debug/{pcr,photo-cruiser}
target/debug/pcr list fixtures --json [--probe]
target/debug/pcr info fixtures/img5.HEIC --json
target/debug/pcr render fixtures/img5.HEIC --crop 10,20,100,50 [--max-side N] -o /tmp/photo-cruiser/x.png
target/debug/pcr export <file> --edits e.json -o /tmp/photo-cruiser/x.jpg --quality 90
```

With `--json`, errors also print `{"error": {"kind", "message"}}` to stdout. Exit codes: 0 ok, 1 usage, 2 I/O or encode, 3 decode or bad edits. Look at the PNG you rendered with the Read tool.

## GUI: script mode (the way to drive the real window)

```bash
scripts/headless.sh target/debug/photo-cruiser <dir> --script steps.json --shot /tmp/photo-cruiser/<name>.png --dump-state /tmp/photo-cruiser/<name>.json [--timeout 30] [--max-side 4096] [--transition 120]
# or: just shot <script> <dir> <name>
```

`scripts/headless.sh` starts a private headless sway (1600×1000 output, own runtime dir), points the app at it with `WAYLAND_DISPLAY`, and tears it down afterwards. The window is real (real wgpu, real compositor), just not on the desktop, and the in-app screenshot is identical. It passes the app's exit code through. `PC_HEADLESS=0` makes it a no-op.

A folder opens in the grid of thumbnails (`view: "grid"`); a file opens in the photo view. It opens the real window and applies each action, waiting until the image and its neighbours have decoded (and, in the grid, the visible thumbnails) before the next step. Then it uploads the current image to the GPU and takes an in-app `window::screenshot`, writes the state JSON, and exits. Exit codes: 0 ok (rejected actions and broken images are recorded in `errors`, not as failures), 1 runtime failure, 2 bad args or script, 3 timeout.

Put scripts and outputs in `/tmp/photo-cruiser/` (`mkdir -p` it), never in the repo. A script is a JSON array of actions:

```json
[
  {"action": "open", "path": "/abs/dir/or/file.jpg"},
  {"action": "next"}, {"action": "prev"},
  {"action": "goto", "index": 3},
  {"action": "up"}, {"action": "down"},
  {"action": "show_photo"}, {"action": "show_photo", "index": 3}, {"action": "show_grid"},
  {"action": "toggle_names"},
  {"action": "toggle_crop"},
  {"action": "set_crop", "x": 40, "y": 30, "w": 200, "h": 150},
  {"action": "clear_crop"}, {"action": "exit_crop"},
  {"action": "quit"}
]
```

- `goto` is 0-based. In the grid, `next`/`prev`/`goto`/`up`/`down` move the selection; `up`/`down` move by a row (the column count depends on the window width) and do nothing in the photo view.
- File names are hidden at start (status line `i / N`, no grid labels); `toggle_names` (key `N`) shows them (`name · i / N`).
- `show_photo` opens the current photo (or `index`); `show_grid` goes back. `toggle_crop`/`set_crop` from the grid switch to the photo view.
- `set_crop` takes source pixels (after orientation), turns crop mode on, and is clamped to the image.
- `quit` ends the script early; the shot and state files are still written.

The state JSON has these fields: `folder`, `count`, `index` (0-based), `position` (`"i / N"`), `file`, `view` (`grid`|`photo`), `mode` (`browse`|`crop`), `fullscreen`, `menu` (menu open), `help` (shortcuts panel open), `names` (file names shown), `crop` (`{x,y,w,h}` or null), `title`, `status`, `loading`, `display` (`sharp`|`preview`|`previous`|`none`: what the image area shows for the current index), `errors`, `window`, `shot`.

Read the PNG with the Read tool to see the window. Mouse drags and keyboard input can't be scripted; they're covered by the `iced_test` suite (`crates/app/src/tests.rs`), which is the place to add interaction checks.

## Interactive run (for the user)

When the user asks to open, show or launch the app for them, open a visible window on their desktop:

- `just run [dir]` for them to use (keys: → ← ↑ ↓ Enter C F/F11 N Esc Ctrl+O). Start it in the background and leave it open; they close it. Kill it only if they ask.
- `PC_HEADLESS=0 just shot <script> <dir> <name>` to show them a scripted state. The window closes by itself after the shot.

Never open a visible window for your own checks.

## Inputs

- `fixtures/`: 9 synthetic photos in all formats, including an EXIF-rotated JPEG and a broken file. Regenerate them with `just fixtures`.
- `~/sandbox/photo-cruiser/test-formats` (7 real photos) and `test-photos` (55): for local checks only. `kalendar_0006.HEIC` is genuinely stored sideways; that isn't a bug.
