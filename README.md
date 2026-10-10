# Photo Cruiser

A fast photo browser and editor written in Rust. Point it at a folder to see a grid of its photos, open one and flip through them. Previews come from embedded thumbnails, and neighbouring photos are preloaded so switching feels instant. It also has a non-destructive crop tool.

It's built to be driven by agents as well as people. The CLI can print JSON, and the GUI can run a script, take a screenshot and dump its state.

Supported formats: JPEG, PNG, GIF, WebP, BMP, and AVIF/HEIC/HEIF through the system libheif. EXIF orientation is respected.

## Layout

- `crates/core`: all photo logic (catalog, decoding, crop and edit stack, rendering, export). No GUI dependencies.
- `crates/app`: the `photo-cruiser` GUI, built on [iced](https://iced.rs).
- `crates/cli`: `pcr`, a command line front end over the same core (`list`, `info`, `render`, `export`, `app`). Add `--json` for machine-readable output.

## Build and run

Requires a Rust toolchain, [just](https://github.com/casey/just), and libheif ≥ 1.17 (`libheif-dev` on Debian/Ubuntu).

```sh
just run ~/Pictures      # launch the GUI on a folder
just cli list fixtures   # run the CLI
just check               # fmt, clippy, tests
```

## Keys

| Key         | Action                                |
| ----------- | ------------------------------------- |
| ← / →       | Previous / next photo                 |
| ↑ / ↓       | Grid: row up / down                   |
| Enter       | Grid: open the photo (or click it)    |
| C           | Toggle crop                           |
| F / F11     | Toggle full screen                    |
| N           | Show / hide file names                |
| Esc         | Leave crop or full screen, then back to the grid |
| Ctrl+O      | Open a folder                         |

## License

[MIT](LICENSE)
