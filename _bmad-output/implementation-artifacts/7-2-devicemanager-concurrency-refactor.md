# Story 7.2: DeviceManager Concurrency Refactor

Status: done

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

- [x] **T1: Replace split unrecognized fields with `UnrecognizedDeviceState`** (AC: #1, #7, #8)
  - [x] Add `pub struct UnrecognizedDeviceState { pub path: PathBuf, pub io: Arc<dyn DeviceIO>, pub friendly_name: Option<String> }` near `ConnectedDevice` in `jellyfinsync-daemon/src/device/mod.rs`.
  - [x] Replace `unrecognized_device_path`, `unrecognized_device_io`, and `unrecognized_device_friendly_name` fields with `unrecognized_device: Arc<RwLock<Option<UnrecognizedDeviceState>>>`.
  - [x] Update `new()`, `handle_device_unrecognized`, `get_unrecognized_device_path`, `get_unrecognized_device_io`, `handle_device_removed`, `initialize_device`, and `list_root_folders`.
  - [x] Keep current public helper names where callers already use them; add `get_unrecognized_device_friendly_name()` only if it reduces duplicate lock reads.
  - [x] Do not expose a path/IO/friendly-name combination from multiple locks. A single read must provide a coherent snapshot.

- [x] **T2: Normalize DeviceManager lock ordering or introduce a combined state lock** (AC: #2, #3)
  - [x] Audit all methods that touch `connected_devices` and `selected_device_path`: `handle_device_detected`, `handle_device_removed`, `get_current_device`, `get_device_io`, `get_manifest_and_io`, `get_connected_devices`, `get_multi_device_snapshot`, `select_device`, `update_manifest`, `get_device_storage`, `initialize_device`.
  - [x] Preferred implementation: introduce `DeviceManagerState { connected_devices: HashMap<PathBuf, ConnectedDevice>, selected_device_path: Option<PathBuf> }` behind one `Arc<RwLock<DeviceManagerState>>`.
  - [x] If keeping separate locks, use the same order everywhere: `connected_devices` first, then `selected_device_path`. Never acquire `selected_device_path` and then `connected_devices`.
  - [x] Do not hold any DeviceManager state lock across `device_io.list_files`, `write_manifest`, DB calls, or any other async IO. Clone the required manifest/backend/path snapshot, drop locks, then await.
  - [x] Preserve `get_manifest_and_io()` as the preferred caller API for atomic manifest/backend snapshots.

- [x] **T3: Preserve multi-device selection semantics** (AC: #3)
  - [x] In `handle_device_removed`, after removing the path, if it was selected and `connected_devices` is non-empty, select the first remaining key.
  - [x] Current code only auto-selects when exactly one device remains; change this to any non-empty remainder.
  - [x] Keep non-selected removal behavior unchanged: selection must remain the existing selected path.

- [x] **T4: Fix MTP observer retry suppression** (AC: #4)
  - [x] In `run_mtp_observer`, do not insert `dev_id` into `known_ids` immediately after `create_mtp_backend` succeeds.
  - [x] Insert only after successfully sending either `DeviceEvent::Detected` or `DeviceEvent::Unrecognized`.
  - [x] If backend creation, `.jellyfinsync.json` read, JSON parsing before event creation, or channel send fails, leave the ID absent so the next physical reconnect can retry.
  - [x] Preserve MSC preference: if `has_msc_drive_for_device` matches, continue without inserting into `known_ids`.

- [x] **T5: Verify libmtp storage behavior and document/code accordingly** (AC: #5)
  - [x] Keep `LIBMTP_Get_Files_And_Folders(dev, 0, parent)` only with a nearby comment citing the libmtp/Debian manpage: storage `0` searches the given parent across all available storages.
  - [x] If explicit storage iteration is chosen instead, thread storage IDs through the libmtp handle carefully; do not disturb Windows WPD `storage_id` behavior from Story 7.1.
  - [x] Do not add a new dependency for this check.

- [x] **T6: Serialize per-device MTP operations in `MtpBackend`** (AC: #6)
  - [x] Add an async serialization primitive to `MtpBackend`, e.g. `operation_lock: Arc<tokio::sync::Mutex<()>>`.
  - [x] Acquire it in every `DeviceIO for MtpBackend` method that calls the handle: `begin_sync_job`, `read_file`, `write_file`, `delete_file`, `list_files`, `free_space`, `take_warnings`, `end_sync_job`.
  - [x] Hold the guard across the `spawn_blocking(...).await` so operations for the same backend cannot overlap.
  - [x] Avoid a global lock; serialization is per backend/device only.
  - [x] Keep existing libmtp internal `Mutex` in `LibmtpHandle`; the new lock is still needed because Windows WPD handle methods currently open sessions in independent blocking tasks.

- [x] **T7: Harden mount scanning and observer state** (AC: #9, #10)
  - [x] Update Linux/macOS `get_mounts` to skip entries on metadata/read errors; do not log noisy errors for removable media races.
  - [x] Ensure any system/boot volume filter is also applied to existing `known_mounts` each scan cycle, so stale unsafe entries are evicted even if they were captured by an older binary.
  - [x] Preserve Windows drive-letter enumeration behavior unless a regression is clearly tied to this story.

- [x] **T8: Tests** (AC: #2, #3, #4, #6, #7, #8)
  - [x] Add or update tests in `jellyfinsync-daemon/src/device/tests.rs` for consolidated unrecognized state, concurrent set/remove, auto-reselect with more than one remaining device, and no duplicate connected entry for an unrecognized path.
  - [x] Add a DeviceManager concurrency test that uses `tokio::time::timeout` around concurrent `select_device` and `update_manifest` calls; the test must fail on deadlock.
  - [x] Add an MTP backend serialization test in `device_io.rs` using a mock `MtpHandle` with an atomic in-flight counter; assert max in-flight operations for one backend is 1.
  - [x] If `run_mtp_observer` is hard to test directly because it loops forever, extract a small pure/helper function for one observed device and test `known_ids` insertion rules there.
  - [x] Run `rtk cargo test` from the repository root.

### Review Findings

- [x] [Review][Patch] `handle_device_unrecognized` acquires `state.write()` and `unrecognized_device.write()` in separate async blocks — a concurrent `handle_device_detected` can re-insert the same path between the two blocks, leaving both `connected_devices` and `unrecognized_device` populated for the same path (two live IO backends). Fix: hold `state.write()` while setting `unrecognized_device` by merging both lock blocks into a single sequential acquisition. [`device/mod.rs:handle_device_unrecognized`]
- [x] [Review][Patch] `initialize_device` unconditionally clears `unrecognized_device` after the async manifest write, without checking whether the pending slot still holds the same path. If the original device was removed and a different device arrived as pending between the snapshot and the clear, the new device's pending state is silently erased. Fix: only clear `unrecognized_device` if it still contains `pending.path`. [`device/mod.rs:initialize_device`]
- [x] [Review][Defer] TOCTOU in `handle_device_detected` — read-lock check then write-lock insert is a pre-existing pattern, not introduced by this diff. [`device/mod.rs:handle_device_detected`] — deferred, pre-existing
- [x] [Review][Defer] `emit_mtp_probe_event` returns `false` on manifest-read failure, causing the MTP observer to re-probe on every 2-second cycle with no backoff — intentional per AC4 but no cooldown mechanism exists. [`device/mod.rs:emit_mtp_probe_event`] — deferred, pre-existing observer loop design
- [x] [Review][Defer] `list_root_folders` TOCTOU — selected path can be removed between lock release and `read_dir`; error propagates via `?`. Pre-existing in the old two-lock version. [`device/mod.rs:list_root_folders`] — deferred, pre-existing
- [x] [Review][Defer] `run_observer` uses `tx.try_send` for `Removed` events (both stale eviction and detection removal); if the channel is full the event is silently dropped and the device entry is removed from `known_mounts` without `DeviceManager` being notified. Pre-existing mechanism, not introduced by this diff. [`device/mod.rs:run_observer`] — deferred, pre-existing
- [x] [Review][Defer] `get_mounts` skips disappearing volumes accidentally (metadata error → `is_mount_point` returns `false`) rather than explicitly; AC9 is met behaviourally but not by deliberate code. [`device/mod.rs:get_mounts`] — deferred, pre-existing accidental-but-correct behaviour

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

GPT-5 Codex

### Debug Log References

- `rtk cargo test` - 195 passed (4 suites, 5.00s)

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- Replaced split pending unrecognized-device locks with a single coherent `UnrecognizedDeviceState` snapshot.
- Made initialization use the pending snapshot backend internally so stale caller IO cannot pair with a newer pending path.
- Introduced combined `DeviceManagerState` for connected devices and selection; removed selected/connected lock-order inversion.
- Fixed selected-device removal to auto-select any remaining managed device and preserve non-selected removal semantics.
- Moved MTP observer `known_ids` insertion behind successful event emission and kept failed manifest reads retryable.
- Added per-backend MTP operation serialization and storage-0 libmtp documentation comments.
- Hardened mount observer stale-state eviction before each detection pass.
- Added regression tests for DeviceManager concurrency, pending state coherence, duplicate same-path backends, MTP retry insertion, and MTP operation serialization.

### File List

- `_bmad-output/implementation-artifacts/7-2-devicemanager-concurrency-refactor.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `jellyfinsync-daemon/src/device/mod.rs`
- `jellyfinsync-daemon/src/device/mtp.rs`
- `jellyfinsync-daemon/src/device/tests.rs`
- `jellyfinsync-daemon/src/device_io.rs`

### Change Log

- 2026-05-07: Implemented Story 7.2 DeviceManager concurrency refactor and MTP serialization hardening.
