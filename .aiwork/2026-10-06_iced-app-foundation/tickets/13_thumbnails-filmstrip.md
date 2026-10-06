---
status: ready
blocked_by: []
references:
  - "Spec: ../spec.md"
---

# 13 — Thumbnails, filmstrip and cache

**What to build:** A filmstrip of thumbnails in the app for quick navigation, backed by a thumbnail cache in core so revisiting a folder (including a network share) is fast. Thumbnail generation never blocks the main image decode.

## Acceptance criteria

- [ ] Clicking a thumbnail goes to that photo; the current photo is highlighted
- [ ] Cache location and invalidation (file size/mtime) are documented; a second visit to a folder skips regeneration
- [ ] Holding → still feels instant on a 50+ photo folder
- [ ] `just verify` green
