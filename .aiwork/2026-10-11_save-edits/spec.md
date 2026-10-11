---
status: agent-done
references:
  - "Foundation spec: ../2026-10-06_iced-app-foundation/spec.md"
  - "Supersedes ticket: ../2026-10-06_iced-app-foundation/tickets/05_sidecars-and-crop-export.md"
---

# Spec — Apply, undo and save edits

Edits (a crop now, tonal adjustments later) are applied to a photo, can be undone, and are
saved by **overwriting the photo file**. The folder never holds two versions of a photo.
The untouched original is kept in a hidden backup folder, so every save is reversible and
re-saving never stacks quality loss.

## Principles

- **The edit stack is the truth.** A photo's look is `render(original, stack)`. The file on
  disk is a rendering of that, made by Save.
- **Always render from the original.** After the first save the original sits in the backup
  folder. Later saves render from it, never from the previous save, so there is no
  generation loss, and a crop can be widened again after it was saved.
- **Fewest steps.** Saving is automatic: leaving a photo saves it. There is no Save step to
  remember and no "unsaved changes" prompt.
- **No duplicates.** Save replaces the photo in the folder. There is no "save as copy".
- **Nothing is lost.** Edits are autosaved to a sidecar as they change, backups are never
  deleted automatically, and Revert brings back the original byte for byte.

## Files

One hidden folder per photo folder. The catalog already skips names starting with `.`.

```
<dir>/
  IMG_1234.jpg                      # the edited result (after Save)
  .photo-cruiser/
    edits/IMG_1234.jpg.json         # sidecar, keyed by the original file name
    originals/IMG_1234.jpg          # the untouched original, created by the first Save
```

Sidecar (`version` follows `EDIT_VERSION`):

```json
{
  "version": 1,
  "ops": [{ "op": "crop", "x": 120, "y": 80, "w": 3000, "h": 2000 }],
  "saved": { "ops": [{ "op": "crop", "x": 120, "y": 80, "w": 3000, "h": 2000 }] },
  "file": { "name": "IMG_1234.jpg", "size": 2811904, "mtime": "2026-10-11T09:12:44Z" }
}
```

- `ops`: the current stack (what the app shows).
- `saved`: the stack the file on disk was rendered from; `null` if never saved.
  **Dirty** = `ops != saved.ops` (no `saved` counts as an empty stack).
- `file`: the photo file as Save left it. The name can differ from the original's when Save
  converts the format (see Formats). Size and mtime detect outside changes.
- No sidecar, or an empty stack and no backup, means an unedited photo. A sidecar with an
  empty stack and no backup is removed.

**Read-only folders** (or SMB shares without write access): the sidecar goes to
`~/.local/share/photo-cruiser/edits/<sha256 of the absolute path>.json`, so edits and undo
still work. Save fails with a clear message that is recorded in `errors`.

**Outside changes:** if the photo's size or mtime no longer match `file`, another program
changed it after our save. The sidecar is stale: it is ignored, with a warning in `errors`,
and the current file counts as the original from then on. On the next Save the old backup
is not overwritten; it is renamed to `originals/<name>~N` first.

## Apply

- In crop mode the selection is a draft. **Enter** or an **Apply** footer button sends
  `apply_crop`. It puts `Op::Crop(rect)` into the stack, leaves crop mode and shows the
  cropped photo.
- A stack holds at most one crop. `EditStack::set_crop` replaces it (and `None` removes it).
  Crops never nest.
- Entering crop mode on a cropped photo shows the **whole original** with the current crop as
  the selection.
- `Esc` in crop mode drops the draft. The applied crop stays.
- Leaving the photo (→, ←, the grid, another folder, quit) with a draft selection applies it
  first, so drawing a crop and pressing → is enough. Only `Esc` throws a draft away.
- Display: the photo view shows `render(original, stack)` at display size. The loader's
  display job takes the stack; after Apply the current photo is rendered again off the UI
  thread, and the old image stays until the new one is ready (as with switching).
- Every change to the stack is written to the sidecar at once (a small JSON write, off the
  UI thread).

## Undo

