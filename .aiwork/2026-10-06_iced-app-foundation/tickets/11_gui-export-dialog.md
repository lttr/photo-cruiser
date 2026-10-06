---
status: ready
blocked_by: [10]
references:
  - "Spec: ../spec.md"
---

# 11 — Export from the GUI

**What to build:** An export dialog in the app: choose format, quality, size and destination, and export the current photo (and optionally the whole folder) with its edits. Uses the same core export path as `pcr`.

## Acceptance criteria

- [ ] Export is an `Action`, so scripts and tests can drive it
- [ ] Exported file matches `pcr export` output for the same photo and settings
- [ ] Progress and errors are visible in the app
- [ ] `just verify` green
