---
status: ready
blocked_by: [03]
references:
  - "Spec: ../spec.md"
---

# 08 — Local contrast (CPU)

**What to build:** A Darktable-like local contrast adjustment (local Laplacian, or a guided-filter approximation, decided in the ticket) as a core op, with a slider, a scriptable action and `pcr` support. CPU only; it defines correctness for the later GPU version.

## Acceptance criteria

- [ ] Visibly improves a low-contrast fixture without halos at default strength (judged on a shot, plus golden tests)
- [ ] Full-resolution export time on a 12 MP photo is measured and recorded
- [ ] `just verify` green
