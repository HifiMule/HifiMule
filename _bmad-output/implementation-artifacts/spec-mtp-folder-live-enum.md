---
title: 'Replace MTP folder-ID cache with live LIBMTP_Get_Folder_List enumeration'
type: 'refactor'
created: '2026-05-13'
status: 'done'
baseline_commit: '5025e0efd3d417c010596746792bd1e674b73c1d'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Folder object IDs for MTP devices like Garmin smartwatches are persisted in the `.hifimule.json` manifest (`folder_ids`) because `LIBMTP_Get_Folder_List` was assumed to not work on those devices. Testing confirmed that `mtp-folders` (which uses `LIBMTP_Get_Folder_List` internally) does retrieve all sub-folders correctly on Garmin devices.

**Approach:** At sync start, call `LIBMTP_Get_Folder_List` once to build the full path→folder-ID map and prime `folder_hints` in-memory, replacing the manifest-cached `folder_ids` load. Stop draining and persisting `folder_ids` back to the manifest after sync. Keep the `folder_ids` field in `DeviceManifest` for backward-compatible JSON parsing (it will naturally disappear from manifests on next write due to `skip_serializing_if = "is_empty"`).

## Boundaries & Constraints

**Always:**
- `prime_folder_hints` replaces (not merges into) the existing hints so each sync starts from a fresh device view.
- Newly created folders during sync must still be merged into `folder_hints` (the existing `discovered` → `folder_hints.extend` path in `ensure_path_raw` stays untouched).
- If `LIBMTP_Get_Folder_List` returns NULL (device doesn't support it), log a warning and continue with empty hints — same graceful fallback as before.
- `prime_folder_hints` in `MtpBackend::DeviceIO` must acquire `operation_lock` (it calls FFI, unlike `load/drain_folder_hints`).
- No change to the WPD backend (Windows path) — `prime_folder_hints` is a no-op at the `MtpHandle` trait level.

**Ask First:** None.

**Never:**
- Do not remove `folder_ids` from the `DeviceManifest` struct — old manifests must still parse.
- Do not call `device_io.prime_folder_hints()` while `operation_lock` is already held by the caller.
- Do not change `ensure_path_raw` or its `discovered`/`hints` parameters.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Garmin with sub-folders | `LIBMTP_Get_Folder_List` returns full tree | `folder_hints` populated with all path→id entries before first `write_file` | N/A |
| `LIBMTP_Get_Folder_List` returns NULL | Device returns no folder list | Empty `folder_hints`; sync proceeds via normal enumeration fallbacks | Log warning, continue |
| Non-MTP backend (MSC, WPD mock) | `prime_folder_hints()` called on DeviceIO | No-op, no panic | N/A |
| New folder created during sync | `ensure_path_raw` creates directory | New id added to `folder_hints` via existing `discovered` merge | N/A |
| Manifest has stale `folder_ids` | Old manifest read on startup | Field parsed but never used; disappears from manifest on next write | N/A |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/device/mtp.rs` -- `LibmtpHandle`: `prime_folder_hints()` BFS via `LIBMTP_Get_Files_And_Folders` per-parent (called from `open()`); no `build_folder_map_raw`, no `LIBMTP_Get_Folder_List_For_Storage`, no `LIBMTP_DeviceStorage_t`
- `hifimule-daemon/src/device_io.rs` -- `prime_folder_hints` removed from all traits and `MtpBackend` (priming now happens at `open()` time, not sync time)
- `hifimule-daemon/src/sync.rs` -- `execute_sync` and `execute_provider_sync`: manifest `folder_ids` load removed; no `prime_folder_hints` call (already primed at open)
- `hifimule-daemon/src/device/mod.rs` -- `DeviceManifest.folder_ids`: kept for backward-compat JSON parsing

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/device/mtp.rs` -- Add `unsafe fn build_folder_map_raw(node: *mut LIBMTP_folder_t, parent_path: &str, out: &mut HashMap<String, u32>)` as a private associated fn of `LibmtpHandle`; iterates siblings, recurses into children, builds full relative paths. Add `fn prime_folder_hints(&self)` that acquires the device mutex, calls `LIBMTP_Get_Folder_List`, runs `build_folder_map_raw` starting at `""`, destroys the folder tree, then replaces `folder_hints` with the result. On NULL result: log `[libmtp] prime_folder_hints: LIBMTP_Get_Folder_List returned NULL — no folder hints available` and return early. On success: log `[libmtp] prime_folder_hints: found N folders` where N is the map size. -- replaces manifest-based hint loading with fresh device enumeration; success log reveals device capability at a glance
- [x] `hifimule-daemon/src/device_io.rs` -- Add `fn prime_folder_hints(&self) {}` to `MtpHandle` trait with default no-op; add `async fn prime_folder_hints(&self) {}` to `DeviceIO` trait with default no-op; in `MtpBackend`'s `DeviceIO` impl add: `async fn prime_folder_hints(&self) { let _guard = self.operation_lock.lock().await; let handle = Arc::clone(&self.handle); tokio::task::spawn_blocking(move || handle.prime_folder_hints()).await.ok(); }` -- wires new method through the async boundary
- [x] `hifimule-daemon/src/sync.rs` -- In `execute_sync`: replace the `folder_ids` snapshot + `load_folder_hints` block with a single `device_io.prime_folder_hints().await` call (keep the managed_path derivation that reads `managed_paths.first()`); remove the post-sync `drain_folder_hints` + `update_manifest(folder_ids.extend)` block. Apply the same two changes to `execute_provider_sync`. -- eliminates manifest persistence of folder IDs

**Acceptance Criteria:**
- Given a libmtp device with sub-folders, when a sync starts, then `LIBMTP_Get_Folder_List` is called once, its results populate `folder_hints`, and `[libmtp] prime_folder_hints: found N folders` appears in the daemon log.
- Given `LIBMTP_Get_Folder_List` returns NULL, when a sync starts, then `[libmtp] prime_folder_hints: LIBMTP_Get_Folder_List returned NULL — no folder hints available` is logged and sync proceeds without crashing.
- Given an existing manifest with `folder_ids`, when the manifest is parsed then written back, then the `folder_ids` key is absent from the output (because the map is never extended and `skip_serializing_if = "is_empty"` is already set).
- Given the WPD or MSC backend, when `prime_folder_hints` is called, then it completes as a no-op with no error.
- `cargo check` passes with no new warnings.

## Spec Change Log

## Design Notes

`build_folder_map_raw` visits the sibling list at each level (while loop) and recurses into each node's `child` pointer, building paths by appending the node name to `parent_path`. Root nodes (from `LIBMTP_Get_Folder_List`) have `parent_id = 0`; the function does not use `parent_id` — it infers the full path from the traversal position:

```rust
// parent_path="" for root call; path = "Music/Artist" at depth 2
let path = if parent_path.is_empty() { name } else { format!("{}/{}", parent_path, name) };
out.insert(path.clone(), (*cur).folder_id);
Self::build_folder_map_raw((*cur).child, &path, out);
cur = (*cur).sibling;
```

`prime_folder_hints` releases the device mutex before acquiring `folder_hints` to match the lock ordering in `write_file` and prevent potential deadlocks.

## Verification

**Commands:**
- `cargo check -p hifimule-daemon` -- expected: zero errors, zero new warnings
- `cargo test -p hifimule-daemon` -- expected: all tests pass

## Suggested Review Order

**Core design — entry point**

- `prime_folder_hints`: acquires device mutex, calls `LIBMTP_Get_Folder_List`, builds map, logs result
  [`mtp.rs:1953`](../../hifimule-daemon/src/device/mtp.rs#L1953)

- `build_folder_map_raw`: DFS traversal building `path → folder_id`; skips null/empty names
  [`mtp.rs:1910`](../../hifimule-daemon/src/device/mtp.rs#L1910)

**Trait wiring**

- `MtpBackend::prime_folder_hints`: acquires `operation_lock` before FFI (unlike `load/drain`)
  [`device_io.rs:412`](../../hifimule-daemon/src/device_io.rs#L412)

- `MtpHandle` trait default no-op (WPD / mock stay unaffected)
  [`device_io.rs:295`](../../hifimule-daemon/src/device_io.rs#L295)

- `DeviceIO` trait default no-op (MSC stays unaffected)
  [`device_io.rs:47`](../../hifimule-daemon/src/device_io.rs#L47)

**Sync integration**

- `execute_sync`: `prime_folder_hints` replaces manifest `folder_ids` load; drain+persist removed
  [`sync.rs:570`](../../hifimule-daemon/src/sync.rs#L570)

- `execute_provider_sync`: same change mirrored in the provider-sync path
  [`sync.rs:1042`](../../hifimule-daemon/src/sync.rs#L1042)

**Supporting**

- `LibmtpHandle.folder_hints` field: comment updated to reflect live-enumeration approach
  [`mtp.rs:1539`](../../hifimule-daemon/src/device/mtp.rs#L1539)

- `DeviceManifest` gains `Default` derive; `folder_ids` field kept for backward compat
  [`mod.rs:60`](../../hifimule-daemon/src/device/mod.rs#L60)
