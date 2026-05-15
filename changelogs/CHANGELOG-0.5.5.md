# HifiMule v0.5.5

> Covers all changes introduced after v0.5.4 (2026-05-13 – 2026-05-15).

---

## Bug Fixes

### Deletes silently skipped on MTP devices (Garmin, Android)

Removing songs or playlists from the basket and syncing to an MTP device had no effect — the items remained on the device. Every delete was silently skipped because the managed-zone security check called `Path::canonicalize()` on the synthetic `mtp://device-id/…` path, which does not exist on the local filesystem, so `canonicalize()` always returned `Err` and hit the `continue` branch.

The fix detects MTP device paths (`starts_with("mtp://")`) and replaces the `canonicalize()`-based check with a string-prefix check against the managed subfolder. Mass-storage (MSC) delete behavior is fully preserved. The managed-zone guard remains in place for MTP — a `local_path` that does not start with the managed subfolder is still refused.

### Android phones in charge-only mode show a spurious Initialize button

When an Android phone is connected via USB in charge-only mode the MTP interface is still advertised by the OS, but `LIBMTP_Get_Storage` fails because no storage is accessible. The app detected the device, emitted `DeviceEvent::Unrecognized`, and presented the Initialize button — which then failed immediately.

`LIBMTP_Get_Storage` failure is now treated as a hard error in `LibmtpHandle::open()`: the device handle is released and `Err` is returned. The observer skips the device silently (logs and moves on). The 2-second poll loop retries the check, so once the user enables MTP/file-transfer mode the device appears normally.

---

## Improvements

### Garmin sub-folder discovery: live enumeration replaces manifest cache

v0.5.4 worked around Garmin's hidden-sub-folder limitation by persisting folder object IDs in the `.hifimule.json` manifest (`folder_ids`). Testing confirmed that `LIBMTP_Get_Folder_List` (the same function used internally by the `mtp-folders` utility) does enumerate all sub-folders on Garmin correctly.

Folder hints are now built from a live BFS over the device at sync time rather than being loaded from the manifest. This eliminates the stale-hint failure mode (hints going out of sync when folders are deleted externally) and removes `folder_ids` from the manifest on the next write (`skip_serializing_if = "is_empty"` drops the field automatically). Old manifests with `folder_ids` are still parsed without error.

### Folder hint BFS is now lazy (first-miss triggered)

The folder-hint BFS that resolves hidden Garmin sub-folders was previously triggered eagerly at device open time, scanning all folders on every connection. This is expensive for large-storage devices (Android phones with thousands of artist/album directories).

The BFS is now triggered lazily: it runs on the first path-not-found miss in `ensure_path_raw`, scoped to the subtree rooted at the last successfully resolved folder. A smartphone opening with a flat music directory incurs zero extra round-trips; a Garmin with nested `Music/Artist/Album` folders triggers the scan only when the second component is not found via normal enumeration. An `AtomicBool` (`hints_primed`) ensures the BFS runs at most once per sync.

---

## Infrastructure

### Avoid macOS Keychain dialogs during `cargo test`

The test suite previously triggered macOS Keychain access dialogs when running credential-related tests, blocking CI on interactive machines. Tests now use an in-process secrets store that bypasses the system keyring entirely.
