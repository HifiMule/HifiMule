---
title: 'Fix MTP Device Init: Show Initialize Button for Unrecognized Devices'
type: 'bugfix'
created: '2026-05-02'
status: 'done'
baseline_commit: '688c063'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Plugging in an unrecognized MTP device (e.g., Garmin watch) never shows the "Initialize" button in the UI. `list_root_folders()` calls `tokio::fs::read_dir` on the synthetic MTP path (`mtp://\\?\usb#...`), which always fails, returning an error that makes `folderInfo = null` in `BasketSidebar` — so `renderDeviceFolders()` never renders the init banner.

**Approach:** Early-exit from `list_root_folders()` when the device path starts with `mtp://`, returning a valid `DeviceRootFoldersResponse` built without filesystem access. Propagate `friendly_name` through `DeviceEvent::Unrecognized` and store it in `DeviceManager` so MTP devices display a human-readable name.

## Boundaries & Constraints

**Always:**
- Keep MSC (mass storage) `list_root_folders` logic unchanged — only add a branch for `mtp://` paths.
- The `friendly_name` field on `DeviceEvent::Unrecognized` is `Option<String>`; MSC observer passes `None`, MTP observer passes `Some(dev.friendly_name.clone())`.
- Locking discipline in `DeviceManager` must mirror the existing pattern: acquire all three RwLocks together (path, io, friendly_name) inside `handle_device_unrecognized` and `handle_device_removed`.

**Ask First:**
- If tests in `device/tests.rs` call `DeviceEvent::Unrecognized` directly and fail to compile after the field addition, halt and ask whether to update them or move them to a helper.

