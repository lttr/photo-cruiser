---
status: ready
blocked_by: []
references:
  - "Spec: ../spec.md"
---

# 15 — Linux CI

**What to build:** CI runs `just check` on every push on Linux, with libheif installed and the pkg-config caveat handled. Window-based script-mode checks run if a headless display is practical, otherwise they're skipped with a note.

## Acceptance criteria

- [ ] CI is green on the current main
- [ ] A failing test or clippy warning fails CI
- [ ] Build cache keeps typical runs reasonably fast
