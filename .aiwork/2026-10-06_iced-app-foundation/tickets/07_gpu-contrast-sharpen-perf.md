---
status: ready
blocked_by: [04, 06]
references:
  - "Spec: ../spec.md"
---

# 07 — GPU contrast and sharpen, 16 ms target

**What to build:** Contrast and sharpen run on the GPU preview path too, with parity tests. Per-slider-update latency at screen resolution is measured and meets the spec target (under 16 ms) on this machine, or the gap is documented.

## Acceptance criteria

- [ ] Parity tests for contrast and sharpen pass
- [ ] A reproducible timing measurement exists and its result is recorded
- [ ] `just verify` green
