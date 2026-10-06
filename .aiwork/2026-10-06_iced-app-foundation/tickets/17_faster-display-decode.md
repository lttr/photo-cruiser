---
status: ready
blocked_by: []
references:
  - "Spec: ../spec.md"
---

# 17 — Faster display decode

**What to build:** Cut the cold-switch time (the photo isn't preloaded and has no embedded preview) by decoding and uploading less. Measured on the release build: 12 MP JPEG decode 50–80 ms, the HEIC 254 ms, and `image::imageops::thumbnail` to 2048 px 65–145 ms. `PC_TRACE=1` reports switch and decode times.

## Ideas, measure each

- Cap the displayed size at the monitor size (× scale factor) instead of the fixed 4096: less downscaling, upload and memory, and room for a wider preload window.
- libheif: `HeifContext::set_max_decoding_threads` (libheif-rs 3).
- JPEG: decode at a reduced size directly (DCT scaling, 1/2–1/8). It isn't known yet whether zune-jpeg supports it; libjpeg-turbo does, if it's available on the system.
- A SIMD resize (e.g. `fast_image_resize`) instead of `imageops::thumbnail`.
- Small previews generated from full decodes and kept for the whole folder, so revisits and held-key browsing always have something better than the previous photo.

## Acceptance criteria

- [ ] Before/after `PC_TRACE` numbers on `~/Documents/test-media` and `PC_PHOTOS`
- [ ] Cold switch to a 12 MP JPEG < 60 ms in the release build
- [ ] Crop coordinates are still in source pixels
