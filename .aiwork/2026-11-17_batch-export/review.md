---
reviewed_sha: 4b76fa0
references:
  - "Spec: spec.md"
  - "Research: research.md"
---

# Review: `pcr export-tree`

Reviewed against archival advice the user pasted in. The advice is inspiration, not a spec:
originals untouched, JPEG for compatibility, don't downscale, keep ICC/EXIF/XMP, sRGB for print,
date-based names, dedup, checksums, 3-2-1.

The export already follows most of it. Originals stay untouched, ICC is embedded, EXIF is
kept and patched, an unedited JPEG is copied unless re-encoding saves 20 %, there is one
lossy step for edited photos, writes are atomic, and the preset was chosen by benchmark.

Numbers below come from `~/Photos` (21k HEIC, 31k JPEG, 2.5k MOV, 1.9k `.xmp`).

## Findings

### 1. XMP is dropped on re-encode

`decode::Metadata` holds only ICC and EXIF (`decode.rs:262`). Re-encoded photos lose
embedded XMP, and `.xmp` sidecars aren't copied. Only the star carries over (`rating::carry`).

In `~/Photos`:
- All 1897 sidecars are darktable's.
- Their `dc:subject` holds only darktable's auto-tags (`darktable|format|jpg`,
  `darktable|changed`).
- 8 of 3000 sampled JPEGs have an embedded title, description, subject or face regions.

Pros:
- Real tags, descriptions and faces would survive.

Cons:
- XMP holds stale or harmful parts:
  - `tiff:Orientation` and dimensions.
  - darktable's `history` and Lightroom's `crs:*`. These would make another editor apply
    edits a second time on a photo that already has them.
  - Apple's HDR gain-map references.
- So the packet must be filtered, not copied.
- Extended XMP over 64 KB needs splitting.
- Merging a sidecar with embedded XMP is fiddly.

Verdict: little value for this library. At most, carry `dc:title`, `dc:description`,
`dc:subject` (minus `darktable|*`) and `mwg-rs:Regions`. Otherwise skip.

Side note: 241 sidecars are tagged `darktable|changed`. Those photos were edited in
darktable, and `export-tree` exports them without those edits.

### 2. Dropped metadata isn't reported

EXIF goes missing in three places:
- EXIF over 65527 bytes is skipped (`export.rs`, `archive_jpeg`).
- `patch_exif` returning `None` drops it (`export::saved_metadata`).
- A failed `rating::carry` loses the star. It prints to stderr only (`rating.rs:70`).

Pros:
- Cheap.
- The agent can tell the user what was lost, which is the advice's "verify" step built in.

Cons:
- A few more report fields.
- Report only losses (the source had it, the output doesn't), not presence. Otherwise
  PNG screenshots without EXIF become noise.

Verdict: do it.

### 3. HEIC with only an nclx profile comes out untagged

`metadata_once` filters out NCLX profiles, and pixels aren't converted to sRGB. A P3 photo
with only nclx would come out untagged and look dull.

In `~/Photos`, every HEIC and HEIF checked (300 + 48) carries a Display P3 ICC. So this isn't
a real problem today.

The related question is converting to sRGB for print labs:

Pros:
- Labs and old software that ignore ICC print the colours correctly.

Cons:
- The P3 gamut is clipped for good, so wide-gamut screens show less.
- Needs a CMS dependency.
- A P3 JPEG with an embedded ICC already shows correctly in browsers and photo apps.

Verdict: skip both. If a lab job comes up, convert that one batch then.

### 4. Skipped files aren't reported

The walk sees only `catalog::EXTENSIONS`. These are left out without a trace:
- 2468 MOV and 339 MP4 files (videos and Live Photo motion clips).
- 15 TIFFs. TIFF isn't a supported format, so they're skipped even though they're photos.
- Documents.

Pros:
- No silent gaps in an archive.
- Cheap.

Cons:
- Noise from companion files. Leave out `.xmp`, `.aae` and our own temp files, and group the
  rest by extension instead of listing every file.

Verdict: do it. TIFF support is a separate question.

### 5. Size limit

Decided by the user: keep a limit, since huge photos shouldn't be stored. The default is
`MAX_SIDE = 4032`. It was picked over 4000 so 12 MP phone photos (4032×3024) aren't resampled
by 0.8 % (`research.md`).

### 6. Checksum manifest

Write a `SHA256SUMS` into the output folder.

Pros:
- Detects bit rot, and `sha256sum -c` checks backups.
- Almost free, since every byte is read anyway.

Cons:
- Goes stale as soon as a file in the output folder is edited, renamed or re-starred.
  Stars write into the JPEG, so starring in the app breaks it, and then it raises false
  alarms.
- Adds a non-photo file to a photo folder.
- Btrfs/ZFS scrubbing or the backup tool (restic, borg) already does this better.

Verdict: skip, or opt-in `--manifest` at most.

## Outcome (user decisions)

- 1, 3, 6: skipped.
- 2: only the star. Reporting lost EXIF gives the user nothing to act on, so a star that can't
  be written into the output (`rating::try_carry`, before the rename) fails that photo (exit 4).
- 4: done. `Report::unreadable` lists files that aren't photos pcr can read (not hidden, not
  `.xmp`/`.aae`). The CLI warns on stderr with counts by extension, exit stays 0, and the skill
  tells the agent to pass the warning on.
- 5: limit kept at 4032.
