# Story 4.0: Device IO Abstraction Layer

Status: ready-for-dev

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

- [ ] **T1: Create DeviceIO trait** (AC: #1)
  - [ ] T1.1: Create `jellyfinsync-daemon/src/device_io.rs` — new file
  - [ ] T1.2: Add dependency `async-trait = "0.1"` to `jellyfinsync-daemon/Cargo.toml` (required for `async fn` in `dyn Trait` — see Dev Notes)
  - [ ] T1.3: Define `FileEntry` struct: `pub path: String, pub name: String, pub size: u64` with `#[serde(rename_all = "camelCase")]`
  - [ ] T1.4: Define `DeviceIO` trait with `#[async_trait]` and all six methods: `read_file`, `write_file`, `write_with_verify`, `delete_file`, `list_files`, `free_space`
  - [ ] T1.5: Add `pub mod device_io;` to `jellyfinsync-daemon/src/main.rs`

- [ ] **T2: Implement MscBackend** (AC: #2)
  - [ ] T2.1: Implement `MscBackend { root: PathBuf }` in `device_io.rs`
  - [ ] T2.2: `read_file(path)` → `tokio::fs::read(self.root.join(path))`
  - [ ] T2.3: `write_file(path, data)` → create parent dirs with `tokio::fs::create_dir_all`, then `tokio::fs::write`
  - [ ] T2.4: `write_with_verify(path, data)` → write to `{path}.tmp`, call `file.sync_all().await`, then `tokio::fs::rename` — mirror the existing `write_manifest` pattern exactly
  - [ ] T2.5: `delete_file(path)` → `tokio::fs::remove_file(self.root.join(path))`
  - [ ] T2.6: `list_files(path)` → recursive async directory walk (mirror `cleanup_tmp_files` traversal pattern in `device/mod.rs`), returning `Vec<FileEntry>` with relative-to-root paths
  - [ ] T2.7: `free_space()` → call the existing `get_storage_info(&self.root)` from `device/mod.rs` and extract `free_bytes`

- [ ] **T3: Add MTP Cargo.toml dependencies** (AC: #3)
  - [ ] T3.1: Add `[target.'cfg(windows)'.dependencies]` entry: `windows = { version = "0.58", features = ["Devices_Portable", "Win32_Devices_PortableDevices", "Win32_System_Com"] }` (note: separate from existing `windows-sys`)
  - [ ] T3.2: Add `[target.'cfg(unix)'.dependencies]` entry: `libmtp-rs = "0.3"` (wraps `libmtp` C library; requires `libmtp-dev` system package on Linux, `libmtp` via Homebrew on macOS)
  - [ ] T3.3: Add `[build-dependencies]` entry for `pkg-config = "0.3"` on Unix to locate `libmtp` headers

- [ ] **T4: Implement MtpBackend** (AC: #3, #4)
  - [ ] T4.1: Define `MtpHandle` as a platform-gated type alias or struct in `device_io.rs`: `#[cfg(windows)] type MtpHandle = ...IPortableDevice handle...`, `#[cfg(unix)] type MtpHandle = libmtp_rs::MtpDevice`
  - [ ] T4.2: Implement `MtpBackend { handle: Arc<MtpHandle> }` with `#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]`
  - [ ] T4.3: `write_file()` → Windows: create WPD object via `IPortableDeviceContent::CreateObjectWithPropertiesAndData`; Linux/macOS: `libmtp_rs send_track_from_handler` or equivalent memory-buffer send
  - [ ] T4.4: `write_with_verify()` → (1) write `".dirty"` marker via `write_file()`, (2) write actual target via `write_file()`, (3) delete marker via `delete_file()`
  - [ ] T4.5: `read_file()` → look up object by path, retrieve data; Windows: `IPortableDeviceContent::Transfer`; Linux/macOS: `libmtp_rs get_file_to_handler`
  - [ ] T4.6: `list_files()` → enumerate storage objects; Windows: `IPortableDeviceContent::EnumObjects`; Linux/macOS: `libmtp_rs get_filelisting`
  - [ ] T4.7: `delete_file()` → Windows: `IPortableDeviceContent::Delete`; Linux/macOS: `libmtp_rs delete_object`
  - [ ] T4.8: `free_space()` → Windows: `IPortableDeviceCapabilities` storage info; Linux/macOS: `libmtp_rs::MtpDevice::get_storageinfo`

- [ ] **T5: Integrate DeviceIO into DeviceManager** (AC: #1, #4)
  - [ ] T5.1: Define `struct ConnectedDevice { pub manifest: DeviceManifest, pub device_io: Arc<dyn DeviceIO> }` in `device/mod.rs`
  - [ ] T5.2: Change `connected_devices: Arc<RwLock<HashMap<PathBuf, DeviceManifest>>>` to `HashMap<PathBuf, ConnectedDevice>` — update all read/write sites in `DeviceManager`
  - [ ] T5.3: In `handle_device_detected()`, instantiate `MscBackend { root: path.clone() }` and store as `Arc<dyn DeviceIO>` in `ConnectedDevice` — **for now always MSC** (MTP detection arrives in Story 2.10)
  - [ ] T5.4: Add `pub async fn get_device_io(&self) -> Option<Arc<dyn DeviceIO>>` to `DeviceManager`
  - [ ] T5.5: Refactor `write_manifest()` standalone function: change signature to `write_manifest(device_io: Arc<dyn DeviceIO>, manifest: &DeviceManifest)` — body calls `device_io.write_with_verify(".jellyfinsync.json", &json_bytes).await`
  - [ ] T5.6: Update `DeviceManager::update_manifest()` to retrieve device_io from `ConnectedDevice` and pass to refactored `write_manifest()`
  - [ ] T5.7: Update `DeviceManager::initialize_device()` — creates `MscBackend` locally (device not in map yet), uses it to call `write_manifest()`, then stores in map
  - [ ] T5.8: Refactor `cleanup_tmp_files()` to accept `device_io: Arc<dyn DeviceIO>` instead of `device_root: &Path` — use `device_io.list_files(path)` to enumerate and `device_io.delete_file(path)` for `.tmp` files
  - [ ] T5.9: On MTP device reconnect: call `device_io.list_files("")` and scan for `".dirty"` marker in root — if found, fire `on_device_dirty` event

- [ ] **T6: Refactor sync.rs** (AC: #5)
  - [ ] T6.1: Add `device_io: Arc<dyn DeviceIO>` parameter to `execute_sync()` signature
  - [ ] T6.2: Replace every `tokio::fs::write` / `tokio::fs::File::create` targeting a device path with `device_io.write_with_verify(relative_path, &buffer).await`
  - [ ] T6.3: Replace every `tokio::fs::remove_file` targeting a device path with `device_io.delete_file(relative_path).await`
  - [ ] T6.4: Replace every `tokio::fs::create_dir_all` targeting a device path with the new `MscBackend::write_file` parent-dir creation (handled internally by the backend)
  - [ ] T6.5: Ensure path arguments to DeviceIO methods are RELATIVE to the device root (e.g., `"Music/Artist/Album/track.mp3"`), not absolute

- [ ] **T7: Refactor RPC and daemon callers** (AC: #5)
  - [ ] T7.1: In `rpc.rs`, the `sync.start` handler: call `state.device_manager.get_device_io().await` and pass to `execute_sync()`
  - [ ] T7.2: In `main.rs`, the `run_auto_sync` function: same — retrieve `device_io` from `DeviceManager` and pass to `execute_sync()`
  - [ ] T7.3: Verify no other callers remain using device paths with `std::fs` or `tokio::fs`

- [ ] **T8: Refactor scrobbler.rs** (AC: #5)
  - [ ] T8.1: Update `scrobbler.rs` — replace direct `tokio::fs::read_to_string(device_path.join(".scrobbler.log"))` with `device_io.read_file(".scrobbler.log").await`, then `String::from_utf8(bytes)`
  - [ ] T8.2: Update the scrobbler caller in `rpc.rs` to retrieve and pass `device_io`

- [ ] **T9: Testing** (AC: #1-#7)
  - [ ] T9.1: Unit tests for `MscBackend` using `tempfile::tempdir()`: read/write/delete/list/write_with_verify (verify Write-Temp-Rename atomicity)
  - [ ] T9.2: Unit tests for `MtpBackend::write_with_verify` dirty-marker logic with a mock `MtpHandle`
  - [ ] T9.3: Unit test for dirty marker detection on reconnect (mock device_io returning a `".dirty"` file in listing)
  - [ ] T9.4: Integration test: `execute_sync()` with a `MockDeviceIO` recording calls, verifying correct relative paths and write_with_verify usage
  - [ ] T9.5: Verify all pre-existing tests pass: `cargo test` — 0 regressions

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

_to be filled by dev agent_

### Debug Log References

_to be filled by dev agent_

### Completion Notes List

_to be filled by dev agent_

### File List

_to be filled by dev agent_