**Never:**
- Do not modify the `DeviceIO` trait or `FileEntry` struct.
- Do not change the UI (`BasketSidebar.ts`, `InitDeviceModal.ts`) — the fix is purely in the daemon.
- Do not add `is_dir` to `FileEntry` — out of scope.
- Do not attempt actual MTP file enumeration in `list_root_folders`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Unrecognized MTP device | `unrecognized_device_path = "mtp://..."`, no manifest | `list_root_folders` returns `{ hasManifest: false, folders: [], device_name: friendly_name }` — UI shows "New Device Detected" banner with Initialize button | — |
| Recognized MTP device | `selected_device_path = "mtp://..."`, manifest present | Returns `{ hasManifest: true, folders: managed_paths, device_name: manifest.name }` — UI shows managed folder list | — |
| Unrecognized MSC device | `unrecognized_device_path = "E:\\"`, no manifest | Existing `fs::read_dir` path unchanged; `{ hasManifest: false }` returned | — |
| MTP device removed | `handle_device_removed` called | `unrecognized_device_path`, `_io`, and `_friendly_name` all cleared atomically | — |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/device/mod.rs:143-155` — `DeviceEvent` enum; add `friendly_name` field to `Unrecognized` variant
- `hifimule-daemon/src/device/mod.rs:172-190` — `DeviceManager` struct + `new()`; add `unrecognized_device_friendly_name` field
- `hifimule-daemon/src/device/mod.rs:300-323` — `handle_device_unrecognized`; accept and store `friendly_name`
- `hifimule-daemon/src/device/mod.rs:333-362` — `handle_device_removed`; clear `friendly_name` atomically
- `hifimule-daemon/src/device/mod.rs:543-616` — `list_root_folders`; early-exit for `mtp://` paths
- `hifimule-daemon/src/device/mod.rs:992-1045` — `run_observer` (MSC); pass `friendly_name: None`
- `hifimule-daemon/src/device/mod.rs:1048-1110` — `run_mtp_observer`; pass `friendly_name: Some(dev.friendly_name.clone())`
- `hifimule-daemon/src/main.rs:300-304` — `DeviceEvent::Unrecognized` handler; destructure `friendly_name` and pass to `handle_device_unrecognized`

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/device/mod.rs` — Apply all changes atomically:
  1. Add `friendly_name: Option<String>` to `DeviceEvent::Unrecognized` (line 151).
  2. Add `unrecognized_device_friendly_name: Arc<RwLock<Option<String>>>` field after `unrecognized_device_io` in `DeviceManager` struct; initialize to `Arc::new(RwLock::new(None))` in `new()`.
  3. In `handle_device_unrecognized`: add `friendly_name: Option<String>` parameter; acquire all three locks together; store `friendly_name`; keep existing overwrite-warning log.
  4. In `handle_device_removed`: extend the atomic block that clears path+io to also clear `friendly_name`.
  5. In `list_root_folders` (after resolving `device_path`, before `read_dir`): if `device_path.to_string_lossy().starts_with("mtp://")`, build and return a `DeviceRootFoldersResponse` from `managed_paths` and stored `friendly_name` without calling `fs::read_dir`. Managed device: `folders = managed_paths.iter().map(|p| DeviceFolderInfo { name: p.clone(), relative_path: p.clone(), is_managed: true }).collect()`. Unrecognized: `folders = vec![]`.
  6. In `run_observer` (MSC `Unrecognized` emit at line ~1022): add `friendly_name: None`.
  7. In `run_mtp_observer` (both `Unrecognized` emits): add `friendly_name: Some(dev.friendly_name.clone())`.
- [x] `hifimule-daemon/src/main.rs` — Update `DeviceEvent::Unrecognized` match arm to destructure `friendly_name` and pass it to `handle_device_unrecognized(path, device_io, friendly_name).await`.

**Acceptance Criteria:**
- Given the daemon is running and a Garmin watch (or any unrecognized MTP device) is plugged in, when the UI polls `get_daemon_state`, then `pendingDevicePath` is non-null AND `device_list_root_folders` returns `{ hasManifest: false }` — the "New Device Detected / Initialize" banner appears in BasketSidebar.
- Given an unrecognized USB drive is plugged in (no Garmin also connected), when the UI polls, then the existing MSC init banner behavior is unchanged.
- Given an MTP device has been initialized (has manifest), when `device_list_root_folders` is called, then it returns `{ hasManifest: true, folders: [managed paths from manifest] }` without error.
- Given `cargo build` is run, then it compiles with zero errors.

## Suggested Review Order

**Core fix — MTP path bypass in `list_root_folders`**

- Early-exit for `mtp://` paths: no filesystem access, returns valid `has_manifest` response.
  [`mod.rs:569`](../../hifimule-daemon/src/device/mod.rs#L569)

- Friendly-name read: extracted before `device_name` expression to avoid `try_read` race.
  [`mod.rs:577`](../../hifimule-daemon/src/device/mod.rs#L577)

**Event & state plumbing**

- `DeviceEvent::Unrecognized` gains `friendly_name` field; all match sites updated.
  [`mod.rs:151`](../../hifimule-daemon/src/device/mod.rs#L151)

- `DeviceManager` stores `unrecognized_device_friendly_name`; init and clear are atomic with path+io.
  [`mod.rs:182`](../../hifimule-daemon/src/device/mod.rs#L182)

- `handle_device_unrecognized` stores `friendly_name`; three locks acquired together.
  [`mod.rs:305`](../../hifimule-daemon/src/device/mod.rs#L305)

- `handle_device_removed` clears `friendly_name` alongside path and io.
  [`mod.rs:356`](../../hifimule-daemon/src/device/mod.rs#L356)

**Observer wiring**

- MTP observer passes `Some(dev.friendly_name.clone())` on both Unrecognized paths.
  [`mod.rs:1119`](../../hifimule-daemon/src/device/mod.rs#L1119)

- MSC observer passes `friendly_name: None`.
  [`mod.rs:1064`](../../hifimule-daemon/src/device/mod.rs#L1064)

- `main.rs` event handler destructures and forwards `friendly_name`.
  [`main.rs:300`](../../hifimule-daemon/src/main.rs#L300)

**Tests**

- Two new MTP-specific `list_root_folders` tests (with and without friendly_name).
  [`tests.rs:884`](../../hifimule-daemon/src/device/tests.rs#L884)

## Spec Change Log

## Verification

**Commands:**
- `cargo build --manifest-path hifimule-daemon/Cargo.toml` -- expected: zero errors, zero new warnings
- `cargo test --manifest-path hifimule-daemon/Cargo.toml` -- expected: all tests pass
