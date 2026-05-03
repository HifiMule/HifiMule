# Deferred Work

## Deferred from: code review of fix-mtp-write-shell-copy (2026-05-03)

- **`IShellFolder::EnumObjects` may fail from MTA on non-free-threaded Shell namespace extensions** — `portabl.dll` is likely free-threaded (it wraps WPD which is MTA), but this is untested on non-Garmin MTP devices. If MTA threading causes issues, replace `CoInitGuard::init()` (COINIT_MULTITHREADED) in `shell_copy_to_device` with a dedicated `std::thread::spawn` STA thread. [`mtp.rs`, `shell_copy_to_device`]
- **Shell copy performance for bulk music sync** — `write_file` now opens a WPD session, drops it, then opens a new Shell session per file; for syncing many files, this two-phase teardown+reconnect adds overhead. Consider batching Shell operations or combining dir-creation and file-copy into a single Shell pass when sync performance is measured. [`mtp.rs`, `write_file`, `shell_copy_to_device`]
- **Temp file name collision on concurrent writes** — `jellyfinsync_{nanos}.tmp` uses SystemTime nanoseconds; theoretically non-unique if two write_file calls start in the same nanosecond on the same `spawn_blocking` thread. Low risk in practice (writes are sequential per device); replace with `uuid` or `tempfile` crate if concurrent writes are added. [`mtp.rs`, `write_file`]
- **`has_msc_drive_for_device` uses volume label exact match (case-insensitive)** — if two devices share a volume label (e.g., both named "BACKUP"), the first MSC match suppresses MTP registration for all. Low risk in practice; a more robust check would compare WPD device hardware IDs to drive GUIDs via `SetupDi` or `DeviceIoControl`. [`device/mod.rs`, `has_msc_drive_for_device`]
- **MTP-only fallback still writes via Shell even for non-Garmin devices** — WPD failure on any device (transient error, device busy) silently triggers the Shell copy path. If a non-Garmin device consistently fails WPD writes and Shell copy also fails, the error message shows the Shell error, not the root WPD cause. Consider logging the WPD error before the fallback even when Shell succeeds. [`mtp.rs`, `write_file`]

## Deferred from: code review of fix-mtp-device-init-ui (2026-05-02)

- **Three separate `RwLock`s for `unrecognized_device_{path,io,friendly_name}` are not truly atomic** — sequential `.write().await` acquisitions leave a window where a reader sees partially-written state. Consolidate into a single `RwLock<Option<UnrecognizedDeviceState>>` struct in a future `DeviceManager` refactor. [`device/mod.rs`, `handle_device_unrecognized`]
- **`unmanaged_count` is always 0 for MTP devices** — MTP cannot enumerate real device folders; the constraint is intentional but means the UI can never surface unmanaged MTP folders or prompt to add new managed paths. Needs a richer folder-enumeration strategy (e.g., `list_files` with directory detection) when MTP folder management is desired.
- **MTP `friendly_name` not pre-filled in `InitDeviceModal`** — The Garmin's friendly name (e.g., "Garmin Forerunner 945") is stored in `unrecognized_device_friendly_name` but the init modal always defaults to "My Device". Wire `pendingDeviceFriendlyName` through `get_daemon_state` RPC to pre-fill the device name input. [`rpc.rs`, `get_daemon_state`; `BasketSidebar.ts`, `openInitDeviceModal`]
- **Empty-string `manifest.name` bypasses `friendly_name` fallback** — `manifest.as_ref().and_then(|m| m.name.clone())` returns `Some("")` for blank manifest names, using the empty string as `device_name`. Add `.filter(|n| !n.is_empty())` before the fallback chain.

## Deferred from: code review of fix-mtp-wpd-com-threading (2026-05-02)

