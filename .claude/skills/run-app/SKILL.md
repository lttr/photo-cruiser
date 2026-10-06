---
name: run-app
description: Launch and drive Photo Cruiser (the iced GUI `photo-cruiser` and the `pcr` CLI) to see a change working in the real app. Use when asked to run, start, drive, screenshot or try the app, to reproduce a GUI bug, or to see what a given folder, photo or crop looks like.
---

# Run and drive Photo Cruiser

Every cargo call needs `PKG_CONFIG=/usr/bin/pkg-config`. `just` sets it for you.

## Hard rules

- **Never** use `wtype`, `xdotool`, `ydotool` or any other OS input injection, and **never** take a desktop screenshot. Drive the GUI only through script mode, below.
- Script mode exits on its own. Kill any window you start without a script (`just run`) as soon as you've finished with it. Never leave windows open.
- Personal photos (`~/sandbox/photo-cruiser/test-formats`, `test-photos`) are read-only inputs. Never copy them into the repo.

## CLI (fastest; no window)

```bash
just build                                  # target/debug/{pcr,photo-cruiser}
target/debug/pcr list fixtures --json [--probe]
target/debug/pcr info fixtures/img5.HEIC --json
target/debug/pcr render fixtures/img5.HEIC --crop 10,20,100,50 [--max-side N] -o /tmp/photo-cruiser/x.png
target/debug/pcr export <file> --edits e.json -o /tmp/photo-cruiser/x.jpg --quality 90
```

With `--json`, errors also print `{"error": {"kind", "message"}}` to stdout. Exit codes: 0 ok, 1 usage, 2 I/O or encode, 3 decode or bad edits. Look at the PNG you rendered with the Read tool.

## GUI: script mode (the way to drive the real window)

```bash
target/debug/photo-cruiser <dir> --script steps.json --shot /tmp/photo-cruiser/<name>.png --dump-state /tmp/photo-cruiser/<name>.json [--timeout 30] [--max-side 4096] [--transition 120]
# or: just shot <script> <dir> <name>
```

It opens the real window and applies each action, waiting until the image and its neighbours have decoded before the next step. Then it uploads the current image to the GPU and takes an in-app `window::screenshot`, writes the state JSON, and exits. Exit codes: 0 ok (rejected actions and broken images are recorded in `errors`, not as failures), 1 runtime failure, 2 bad args or script, 3 timeout.

Put scripts and outputs in `/tmp/photo-cruiser/` (`mkdir -p` it), never in the repo. A script is a JSON array of actions:

```json
[
  {"action": "open", "path": "/abs/dir/or/file.jpg"},
  {"action": "next"}, {"action": "prev"},
  {"action": "goto", "index": 3},
  {"action": "toggle_crop"},
  {"action": "set_crop", "x": 40, "y": 30, "w": 200, "h": 150},
  {"action": "clear_crop"}, {"action": "exit_crop"},
  {"action": "quit"}
]
```

- `goto` is 0-based.
- `set_crop` takes source pixels (after orientation), turns crop mode on, and is clamped to the image.
- `quit` ends the script early; the shot and state files are still written.

The state JSON has these fields: `folder`, `count`, `index` (0-based), `position` (`"i / N"`), `file`, `mode` (`browse`|`crop`), `crop` (`{x,y,w,h}` or null), `title`, `status`, `loading`, `display` (`sharp`|`preview`|`previous`|`none`: what the image area shows for the current index), `errors`, `window`, `shot`.

Read the PNG with the Read tool to see the window. Mouse drags and keyboard input can't be scripted; they're covered by the `iced_test` suite (`crates/app/src/tests.rs`), which is the place to add interaction checks.

## Interactive run (for the user)

`just run [dir]` opens the window for a person to use (keys: → ← C F/F11 Esc Ctrl+O). Only start it if the user asks, and kill it afterwards if you started it.

## Inputs

- `fixtures/`: 9 synthetic photos in all formats, including an EXIF-rotated JPEG and a broken file. Regenerate them with `just fixtures`.
- `~/sandbox/photo-cruiser/test-formats` (7 real photos) and `test-photos` (55): for local checks only. `kalendar_0006.HEIC` is genuinely stored sideways; that isn't a bug.
