---
title: 'Fix sync failure when MTP device omits folders from file listing'
type: 'bugfix'
created: '2026-05-12'
status: 'done'
route: 'one-shot'
---

## Intent

**Problem:** On some MTP devices (observed on a Garmin smartwatch, VID=091e/PID=50a4), `LIBMTP_Get_Files_And_Folders` only returns file objects — not folder objects — when listing directory contents. `ensure_path_raw` therefore fails to find an already-existing artist directory such as "Gino Vannelli" in the root listing, attempts to create it via `LIBMTP_Create_Folder`, gets back 0 (failure, because the folder already exists), and propagates an error: *"Failed to write file: libmtp: failed to create directory 'Gino Vannelli'"*. Every track destined for that artist fails for the same reason.

Diagnostic investigation revealed that this device also does NOT support the PTP "all objects" query: `LIBMTP_Get_Files_And_Folders(dev, any_storage, 0xFFFFFFFF)` returns only 5 root-level items, and `LIBMTP_Get_Folder_List` returns only 4 root-level association objects. There is no enumeration API that exposes sub-folders on this device.

**Approach (three layers):**

1. **`find_folder_in_list` (committed in a prior fix):** After `LIBMTP_Create_Folder` returns 0, fall back to `LIBMTP_Get_Folder_List`. Works for devices where the folder list returns sub-folders but `Get_Files_And_Folders` misses them. Does not work for Garmin.

2. **`find_folder_in_all_objects` (committed in a prior fix):** If `find_folder_in_list` fails, fall back to a flat all-objects scan via `LIBMTP_Get_Files_And_Folders(dev, storage, ROOT)`. Works for devices that support the all-objects query. Does not work for Garmin.

3. **Folder ID cache (this change — the Garmin fix):** Since no enumeration API can discover existing sub-folders on Garmin, the only reliable approach is to persist folder object IDs at creation time and reuse them on subsequent syncs. When `ensure_path_raw` successfully creates a new folder (`LIBMTP_Create_Folder` returns non-zero), the ID is stored in a per-sync `discovered` map, which is merged back into `LibmtpHandle::folder_hints` after the write. At sync end, all hints are drained from the handle and written to `DeviceManifest::folder_ids` in `.hifimule.json`. On the next sync, the manifest's `folder_ids` are loaded into the handle via `load_folder_hints`, and `ensure_path_raw` checks the hints map before attempting to create a path component that enumeration fails to find.

**Bootstrap note:** The first sync after deploying this fix will still fail for folders that existed before the fix (no cached IDs in the old manifest). Users experiencing this must clear `.hifimule.json` from the device to trigger a full re-sync, which will then cache all created folder IDs.

## Suggested Review Order

1. [mtp.rs:1773-1881](../../../hifimule-daemon/src/device/mtp.rs) — `ensure_path_raw`: new `hints`/`discovered` params, `acc_path` tracking, hint check before create, `discovered.insert` on success
2. [mtp.rs:1935-1975](../../../hifimule-daemon/src/device/mtp.rs) — `write_file`: hints snapshot, discovered merge back into `self.folder_hints`
3. [mtp.rs:1903-1915](../../../hifimule-daemon/src/device/mtp.rs) — `load_folder_hints` / `drain_folder_hints` on `LibmtpHandle` (replace vs. take semantics)
4. [device_io.rs:259-406](../../../hifimule-daemon/src/device_io.rs) — trait extension on `MtpHandle` and `DeviceIO`; `MtpBackend` delegation (note: intentionally no `operation_lock` for in-memory-only methods)
5. [sync.rs:568-600](../../../hifimule-daemon/src/sync.rs) — `execute_sync`: hint load from manifest, drain + manifest update before `end_sync_job`
6. [device/mod.rs:89-92](../../../hifimule-daemon/src/device/mod.rs) — `folder_ids` field on `DeviceManifest` (backward compat via `serde(default)`)

## Spec Change Log

- 2026-05-12: Extended with folder ID cache approach after diagnostic confirmed Garmin does not support any enumeration-based recovery.

## Design Notes

The hint cache is intentionally simple: `HashMap<String, u32>` mapping device-relative path (e.g. `"Music/Gino Vannelli"`) to LIBMTP folder object ID. `storage_id` is not cached — it propagates from the parent path component, which is always enumerable on Garmin (root items are visible). The `drain_folder_hints` call returns the full hints map (both pre-loaded and newly discovered entries); the sync code uses `extend` which is idempotent for pre-loaded keys. Factory resets clear `.hifimule.json` from the device, which naturally invalidates all cached IDs on the next fresh init.
