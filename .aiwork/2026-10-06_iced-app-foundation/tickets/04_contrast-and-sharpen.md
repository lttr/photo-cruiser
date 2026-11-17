---
status: abandoned
superseded_by:
  - ../../2026-10-12_adjust-tools/tickets/10_contrast.md
  - ../../2026-10-12_adjust-tools/tickets/08_sharpen.md
blocked_by: [03]
references:
  - "Spec: ../spec.md"
---

Superseded by the one-slider tools Contrast (adjust-tools 10) and Sharpen (adjust-tools 08).

# 04 — Contrast and sharpen

**What to build:** Two more adjustments following the exposure pattern: Contrast (in linear space around a mid-grey pivot) and Sharpen (unsharp mask with amount and radius). Available in the edit stack, `pcr`, the side panel and scripts.

## Acceptance criteria

- [ ] Both ops in the edit stack JSON, with golden tests
- [ ] Sliders in the side panel and scriptable actions for both
- [ ] Op order in the stack is respected by render
- [ ] `just verify` green
