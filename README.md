# Photo Cruiser

A fast photo browser and editor written in Rust. Point it at a folder to see a grid of its photos, open one and flip through them. Previews come from embedded thumbnails, and neighbouring photos are preloaded so switching feels instant. It also edits: crop, turn, and one-slider tools for exposure, white balance, tones, detail and colour. Edits are non-destructive, and the original is always kept.

It's built to be driven by agents as well as people. The CLI can print JSON, and the GUI can run a script, take a screenshot and dump its state.

Supported formats: JPEG, PNG, GIF, WebP, BMP, and AVIF/HEIC/HEIF through the system libheif. EXIF orientation is respected.

## Principle: fewest steps

Processing photos should take as few steps as possible. Whatever the app can do on its own, it does, instead of asking. For example, edits are saved when you move on to the next photo: there's no "save changes?" prompt and no Save step to remember. Anything done automatically must be undoable; the original photo is always kept.

## Layout

- `crates/core`: all photo logic (catalog, decoding, crop and edit stack, rendering, export). No GUI dependencies.
- `crates/app`: the `photo-cruiser` GUI, built on [iced](https://iced.rs).
- `crates/cli`: `pcr`, a command line front end over the same core (`list`, `info`, `star`, `render`, `export`, `export-tree`, `edits`, `save`, `revert`, `app`, `skill`). Add `--json` for machine-readable output.

## Build and run

Requires a Rust toolchain, [just](https://github.com/casey/just), and libheif ≥ 1.17 (`libheif-dev` on Debian/Ubuntu).

```sh
just run ~/Pictures      # launch the GUI on a folder
just cli list fixtures   # run the CLI
just check               # fmt, clippy, tests
```

## Editing

Each tool is one slider. It opens at a value picked for the photo (auto exposure, auto white balance, or the darktable default), so often opening it is the whole edit. Shift+A (or double-clicking the slider) goes back to that value, 0 to no change, Esc cancels the tool. `j`/`k` or ↓/↑ move the slider, with Alt for fine steps.

| Key | Tool | Opens at |
| --- | --- | --- |
| C | Crop (R: next aspect ratio, X: portrait / landscape, Enter: apply) | |
| Shift+R / Shift+L | Turn right / left | |
| Q | Straighten: level the horizon, empty corners cut off (shows the whole photo while open; the crop is part of the straightened photo) | auto (from the photo's lines) |
| A | Auto levels: per-channel black and white point, for photos of documents | strength 1 |
| W | Warmth: white balance, warmer / cooler | auto (grey world) |
| M | Dehaze: remove haze | 0.2 |
| E | Exposure | auto (average to middle grey) |
| T | Shadows & highlights: lift shadows, pull back highlights | 0.45 |
| D | Local contrast: bring out detail, without halos | 125% |
| B | Contrast: an S-curve, darker shadows, brighter highlights | 1.2 |
| S | Colour: more in the shadows, less in the highlights | 1× |
| P | Sharpen: crisper edges and detail (unsharp mask) | 0.5 |
| V | Vignette: darker or brighter corners of the cropped frame | −0.5 |

Tools are applied in this order, whatever order you use them in. Hold O (or `\`) to see the original. The same tools work from `pcr`: `pcr render photo.jpg --adjust exposure=0.5 -o out.jpg`, or `pcr render --help` for all of them.

## Export and archive

The last step of cull → edit → export: star the photos worth keeping, edit them, then export the starred ones as JPEGs into a new folder:

```sh
pcr export-tree ~/Photos/2026 -o ~/Archive/2026 --starred
```

The sub-folders are mirrored. One command does everything, with these defaults:

| Choice | Default | Why |
| --- | --- | --- |
| Output folder | must be new or empty | Nothing is ever overwritten or merged. To export again, pick a new folder. |
| Which photos | all; `--starred` for starred only | Hidden folders such as `.photo-cruiser/` are skipped. |
| Format | JPEG | Opens everywhere. HEIC, PNG, WebP, … become `<name>.jpg`; JPEGs keep their names. |
| Encoder | [`mozjpeg-rs`](https://crates.io/crates/mozjpeg-rs) 0.9 (pure Rust port of Mozilla's mozjpeg, BSD-3-Clause), preset `max_compression`: progressive, optimised scans, trellis quantisation, deringing | About 25 % smaller than the `jpeg-encoder` crate (used before, still used by Save) at the same visual quality. Progressive adds 4 % at no loss. Encoding is slower (about 0.4 s per photo on 8 cores), which is fine for an archive. Not chosen: `jpegli-rs` (AGPL-3.0, deprecated) and the C `mozjpeg` crate (same output, needs cmake and nasm). |
| Decoding | `image` crate (JPEG via zune-jpeg, PNG, GIF, WebP, BMP); system libheif via `libheif-rs` (HEIC, AVIF) | The same decoders as the rest of the app. Progressive output was checked to decode identically with zune-jpeg and libjpeg. |
| Scaling | `image` crate, Lanczos3 | Sharper than the quick filter the app uses for previews. |
| Quality | 92 (`--quality N`) | Visually indistinguishable full screen on a 4K display (SSIMULACRA2 about 90). An A4 print shows no difference either. 95 is 31 % bigger and only helps when zooming to 100 %. |
| Chroma subsampling | as in the original | 4:2:2 stays 4:2:2, 4:2:0 stays 4:2:0, PNG and other lossless formats get 4:4:4. No colour resolution is lost, and no bytes are spent on colour detail that isn't there. |
| Size | longer side at most 4032 px (`--max-side N`, `0` = keep) | 4032 is the long side of 12 MP phone photos, so those keep every pixel. Bigger photos are scaled down (Lanczos). Fills a 5K screen; an A4 print gets about 345 ppi. |
| Edits | applied, rendered once from the original | Saved photos aren't encoded twice: the export starts from the backup of the original. |
| Unedited JPEGs | copied byte for byte unless re-encoding saves ≥ 20 % or the photo is over the size limit | No needless second lossy step; camera JPEGs still shrink by 40–60 %. |
| Metadata | EXIF (turned upright), colour profile, star, file time | The archive keeps dates, camera, GPS and ratings. |
| Failures | the rest is exported; exit code 4 and the report lists them | One broken photo doesn't stop an archive run. |

Measurements behind these choices: `.aiwork/2026-11-17_batch-export/research.md`.

## Install

```sh
just update       # build-release + install
just build-release
just install      # ~/.local: photo-cruiser, pcr, desktop entry, icon
just uninstall
```

`sudo just prefix=/usr/local install` installs for all users (run `just build-release` first, as yourself).

## Keys

The app lists every shortcut under `?` (or F1); Ctrl+K finds any action by name.

| Key | Action |
| --- | --- |
| ← / →, h / l | Previous / next photo |
| ↓ / ↑, j / k | Grid: row down / up; photo: next / previous; tool: slider down / up |
| Home / End, g / G | First / last photo |
| Enter | Grid: open the photo; crop: apply; tool: done |
| Esc | Close menu, cancel the tool, drop the crop selection, zoom out, leave full screen, back to the grid |
| 1 | Star on / off: a photo to keep |
| Ctrl+1 | Only the starred photos / all photos |
| C, Q, A, W, M, E, T, D, B, S, P, V | Crop and the tools, see [Editing](#editing) |
| Shift+R / Shift+L | Turn right / left |
| Ctrl+Z, u / Ctrl+Shift+Z, Ctrl+Y | Undo / redo |
| Ctrl+S | Save now (leaving a photo saves it) |
| O, `\` (hold) | Show the original photo, without edits |
| + / -, wheel | Zoom in / out (drag to move around) |
| Shift+1, double-click | Zoom: whole photo / actual pixels |
| F, F11 | Full screen on / off |
| N | File names on / off |
| I | Info panel: histogram and camera settings |
| Shift+E | Edits panel: the photo's edits, in order |
| Z | Grid: small / big thumbnails / justified |
| Ctrl+K | Find an action by name |
| Ctrl+O | Open a folder |
| F10 | Menu |
| ?, F1 | Keyboard shortcuts |

## License

[MIT](LICENSE)
