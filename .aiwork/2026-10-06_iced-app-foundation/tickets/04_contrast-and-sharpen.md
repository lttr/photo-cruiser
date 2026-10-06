---
status: ready
blocked_by: [03]
references:
  - "Spec: ../spec.md"
---

# 04 — Contrast and sharpen

**What to build:** Two more adjustments following the exposure pattern: Contrast (in linear space around a mid-grey pivot) and Sharpen (unsharp mask with amount and radius). Available in the edit stack, `pcr`, the side panel and scripts.

## Acceptance criteria

- [ ] Both ops in the edit stack JSON, with golden tests
- [ ] Sliders in the side panel and scriptable actions for both
- [ ] Op order in the stack is respected by render
- [ ] `just verify` green
