---
status: ready
blocked_by: []
references:
  - "Spec: ../spec.md"
---

# 14 — Desktop integration and live theme reload

**What to build:** The app installs as a proper desktop app: a `.desktop` file and icon matching the application id, which is verified to work (window grouping/icon in the dock). The COSMIC theme reloads live when the user changes it, with the dark fallback elsewhere.

## Acceptance criteria

- [ ] A documented install step puts the `.desktop` file and icon in place; the running window is associated with it
- [ ] Switching COSMIC dark/light or accent updates the open app without restart
- [ ] `just verify` green
