---
status: ready
blocked_by: []
references:
  - "Spec: ../spec.md"
---

# 12 — Open smb:// locations via GVfs

**What to build:** Decoding reads bytes through the `Source` trait (prefactor), and `smb://` locations open in both `pcr` and the app by resolving them to their GVfs mount (mounting if needed). A native SMB client stays out of scope.

## Acceptance criteria

- [ ] Decode goes through `Source`; all existing tests still pass
- [ ] `pcr list smb://…` and `photo-cruiser smb://…` work on a reachable share; an unreachable share gives a clear error
- [ ] Browsing a share stays responsive (one decode in flight still holds)
- [ ] `just verify` green
