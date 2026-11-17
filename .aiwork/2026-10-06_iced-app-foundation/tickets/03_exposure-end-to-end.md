---
status: done
verified: [behaviour]
blocked_by: [01]
references:
  - "Spec: ../spec.md"
---

Delivered as the first one-slider tool by ../../2026-10-12_adjust-tools/tickets/01_adjust-tools-exposure.md (3cc2d2f).

# 03 — Exposure, end to end

**What to build:** The first tonal adjustment. Core gains a linear f32 pipeline (sRGB decode → linear → ops → sRGB) and an Exposure op (EV). It works everywhere: `pcr render --edits` with an exposure op, a side panel in the app with an exposure slider, and a scriptable action to set it. This ticket sets the pattern (op type, panel row, action, tests) that later adjustments copy.

## Acceptance criteria

- [x] Exposure is part of the versioned edit stack JSON; older files without it still load
- [x] Golden tests cover a few EV values; 0 EV is a no-op within tolerance
- [x] App side panel with an exposure slider; the preview updates and stays responsive on a full-size photo (CPU path; GPU comes later)
- [x] A scriptable action sets exposure; a script-mode shot shows the change and the state JSON reports it
- [x] `just verify` green

## Verification (behaviour)

- `just verify` passed, including its exposure step on `fixtures/img2.jpg`: state `edits` = exposure 0.8, slider 0.8, opening −1.51 (auto), preview on, no errors. Leaving the photo and coming back saved it (`dirty` false, `pcr edits` reports `saved: [exposure 0.8]`, backup in `.photo-cruiser/originals/`).
- Headless real window on a full-size 20 MP real photo (3648×5472, copied to `/tmp/photo-cruiser/exp-pass`): open Exposure (opens at +0.09 auto), drag to 0.4, set 0.8. State: exposure 0.8, preview on, no errors; the footer shows "+0.80 EV". The photo's mean brightness in the shot goes from 99 to 126. The whole scripted run, startup and decode included, took 1.5 s, the same as just showing the photo (2.1 s): the preview renders at screen size, so the slider stays responsive on a full-size photo.

