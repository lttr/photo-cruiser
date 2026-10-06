---
status: ready
blocked_by: [14, 15]
references:
  - "Spec: ../spec.md"
---

# 16 — Packaging and release

**What to build:** Friends on other OSes can install the app. A Linux package (AppImage or Flatpak), Windows and macOS builds with libheif bundled, and a release pipeline (cargo-dist or equivalent) that produces them from a tag. Expect to split this ticket when it starts.

## Acceptance criteria

- [ ] A tagged release produces installable artifacts for Linux, Windows and macOS
- [ ] Each artifact opens HEIC and AVIF on a clean machine
- [ ] Licence decided and included (open question in the spec; MIT OR Apache-2.0 is the current placeholder)
