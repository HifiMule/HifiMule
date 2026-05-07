# Story 7.2: DeviceManager Concurrency Refactor

Status: ready-for-dev

## Story

As a **System Admin (Alexis)**,
I want the DeviceManager to be free of lock-order inversion, partial-state windows, and silent retry-suppression bugs,
so that multi-device scenarios are reliable and concurrent operations never deadlock.

## Acceptance Criteria

1. **Consolidated unrecognized-device state**: `unrecognized_device_path`, `unrecognized_device_io`, and `unrecognized_device_friendly_name` are replaced by one `RwLock<Option<UnrecognizedDeviceState>>`, where `UnrecognizedDeviceState { path: PathBuf, io: Arc<dyn DeviceIO>, friendly_name: Option<String> }`.
2. **No DeviceManager lock-order inversion**: `update_manifest` and `select_device` cannot acquire `selected_device_path` and `connected_devices` in opposite orders. Use one combined state lock or one documented acquisition order everywhere.
3. **Auto-reselect on selected-device removal**: when `handle_device_removed` removes the selected device and any other managed device remains, `selected_device_path` is set to one remaining path instead of `None`.
4. **MTP retry suppression fixed**: `run_mtp_observer` inserts into `known_ids` only after manifest probe succeeds enough to emit `Detected` or `Unrecognized`; transient backend-open or manifest-read failures must not suppress future reconnect attempts.
5. **libmtp storage-zero behavior verified**: Linux/macOS `LIBMTP_Get_Files_And_Folders(dev, 0, parent)` is either retained with a source-backed comment confirming storage `0` searches all available storages, or replaced with explicit storage iteration.
6. **Per-device MTP operation serialization**: concurrent `MtpBackend` operations for the same physical device execute sequentially, while different devices may still run independently.
7. **Concurrent unrecognized-state test**: at least one test exercises simultaneous set and clear/removal events and proves the consolidated state cannot expose path without IO or IO without path.
8. **No duplicate live backend per path**: `handle_device_unrecognized` removes any existing connected-device entry for the same path before storing the pending backend, so a re-probe cannot leave two live `Arc<dyn DeviceIO>` values for one path.
9. **Mount-scan race hardening**: `get_mounts` skips or defers a volume that disappears/remounts between metadata checks instead of surfacing a hard error.
10. **Boot-volume stale-state eviction**: any boot/system-volume path already present in mount observer state from a pre-fix binary is evicted on the next scan cycle, for example by filtering `known_mounts` against current safe mounts before removal/detection logic.

## Tasks / Subtasks

