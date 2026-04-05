# Deferred Work

## Deferred from: code review of 6-3-macos-installer-dmg (2026-04-05)

- **beforeBuildCommand CWD assumption unverified cross-platform** — `npm run build && node ../scripts/prepare-sidecar.mjs` verified on macOS with npm/npx tauri invocation; untested with `cargo tauri` (which may use workspace root as CWD) on Windows/Linux. Verify in stories 6.4 and 6.5 CI setup.
- **minimumSystemVersion compatibility risk** — Setting `10.15` is correct for Tauri v2 minimum, but if any future dependency raises the floor this config will silently advertise false compatibility. Monitor when adding macOS-specific deps.
- **prepare-sidecar.mjs mid-execution failure leaves partial state** — No atomic swap or rollback. If `copyFileSync` fails after `cargo build --release`, stale binary remains in `sidecars/`. Pre-existing in `scripts/prepare-sidecar.mjs`.
- **Stale sidecar binaries from other architectures never cleaned** — `sidecars/` accumulates binaries from prior builds on different architectures. Pre-existing in `scripts/prepare-sidecar.mjs`.
- **execSync error propagation unreliable in prepare-sidecar.mjs** — Uncaught exceptions from `rustc -vV` or `cargo build` may not correctly fail the build chain. Pre-existing.
- **npm dependencies not pre-checked before sidecar script runs** — Fresh clone or CI without prior `npm install` will fail at `npm run build`. Pre-existing.
