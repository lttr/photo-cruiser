# Implementation notes

## Spec ambiguities resolved

- **Crop mode stays on across → / ←.** The draft is applied and saved, and the next photo
  opens in crop mode with its own crop selected. So "draw, →, draw, →" crops a series.
  `C` (toggle off) applies the draft as well; only `Esc` drops it.
- **A stale sidecar is deleted when it's detected** (`store::load`), so the warning shows
  once and the current file counts as the original from then on. The old backup is still
  moved aside to `~N` on the next save.
- **The app never reloads a photo's sidecar during the session** (except right after its
  own save). Reloading could race with the save worker, which moves the photo for a moment.

## Deviations

- `webp` 0.3.1 depends on `libwebp-sys` 0.9.x, not 0.14.4 (still static, built from
  source). The crate can't write metadata, so `meta::webp_with_metadata` adds a `VP8X`
  header, `ICCP` and `EXIF` itself. That is tested in a unit test, not with other viewers.
- Optimized Huffman tables are only used for 4:4:4 JPEGs. With subsampled chroma,
  `jpeg-encoder` then writes one scan per component, and zune-jpeg (the decoder in
  `image`) turns those files green. Subsampled saves use the standard tables.
- APNGs are refused like animated GIF/WebP.

## For a human to check

- Revert of a converted photo sends our JPEG to the desktop trash (`trash` crate). It was
  checked with a private `XDG_DATA_HOME` in the CLI test, not in the real trash.
- Toggling crop mode on an already edited photo decodes the original again (there's no
  cache of the uncropped decode). On big HEICs that's a short wait before the whole
  photo shows.
- Dirty photos found at startup come only from the folder's own sidecars, not from the
  read-only fallback (a read-only folder can't be saved anyway).
