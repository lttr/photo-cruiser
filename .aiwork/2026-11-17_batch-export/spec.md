---
status: not-started
references:
  - "Process: ../2026-11-05_cull-edit-export/intent.md (step 3)"
  - "Save path it reuses: ../2026-10-11_save-edits/spec.md"
---

# Spec — Export a folder tree of starred photos

Step 3 of cull → edit → export. The user asks an AI agent to do it in one sentence, so
there is no app UI. Instead it's one `pcr` command, documented in the `pcr` skill:

> Look at folder X, create Y next to it, keep X's sub-folder structure, and export every
> starred photo into Y as JPEG at archive quality, keeping EXIF.

```
pcr export-tree <X> -o <Y> [--starred] [--json]
```

(Name to settle in implementation: a separate subcommand keeps `pcr export FILE -o out.jpg`
unchanged. An alternative is letting `pcr export` take a folder.)

## Decisions

- **Output folder must be new or empty.** If Y exists and holds anything, the command fails
  before reading a single photo. Y's parent must exist. So there's no overwrite or skip
  logic and no merging with an older export. To export again, the user picks a new Y.
- **Mirror the tree.** Walk X recursively. Skip hidden entries (`.photo-cruiser/` included)
  and non-photos (`catalog::EXTENSIONS`). `X/a/b/p.heic` goes to `Y/a/b/p.jpg`. Folders with
  nothing to export aren't created.
- **`--starred`** keeps only photos with `rating::is_starred`. Without it, every photo is
  exported. The agent passes it for the archive use case.
- **One lossy step for edited photos.** Render `store::original(photo)` (the backup if the
  photo was saved) with the photo's current edit stack from its sidecar, then encode once.
  The photo file in X is never the source when a backup exists, because it's already a
  re-encode.
- **Every output is JPEG.** HEIC, PNG, WebP, AVIF and the rest are decoded and encoded.
  GIF uses its first frame.
- **Unedited JPEGs: copy unless too big.** Encode it with the archive preset. Keep that
  result only if it's at least *T* % smaller than the source (start at 20 %, tune in the
  benchmark). Otherwise copy the source byte for byte. So a q98 camera JPEG shrinks, and a
  q85 phone JPEG isn't re-encoded for nothing. This costs one encode per JPEG, which is fine
  because compute time is traded for size.
- **Metadata kept.** ICC profile plus EXIF, patched the way Save does it (`meta::patch_exif`:
  orientation 1, new pixel size, stale thumbnail dropped). The star carries over into the
  output's XMP (`rating::carry`). File modification time is the source's. Copied files keep
  everything as is.
  - Fixes a bug on the way: today's `pcr export` writes **no** EXIF or ICC
    (`export::save` passes `Metadata::default()`). Make single-file export use the same
    metadata path.
- **Name clashes.** `p.heic` and `p.jpg` in the same folder would both become `p.jpg`. The
  second one in natural order gets `p (2).jpg`, and the report says so.
- **Failures don't stop the batch.** Each photo is independent. The report lists failures
  with the error kind. Exit code is non-zero if any failed. Partial output stays in Y.
  Writes go through a temp name and are renamed, so no half-written JPEGs.
- **Parallel.** Photos run across CPU cores (a fixed worker pool, no async runtime in
  core). On gvfs sources, file access is already serialised by `source::one_at_a_time`.
- **Full resolution.** No downscale. The target is large screens and the occasional print.

## Archive preset

Goal: visually lossless at 100 % zoom on a large screen, fine for an occasional print,
and as small as possible at that quality. Encode time doesn't matter much.

Pick by a benchmark on `~/sandbox/photo-cruiser/test-photos` (local only, never committed),
run before the encoder code is written:

- Encoders: the current `jpeg-encoder` 0.7, plus mozjpeg-style (trellis quantisation,
  progressive) and jpegli-style. Candidate crates on crates.io: `mozjpeg` 0.10 (C library
  wrapper), `mozjpeg-rs` 0.9 (pure Rust), `jpegli` 0.1 (wrapper), `jpegli-rs` 0.12 (pure
  Rust). Their maturity isn't checked yet. Prefer pure Rust if it holds up, so the build
  doesn't gain another system library besides libheif.
- Settings: quality around 88–94, chroma 4:2:0 vs 4:4:4.
- Measure: bytes, encode time, and a perceptual score against the source render. SSIMULACRA2
  or butteraugli; check which tool is available. Plus a side-by-side visual check of a few
  crops by the user.
- Must decode correctly with our own decoder (`image`/zune-jpeg). `export.rs` already works
  around zune misreading non-interleaved scans, and progressive output needs the same
  round-trip test.

Write the chosen preset as a named constant in `core::export`. A `--quality` override is
allowed, but the default is the preset.

## Layout

- `core::export_tree` (new): walk, filter, plan (source → target path, copy vs encode),
  run, report. All logic and tests live here.
- `core::export`: archive preset encoder, metadata-carrying single export.
- `cli`: `export-tree` subcommand, text and `--json` report (per file: source, target,
  action `encoded` / `copied` / `failed`, bytes, ms; totals).
- `crates/cli/src/skill.md`: document the archive workflow so the agent maps the sentence
  above onto one command.

## Tests

On temp copies of `fixtures/` in a nested tree:

- A non-empty Y fails with nothing written. A missing Y is created.
- The tree is mirrored, hidden folders skipped, empty folders not created.
- `--starred` filters, and the star is present in the outputs.
- A saved photo renders from its backup with its stack. The output matches
  `render(original, stack)`.
- Unedited JPEG: a high-quality one is re-encoded, a low-quality one is copied byte for byte.
- EXIF kept and patched (orientation 1, dimensions). ICC kept. mtime kept.
- Name clash gets `(2)`. One corrupt photo fails alone.
- `pcr export` (single file) now keeps EXIF.

## Out of scope

Resizing, other output formats, a GUI action, incremental re-export into an existing Y.
