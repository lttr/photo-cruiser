---
status: ready
blocked_by: [03]
references:
  - "Spec: ../spec.md"
---

# 06 — GPU preview with exposure

**What to build:** A `gpu` crate (wgpu) mirrors the core exposure op and feeds a shader widget, so the app preview of edits runs on the GPU. The CPU pipeline stays the reference: a parity test renders the same photo and edits on both and compares within tolerance. The final export still uses the CPU path.

## Acceptance criteria

- [ ] `gpu` depends on `core`, never the other way round
- [ ] Parity test GPU vs CPU for exposure passes headlessly
- [ ] App preview uses the GPU path; a script-mode shot matches the CPU render of the same view
- [ ] `just verify` green
