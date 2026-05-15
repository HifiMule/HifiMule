---
title: 'Fix: MTP Deletes Silently Skipped by canonicalize() on Synthetic Path'
type: 'bugfix'
created: '2026-05-15'
status: 'done'
baseline_commit: 'e41892622bf668d9899baaf36d287f80313ecb86'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** After removing songs or playlists from the basket and syncing to an MTP device (e.g. Garmin smartwatch on macOS), the items are never deleted from the device. Every delete is silently skipped because the managed zone security check calls `Path::canonicalize()` on the synthetic `mtp://device-id/…` path, which does not exist on the local filesystem, so `canonicalize()` always returns `Err` and hits the `continue` branch.

**Approach:** Detect whether the device path is MTP (`starts_with("mtp://")`). For MTP, replace the `canonicalize()`-based check with a string-prefix check on the relative `local_path` against the managed subfolder. For MSC devices, keep the existing behavior unchanged.

## Boundaries & Constraints

**Always:**
- MSC (mass-storage) delete behavior is fully preserved — the canonicalize path is unchanged for non-MTP devices.
- MTP deletes must still be rejected if `local_path` does not start with the managed subfolder (defense-in-depth).
- The MTP detection must use the same `mtp://` prefix logic already used elsewhere in the codebase (`device/mod.rs`).
- Manifest must be updated after every successful delete (existing logic, must not regress).

**Ask First:**
- If the Garmin device also fails to delete objects via libmtp (e.g. permission error from `LIBMTP_Delete_Object`), that is a separate MTP-hardware limitation and is out of scope for this fix — confirm with Alexis before adding workarounds.

**Never:**
- Do not remove or weaken the managed zone check for MSC devices.
- Do not add a new abstraction layer or trait method for "is_mtp" — use the inline string check.
- Do not modify `execute_provider_sync` (non-Jellyfin path) — it already calls `device_io.delete_file()` directly without this check and works correctly.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| MTP song deleted from basket | `device_path = "mtp://abc"`, `local_path = "Music/Artist/Album/track.mp3"`, managed subfolder = "Music" | `device_io.delete_file("Music/Artist/Album/track.mp3")` called; manifest updated | Errors reported in `errors` vec |
| MTP path outside managed zone | `device_path = "mtp://abc"`, `local_path = "Other/track.mp3"`, managed subfolder = "Music" | Delete refused; `SyncFileError` pushed with "not in managed zone" message | Error surfaced to caller |
| MSC song deleted from basket | `device_path = "/Volumes/PLAYER"`, local file exists | Existing canonicalize path taken; no behavior change | Existing error handling |
| MSC file missing at delete time | `device_path = "/Volumes/PLAYER"`, file does not exist on disk | `canonicalize()` fails → silent skip (existing behavior, preserved) | Silent skip (existing) |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/sync.rs:528` — `execute_sync()`: the Jellyfin sync path; contains the broken delete loop at ~line 831–898
- `hifimule-daemon/src/sync.rs:569` — `managed_path` computation (device_path + managed subfolder) — already computed before the delete loop
- `hifimule-daemon/src/device/mod.rs:147` — `device_class_from_path()` — reference for `mtp://` prefix detection pattern used throughout the codebase

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/sync.rs` — In `execute_sync()`, before the `for delete_item in &delta.deletes` loop, compute `let is_mtp = device_path.to_string_lossy().starts_with("mtp://");` and compute `managed_subfolder` (strip `device_path` prefix from `managed_path`, normalize to forward slashes). Inside the loop, replace the `canonicalize()` block with a branch: if `is_mtp`, do the string-prefix check; else, keep the existing canonicalize logic.

**MTP string-prefix check (reference):**
```rust
let local_norm = delete_item.local_path.replace('\\', "/");
if !managed_subfolder.is_empty()
    && !local_norm.starts_with(&format!("{}/", managed_subfolder))
{
    errors.push(SyncFileError {
        jellyfin_id: delete_item.jellyfin_id.clone(),
        filename: delete_item.name.clone(),
        error_message: "File is not in managed zone - refusing to delete".to_string(),
    });
    continue;
}
```

**Acceptance Criteria:**
- Given an MTP device is selected and songs were removed from the basket, when sync runs, then `device_io.delete_file()` is called for each removed item and the manifest is updated to remove the corresponding entries.
- Given an MTP device, when a `SyncDeleteItem.local_path` does not begin with the managed subfolder, then the delete is refused and a `SyncFileError` is pushed (managed zone check still enforced).
- Given an MSC device with a file physically absent from disk, when sync runs, then the existing behavior is preserved (silent skip via canonicalize failure, no regression).
- Given a successful delete run, when sync completes, then no deleted items remain in the manifest.

## Spec Change Log

## Design Notes

The `canonicalize()` approach was appropriate for MSC devices (where files exist on the local filesystem) to prevent directory traversal attacks. MTP files never exist on the local filesystem — they are accessed via libmtp object IDs — so canonicalize is both unnecessary and fatal to the operation. The string-prefix check is safe for MTP because `local_path` values are written by the sync engine itself from manifest entries that were already validated at write time, and the MTP DeviceIO backend resolves paths internally via `path_to_object_id_raw`.

## Suggested Review Order

- Entry point: MTP-aware managed zone check replacing the broken canonicalize path.
  [`sync.rs:831`](../../hifimule-daemon/src/sync.rs#L831)

- `is_mtp` detection — exact-case `"mtp://"` prefix, consistent with `device_class_from_path`.
  [`sync.rs:838`](../../hifimule-daemon/src/sync.rs#L838)

- `managed_subfolder_for_delete` as `Option<String>` — None = fail-safe; Some("") = root managed; Some("X") = prefix guard.
  [`sync.rs:843`](../../hifimule-daemon/src/sync.rs#L843)

- MTP branch match: None → reject, non-empty → prefix check, empty → allow.
  [`sync.rs:856`](../../hifimule-daemon/src/sync.rs#L856)

- MSC branch: canonicalize path preserved exactly — no regression for mass-storage devices.
  [`sync.rs:882`](../../hifimule-daemon/src/sync.rs#L882)

## Verification

**Commands:**
- `cd hifimule-daemon && cargo check` — expected: no compilation errors
- `cd hifimule-daemon && cargo test` — expected: all existing tests pass

**Manual checks (if no CLI):**
- After fix: remove a song from the basket and sync to the Garmin smartwatch; confirm the track is removed from the device and absent from the manifest (`cat ~/.hifimule/<device-id>/.hifimule.json | jq '.syncedItems | length'` should decrease).
