# Deferred Work

## Deferred from: code review of 6-3-macos-installer-dmg (2026-04-05)

- **beforeBuildCommand CWD assumption unverified cross-platform** — `npm run build && node ../scripts/prepare-sidecar.mjs` verified on macOS with npm/npx tauri invocation; untested with `cargo tauri` (which may use workspace root as CWD) on Windows/Linux. Verify in stories 6.4 and 6.5 CI setup.
- **minimumSystemVersion compatibility risk** — Setting `10.15` is correct for Tauri v2 minimum, but if any future dependency raises the floor this config will silently advertise false compatibility. Monitor when adding macOS-specific deps.
- **prepare-sidecar.mjs mid-execution failure leaves partial state** — No atomic swap or rollback. If `copyFileSync` fails after `cargo build --release`, stale binary remains in `sidecars/`. Pre-existing in `scripts/prepare-sidecar.mjs`.
- **Stale sidecar binaries from other architectures never cleaned** — `sidecars/` accumulates binaries from prior builds on different architectures. Pre-existing in `scripts/prepare-sidecar.mjs`.
- **execSync error propagation unreliable in prepare-sidecar.mjs** — Uncaught exceptions from `rustc -vV` or `cargo build` may not correctly fail the build chain. Pre-existing.
- **npm dependencies not pre-checked before sidecar script runs** — Fresh clone or CI without prior `npm install` will fail at `npm run build`. Pre-existing.

## Deferred from: code review of 6-4-linux-packages-appimage-deb (2026-04-06)

- **No unit tests for boot-volume exclusion guard** — `get_mounts` on macOS `/Volumes` has no test coverage for the new `canonicalize`-based root check. Requires platform-specific filesystem mocking to implement. [`jellyfinsync-daemon/src/device/mod.rs:968-975`]
- **TOCTOU race between `canonicalize` and `is_mount_point` in `get_mounts`** — Pre-existing pattern in the function; a volume could be remounted between the two sequential filesystem calls. Narrow window in practice but real under rapid device changes.
- **`known_mounts` may retain boot-volume path from pre-fix binary on hot-reload** — If the daemon was running before the fix was deployed, the boot volume may already be in `known_mounts` and is never re-evaluated by the new guard. Only affects in-place upgrades without daemon restart.
- **AC2/AC4/AC5: .deb not functionally installed or launched** — `sudo dpkg -i` install + app-menu launch + `localhost:19140` health-check not executed due to `sudo` unavailability during dev. Structural package inspection accepted for MVP. Cover in story 6.6 smoke tests.