- **`known_ids` inserted before async manifest probe completes** — in `run_mtp_observer`, `known_ids.insert(dev_id.clone())` at line 1117 runs before the `read_file` probe and `DeviceEvent` send; a transient probe failure permanently suppresses re-detection for that device ID until physical reconnect. Pre-existing design: the guard against duplicate-detection loops requires early insertion. [`device/mod.rs`, `run_mtp_observer:1117,1120`]

## Deferred from: code review of fix-wpd-mtp-io-operations (2026-05-02)

- **Multi-storage device: `ensure_dir_chain` and `free_space` always use first storage child** — devices with both internal and SD-card storage will always resolve paths and free space against whichever storage object is enumerated first. Requires a storage-selection mechanism or explicit storage-ID tracking. [`mtp.rs`, `ensure_dir_chain`, `free_space`]
- **`write_file` delete-then-create is not atomic** — if `CreateObjectWithPropertiesAndData` or write fails after delete, the old file is destroyed with no rollback. Pre-existing design pattern; mitigation is the dirty-marker strategy in `write_with_verify`. [`mtp.rs`, `write_file`]
- **TOCTOU in `ensure_dir_chain` folder creation** — `find_child_object_id → None → CreateObjectWithPropertiesOnly` race with another process; creates a hard error instead of tolerating the duplicate. Low risk in practice. [`mtp.rs`, `ensure_dir_chain`]
- **`collect_files_recursive` swallows subdirectory enumeration errors** — a directory that fails to enumerate is silently skipped, producing an incomplete file list. Pre-existing tolerance decision; surfacing these errors would require a structured warning mechanism. [`mtp.rs`, `collect_files_recursive`]

## Deferred from: code review of 2-10-mtp-device-detection (2026-05-02)

- **`broadcast_device_state` re-triggers `handle_device_detected` on already-connected device** — can cause duplicate insertion logic and spurious dirty-state transitions; signature changed in this diff but behavior is pre-existing. [`rpc.rs`, `broadcast_device_state`]
- **`storage_id=0` in `LIBMTP_Get_Files_And_Folders`** — may fail on devices with non-default storage IDs; needs libmtp API verification of the `0` "all storages" semantics. [`mtp.rs`, libmtp module]
- **No unit test for `path_to_object_id` traversal logic with fixture data** — FFI-dependent traversal logic is untestable without a mock device handle; `split_path_components` tests cover the extractable path-parsing logic only. [`mtp.rs`]
- **`get_device_storage` returns `None` for MTP devices** — synthetic `mtp://` path fails all platform filesystem space queries; `free_space()` is also stubbed on all MTP handles; storage capacity silently absent in UI for MTP devices. [`device/mod.rs`, `get_device_storage`]
- **Stale IO handle in `initialize_device` after device reconnect** — `Arc<dyn DeviceIO>` stored from the original `Unrecognized` event may refer to a disconnected handle if the physical device was removed and reinserted between detection and the user completing initialization. [`device/mod.rs`, `initialize_device`]

## Deferred from: code review of 2-6-initialize-new-device-manifest (2026-05-01)

- **Partial failure / task cancellation between cleanup steps** — `initialize_device` clears `unrecognized_device_path` and `unrecognized_device_io` in separate lock scopes; a panic or cancellation between them leaves `unrecognized_device_io` stale until the next device event. Theoretical; no production impact in practice.
- **Path-aliasing window during backend construction** — `MscBackend::new(path)` is created before `connected_devices.remove(&path)` runs in `handle_device_unrecognized`; a re-probed path could briefly have two live IO backends. Negligible window; mention if race hardening is done.
- **No concurrent stress tests for path/IO invariant** — `unrecognized_device_path` and `unrecognized_device_io` must always be set/cleared together; only sequential tests cover this. Add concurrent event tests if a multi-device race bug surfaces.

## Deferred from: code review of fix-mtp-write-not-ready (2026-05-03)

