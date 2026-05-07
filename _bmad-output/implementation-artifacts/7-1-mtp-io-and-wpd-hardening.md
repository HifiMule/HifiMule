# Story 7.1: MTP IO & WPD Hardening

Status: ready-for-dev

## Story

As a **System Admin (Alexis)**,
I want the MTP IO layer to be reliable and efficient across all device types,
So that bulk syncs to MTP devices are fast, atomic, and free of latent data-loss risks.

## Acceptance Criteria

1. **`path_to_object_id` single-handle refactor**: `path_to_object_id` accepts `&IPortableDeviceContent` (not `&IPortableDevice`), and all callers in `read_file`, `delete_file`, and `list_files` acquire a single `Content()` handle that is passed to both `path_to_object_id` and the subsequent operation — no double `Content()` acquisition exists anywhere in the module.

2. **`IStream::Write` S_FALSE handling**: The `write_file` stream write loop treats `S_FALSE` from `IStream::Write` as a partial write; it retries or returns an explicit error (not silently `Ok(())`).

3. **`ensure_dir_chain` PWSTR memory safety**: When `PWSTR::to_string()` fails after `CreateObjectWithPropertiesOnly`, `CoTaskMemFree` is called before the `?` propagation (scopeguard or inline free).

4. **Multi-storage `storage_id` selection**: `DeviceManifest` gains `storage_id: Option<String>` (with `#[serde(default)]`). Both `ensure_dir_chain` and `free_space` use the manifest's `storage_id` to select the target storage object rather than always using the first enumerated child under DEVICE.

5. **`shell_copy_to_device` STA thread**: `shell_copy_to_device` is executed on a dedicated STA OS thread (not a `spawn_blocking` MTA task), resolving the `IShellFolder::EnumObjects` threading constraint.

6. **Shell session batching**: Directory creation and file copy share a single Shell session per sync job (not one open/close per file), reducing teardown/reconnect overhead.

7. **UUID temp file naming**: Temp filenames in `write_file` use `uuid::Uuid::new_v4()` (or `tempfile` crate) rather than nanosecond timestamps to guarantee uniqueness under concurrent writes.

8. **Dirty-marker test content assertion**: `mtp_dirty_marker_detected_on_reconnect` in `device_io.rs` asserts the sentinel content is `b"\x00"` (not just presence and call order).

9. **Shell fallback warn logging**: When the WPD write fails and the Shell fallback is attempted, the original WPD error is logged at `warn` level before the fallback succeeds.

10. **`collect_files_recursive` directory enumeration error surfacing**: When a sub-directory fails to enumerate, the failure is surfaced as a structured warning in the sync result rather than silently skipped (`let _` discarded).

11. **`has_msc_drive_for_device` hardware GUID comparison**: The match uses hardware-level GUIDs (`CM_Get_Device_ID` or `SetupDiGetDeviceInstanceId`) rather than case-insensitive volume label comparison, so two drives with the same label do not incorrectly suppress MTP registration. [`device/mod.rs`, `has_msc_drive_for_device`]

12. **`write_file` delete-before-replace error path logging**: When `CreateObjectWithPropertiesAndData` or the write stream fails after the existing object has been deleted, the deleted object ID is logged so the incomplete state is diagnosable. [`mtp.rs`, `write_file`]

13. **`ensure_dir_chain` concurrent creation tolerance**: When a concurrent process creates the same directory between the check and `CreateObjectWithPropertiesOnly`, the resulting "already exists" COM error is caught and treated as success (tolerated), not a hard failure. [`mtp.rs`, `ensure_dir_chain`]

14. **`path_to_object_id` unit test with mock**: At least one test exercises the traversal logic using a mock `IPortableDeviceContent` fixture, covering path-component splitting and recursive child lookup without a physical device. [`mtp.rs`, `path_to_object_id`]

## Tasks / Subtasks