- [ ] **T1: Replace split unrecognized fields with `UnrecognizedDeviceState`** (AC: #1, #7, #8)
  - [ ] Add `pub struct UnrecognizedDeviceState { pub path: PathBuf, pub io: Arc<dyn DeviceIO>, pub friendly_name: Option<String> }` near `ConnectedDevice` in `jellyfinsync-daemon/src/device/mod.rs`.
  - [ ] Replace `unrecognized_device_path`, `unrecognized_device_io`, and `unrecognized_device_friendly_name` fields with `unrecognized_device: Arc<RwLock<Option<UnrecognizedDeviceState>>>`.
  - [ ] Update `new()`, `handle_device_unrecognized`, `get_unrecognized_device_path`, `get_unrecognized_device_io`, `handle_device_removed`, `initialize_device`, and `list_root_folders`.
  - [ ] Keep current public helper names where callers already use them; add `get_unrecognized_device_friendly_name()` only if it reduces duplicate lock reads.
  - [ ] Do not expose a path/IO/friendly-name combination from multiple locks. A single read must provide a coherent snapshot.

- [ ] **T2: Normalize DeviceManager lock ordering or introduce a combined state lock** (AC: #2, #3)
  - [ ] Audit all methods that touch `connected_devices` and `selected_device_path`: `handle_device_detected`, `handle_device_removed`, `get_current_device`, `get_device_io`, `get_manifest_and_io`, `get_connected_devices`, `get_multi_device_snapshot`, `select_device`, `update_manifest`, `get_device_storage`, `initialize_device`.
  - [ ] Preferred implementation: introduce `DeviceManagerState { connected_devices: HashMap<PathBuf, ConnectedDevice>, selected_device_path: Option<PathBuf> }` behind one `Arc<RwLock<DeviceManagerState>>`.
  - [ ] If keeping separate locks, use the same order everywhere: `connected_devices` first, then `selected_device_path`. Never acquire `selected_device_path` and then `connected_devices`.
  - [ ] Do not hold any DeviceManager state lock across `device_io.list_files`, `write_manifest`, DB calls, or any other async IO. Clone the required manifest/backend/path snapshot, drop locks, then await.
  - [ ] Preserve `get_manifest_and_io()` as the preferred caller API for atomic manifest/backend snapshots.

- [ ] **T3: Preserve multi-device selection semantics** (AC: #3)
  - [ ] In `handle_device_removed`, after removing the path, if it was selected and `connected_devices` is non-empty, select the first remaining key.
  - [ ] Current code only auto-selects when exactly one device remains; change this to any non-empty remainder.
  - [ ] Keep non-selected removal behavior unchanged: selection must remain the existing selected path.

- [ ] **T4: Fix MTP observer retry suppression** (AC: #4)
  - [ ] In `run_mtp_observer`, do not insert `dev_id` into `known_ids` immediately after `create_mtp_backend` succeeds.
  - [ ] Insert only after successfully sending either `DeviceEvent::Detected` or `DeviceEvent::Unrecognized`.
  - [ ] If backend creation, `.jellyfinsync.json` read, JSON parsing before event creation, or channel send fails, leave the ID absent so the next physical reconnect can retry.
  - [ ] Preserve MSC preference: if `has_msc_drive_for_device` matches, continue without inserting into `known_ids`.

- [ ] **T5: Verify libmtp storage behavior and document/code accordingly** (AC: #5)
  - [ ] Keep `LIBMTP_Get_Files_And_Folders(dev, 0, parent)` only with a nearby comment citing the libmtp/Debian manpage: storage `0` searches the given parent across all available storages.
  - [ ] If explicit storage iteration is chosen instead, thread storage IDs through the libmtp handle carefully; do not disturb Windows WPD `storage_id` behavior from Story 7.1.
  - [ ] Do not add a new dependency for this check.

- [ ] **T6: Serialize per-device MTP operations in `MtpBackend`** (AC: #6)
  - [ ] Add an async serialization primitive to `MtpBackend`, e.g. `operation_lock: Arc<tokio::sync::Mutex<()>>`.
  - [ ] Acquire it in every `DeviceIO for MtpBackend` method that calls the handle: `begin_sync_job`, `read_file`, `write_file`, `delete_file`, `list_files`, `free_space`, `take_warnings`, `end_sync_job`.
  - [ ] Hold the guard across the `spawn_blocking(...).await` so operations for the same backend cannot overlap.
  - [ ] Avoid a global lock; serialization is per backend/device only.
  - [ ] Keep existing libmtp internal `Mutex` in `LibmtpHandle`; the new lock is still needed because Windows WPD handle methods currently open sessions in independent blocking tasks.

- [ ] **T7: Harden mount scanning and observer state** (AC: #9, #10)
  - [ ] Update Linux/macOS `get_mounts` to skip entries on metadata/read errors; do not log noisy errors for removable media races.
  - [ ] Ensure any system/boot volume filter is also applied to existing `known_mounts` each scan cycle, so stale unsafe entries are evicted even if they were captured by an older binary.
  - [ ] Preserve Windows drive-letter enumeration behavior unless a regression is clearly tied to this story.

- [ ] **T8: Tests** (AC: #2, #3, #4, #6, #7, #8)
  - [ ] Add or update tests in `jellyfinsync-daemon/src/device/tests.rs` for consolidated unrecognized state, concurrent set/remove, auto-reselect with more than one remaining device, and no duplicate connected entry for an unrecognized path.
  - [ ] Add a DeviceManager concurrency test that uses `tokio::time::timeout` around concurrent `select_device` and `update_manifest` calls; the test must fail on deadlock.
  - [ ] Add an MTP backend serialization test in `device_io.rs` using a mock `MtpHandle` with an atomic in-flight counter; assert max in-flight operations for one backend is 1.
  - [ ] If `run_mtp_observer` is hard to test directly because it loops forever, extract a small pure/helper function for one observed device and test `known_ids` insertion rules there.
  - [ ] Run `rtk cargo test` from the repository root.

## Dev Notes

### Scope

Primary files:
- `jellyfinsync-daemon/src/device/mod.rs` - `DeviceManager`, mount observers, MTP observer, mount scanning.
- `jellyfinsync-daemon/src/device/tests.rs` - DeviceManager regression and concurrency tests.
- `jellyfinsync-daemon/src/device_io.rs` - `MtpBackend` per-device serialization and mock-handle tests.
- `jellyfinsync-daemon/src/device/mtp.rs` - libmtp storage-zero comment or explicit storage iteration only.

Secondary files only if signatures require updates:
- `jellyfinsync-daemon/src/rpc.rs` - currently reads pending path and IO separately in `handle_device_initialize`; prefer a coherent snapshot helper if one is added.
- `jellyfinsync-daemon/src/main.rs` - event dispatch calls `handle_device_detected`, `handle_device_unrecognized`, and `handle_device_removed`; keep behavior unchanged.

No UI changes, no new RPC methods, and no DB schema changes are required for this story.

### Existing Code State

`DeviceManager` currently stores two independent locks for recognized-device state:
- `connected_devices: Arc<RwLock<HashMap<PathBuf, ConnectedDevice>>>`
- `selected_device_path: Arc<RwLock<Option<PathBuf>>>`

It also stores pending initialization state in three independent locks:
- `unrecognized_device_path: Arc<RwLock<Option<PathBuf>>>`
- `unrecognized_device_io: Arc<RwLock<Option<Arc<dyn DeviceIO>>>>`
- `unrecognized_device_friendly_name: Arc<RwLock<Option<String>>>`

Several methods already try to avoid lock-order problems, but the pattern is inconsistent. `select_device` locks `connected_devices` then `selected_device_path`; `update_manifest` reads `selected_device_path` first, then writes `connected_devices`. This is the specific lock inversion called out by the epic. `get_multi_device_snapshot` also reads `connected_devices` then `selected_device_path`; any retained split-lock design must make this order universal.

`handle_device_removed` currently auto-selects a remaining device only when `remaining_keys.len() == 1`. AC #3 requires selecting a remaining device when `remaining_keys` is any non-empty length.

`handle_device_unrecognized` already removes `connected_devices.remove(&path)` before setting pending state. Preserve that order, but make the pending state one write. This prevents an unrecognized re-probe from leaving a recognized entry and a pending backend for the same path.

`initialize_device` currently accepts `device_io` as a parameter after the RPC handler calls `get_unrecognized_device_io()`, while the method separately calls `get_unrecognized_device_path()`. That split read can pair a stale IO with a newer pending path. Prefer a helper such as `take_unrecognized_device()` or `get_unrecognized_device_snapshot()` so initialization uses one coherent pending state. If changing the public method signature is too broad, keep RPC behavior unchanged but make the helper atomic inside `DeviceManager`.

`list_root_folders` uses `unrecognized_device_friendly_name` for MTP pending devices. After consolidation, it must read friendly name from `UnrecognizedDeviceState` without reintroducing split locks.

`run_mtp_observer` currently inserts `known_ids` immediately after `create_mtp_backend` succeeds, before manifest read/parse and event send. This suppresses retry if the async probe path fails later.

`MtpBackend` currently dispatches each operation with independent `tokio::task::spawn_blocking` calls. Story 7.1 improved WPD session handling, but concurrent calls for the same backend can still overlap. Add per-backend serialization in `device_io.rs`; do not push serialization burden into callers like sync, scrobbler, or RPC.

### Architecture Compliance

- Rust daemon uses `tokio` async runtime and co-located Rust tests. Keep new async tests under existing `#[tokio::test]` patterns.
- Device file operations must still go through `DeviceIO`; do not add direct `std::fs` access to device paths outside `MscBackend`.
- `DeviceManifest` compatibility remains important; this story should not add manifest fields. If a field becomes unavoidable, use `#[serde(default)]`.
- Public IPC contracts remain unchanged: `get_daemon_state`, `device.list`, `device.select`, `device_initialize`, and `device_list_root_folders` should keep their response shapes.
- Avoid holding async locks across device IO or DB access. This is both a deadlock guardrail and a responsiveness requirement.

### Previous Story Intelligence

Story 7.1 completed MTP IO hardening across `mtp.rs`, `device/mod.rs`, `device_io.rs`, `sync.rs`, `rpc.rs`, and tests with 188 tests passing. Relevant carry-forward items:
- `DeviceManifest.storage_id: Option<String>` already exists and is threaded through Windows WPD. Do not remove or rename it.
- `MtpBackend`/`WpdHandle` now contain Shell worker/session scaffolding and warning collection. Serialization must preserve `begin_sync_job`, `take_warnings`, and `end_sync_job` behavior.
- libmtp still has `LIBMTP_Get_Files_And_Folders(dev, 0, parent)` call sites in `mtp.rs`; Story 7.1 explicitly deferred verifying whether `0` means all storages.
- `has_msc_drive_for_device` was changed to prefer hardware GUID matching and volume-label fallback. Preserve this MSC-over-MTP suppression behavior in `run_mtp_observer`.
- `broadcast_device_state` in `rpc.rs` currently calls `handle_device_detected` using the current manifest/backend snapshot. Story 7.3 later calls out duplicate insertion concerns, so do not expand this pattern.

### Testing Guidance

Use focused daemon tests rather than hardware tests:
- DeviceManager tests can use `tempfile::tempdir()`, `Database::memory()`, and `MscBackend::new(...)` as existing tests do.
- Deadlock tests should use `tokio::time::timeout` with concurrent tasks. Keep timeouts short enough for test speed but long enough to avoid scheduler flakes.
- MTP serialization test should use a mock `MtpHandle` that sleeps or blocks briefly while incrementing an atomic counter. The expected max concurrent count for one `MtpBackend` is `1`.
- If extracting an observer helper for `run_mtp_observer`, make it small and private/test-visible; avoid reshaping the whole observer loop.

### Latest Technical Information

The Debian libmtp manpage for `LIBMTP_Get_Files_And_Folders` states that passing storage `0` searches the given parent across all available storages, and also notes that the device must be opened uncached for this operation. Source: https://manpages.debian.org/testing/libmtp-doc/mtp_files.3.en.html

The libmtp source implements `storage == 0` by mapping it to `PTP_GOH_ALL_STORAGE` before calling `ptp_getobjecthandles`, which supports keeping the current `0` behavior with a comment. Source: https://chromium.googlesource.com/chromium/deps/libmtp/+/refs/heads/main/src/libmtp.c

### References

- Epic 7 Story 7.2: `_bmad-output/planning-artifacts/epics.md`
- Architecture DeviceManager and DeviceIO sections: `_bmad-output/planning-artifacts/architecture.md`
- Previous story: `_bmad-output/implementation-artifacts/7-1-mtp-io-and-wpd-hardening.md`
- Current DeviceManager: `jellyfinsync-daemon/src/device/mod.rs`
- Current MTP backend: `jellyfinsync-daemon/src/device_io.rs`
- Current MTP platform code: `jellyfinsync-daemon/src/device/mtp.rs`

## Project Structure Notes

- Keep implementation inside the daemon crate.
- Keep tests co-located in Rust test modules: `device/tests.rs`, `device_io.rs`, and only `mtp.rs` if the libmtp storage behavior changes.
- Do not create a new synchronization crate or runtime abstraction. `tokio::sync` is already in the workspace feature set.

## Dev Agent Record

### Agent Model Used

{{agent_model_name_version}}

### Debug Log References

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.

### File List
