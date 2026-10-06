---
status: ready
blocked_by: []
references:
  - "Spec: ../spec.md"
---

# 02 — PC_PHOTOS opt-in large-set checks

**What to build:** Setting `PC_PHOTOS=<dir>` enables extra checks against a large local photo folder: every photo decodes without error, and the app browses the whole folder in a burst (held-arrow style) ending on the last photo. Without the variable these checks are skipped. Personal photos are never copied into the repo.

## Acceptance criteria

- [ ] Without `PC_PHOTOS` the checks are skipped and say so
- [ ] With `PC_PHOTOS=~/sandbox/photo-cruiser/test-photos` all photos decode, and a burst browse ends on the last one with its image shown
- [ ] Decode timings (min/median/max) are reported
- [ ] Documented in CLAUDE.md / the verify-app skill
