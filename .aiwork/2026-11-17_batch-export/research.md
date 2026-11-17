# Archive JPEG preset: benchmark

12 real photos from `~/Photos` (not committed): 4 Nikon D3200 JPEGs (4512×3000, 4.0–4.5 MB,
4:2:2), 4 realme 6s JPEGs (12–16 MP, 3.1–6.1 MB, 4:2:0), 4 iPhone SE HEICs (4032×3024, 2.2–3.3 MB).
Each was decoded and made upright, then encoded. The score is SSIMULACRA2 against that decoded
reference, so it measures only the loss the export adds. Harness: `/tmp/photo-cruiser/bench`.
Times are per photo with 12 photos and their `ssimulacra2` runs in parallel, so they are high
in absolute terms. Only the ratios between rows mean anything.

| encoder | bits/px | % of source | SSIMULACRA2 mean / min | ms |
| --- | --- | --- | --- | --- |
| jpeg-encoder q92 4:2:0 | 1.89 | 85 | 85.3 / 82.4 | 290 |
| jpeg-encoder q90 4:4:4 | 1.87 | 84 | 86.7 / 83.2 | 430 |
| mozjpeg-rs baseline q92 4:2:0 | 1.48 | 66 | 85.2 / 81.2 | 2100 |
| **mozjpeg-rs progressive q92 4:2:0** | **1.41** | **63** | **85.2 / 81.2** | 6300 |
| mozjpeg-rs progressive q90 4:4:4 | 1.46 | 65 | 85.4 / 80.7 | 7800 |
| mozjpeg-rs progressive q94 4:2:0 | 1.74 | 78 | 87.4 / 83.5 | 5800 |
| mozjpeg-rs progressive q94 4:4:4 | 2.03 | 91 | 88.2 / 84.3 | 10400 |

- At the same score, mozjpeg-rs (trellis quantisation, its own tables) is about 25 % smaller than
  `jpeg-encoder` with 4:2:0, and about 10 % smaller with 4:4:4.
- Progressive with optimised scans saves another 4 % over baseline, at 3–4× the time.
- 4:2:0 and 4:4:4 lie on about the same size/score curve. 4:2:0 matches what the cameras write.
- Every output decodes with our own decoder (`image`/zune-jpeg), progressive ones included.

Chosen: **`mozjpeg-rs` 0.9.2 (crates.io, BSD-3-Clause, pure Rust), `Encoder::max_compression` (progressive, optimised scans, trellis, deringing), q92, 4:2:0** (later: subsampling follows the original). Camera JPEGs end up
at 36–62 % of their size (SSIMULACRA2 85.5–89.3). HEICs become JPEGs at 79–94 % of the HEIC's size
(81–83). q94 would keep more (87.4 mean), but camera JPEGs would then shrink by only about 25 %,
close to the 20 % threshold below which an unedited JPEG is copied.

Not considered: `jpegli-rs` is AGPL-3.0, and deprecated in favour of `zenjpeg`. The C `mozjpeg`
crate gives the same output as `mozjpeg-rs` (per its README) and needs cmake and nasm.

Real run (`pcr export-tree`, release build, 8 workers): 22 JPEGs and 10 HEICs, 104 MB → 61 MB in
13.7 s. All encoded; none copied, since no JPEG would have saved less than 20 %.

## Follow-up decisions (user)

- **Subsampling follows the original**, not a fixed 4:2:0.
- **Maximum size 4032 px.** On the 12 photos, with the longer side limited: no limit 24.1 MB,
  4000 px 22.4 MB (93 %), 3000 px 13.7 MB (57 %). 3000 would throw away detail from every
  current photo. 4032 was picked over 4000 so 12 MP phone photos aren't resampled by 0.8 %.
- **Progressive output decodes correctly with our decoder (zune-jpeg).** Our decoder and
  libjpeg (ImageMagick) agree at 54–64 dB PSNR on all 12 exports. That is rounding; a misread
  would be under 30 dB.
- With both: 12 photos, 45.1 MB → 26.5 MB in 6.0 s. A 100 % crop of a scaled Nikon photo looks
  the same as the original scaled by ImageMagick.

## Quality 92 vs 95 (user asked; 92 kept)

The same 12 photos at the export size (4032 px, original subsampling). SSIMULACRA2 against
the uncompressed image, mean (worst):

| Viewing | q92 | q95 |
| --- | --- | --- |
| Full screen on 4K (both scaled to fit 3840×2160, Lanczos) | 89.8 (88.2) | 91.2 (89.8) |
| 1:1, every pixel (worst case for an A4 print at about 345 ppi) | 85.3 (81.2) | 87.9 (84.2) |
| Size of the 12 | 26.1 MB | 34.3 MB (+31 %) |

The scale, from memory of the tool's README (not re-checked): 90 can't be told from the
original side by side at 1:1; 80 isn't noticed at normal viewing distance. Lowest scores come
from photos that were iPhone HEICs, which were compressed once before. q95 would pay off only
for zooming to 100 % or heavy crops from the archive. `--quality 95` is there for that.