| Action | Keys | Effect |
| --- | --- | --- |
| `undo` / `redo` | Ctrl+Z / Ctrl+Shift+Z, Ctrl+Y | Step through this photo's stack history |
| `reset_edits` | menu "Reset edits" | Stack becomes empty (undoable) |
| `revert` | menu "Revert to original" | `reset_edits`, then `save` |

- `edit::History { past: Vec<EditStack>, future: Vec<EditStack> }` in core, at most 100
  entries. A new change clears `future`.
- History is per photo and lasts for the session (not persisted). Switching photos keeps each
  photo's history.
- Undo works across a save: it changes the stack, the photo becomes dirty again, and it is
  saved again when you leave it.

## Save

### When

- **Automatically, when you leave a dirty photo:** →/←, Home/End, back to the grid, opening
  another folder, or quitting. The save is queued in the background, and browsing goes on
  at once.
- `revert` saves at once (there is nothing to wait for).
- `save` (Ctrl+S, menu) saves the current photo now, for when you want the file updated
  without leaving it. `pcr save <photo>` does the same from the CLI.
- A save of a photo that isn't dirty does nothing.
- **Quit** waits for queued saves to finish (the window shows "Saving 2 photos…"), then exits.
- **A crash or kill** before a queued save finishes loses nothing: the edits are in the
  sidecar, still dirty. On the next start, dirty photos in the opened folder are saved in the
  background (a one-line status message says so).
- **A failed save** (read-only folder, name taken, animated image) keeps the photo dirty. The
  error is in the status line and `errors`, and the grid shows a dot on it. The edits stay in
  the sidecar.

### How

1. **Source:** `originals/<name>` if it exists, else the photo file.
2. **Empty stack** (revert): move the backup back over the file (and remove a converted file,
   see Formats), then remove the sidecar. No decode and no encode.
3. **Otherwise:** decode the source at full resolution, `render`, then encode into a temp file
   `.<name>.pc-tmp` in the same folder.
4. **First save only:** rename the photo to `originals/<name>`. It is a rename on the same
   file system, so it is atomic and needs no copy.
