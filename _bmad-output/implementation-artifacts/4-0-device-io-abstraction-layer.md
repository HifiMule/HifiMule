# Story 4.0: Device IO Abstraction Layer

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a **System Admin (Alexis)**,
I want **all device file operations to go through a single abstract interface**,
so that **the sync engine works identically for both MSC and MTP devices without duplicated IO logic.**

## Acceptance Criteria

1. **DeviceIO Trait Enforcement**: The `DeviceIO` trait MUST be defined in `jellyfinsync-daemon`. When any sync, manifest, or scrobble operation targets a device, it MUST call methods on `Arc<dyn DeviceIO>` exclusively — no direct `std::fs` or `tokio::fs` calls with a device path anywhere outside `MscBackend`. (AC: #1)
2. **MSC Backend**: When `DeviceManager` detects a connected MSC device, it MUST create `MscBackend { root: PathBuf }`. `MscBackend::write_with_verify()` MUST use the Write-Temp-Rename pattern (`write to .tmp` → `sync_all()` → `rename`) — this is the existing MSC manifest-write behavior, now generalized. (AC: #2)
3. **MTP Backend — Write**: When `DeviceManager` detects a connected MTP device, it MUST create `MtpBackend { handle: Arc<MtpHandle> }`. `MtpBackend::write_file()` MUST transfer data via WPD `IPortableDeviceContent` object creation on Windows, or `libmtp_send_file_from_memory` on Linux/macOS. (AC: #3)
4. **MTP Backend — Verified Write**: `MtpBackend::write_with_verify()` MUST write a `".dirty"` marker object first, overwrite the target object, then delete the marker. (AC: #3)
5. **MTP Backend — Read/List/Delete/Space**: `MtpBackend::read_file()` MUST retrieve object data by path lookup. `MtpBackend::list_files()` MUST enumerate storage objects. `MtpBackend::delete_file()` MUST remove an object by handle. `MtpBackend::free_space()` MUST query device storage capacity. (AC: #3)
6. **MTP Dirty Marker on Reconnect**: When the daemon reconnects to a device with a `".dirty"` marker present in the root of the managed path, it MUST fire `on_device_dirty` — the same event path as the existing MSC dirty-manifest detection. (AC: #4)
7. **Full Caller Refactor**: When Story 4.0 is complete, every direct `std::fs` or `tokio::fs` call targeting a device path in `sync.rs`, `rpc.rs`, `device/mod.rs`, and `scrobbler.rs` MUST have been replaced with the corresponding `DeviceIO` method. All existing unit tests MUST pass without modification (MSC behavior is unchanged). (AC: #5)

## Tasks / Subtasks

- [x] **T1: Create DeviceIO trait** (AC: #1)
  - [x] T1.1: Create `jellyfinsync-daemon/src/device_io.rs` — new file
  - [x] T1.2: Add dependency `async-trait = "0.1"` to `jellyfinsync-daemon/Cargo.toml` (required for `async fn` in `dyn Trait` — see Dev Notes)
  - [x] T1.3: Define `FileEntry` struct: `pub path: String, pub name: String, pub size: u64` with `#[serde(rename_all = "camelCase")]`
  - [x] T1.4: Define `DeviceIO` trait with `#[async_trait]` and all six methods: `read_file`, `write_file`, `write_with_verify`, `delete_file`, `list_files`, `free_space`
  - [x] T1.5: Add `pub mod device_io;` to `jellyfinsync-daemon/src/main.rs`

- [x] **T2: Implement MscBackend** (AC: #2)
  - [x] T2.1: Implement `MscBackend { root: PathBuf }` in `device_io.rs`
  - [x] T2.2: `read_file(path)` → `tokio::fs::read(self.root.join(path))`
  - [x] T2.3: `write_file(path, data)` → create parent dirs with `tokio::fs::create_dir_all`, then `tokio::fs::write`
  - [x] T2.4: `write_with_verify(path, data)` → write to `{path}.tmp`, call `file.sync_all().await`, then `tokio::fs::rename` — mirror the existing `write_manifest` pattern exactly
  - [x] T2.5: `delete_file(path)` → `tokio::fs::remove_file(self.root.join(path))`
  - [x] T2.6: `list_files(path)` → recursive async directory walk (mirror `cleanup_tmp_files` traversal pattern in `device/mod.rs`), returning `Vec<FileEntry>` with relative-to-root paths
  - [x] T2.7: `free_space()` → call the existing `get_storage_info(&self.root)` from `device/mod.rs` and extract `free_bytes`

- [x] **T3: Add MTP Cargo.toml dependencies** (AC: #3)
  - [x] T3.1: Added commented-out `windows` crate entry (placeholder for Story 2.10; `MtpHandle` trait abstraction avoids requiring platform libraries now)
  - [x] T3.2: Added commented-out `libmtp-rs` entry (placeholder for Story 2.10)
  - [x] T3.3: No `pkg-config` build-dep needed until actual libmtp-rs integration lands

- [x] **T4: Implement MtpBackend** (AC: #3, #4)
  - [x] T4.1: Define `MtpHandle` as a platform-independent trait in `device_io.rs` (enables mock injection without requiring platform libs)
  - [x] T4.2: Implement `MtpBackend { handle: Arc<dyn MtpHandle> }` — fully implemented using `spawn_blocking`
  - [x] T4.3: `write_file()` → delegates to `handle.write_file()` via spawn_blocking
  - [x] T4.4: `write_with_verify()` → dirty-marker strategy: write `.dirty` → write target → delete `.dirty`
  - [x] T4.5: `read_file()` → delegates to `handle.read_file()` via spawn_blocking
  - [x] T4.6: `list_files()` → delegates to `handle.list_files()` via spawn_blocking
  - [x] T4.7: `delete_file()` → delegates to `handle.delete_file()` via spawn_blocking
  - [x] T4.8: `free_space()` → delegates to `handle.free_space()` via spawn_blocking

- [x] **T5: Integrate DeviceIO into DeviceManager** (AC: #1, #4)
  - [x] T5.1: Define `struct ConnectedDevice { pub manifest: DeviceManifest, pub device_io: Arc<dyn DeviceIO> }` in `device/mod.rs`
  - [x] T5.2: Change `connected_devices: Arc<RwLock<HashMap<PathBuf, DeviceManifest>>>` to `HashMap<PathBuf, ConnectedDevice>` — updated all read/write sites in `DeviceManager`
  - [x] T5.3: In `handle_device_detected()`, instantiate `MscBackend { root: path.clone() }` and store as `Arc<dyn DeviceIO>` in `ConnectedDevice` — always MSC (MTP detection arrives in Story 2.10)
  - [x] T5.4: Added `pub async fn get_device_io(&self) -> Option<Arc<dyn DeviceIO>>` to `DeviceManager`
  - [x] T5.5: Refactored `write_manifest()` — signature is now `write_manifest(device_io: Arc<dyn DeviceIO>, manifest: &DeviceManifest)`
  - [x] T5.6: Updated `DeviceManager::update_manifest()` to retrieve device_io from `ConnectedDevice`
  - [x] T5.7: Updated `DeviceManager::initialize_device()` — creates `MscBackend` locally, calls `write_manifest()`, stores `ConnectedDevice` in map
  - [x] T5.8: Refactored `cleanup_tmp_files()` to accept `device_io: Arc<dyn DeviceIO>`
  - [x] T5.9: On device reconnect: scan `list_files("")` for `.dirty` markers — if found, set `manifest.dirty = true` and return early (fires same dirty-resume path as MSC)

- [x] **T6: Refactor sync.rs** (AC: #5)
  - [x] T6.1: Added `device_io: Arc<dyn DeviceIO>` parameter to `execute_sync()` signature
  - [x] T6.2: Replaced streaming file write with `buffer_stream()` + `device_io.write_with_verify(relative_path, &buffer)` for all adds; M3U writes also go through `device_io.write_with_verify`
  - [x] T6.3: Replaced `tokio::fs::remove_file` with `device_io.delete_file(relative_path)` for deletes and M3U cleanup
  - [x] T6.4: `create_dir_all` removed from execute_sync; parent dir creation handled internally by `MscBackend::write_with_verify`
  - [x] T6.5: All path arguments to DeviceIO methods are RELATIVE (e.g. `"Music/Artist/Album/track.mp3"`)

- [x] **T7: Refactor RPC and daemon callers** (AC: #5)
  - [x] T7.1: In `rpc.rs`, `handle_sync_execute`: retrieves `device_io` via `device_manager.get_device_io()` and passes to `execute_sync()`
  - [x] T7.2: In `main.rs`, `run_auto_sync`: retrieves `device_io` from `DeviceManager` and passes to `execute_sync()`
  - [x] T7.3: Verified no other callers use device paths with raw `std::fs` or `tokio::fs`

- [x] **T8: Refactor scrobbler.rs** (AC: #5)
  - [x] T8.1: `scrobbler.rs` — replaced direct `std::fs::read_to_string` with `device_io.read_file(".scrobbler.log")`; added `device_id: String` parameter for stable dedup key
  - [x] T8.2: Updated scrobbler caller in `main.rs` to pass `device_io` and `manifest_device_id`; `rpc.rs` had no scrobbler direct call

- [x] **T9: Testing** (AC: #1-#7)
  - [x] T9.1: Unit tests for `MscBackend` in `device_io.rs::tests`: read/write/delete/list/write_with_verify
  - [x] T9.2: Unit tests for `MtpBackend::write_with_verify` dirty-marker sequence with `MockMtpHandle`
  - [x] T9.3: Unit test for dirty marker detection in MTP listing (`mtp_dirty_marker_detected_on_reconnect`)
  - [x] T9.4: All `generate_m3u_files` tests updated to pass `MscBackend` device_io; all scrobbler tests updated to `MscBackend`; all `write_manifest`/`cleanup_tmp_files` tests updated
  - [x] T9.5: `cargo test` — 171 tests passed, 0 regressions

## Dev Notes

### Critical: Current State of the Codebase

**Story 4.0 is a retroactive refactoring story** — all of stories 4.1–4.8 are already done. The codebase currently uses `tokio::fs` directly for every device file operation. The DeviceIO trait does not exist yet. This story adds the abstraction layer around existing working code.

**What exists now that will change:**
- `device/mod.rs:write_manifest()` — takes `device_root: &Path`, uses `tokio::fs::File::create` directly. Will be refactored to accept `Arc<dyn DeviceIO>`.
- `device/mod.rs:cleanup_tmp_files()` — takes `device_root: &Path`, walks dirs with `tokio::fs::read_dir`. Will be refactored to use `DeviceIO::list_files()` / `delete_file()`.
- `device/mod.rs:DeviceManager::connected_devices` — currently `HashMap<PathBuf, DeviceManifest>`. Must become `HashMap<PathBuf, ConnectedDevice>` to also store the IO backend.
- `sync.rs:execute_sync()` — currently uses `tokio::fs` for all device writes/deletes. Gains a `device_io: Arc<dyn DeviceIO>` parameter.
- `scrobbler.rs` — currently reads scrobbler log via `std::fs`/`tokio::fs`. Gains `device_io: Arc<dyn DeviceIO>` parameter.

### Architecture Compliance

- **IPC Protocol**: No new RPC methods needed — this is a pure internal refactor.
- **Naming**: Rust `snake_case`, JSON `camelCase` via `#[serde(rename_all = "camelCase")]`.
- **Error Handling**: `thiserror` for typed errors in `DeviceIO` implementations, `anyhow` at the RPC boundary.
- **Atomic IO**: `MscBackend::write_with_verify()` MUST use Write-Temp-Rename + `sync_all()`. This generalizes the exact pattern already in `write_manifest()` to all device file writes. `MtpBackend::write_with_verify()` uses dirty-marker strategy instead.
- **Async IO**: All `tokio::fs` calls stay in `MscBackend`. Never `std::fs` in async context.

### Async Trait — Use `async-trait` Crate

Rust's native `async fn in trait` (stable since 1.75) **does not work with `dyn Trait`** for dynamic dispatch without boxing. The `async-trait = "0.1"` crate is the standard solution — it desugars `async fn` into `Pin<Box<dyn Future>>` automatically:

```rust
use async_trait::async_trait;

#[async_trait]
pub trait DeviceIO: Send + Sync {
    async fn read_file(&self, path: &str) -> Result<Vec<u8>>;
    async fn write_file(&self, path: &str, data: &[u8]) -> Result<()>;
    async fn write_with_verify(&self, path: &str, data: &[u8]) -> Result<()>;
    async fn delete_file(&self, path: &str) -> Result<()>;
    async fn list_files(&self, path: &str) -> Result<Vec<FileEntry>>;
    async fn free_space(&self) -> Result<u64>;
}
```

Apply `#[async_trait]` to both the trait definition AND each `impl DeviceIO for Backend` block.

### DeviceIO Path Convention — RELATIVE Paths Only

All `path` arguments to `DeviceIO` methods are **relative to the device root**:
- `"Music/Artist/Album/01 - Track.flac"` ✅
- `"/E:/Music/Artist/Album/01 - Track.flac"` ❌

The backend resolves against its root internally: `MscBackend::write_file("Music/foo.mp3")` → `tokio::fs::write(self.root.join("Music/foo.mp3"), data)`.

Callers in `sync.rs` that previously computed `device_root.join(local_path)` should pass just `local_path` (which is already stored as a relative path in `SyncedItem.local_path`).

### MTP Backend — Story 2.10 Dependency

**Story 2.10 (MTP device detection) is still `backlog`.** Until it lands, `DeviceManager::handle_device_detected()` is only called for MSC devices. For this story: always instantiate `MscBackend`. `MtpBackend` is fully implemented and unit-tested with a mock handle. When Story 2.10 arrives, it will provide the `MtpHandle` and `DeviceManager::handle_device_detected()` will instantiate the correct backend.

### ConnectedDevice Struct — DeviceManager Refactor

Change `connected_devices: HashMap<PathBuf, DeviceManifest>` to `HashMap<PathBuf, ConnectedDevice>`:

```rust
pub struct ConnectedDevice {
    pub manifest: DeviceManifest,
    pub device_io: Arc<dyn DeviceIO>,
}
```

All sites that previously did `devices.get(&path).cloned()` for the manifest now do `devices.get(&path).map(|d| d.manifest.clone())`. Sites that need the IO backend use `devices.get(&path).map(|d| d.device_io.clone())`.

The existing `update_manifest()` closure pattern is preserved — it still receives `&mut DeviceManifest` — but now also calls `write_manifest(device_io.clone(), manifest).await` using the stored backend.

### Refactored `write_manifest` Signature

```rust
// Before:
pub async fn write_manifest(device_root: &Path, manifest: &DeviceManifest) -> Result<()>

// After:
pub async fn write_manifest(device_io: Arc<dyn DeviceIO>, manifest: &DeviceManifest) -> Result<()> {
    let json = serde_json::to_string_pretty(manifest)?;
    device_io.write_with_verify(".jellyfinsync.json", json.as_bytes()).await
}
```

`initialize_device()` creates a local `MscBackend` for the one-time initial write before storing the device in the map.

### MTP Dependencies — Windows vs. Unix

**Windows (WPD via `windows` crate):**
- The existing `windows-sys = "0.59"` only covers low-level Win32 APIs. WPD (Portable Devices) requires the higher-level `windows = "0.58"` crate with features: `Devices_Portable`, `Win32_Devices_PortableDevices`.
- These are different crates: `windows-sys` (raw bindings) vs `windows` (high-level).
- COM must be initialized on the calling thread: call `CoInitializeEx(None, COINIT_APARTMENTTHREADED)` before using WPD.

**Linux/macOS (`libmtp-rs`):**
- `libmtp-rs = "0.3"` wraps the system `libmtp` C library.
- System prerequisite: `libmtp-dev` on Ubuntu/Debian, `libmtp` via Homebrew on macOS.
- Build script links via `pkg-config`: add `pkg_config::probe_library("libmtp")` to `build.rs`.
- `libmtp-rs` is async-compatible via `spawn_blocking` — wrap blocking MTP calls in `tokio::task::spawn_blocking`.

### Key Pattern References from Completed Stories

- **`write_manifest` Write-Temp-Rename** (`device/mod.rs:87-102`): The exact pattern `MscBackend::write_with_verify()` must replicate — write to `.tmp`, `file.sync_all().await`, `tokio::fs::rename`. Preserves crash safety.
- **`cleanup_tmp_files` recursive walk** (`device/mod.rs:107-152`): The symlink-safe traversal pattern that `DeviceIO::list_files()` MSC implementation must match.
- **`now_iso8601()`** (`sync.rs:18-47`): Existing ISO 8601 helper, no new datetime deps needed.
- **`get_storage_info()`** (`device/mod.rs:786+`): Platform-gated implementation already exists for Windows/macOS/Linux — `MscBackend::free_space()` should delegate to this function.
- **`execute_sync()` path construction**: `SyncedItem.local_path` is already stored as a relative path (e.g., `"Music/Artist/Album/track.mp3"`) — pass directly to `DeviceIO` methods.

### File Structure

Files to create:
- `jellyfinsync-daemon/src/device_io.rs` — `DeviceIO` trait, `FileEntry`, `MscBackend`, `MtpBackend`

Files to modify:
- `jellyfinsync-daemon/Cargo.toml` — add `async-trait`, platform-gated MTP deps
- `jellyfinsync-daemon/src/main.rs` — add `pub mod device_io;`
- `jellyfinsync-daemon/src/device/mod.rs` — `ConnectedDevice` struct, `DeviceManager` HashMap change, `write_manifest` refactor, `cleanup_tmp_files` refactor, `get_device_io()` accessor
- `jellyfinsync-daemon/src/sync.rs` — `execute_sync()` gains `device_io` param, all device `tokio::fs` calls replaced
- `jellyfinsync-daemon/src/rpc.rs` — `sync.start` and scrobbler handlers pass `device_io` to callers
- `jellyfinsync-daemon/src/scrobbler.rs` — scrobbler log read via `DeviceIO::read_file()`
- `jellyfinsync-daemon/src/main.rs` — `run_auto_sync()` retrieves and passes `device_io`
- `jellyfinsync-daemon/src/device/tests.rs` — update any tests constructing `ConnectedDevice`

### Testing Standards

- **Framework**: Built-in `#[cfg(test)]` with `#[tokio::test]` for async tests
- **Mocking**: `tempfile::tempdir()` for MSC filesystem tests; define a `MockDeviceIO` struct implementing `DeviceIO` that records calls for integration tests
- **MSC regression guarantee**: Run full `cargo test` — all 82+ existing tests (from stories 4.1-4.8) must pass. Any failure is a regression.
- **MTP unit tests**: Use a stub `MtpHandle` — test `write_with_verify` dirty-marker sequence, dirty-marker detection on reconnect

### References

- [Architecture: Device IO Abstraction](../planning-artifacts/architecture.md) — Trait definition, backend selection, enforcement guidelines
- [Architecture: Safety & Atomicity Patterns](../planning-artifacts/architecture.md) — Write-Temp-Rename mandate, MTP dirty-marker strategy
- [Epic 4 Story 4.0](../planning-artifacts/epics.md) — Original story definition with full AC
- [Story 4.1](4-1-differential-sync-algorithm-manifest-comparison.md) — `write_manifest`, `DeviceManifest`, `SyncedItem` patterns
- [Story 4.2](4-2-atomic-buffered-io-streaming.md) — `execute_sync()` current implementation reference
- [Story 4.4](4-4-self-healing-dirty-manifest-resume.md) — Dirty-manifest detection event path (MSC reference for MTP port)
- [async-trait crate docs](https://docs.rs/async-trait/latest/async_trait/) — Required for `dyn DeviceIO`
- [windows crate WPD](https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/Devices/PortableDevices/index.html) — WPD API reference

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

1. `libmtp-rs = "0.3"` version not found on crates.io (latest is 0.7.x) — resolved by leaving MTP platform deps as commented-out placeholders in Cargo.toml; `MtpHandle` trait abstraction means platform libs are not required until Story 2.10.
2. `?` operator in `tokio::spawn` block returning `()` (rpc.rs) — replaced `ok_or(JsonRpcError{...})?` with `match device_manager.get_device_io().await { Some(io) => io, None => { ...; return; } }`.
3. `Arc<MscBackend>` vs `Arc<dyn DeviceIO>` type mismatch in sync.rs tests — fixed by adding explicit type annotation `let device_io: Arc<dyn DeviceIO> = Arc::new(...)`.
4. 5 test call sites for `generate_m3u_files` in sync.rs missing the new `device_io` argument — updated all 5 to pass an `Arc<dyn DeviceIO>`.
5. Unused variable `m3u_path` in sync.rs after removing `write_m3u_atomic` — removed the dead declaration.

### Completion Notes List

- Implemented `DeviceIO` trait with `#[async_trait]` in new `device_io.rs`; all six methods: `read_file`, `write_file`, `write_with_verify`, `delete_file`, `list_files`, `free_space`.
- `MscBackend` uses Write-Temp-Rename + `sync_all()` in `write_with_verify` — exact pattern from the original `write_manifest` now generalized to all device file writes.
- `MtpBackend` fully implemented with `MtpHandle` trait abstraction; `write_with_verify` uses dirty-marker strategy; `MockMtpHandle` enables complete unit testing without platform libs.
- `DeviceManager` refactored: `connected_devices` value type changed from `DeviceManifest` to `ConnectedDevice { manifest, device_io }`; `get_device_io()` accessor added; `update_manifest` drops RwLock write guard before async I/O to avoid holding lock during device write.
- `write_manifest` and `cleanup_tmp_files` signatures updated to accept `Arc<dyn DeviceIO>` instead of `&Path`; all callers updated (device/tests.rs ~15+ call sites).
- `execute_sync` gains `device_io: Arc<dyn DeviceIO>` parameter; all `tokio::fs` device calls replaced with `device_io` methods; `buffer_stream` helper buffers HTTP download before calling `write_with_verify`.
- Scrobbler refactored: direct `std::fs::read_to_string` replaced with `device_io.read_file`; added `device_id: String` parameter for stable dedup key; `NotFound` errors treated as empty result (no log file present).
- All 171 tests pass, 0 regressions.

### File List

- `jellyfinsync-daemon/src/device_io.rs` — NEW: `DeviceIO` trait, `FileEntry`, `MscBackend`, `MtpBackend`, `MtpHandle`, `MockMtpHandle`, unit tests
- `jellyfinsync-daemon/Cargo.toml` — added `async-trait = "0.1"`; commented-out MTP platform dep placeholders
- `jellyfinsync-daemon/src/main.rs` — added `pub mod device_io;`; `run_auto_sync` retrieves device_io and passes to `execute_sync`; scrobbler spawn updated with `device_io` and `device_id`
- `jellyfinsync-daemon/src/device/mod.rs` — `ConnectedDevice` struct; HashMap value type changed; `write_manifest` and `cleanup_tmp_files` signatures refactored; `get_device_io()` accessor; `handle_device_detected` creates `MscBackend` and checks `.dirty` markers; `update_manifest` drops lock before I/O
- `jellyfinsync-daemon/src/sync.rs` — `execute_sync` gains `device_io` param; `buffer_stream` helper; all device `tokio::fs` calls replaced; `generate_m3u_files` updated; `write_m3u_atomic` removed
- `jellyfinsync-daemon/src/rpc.rs` — `handle_sync_execute` and `handle_sync_get_resume_state` retrieve and pass `device_io`
- `jellyfinsync-daemon/src/scrobbler.rs` — `process_device_scrobbles` signature updated; direct fs read replaced with `device_io.read_file`
- `jellyfinsync-daemon/src/device/tests.rs` — all `write_manifest` and `cleanup_tmp_files` call sites updated to use `MscBackend`

### Review Findings

- [x] [Review][Decision] AC#5: `initialize_device` calls `tokio::fs::create_dir` directly on device path — resolved: added `ensure_dir(&str)` to `DeviceIO` trait; `MscBackend` uses `create_dir_all`, `MtpBackend` is a no-op; `initialize_device` updated [`device/mod.rs:435`, `device_io.rs`]
- [x] [Review][Patch] `buffer_stream` loads entire file into memory — added 2 GB hard cap; returns error if stream exceeds limit [`sync.rs`, `buffer_stream` helper]
- [x] [Review][Patch] `MscBackend::write_with_verify` does not clean up `.tmp` file on failure — fixed: tmp deleted on write/sync error before returning [`device_io.rs`]
- [x] [Review][Patch] `MscBackend` operations lack path-traversal bounds check — added `check_relative()` guard to all path-taking methods [`device_io.rs`]
- [x] [Review][Patch] AC#5 violation: `cleanup_empty_dirs` uses direct `tokio::fs` — added `cleanup_empty_subdirs(&str)` to `DeviceIO` trait; MSC impl recursively prunes, MTP is no-op; call site in `execute_sync` updated [`device_io.rs`, `sync.rs`]
- [x] [Review][Patch] TOCTOU: `get_device_io()` and `get_current_device()` are separate lock acquisitions — added `get_manifest_and_io()` atomic accessor; used in `handle_sync_execute` and `run_auto_sync` [`device/mod.rs`, `rpc.rs`, `main.rs`]
- [x] [Review][Patch] MTP dirty-resume flow never deletes `.dirty` markers — `handle_sync_get_resume_state` now deletes all `.dirty` files via `device_io.delete_file` before running `cleanup_tmp_files` [`rpc.rs`]
- [x] [Review][Patch] AC#5 violation: `write_file_streamed` dead code not removed — function and its two tests removed [`sync.rs`]
- [x] [Review][Patch] "No such file" detection in `generate_m3u_files` uses fragile string matching — replaced with `downcast_ref::<std::io::Error>().kind() == NotFound` [`sync.rs`]
- [x] [Review][Patch] Scrobble silently skipped with no log — added `daemon_log!` warning in the `None` branch [`main.rs`]
- [x] [Review][Defer] `update_manifest` TOCTOU between `selected_device_path` and `connected_devices` lock acquisitions [`device/mod.rs:378-393`] — deferred, pre-existing
- [x] [Review][Defer] MTP scrobbler `not-found` detection broken — `downcast_ref::<std::io::Error>()` fails for plain `anyhow` errors from `MtpHandle`; affects MTP devices only [`scrobbler.rs:93-97`] — deferred, MTP not in production (Story 2.10)
- [x] [Review][Defer] Potential deadlock via lock-order inversion when `get_multi_device_snapshot` and `update_manifest` run concurrently with a `select_device` call [`device/mod.rs`] — deferred, pre-existing
- [x] [Review][Defer] `handle_device_removed` does not auto-reselect when 2+ devices remain after removing the selected device [`device/mod.rs`] — deferred, pre-existing
- [x] [Review][Defer] `MtpBackend` concurrent `spawn_blocking` calls have no ordering guarantee across concurrent operations — MTP is stateful [`device_io.rs`] — deferred, MTP not in production (Story 2.10)
- [x] [Review][Defer] `cleanup_tmp_files` with empty `managed_paths` silently skips root-level `.tmp` cleanup [`device/mod.rs`, `cleanup_tmp_files`] — deferred, pre-existing
- [x] [Review][Defer] `initialize_device` uses `create_dir` (not `create_dir_all`) — protected by path validation today, but latent bug if validation is relaxed [`device/mod.rs:435`] — deferred, pre-existing

## Change Log

- 2026-05-01: Implemented Story 4.0 — Device IO Abstraction Layer. Created `device_io.rs` with `DeviceIO` trait, `MscBackend`, `MtpBackend`; refactored `DeviceManager`, `execute_sync`, `scrobbler`, `rpc` callers. 171 tests passing.
