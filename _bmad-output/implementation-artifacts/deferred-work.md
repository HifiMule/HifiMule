# Deferred Work

## Deferred from: code review of 2-6-initialize-new-device-manifest (2026-05-01)

- **Partial failure / task cancellation between cleanup steps** — `initialize_device` clears `unrecognized_device_path` and `unrecognized_device_io` in separate lock scopes; a panic or cancellation between them leaves `unrecognized_device_io` stale until the next device event. Theoretical; no production impact in practice.
- **Path-aliasing window during backend construction** — `MscBackend::new(path)` is created before `connected_devices.remove(&path)` runs in `handle_device_unrecognized`; a re-probed path could briefly have two live IO backends. Negligible window; mention if race hardening is done.
- **No concurrent stress tests for path/IO invariant** — `unrecognized_device_path` and `unrecognized_device_io` must always be set/cleared together; only sequential tests cover this. Add concurrent event tests if a multi-device race bug surfaces.

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
