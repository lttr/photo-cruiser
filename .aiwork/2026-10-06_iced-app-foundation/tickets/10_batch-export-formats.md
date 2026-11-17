---
status: abandoned
superseded_by: ../../2026-11-17_batch-export/spec.md
blocked_by: [05]
references:
  - "Spec: ../spec.md"
---

Superseded by `pcr export-tree` (../../2026-11-17_batch-export/spec.md): folder export, JPEG only; other output formats are out of scope there.

# 10 — Batch export to JPEG, PNG, WebP, AVIF

**What to build:** `pcr export` handles a whole folder (or a list of files) and writes JPEG, PNG, WebP or AVIF, applying each photo's sidecar edits. HEIC output stays out of scope.

## Acceptance criteria

- [x] Folder export writes one output per photo, skips broken files with a per-file error, and exits non-zero if any failed
- [ ] WebP and AVIF outputs decode back correctly (tests)
- [x] Quality and max-side options apply to every format where they make sense
- [ ] `just verify` green