- **Double `Content()` acquisition in `read_file`, `delete_file`, `list_files`** — `path_to_object_id` calls `device.Content()` internally, then each method body calls `device.Content()` again; two `IPortableDeviceContent` references coexist within one session. `write_file` was fixed to use a single handle; the remaining three methods were not in scope. Refactor `path_to_object_id` to accept `&IPortableDeviceContent` to eliminate the pattern. [`mtp.rs`, `path_to_object_id`, `read_file`, `delete_file`, `list_files`]
- **`IStream::Write` with `.ok()?` silently discards S_FALSE partial-write warnings** — `S_FALSE` is a success HRESULT but signals fewer bytes written; `HRESULT::ok()` returns `Ok(())` for it, so the stall guard (written == 0) is the only check. On a slow driver this could mask degraded throughput indefinitely. Replace with explicit HRESULT comparison or use a wrapper that maps `S_FALSE` to `Err`. [`mtp.rs`, `write_file` stream write loop]
- **`ensure_dir_chain` leaks `new_id_pwstr` when `PWSTR::to_string()` fails** — after `CreateObjectWithPropertiesOnly` populates `new_id_pwstr`, a `?`-propagated error from `to_string()` skips `CoTaskMemFree`. Pre-existing; fix with a `scopeguard` or inline free before the `?`. [`mtp.rs`, `ensure_dir_chain`]
- **Dirty-marker unit test does not assert 1-byte sentinel content** — `mtp_write_with_verify_dirty_marker_sequence` checks call order and path but not that the sentinel data is `b"\x00"`. A regression back to `b""` would not be caught. Add a data-content assertion in the mock test. [`device_io.rs`, `mtp_write_with_verify_dirty_marker_sequence`]

## Deferred from: code review of 4-0-device-io-abstraction-layer (2026-05-01)

- **`update_manifest` TOCTOU** — `selected_device_path` and `connected_devices` are acquired under separate locks; device disconnect between the two reads causes `get_mut` to return `None` and partial manifest updates to be silently dropped. Pre-existing lock ordering; fix in a future DeviceManager refactor.
- **MTP scrobbler `not-found` detection broken** — `downcast_ref::<std::io::Error>()` fails for plain `anyhow` errors returned by `MtpHandle::read_file`; MTP "no scrobbler log" is treated as a read error. No production impact until Story 2.10 wires up MTP detection.
- **Potential deadlock via lock-order inversion** — `get_multi_device_snapshot` acquires `connected_devices.read` then `selected_device_path.read`; `update_manifest` acquires them in reverse order; a concurrent `select_device` waiting for `selected_device_path.write` can create a 3-thread circular wait. Pre-existing pattern; needs a single combined lock or a consistent acquisition order.
- **`handle_device_removed` does not auto-reselect when 2+ devices remain** — after removing the selected device, `selected_device_path` is set to `None` even when other devices are still connected. Pre-existing UX issue; users must manually re-select.
- **`MtpBackend` concurrent `spawn_blocking` ordering not guaranteed** — MTP is stateful; concurrent IO operations dispatched as independent `spawn_blocking` tasks can interleave. No production impact until Story 2.10; add a per-device serialization mechanism then.
- **`cleanup_tmp_files` skips root-level `.tmp` cleanup when `managed_paths` is empty** — pre-existing behavior; any `.tmp` files written outside `managed_paths` (e.g., a failed manifest write at the device root) are never cleaned up.
- **`initialize_device` uses `create_dir` (not `create_dir_all`)** — safe today because path validation rejects nested paths; becomes a latent bug if validation is ever relaxed to allow multi-level managed paths.

## Deferred from: code review of 6-3-macos-installer-dmg (2026-04-05)

