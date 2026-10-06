---
status: ready
blocked_by: [03]
references:
  - "Spec: ../spec.md"
---

# 05 — Sidecar edit files, crop applied on export

**What to build:** Edits persist per photo in a sidecar file. The app loads a photo's edits when it is shown and saves them when they change; its crop becomes part of the edit stack. `pcr render`/`export` use the sidecar by default (explicit `--edits` wins), so exporting a photo applies its crop and adjustments.

⚠ Open question from the spec: sidecar location (next to the photo vs a central store for read-only and SMB sources). Decide before starting; the spec's default is `<photo>.pcedit.json` next to the photo.

## Acceptance criteria

- [ ] Browsing away and back (and restarting the app) restores a photo's crop and adjustments
- [ ] Read-only folders don't crash the app; the failure is shown and recorded in state
- [ ] `pcr export <photo> -o out.jpg` applies the sidecar edits
- [ ] Sidecar files are ignored by folder listing
- [ ] `just verify` green