5. Rename the temp file over the photo's name, and set its mtime to the original's (so
   date-sorted views elsewhere don't reshuffle).
6. Update the sidecar: `saved = ops`, plus the new `file`.

- If any step fails, the photo is still there under its name, either the old file or the
  original. A temp file is removed on failure. A leftover `.pc-tmp` from a crash is hidden
  and gets overwritten by the next save.
- The app saves on one background worker, in order. A photo queued twice is saved once,
  with its latest stack. Coming back to a photo while it is being saved is fine: it shows
  `render(original, stack)` as always, which is independent of the file being written.
- Afterwards the app drops the photo's cached decode, embedded preview, thumbnail and size,
  and loads them again.

### Metadata

`image` 0.25.10 decoders expose `exif_metadata()` and `icc_profile()`. The JPEG encoder
(`jpeg-encoder`, see Print quality) has `add_exif_metadata` and `add_icc_profile` (WebP: see
Formats). Before writing, the EXIF
block is patched in core, extending the TIFF walker that `exif_thumbnail` already has:

- **Orientation → 1:** the pixels are already turned.
- **The IFD1 thumbnail is dropped:** otherwise `embedded_preview` would flash the uncropped
  photo. Its 3% aspect check doesn't catch this, because the default crop keeps the photo's
  aspect.
- **PixelXDimension and PixelYDimension** are set to the new size.

The ICC profile is carried over unchanged. Other metadata (XMP, maker notes) may be lost; a
note in the code says so.

### Formats

JPEG, PNG and WebP keep their format: each is a deliberate choice (a camera photo, lossless
graphics, a picture made for the web). Everything else is **replaced** by a JPEG with the same
stem. That is still one file, and the original is in the backup. It also means one encode path
for most photos, and no dependence on which libheif encoder plugins a machine has.

| Original | Saved as |
| --- | --- |
| JPEG | JPEG, the original's tables raised to at least quality 95 (see Print quality) |
| PNG | PNG (keeps transparency) |
| WebP, lossy | WebP, lossy, quality 95 |
| WebP, lossless | WebP, lossless |
| HEIC/HEIF, AVIF, BMP, still GIF | JPEG, quality 95, 4:4:4 |
| GIF or WebP, animated | Save refused: "animated images can't be edited" |

- WebP: `image` only writes lossless WebP, so lossy WebP needs libwebp. The `webp` crate
  (0.3.1) wraps `libwebp-sys` (0.14.4), which builds libwebp from source and links it
  statically by default, so there is no new system dependency for packaging. Lossy and
  lossless are told apart by the first chunk (`VP8 ` vs `VP8L`, or the `VP8X` flags).
  Whether EXIF/ICC can be written into WebP through these crates (libwebp's mux API) is
  **unverified**; if not, WebP saves drop that metadata, and a note in the code says so.
- JPEG output has no alpha. A transparent HEIC/AVIF/GIF is flattened onto white.
- Replacing `IMG.heic` with `IMG.jpg` changes the file name. The app keeps the photo at its
  index and position. If `IMG.jpg` already exists (another photo), Save is refused instead of
  overwriting it.
- Revert of a converted file: the backup returns as `IMG.heic` and our `IMG.jpg` goes to the
  trash.

### Print quality

Saving defaults to quality good enough for printing: never worse than the original, never
below a high floor, and never smaller in pixels. Files are allowed to get bigger for it;
saving never shrinks a photo to save space.

- **Full resolution, no resampling.** Save renders from the full-resolution original. A crop
  copies pixels and doesn't resample them.
- **JPEG → JPEG: the original's tables, raised to a quality floor.** A small marker parser in
  core reads the original's DQT tables, the SOF sampling factors and the JFIF density. Each
  table entry becomes `min(original, IJG table at quality 95)`. A high-quality original
  (most cameras, q 95–98) keeps its own tables, so re-encoding adds almost no loss. A
  low-quality original (a q 75 web download) gets finer tables, so saving adds no further
  visible loss on top of what the original already lost.
  - `image` 0.25.10's encoder can't do this: it has fixed tables scaled by a quality number and
    fixed subsampling. JPEG output moves to the `jpeg-encoder` crate (0.7.1, pure Rust). It
    takes custom tables (`QuantizationTableType::Custom`), a `SamplingFactor`, optimized
    Huffman tables (on: smaller files, no loss), density, and `add_exif_metadata` /
    `add_icc_profile`.
  - `Custom` tables are in natural (row-major) order: its built-in Annex K table is listed
    that way, and `write_quantization_segment` writes them out through `ZIGZAG`. DQT stores
    zigzag order, so the parser de-zigzags. A test re-encodes a q 98 fixture and compares the
    DQT bytes. 16-bit DQT entries (rare) are clamped to 255.
  - If the tables can't be read, the fallback is quality 95.
- **Subsampling:** the original's (re-subsampling 4:2:0 as 4:4:4 only grows the file).
  Converted files and 4:4:4 originals use 4:4:4.
  - **Chroma grid:** on a 4:2:0 or 4:2:2 source, the crop's x and y are snapped down to even
    pixels when saving. Then the chroma samples line up with the original's, and the crop
    adds no extra colour blur. The 1 px shift is invisible.
- **HEIC/AVIF/BMP/GIF → JPEG:** quality 95, 4:4:4.
- **WebP lossy → WebP:** quality 95. WebP doesn't store its quality, and estimating it from
  the quantizer would be guesswork for little gain; a fixed high quality meets the print
  goal. The file may grow. WebP is a web format; a print shop gets a JPEG through
  `pcr export`.
- **PNG, lossless WebP:** lossless.
- **Print metadata kept:** the ICC profile (colour accuracy in print), the JFIF density, and
  the EXIF XResolution/YResolution (print size in inches). Dimensions are updated, the DPI is
  not changed: a crop prints smaller at the same DPI.
- **Bit depth:** the pipeline is 8-bit RGBA. A 16-bit PNG would lose depth, so Save refuses
  16-bit PNGs ("16-bit images can't be saved yet") until the pipeline is f32 end to end
  (the exposure ticket). 10-bit HEIC/AVIF from phones become 8-bit JPEG, as any JPEG would.
- `SaveReport` says what was used, e.g. `"quality": "original tables"`, `"raised to 95"`,
  `"default 95"`. `pcr save --json` prints it.

## Core API (`photo_cruiser_core`)

- `edit`: `EditStack::crop() -> Option<Rect>`, `EditStack::set_crop(Option<Rect>)`, `History`.
- `store` (new):
  - `PhotoEdits { stack, saved, file }`
  - `fn load(photo: &Path) -> Result<Option<PhotoEdits>>`: also does the stale check
  - `fn store(photo: &Path, edits: &PhotoEdits) -> Result<()>`: with the read-only fallback
  - `fn original(photo: &Path) -> PathBuf`: the backup if present, else the photo
  - `fn save(photo: &Path, stack: &EditStack) -> Result<SaveReport>`: report has `path`
    (possibly renamed), `format`, `bytes`, `ms`
- `export`: JPEG via `jpeg-encoder` with `JpegSettings { tables, sampling, density }` read by
  `decode::jpeg_settings(path)`. Also carries the EXIF and ICC
  metadata, and `patch_exif` (orientation, IFD1, dimensions).
  lossy WebP via the `webp` crate, `save_format(Format, lossless, animated) -> Result<OutFormat>`.
- `Error`: `ReadOnly`, `NameTaken`, `Animated`, `HighBitDepth`, `StaleEdits` (a warning, not
  fatal).

## CLI (`pcr`)

```
pcr edits <photo> [--json]            # stack, saved, dirty, backup path
pcr save <photo> [--edits e.json]     # bake the sidecar (or --edits) into the file
pcr revert <photo>                    # restore the original, drop the sidecar
pcr render|export <photo> ...         # use the sidecar by default; --edits wins; --no-edits
```

`pcr export` still writes a new file elsewhere: it is conversion, not saving. It renders from
the backup original, so a saved photo is not cropped twice.

## App

- Actions: `apply_crop`, `undo`, `redo`, `reset_edits`, `revert`, `save`.
- State: `edits: HashMap<PathBuf, (PhotoEdits, History)>`, filled when a photo is shown;
  `saving: Option<usize>`.
- Status line: `— saving…` while saves are queued, and the error when one failed. The grid
  shows a small dot on photos whose save failed.
- State dump: `edits` (current stack), `dirty`, `saved`, `saving` (queued paths), `backup`.
- No prompts anywhere: leaving the photo or the app saves.

## Testing

- **core:** in a tempdir with fixture copies:
  - save → file cropped, backup byte-identical to the input
  - save twice with a widened crop renders from the backup
  - revert → byte-identical to the input
  - the EXIF-rotated fixture is turned once only, and has Orientation 1 and no IFD1
  - read-only folder → `ReadOnly`, sidecar in the fallback location
  - an outside change → stale and ignored, backup kept as `~1`
  - HEIC → JPEG replacement and its revert; lossy WebP stays lossy WebP, lossless stays lossless
  - re-saving a q 98 JPEG keeps its DQT tables and sampling factors exactly; a q 75 JPEG
    gets tables no coarser than q 95; density and ICC survive
  - on a 4:2:0 source, a crop at odd x/y is saved at even x/y
  - a 16-bit PNG is refused
  - an animated GIF is refused (fixtures: add an animated GIF and a lossy WebP if missing)
- **cli:** `assert_cmd` for `edits`, `save`, `revert`, and `export` using the sidecar.
- **app:** `iced_test`: Enter applies, → with a draft applies and saves, Esc drops a draft,
  Ctrl+Z / Ctrl+Shift+Z, crop mode shows the original
  with the crop selected. Script mode: `[toggle_crop, set_crop, next]` (no explicit apply or save) on a temp
  copy of the fixtures. Script mode waits for the save queue to drain, as it waits for
  loading. The dump shows the previous photo `dirty: false`, and the file is cropped.
- Tests and scripts **never** save into `fixtures/` or the personal photo folders; they work
  on temp copies.

## Out of scope

- Lossless JPEG crop (jpegtran-style, MCU-aligned): a later improvement for crop-only stacks.
- Cleaning up old backups.
- Persisting undo history across sessions.
- Rotation and flip ops (they'll slot into the same stack and save path).

## Open questions

- None open.
