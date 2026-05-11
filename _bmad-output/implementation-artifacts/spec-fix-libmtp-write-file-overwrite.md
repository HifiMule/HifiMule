---
title: 'Fix libmtp write_file overwrite on macOS'
type: 'bugfix'
created: '2026-05-11'
status: 'done'
route: 'one-shot'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** On macOS, syncing a previously-initialized MTP device fails with "Failed to mark manifest dirty, aborting sync: libmtp write_file failed: rc=-1". `LIBMTP_Send_File_From_File` only creates new MTP objects — it cannot overwrite an existing one — so the second write of `.hifimule.json` (the dirty-mark at sync start) fails because the file was already created during device initialization.

**Approach:** In `LibmtpHandle::write_file`, probe for an existing object at the target path before calling `LIBMTP_Send_File_From_File`. If one is found, delete it first (delete-then-create). Log a warning if the pre-delete fails but continue — the send will produce the definitive error if the path is still blocked.

## Suggested Review Order

1. [mtp.rs:1611–1623 — pre-delete gate](../../hifimule-daemon/src/device/mtp.rs#L1611-L1623) — does the existence check correctly fall through when the file isn't there, and log on delete failure?
2. [mtp.rs:1603–1653 — full `write_file`](../../hifimule-daemon/src/device/mtp.rs#L1603-L1653) — verify guard is held across the delete and the send, and that the temp file is always cleaned up.
3. [rpc.rs:2088–2103 — dirty-mark caller](../../hifimule-daemon/src/rpc.rs#L2088-L2103) — confirm the error path that surfaced the bug is now reachable only on a genuine send failure.

</frozen-after-approval>