- [ ] **T1: Refactor `path_to_object_id` to accept `&IPortableDeviceContent`** (AC: #1)
  - [ ] T1.1: Change signature of `path_to_object_id` in `windows_wpd` from `(device: &IPortableDevice, path: &str)` to `(content: &IPortableDeviceContent, path: &str)` — remove the `device.Content()` call inside it and use the passed `content` directly
  - [ ] T1.2: Update `read_file`: acquire `content = device.Content()?` once; pass `&content` to `path_to_object_id`; pass the same `content` to the subsequent `Transfer()` call
  - [ ] T1.3: Update `delete_file`: acquire `content = device.Content()?` once; pass `&content` to `path_to_object_id`; pass the same `content` to `content.Delete()`
  - [ ] T1.4: Update `list_files`: acquire `content = device.Content()?` once; pass `&content` to `path_to_object_id`; pass the same `content` to `collect_files_recursive`
  - [ ] T1.5: Verify that `find_child_object_id` already accepts `&IPortableDeviceContent` (it does — no change needed)

- [ ] **T2: Fix `IStream::Write` S_FALSE handling** (AC: #2)
  - [ ] T2.1: In the `write_file` stream write loop, change `.ok()?` on `stream.Write(...)` to explicitly check the HRESULT: if `S_OK` proceed normally; if `S_FALSE` (partial write, `written < slice.len()`) return `Err(anyhow::anyhow!("WPD write_file: partial write at offset {}", offset))`; other errors propagate normally
  - [ ] T2.2: Note: The existing `if written == 0 { return Err(...) }` guard remains as a safeguard

- [ ] **T3: Fix `ensure_dir_chain` PWSTR memory safety** (AC: #3)
  - [ ] T3.1: After `content.CreateObjectWithPropertiesOnly(&props, &mut new_id_pwstr)?`, wrap the `to_string()` call with an inline free: call `to_string()`, immediately call `CoTaskMemFree(Some(new_id_pwstr.0 as *const _))`, then propagate any error with `?`
  - [ ] T3.2: Pattern: `let new_id = { let s = new_id_pwstr.to_string(); CoTaskMemFree(...); s? };`

- [ ] **T4: Add `storage_id` to `DeviceManifest` and thread through storage selection** (AC: #4)
  - [ ] T4.1: In `device/mod.rs`, add `#[serde(default)] pub storage_id: Option<String>` to `DeviceManifest` — backward-compatible with existing manifests (defaults to `None`)
  - [ ] T4.2: In `mtp.rs`, add `storage_id: Option<&str>` parameter to `path_to_object_id` and `ensure_dir_chain`
  - [ ] T4.3: In `path_to_object_id`: when `storage_id` is `Some(id)`, use it directly as the storage object ID instead of enumerating and taking the first child under DEVICE
  - [ ] T4.4: In `ensure_dir_chain`: same — use `storage_id` when provided
  - [ ] T4.5: In `free_space`: use `storage_id` from the device manifest when selecting the storage object for `WPD_STORAGE_FREE_SPACE_IN_BYTES` query
  - [ ] T4.6: Thread `storage_id` from `MtpBackend` through to `WpdHandle` methods — `MtpBackend` stores the manifest's `storage_id` at construction time and passes it to all WPD calls
  - [ ] Note: On Linux/macOS `libmtp`, `storage_id=0` passed to `LIBMTP_Get_Files_And_Folders` means "enumerate all storages" — this must be verified against libmtp docs and replaced with explicit storage ID iteration if the behavior is ambiguous (see Story 7.2 for libmtp scope)

- [ ] **T5: Move `shell_copy_to_device` to dedicated STA thread** (AC: #5)
  - [ ] T5.1: Replace the `CoInitGuard::init_sta()` inside `shell_copy_to_device` with a dedicated `std::thread::spawn` + channel result pattern: spin a new OS thread, call `init_sta()` on it, perform all Shell operations, send result back via `std::sync::mpsc::channel`
  - [ ] T5.2: The `write_file` call site awaits the thread's result via `thread.join()`
  - [ ] T5.3: Remove `let _com = CoInitGuard::init_sta()?;` from inside `shell_copy_to_device` (it moves to the spawned thread's entry point)

- [ ] **T6: Shell session batching** (AC: #6)
  - [ ] T6.1: Introduce a `ShellSession` RAII struct that holds a `CoInitGuard` (STA) and an `IFileOperation` for the current sync job
  - [ ] T6.2: `ShellSession::new()` opens COM STA and creates `IFileOperation` once; `Drop` calls `CoUninitialize`
  - [ ] T6.3: Modify `shell_copy_to_device` to accept an optional `&ShellSession` reference; when provided, reuse the session's `IFileOperation` rather than creating a new one
  - [ ] T6.4: In `execute_sync` (or the caller that drives file writes), create a single `ShellSession` at sync-start for Garmin-style devices and pass it through the IO calls for the duration of the job
  - [ ] T6.5: If session batching is not viable in this story's scope, at minimum factor `ShellSession` into a named struct so the interface is ready for later use

- [ ] **T7: UUID temp file naming** (AC: #7)
  - [ ] T7.1: In both temp-file creation paths inside `write_file` (Garmin pre-copy path ~line 831 and Shell fallback path ~line 963), replace `format!("jellyfinsync_{}", std::time::SystemTime::now()...)` with `format!("jellyfinsync_{}", uuid::Uuid::new_v4())`
  - [ ] T7.2: Verify `uuid` crate is already in `Cargo.toml` for `jellyfinsync-daemon` (it is — used for device ID generation in `device/mod.rs:516`)

- [ ] **T8: Improve `mtp_dirty_marker_detected_on_reconnect` test** (AC: #8)
  - [ ] T8.1: In `device_io.rs` test `mtp_dirty_marker_detected_on_reconnect`, change the dirty marker pre-population from `vec![]` (empty) to `b"\x00".to_vec()`
  - [ ] T8.2: Add an assertion: after `backend.list_files("").await`, read the dirty marker file and assert its content is `b"\x00"`: `let content = backend.read_file("Music/track.mp3.dirty").await.unwrap(); assert_eq!(content, b"\x00");`

- [ ] **T9: Explicit `warn` log before Shell fallback** (AC: #9)
  - [ ] T9.1: In `write_file`, before the Shell fallback block, change `crate::daemon_log!(...)` to use `tracing::warn!(...)` or `eprintln!("[WPD WARN] ...")` to explicitly mark the WPD error as a warning (distinguish it from info-level trace logs)
  - [ ] T9.2: Confirm `daemon_log!` maps to warn-level in its macro definition; if it already does, no change needed — verify by checking the macro definition in `main.rs` or `lib.rs`

- [ ] **T10: Surface `collect_files_recursive` enumeration errors** (AC: #10)
  - [ ] T10.1: In `collect_files_recursive`, change the `let _ = collect_files_recursive(...)` recursion call to propagate a `warn`-level log entry when the recursive call fails: `if let Err(e) = collect_files_recursive(...) { crate::daemon_log!("[WPD WARN] collect_files_recursive: failed to enumerate {:?}: {}", obj_id, e); }`
  - [ ] T10.2: The function signature can optionally gain a `warnings: &mut Vec<String>` accumulator parameter if the caller needs structured access to failures; otherwise a log is sufficient

- [ ] **T11: Hardware GUID comparison in `has_msc_drive_for_device`** (AC: #11)
  - [ ] T11.1: In `device/mod.rs` `has_msc_drive_for_device`, add a per-drive-letter hardware instance ID lookup using `SetupDiGetDeviceInstanceId` via `windows-sys::Win32::Devices::DeviceAndDriverInstallation`
  - [ ] T11.2: For each removable drive letter, retrieve its `DeviceInstanceId` (stable across renames); compare against the MTP device's `CM_Get_Device_ID` result — match on hardware ID rather than volume label
  - [ ] T11.3: Volume label comparison falls back only if hardware ID lookup fails for a given drive

- [ ] **T12: Log deleted object ID on write failure** (AC: #12)
  - [ ] T12.1: In `write_file`, in the cleanup block after WPD write failure (`if result.is_err()`) that calls `find_child_object_id` and `content.Delete(...)`, also log the deleted `bad_id` at debug/warn level: `crate::daemon_log!("[WPD] write_file: deleted erroneous object {:?} at path={}", bad_id, path);`
  - [ ] T12.2: For the delete-before-replace path (where the pre-existing object is deleted before the new write), log the object ID that was deleted so interrupted syncs are diagnosable

- [ ] **T13: Tolerate concurrent directory creation in `ensure_dir_chain`** (AC: #13)
  - [ ] T13.1: In `ensure_dir_chain`, wrap `content.CreateObjectWithPropertiesOnly(&props, &mut new_id_pwstr)?` to catch the "object already exists" HRESULT (`0x8007000B` — `ERROR_BAD_FORMAT` is not right; check for `HRESULT_FROM_WIN32(ERROR_ALREADY_EXISTS)` = `0x800700B7` or the WPD-specific equivalent)
  - [ ] T13.2: On "already exists" error: call `find_child_object_id(content, &current_id, component)?` to retrieve the existing object's ID and use that as `current_id` — so the chain continues as if creation succeeded

- [ ] **T14: `path_to_object_id` unit test with mock** (AC: #14)
  - [ ] T14.1: Add a `#[test]` in `mtp.rs` that exercises `split_path_components` + simulates the traversal logic for a two-level path (e.g., `"Music/Artist"`) using a fixture-based helper that mimics the child lookup loop without real COM objects
  - [ ] T14.2: Since the actual WPD COM objects cannot be mocked in unit tests without a physical device, the test should cover: (a) empty-path root return, (b) single-component path resolution, (c) path-not-found error — all using the `split_path_components` helper that IS unit-testable (already tested) plus any pure-Rust logic factored out of the unsafe COM block

## Dev Notes

### Scope

**Files in scope:**
- `jellyfinsync-daemon/src/device/mtp.rs` — all WPD fixes (T1–T3, T5–T7, T9–T10, T12–T14)
- `jellyfinsync-daemon/src/device/mod.rs` — `DeviceManifest.storage_id` field (T4.1) + `has_msc_drive_for_device` (T11)
- `jellyfinsync-daemon/src/device_io.rs` — test improvement only (T8)

**No new RPC methods** — this is a pure internal IO hardening story. Zero UI changes.

### Existing Code State (READ BEFORE TOUCHING)

#### `mtp.rs` — Windows WPD (`windows_wpd` module)

**Double `Content()` acquisition bug (T1):**
The current code in `read_file` (line 773), `delete_file` (line 980), and `list_files` (line 993) all call `path_to_object_id(&device, path)` which internally calls `device.Content()`, then immediately call `device.Content()` AGAIN to get the same handle for the subsequent operation. This is the primary target of T1.

Current `path_to_object_id` signature (line 278): `fn path_to_object_id(device: &IPortableDevice, path: &str) -> Result<HSTRING>`
Target signature: `fn path_to_object_id(content: &IPortableDeviceContent, path: &str) -> Result<HSTRING>`

The function body already has `let content = device.Content()?;` as its first line — remove this line and use the passed `content` parameter directly.

`find_child_object_id` (line 358) already takes `content: &IPortableDeviceContent` — good, no change needed.

**`IStream::Write` S_FALSE problem (T2):**
Current code at ~line 914–919:
```rust
stream.Write(slice.as_ptr() as *const _, slice.len() as u32, Some(&mut written)).ok()?;
offset += written as usize;
if written == 0 {
    return Err(anyhow::anyhow!("WPD write_file: stream stalled (zero bytes written)"));
}
```
The `.ok()?` converts S_FALSE (partial write) to an error — but S_FALSE from `IStream::Write` is legitimate and indicates `written < requested`, not a hard failure. The guard should be: after checking `S_OK`, if HRESULT is S_FALSE treat as partial write and either continue (the loop will retry remaining bytes) or return explicit error.

**`ensure_dir_chain` PWSTR leak (T3):**
Current code at ~line 677–679:
```rust
content.CreateObjectWithPropertiesOnly(&props, &mut new_id_pwstr)?;
let new_id = new_id_pwstr.to_string()?;  // If this fails, new_id_pwstr leaks!
CoTaskMemFree(Some(new_id_pwstr.0 as *const _));
```
Fix: free before propagating error:
```rust
content.CreateObjectWithPropertiesOnly(&props, &mut new_id_pwstr)?;
let new_id = new_id_pwstr.to_string();
CoTaskMemFree(Some(new_id_pwstr.0 as *const _));
let new_id = new_id?;
```

**`shell_copy_to_device` STA thread problem (T5):**
Currently `shell_copy_to_device` calls `let _com = CoInitGuard::init_sta()?;` at its start and is called from within a `spawn_blocking` task. The issue: `spawn_blocking` uses the MTA thread pool; `CoInitializeEx(COINIT_APARTMENTTHREADED)` on an MTA thread returns `RPC_E_CHANGED_MODE`, causing the function to fail. The fix: call `shell_copy_to_device` on a newly spawned OS thread (`std::thread::spawn`) that starts with a fresh STA context.

**Temp file naming (T7):**
Two places in `write_file`:
1. Garmin pre-copy path (~line 830–831): `std::env::temp_dir().join(format!("jellyfinsync_{}", ...))`
2. Shell fallback path (~line 963–964): same pattern
Both should use `uuid::Uuid::new_v4()` which is already in scope (used elsewhere in the crate).

#### `device/mod.rs` — `has_msc_drive_for_device` (T11)

Current implementation (line 1060–1101): Iterates drive letters, gets volume label via `GetVolumeInformationW`, and does `label.eq_ignore_ascii_case(friendly_name)`. This is the bug: two drives named "BACKUP" would both match.

The fix requires adding `SetupDi` hardware ID lookup. Key crate: `windows-sys` is already a dependency (used in `get_storage_info` and `is_removable_drive`). Additional API needed: `windows_sys::Win32::Devices::DeviceAndDriverInstallation::*`.

Pattern:
1. Get the volume GUID path for each drive letter using `GetVolumeNameForVolumeMountPointW`
2. Use that to query the hardware instance ID via `SetupDiGetDevicePropertyW` or `CM_Get_Device_IDW`
3. Compare the hardware instance ID against the MTP device's known ID

Alternatively (simpler): `DeviceIoControl` with `IOCTL_STORAGE_GET_DEVICE_NUMBER` to get `StorageDeviceProperty` serial number per drive, then compare against the WPD device ID.

#### `device_io.rs` — Test (T8)

Current `mtp_dirty_marker_detected_on_reconnect` test (line 484–503):
```rust
mock.files.lock().unwrap().insert("Music/track.mp3.dirty".to_string(), vec![]);
```
Change to `b"\x00".to_vec()` and add content assertion after `list_files`.

### Architecture Compliance

- **DeviceIO trait**: `MtpBackend` must remain the only implementation that dispatches to `MtpHandle` methods via `spawn_blocking`. Do not add direct IO to any other path.
- **No direct `std::fs` calls** with device paths (Story 4.0 invariant): all device file operations must go through `Arc<dyn DeviceIO>`. The temp file operations in `write_file` touch the *local* temp dir (not the device), so `std::fs::write/remove_file` on temp paths is correct and intentional.
- **`#[serde(default)]` on new fields**: Any new field added to `DeviceManifest` must have `#[serde(default)]` for backward compatibility with existing manifests on user devices.
- **No new RPC methods, no UI changes, no database schema changes** (unless T4 requires persisting `storage_id`, which should go in the manifest only — not a new DB column for this story).

### Critical Preserved Behaviors

Do NOT break these:
- `MscBackend::write_with_verify()` Write-Temp-Rename + `sync_all()` pattern — untouched
- `WpdHandle::prefers_shell_copy()` Garmin detection logic — untouched, only execution context changes
- The dirty-flag detection path in `handle_device_detected` (`device/mod.rs` line 215–258) — untouched
- All existing unit tests in `mtp.rs` (`test_split_path_components_*`) and `device_io.rs` must continue to pass

### Crate Dependencies (Already in Cargo.toml)

- `uuid` — already used (`uuid::Uuid::new_v4()` in `device/mod.rs:516`)
- `windows` — already used for WPD COM types
- `windows-sys` — already used for `GetDiskFreeSpaceExW`, `GetDriveTypeW`, `GetLogicalDrives`
- `anyhow` — already used throughout
- `tokio` — already used

For T11, may need to add `windows-sys` features for `Win32::Devices::DeviceAndDriverInstallation`. Check `Cargo.toml` feature list before adding — the crate is already present.

### Project Structure Notes

- `mtp.rs` lives at `jellyfinsync-daemon/src/device/mtp.rs` — it's the `mtp` submodule of `device`
- `device_io.rs` lives at `jellyfinsync-daemon/src/device_io.rs` — top-level module in the daemon crate
- `device/mod.rs` lives at `jellyfinsync-daemon/src/device/mod.rs`
- Tests in `device_io.rs` are in `pub mod tests { ... }` (line 300) — `pub` so `device/mod.rs` can reference them
- Tests in `mtp.rs` are in `mod tests { ... }` (line 1481)

### Previous Story Intelligence (Epic 6 → Epic 7)

From the git history, Epic 6 was packaging and CI/CD. The codebase is production-ready and the MTP layer was added in Epic 4 (Story 4.0). The key prior work:
- Story 4.0 established `DeviceIO` trait, `MscBackend`, `MtpBackend`, and `MockMtpHandle`
- Story 2.10 added WPD enumeration (`enumerate_mtp_devices`, `create_mtp_backend`)
- Story 6.6 confirmed `device_io.rs` tests exist and pass (verified during review)

The `daemon_log!` macro needs to be verified: it appears in `mtp.rs` as `crate::daemon_log!(...)`. Its definition in `main.rs` or `lib.rs` determines whether it already logs at warn level. If it's just `println!`, T9 needs a real `tracing::warn!` or `eprintln!("[WARN]")` upgrade.

### References

- Epic 7, Story 7.1 full ACs: [`_bmad-output/planning-artifacts/epics.md#Story-7.1`]
- Primary target file: [`jellyfinsync-daemon/src/device/mtp.rs`]
- Secondary targets: [`jellyfinsync-daemon/src/device_io.rs`], [`jellyfinsync-daemon/src/device/mod.rs`]
- Architecture doc (DeviceIO trait, MTP patterns): [`_bmad-output/planning-artifacts/architecture.md`]

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

### File List