- **beforeBuildCommand CWD assumption unverified cross-platform** — `npm run build && node ../scripts/prepare-sidecar.mjs` verified on macOS with npm/npx tauri invocation; untested with `cargo tauri` (which may use workspace root as CWD) on Windows/Linux. Verify in stories 6.4 and 6.5 CI setup.
- **minimumSystemVersion compatibility risk** — Setting `10.15` is correct for Tauri v2 minimum, but if any future dependency raises the floor this config will silently advertise false compatibility. Monitor when adding macOS-specific deps.
- **prepare-sidecar.mjs mid-execution failure leaves partial state** — No atomic swap or rollback. If `copyFileSync` fails after `cargo build --release`, stale binary remains in `sidecars/`. Pre-existing in `scripts/prepare-sidecar.mjs`.
- **Stale sidecar binaries from other architectures never cleaned** — `sidecars/` accumulates binaries from prior builds on different architectures. Pre-existing in `scripts/prepare-sidecar.mjs`.
- **execSync error propagation unreliable in prepare-sidecar.mjs** — Uncaught exceptions from `rustc -vV` or `cargo build` may not correctly fail the build chain. Pre-existing.
- **npm dependencies not pre-checked before sidecar script runs** — Fresh clone or CI without prior `npm install` will fail at `npm run build`. Pre-existing.

## Deferred from: code review of 6-5-cicd-cross-platform-build-pipeline (2026-04-06)

- **`pnpm/action-setup@v4` uses `version: latest`** — Non-deterministic; a new pnpm major could silently change lockfile format or behavior. Pin to a specific version when hardening for reproducible releases. [`.github/workflows/release.yml:34`]
- **`node-version: lts/*` is floating** — Node LTS promotions could introduce breaking changes across releases. Pin to `20` when hardening. [`.github/workflows/release.yml:40`]
- **`tauri-apps/tauri-action@v0` is a floating major-version tag** — Supply chain risk; a force-push to `@v0` could inject code into released artifacts. Pin to a commit SHA for production hardening. [`.github/workflows/release.yml:87`]
- **`rustc -vV` parsing in `prepare-sidecar.mjs` is fragile** — The `.split(": ")[1]?.trim()` extraction could return `undefined` if Rust output format changes. Pre-existing in `scripts/prepare-sidecar.mjs`, not introduced by this story.

## Deferred from: code review of 6-4-linux-packages-appimage-deb (2026-04-06)

- **No unit tests for boot-volume exclusion guard** — `get_mounts` on macOS `/Volumes` has no test coverage for the new `canonicalize`-based root check. Requires platform-specific filesystem mocking to implement. [`jellyfinsync-daemon/src/device/mod.rs:968-975`]
- **TOCTOU race between `canonicalize` and `is_mount_point` in `get_mounts`** — Pre-existing pattern in the function; a volume could be remounted between the two sequential filesystem calls. Narrow window in practice but real under rapid device changes.
- **`known_mounts` may retain boot-volume path from pre-fix binary on hot-reload** — If the daemon was running before the fix was deployed, the boot volume may already be in `known_mounts` and is never re-evaluated by the new guard. Only affects in-place upgrades without daemon restart.
- **AC2/AC4/AC5: .deb not functionally installed or launched** — `sudo dpkg -i` install + app-menu launch + `localhost:19140` health-check not executed due to `sudo` unavailability during dev. Structural package inspection accepted for MVP. Cover in story 6.6 smoke tests.

## Deferred from: code review of 6-6-installation-smoke-tests (2026-04-06)

- **Xvfb `:99` port collision** — If another Xvfb is running on `:99` on a shared runner, Xvfb starts but the display is wrong. Ephemeral CI runners make this low-risk; address if flakiness observed.
- **Windows install dir hardcoded to `C:\Program Files\JellyfinSync`** — WiX MSI default; valid for current build. If NSIS targets added or `INSTALLDIR` customized, exe search will fail.
- **macOS DMG `.app` name assumed to match `APP_NAME`** — `cp "${MOUNT_POINT}/JellyfinSync.app"` hard-assumes the bundle name. Controlled by tauri.conf.json; revisit if productName changes.
- **No automated post-release trigger** — workflow is `workflow_dispatch` only by spec design. Consider adding `workflow_call` once release pipeline matures.
