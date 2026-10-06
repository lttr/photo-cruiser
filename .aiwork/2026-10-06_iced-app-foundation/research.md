---
references:
  - "Prototype repo: ~/sandbox/photo-cruiser (README.md, .aiwork/2026-10-06_photo-browser-prototypes/spec*.md)"
  - "Reference implementation to port: ~/sandbox/photo-cruiser/iced/src/{decode,crop,theme}.rs"
  - https://html.lukastrumm.com/photo-cruiser-stack-comparison/
  - https://docs.rs/iced_test/0.14.0/iced_test/
---

# Findings from the prototypes

Ten prototypes implemented the same features (folder browse with ←/→, natural sort, AVIF+HEIC,
free-ratio crop with 50% dim): web, Electron, Tauri, COSMIC, kitty, Deno desktop, egui, Slint,
GTK4, plain iced. Weighted comparison → **iced 0.14** (MIT): native wgpu speed, shader widget for
GPU previews, real layouts + multi-window, headless `iced_test`, COSMIC-like theming, and a path to
full libcosmic later (libcosmic is built on iced). Slint scored highest but its licence was ruled out.

## Decoding & formats

- `image` crate (jpeg/png/gif/webp/bmp) + `libheif-rs = { version = "3", default-features = false, features = ["v1_17"] }` for AVIF/HEIC/HEIF. The default feature targets libheif 1.23; Ubuntu 24.04 ships 1.17.6.
- Build deps: `libheif-dev`; runtime: `libheif1` + libde265 and aom plugins. On Windows/macOS libheif must be bundled (vcpkg / Homebrew) — the main distribution hurdle.
- libheif applies HEIF rotation/mirror itself; JPEG needs EXIF orientation via `image`'s decoder orientation.
- HEIC **encoding** isn't available (no x265 plugin locally, sharp/libvips prebuilds exclude HEVC). Not needed: the user doesn't need HEIC output.
- `image`'s AVIF *decode* needs native dav1d → go through libheif instead.
- Decode cost: 2500² JPEG ~tens of ms, 2500² HEIC ~0.15–0.5 s native (WASM ~1.5 s).
- GPU texture limits: the test wgpu backend caps textures at 2048 px → downscale display images to `max_texture_side`, keep source dimensions for coordinates.
- iced_wgpu uploads images > 2 MB asynchronously (first frame may lack the image).

## Responsiveness

- "Ignore stale results" is not enough: all decodes still run and the UI lags seconds behind on a held arrow key. Working pattern everywhere: **one decode in flight, always for the latest requested index**; preload ±1 when idle; generation counter per folder to drop results from an old folder.
- Decode off the UI thread; deliver results as messages.

## Crop

- Selection stored in **source-image pixels** (survives resize, trivial to apply later); mapped to the contain-fit display rect.
- Draw / move (drag inside) / resize (edges & corners, ~8 px grab zone), clamped to image. Arrow keys clear the selection.
- 50% black dim outside the selection. **Blending space matters**: iced with the `web-colors` feature blends in sRGB (exactly 50%); without it wgpu blends linearly and 50% black only darkens ~27%. Kitty also blends linearly (needed alpha ≈ 0.78).

## iced 0.14 specifics

- Features used: `default-features = false` + `wgpu, tiny-skia, crisp, web-colors, thread-pool, x11, wayland, canvas, image-without-codecs`.
- `iced_test` Emulator runs the real app headless: click by text/selector, raw mouse press/move/release through real widgets, GPU snapshots.
  Limits: no arrow keys in instructions (only Enter/Esc/Tab/Backspace), **subscriptions receive no events**, `Emulator::screenshot` misses async-uploaded images and breaks later interaction → use `Simulator::snapshot` with a delay.
  Consequence: route keyboard shortcuts through a widget (e.g. the canvas overlay), not `event::listen`.
- `window::screenshot` gives real-window evidence without desktop screenshots.
- Thin docs, API churn between versions; reading crate sources was necessary.

## Theming / native look

- COSMIC theme is readable RON under `~/.config/cosmic/`: `com.system76.CosmicTheme.Mode/v1/is_dark`, `com.system76.CosmicTheme.{Dark,Light}/v1/{accent,background,primary,button,corner_radii,…}`, font in `com.system76.CosmicTk/v1/interface_font` (Open Sans). Parse leniently with `ron` + serde; fall back to a built-in dark palette on other OSes.
- Pill buttons via `radius_xl` (160). No COSMIC header bar in plain iced (compositor decorations). Live theme reload not done yet.

## Advanced processing (future)

- Needs float (f32, linear) pipeline, GPU preview (wgpu shaders/compute), CPU full-res export.
- Local contrast à la Darktable = local Laplacian / bilateral-style filter; CLAHE (libvips/sharp) is a cheaper approximation.
- Any web view stack pays per-preview transfer (Tauri: Rust → JPEG → WebKit ≈ 30–80 ms est.). Native wgpu avoids it.

## Linux integration

- Folder dialog: `rfd` 0.17 (XDG portal; no `tokio` feature anymore, use defaults).
- Samba: start with GVfs-mounted paths (`/run/user/$UID/gvfs/smb-share:…`), which work as normal paths; native SMB client later.
- Set an application id + desktop entry (not done in the prototype).
- Harmless: `xkbcommon … Compose file for locale "en_US.UTF8"` (user locale lacks the dash).

## Machine & agent gotchas

- Linuxbrew `pkg-config` shadows the system one → every Rust build needs `PKG_CONFIG=/usr/bin/pkg-config`.
- **Never** use `wtype`/OS input injection (sent keys to the user's Firefox) and **never** take full-desktop screenshots (capture the user's private screen). Use headless tests, in-app screenshots, and CLI output instead.
- Personal test photos (`~/sandbox/photo-cruiser/test-photos`, `test-formats`) must not be committed.
