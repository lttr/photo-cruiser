---
status: ready
blocked_by: [01]
references:
  - "Spec: ../spec.md"
---

# 03 — Exposure, end to end

**What to build:** The first tonal adjustment. Core gains a linear f32 pipeline (sRGB decode → linear → ops → sRGB) and an Exposure op (EV). It works everywhere: `pcr render --edits` with an exposure op, a side panel in the app with an exposure slider, and a scriptable action to set it. This ticket sets the pattern (op type, panel row, action, tests) that later adjustments copy.

## Acceptance criteria

- [ ] Exposure is part of the versioned edit stack JSON; older files without it still load
- [ ] Golden tests cover a few EV values; 0 EV is a no-op within tolerance
- [ ] App side panel with an exposure slider; the preview updates and stays responsive on a full-size photo (CPU path; GPU comes later)
- [ ] A scriptable action sets exposure; a script-mode shot shows the change and the state JSON reports it
- [ ] `just verify` green
