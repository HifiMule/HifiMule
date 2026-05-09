# Story 6.1: Tauri Bundler Configuration & Sidecar Packaging

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a **System Admin (Alexis)**,
I want the Tauri bundler configured to include the `hifimule-daemon` binary as a sidecar,
so that a single installer delivers both the UI and the headless engine as a cohesive application.

## Acceptance Criteria

1. **Single installer output**: Given the Cargo workspace with both crates built, when I run `cargo tauri build`, then the output produces a platform-native installer containing both the Tauri UI and the daemon sidecar.
2. **Daemon launchable from sidecar path**: The installed application can launch the daemon from the bundled sidecar path using the Tauri sidecar API.
3. **Correct app metadata**: The application icon, name ("HifiMule"), and metadata are correctly embedded in the installer.
4. **Cross-platform sidecar naming**: The sidecar binary is correctly named per Tauri's platform-specific triple convention (e.g., `hifimule-daemon-x86_64-pc-windows-msvc.exe` on Windows).
5. **Build succeeds on current dev platform**: `cargo tauri build` completes without errors on the developer's OS and produces a valid installer artifact.

## Tasks / Subtasks

- [x] **T1: Configure Tauri Sidecar in `tauri.conf.json`** (AC: #1, #2, #4)
  - [x] T1.1: Add `bundle.externalBin` array pointing to the daemon binary path with Tauri's target-triple placeholder (e.g., `"../target/release/hifimule-daemon"`)
  - [x] T1.2: Verify the sidecar binary name follows Tauri v2's naming convention: `<binary-name>-<target-triple>[.exe]`
- [x] **T2: Update App Metadata in `tauri.conf.json`** (AC: #3)
  - [x] T2.1: Change `productName` from `"hifimule-ui"` to `"HifiMule"`
  - [x] T2.2: Update `identifier` from `"hifimule.github.io-ui"` to `"hifimule.github.io"`
  - [x] T2.3: Verify icon paths in `bundle.icon` array resolve correctly
- [x] **T3: Update UI Daemon Launch to Use Sidecar API** (AC: #2)
  - [x] T3.1: Replace any direct `Command::new()` or manual process spawn of the daemon with Tauri's `tauri::api::process::Command::new_sidecar("hifimule-daemon")` (or equivalent Tauri v2 sidecar API)
  - [x] T3.2: Ensure the daemon launch handles sidecar path resolution in both dev and bundled modes
  - [x] T3.3: Add the `shell > sidecar` permission in Tauri v2 capabilities if required
- [x] **T4: Add Build Script or Pre-build Step** (AC: #1, #4)
  - [x] T4.1: Ensure `cargo build --release -p hifimule-daemon` runs before `cargo tauri build` (via `beforeBuildCommand` or a wrapper script)
  - [x] T4.2: Add a script/step to copy/rename the daemon binary to the expected sidecar name with target-triple suffix
- [x] **T5: Validate Build Output** (AC: #1, #3, #5)
  - [x] T5.1: Run `cargo tauri build` and verify the installer is produced
  - [x] T5.2: Install and verify the daemon sidecar is present alongside the main executable
  - [x] T5.3: Launch the installed app and verify the daemon starts from the sidecar path

## Dev Notes

### Architecture & Technical Requirements

- **Tauri v2 Sidecar:** Tauri v2 bundles external binaries via `bundle.externalBin` in `tauri.conf.json`. Each entry is a path to the binary (without extension). Tauri appends the target triple and `.exe` on Windows automatically.
- **Binary naming convention:** The daemon binary MUST be named `hifimule-daemon-{target-triple}` (e.g., `hifimule-daemon-x86_64-pc-windows-msvc.exe`) for Tauri to find it at build time. This is a **hard requirement** — Tauri will fail to bundle if the binary doesn't match the expected name.
- **Sidecar launch in Tauri v2:** Use `tauri_plugin_shell::ShellExt` and `app.shell().sidecar("hifimule-daemon")` to spawn the sidecar process. This requires adding `tauri-plugin-shell` to the UI crate dependencies with the `sidecar` Cargo feature.
- **Permissions:** Tauri v2 uses a capability-based permission system. Sidecar execution requires `shell:allow-execute` and `shell:allow-spawn` permissions in the app's capabilities config (`src-tauri/capabilities/`).
- **IPC unchanged:** The daemon still communicates over JSON-RPC on localhost HTTP. The sidecar is just a packaging/launch mechanism — no protocol changes needed.

### Current State Analysis

- `tauri.conf.json` currently has NO sidecar configuration — `bundle.externalBin` is absent
- `productName` is `"hifimule-ui"` — must become `"HifiMule"` for proper installer naming
- `identifier` is `"hifimule.github.io-ui"` — should be `"hifimule.github.io"`
- The daemon binary name is `hifimule-daemon` (defined in `hifimule-daemon/Cargo.toml` `[[bin]]`)
- Current `beforeBuildCommand` only runs `npm run build` (frontend) — needs to also build the daemon binary
- The UI Cargo.toml (`hifimule-ui/src-tauri/Cargo.toml`) does NOT include `tauri-plugin-shell` — it needs to be added with the `sidecar` feature

### Key Files to Modify

| File | Change |
|------|--------|
| `hifimule-ui/src-tauri/tauri.conf.json` | Add `bundle.externalBin`, update `productName`, `identifier` |
| `hifimule-ui/src-tauri/Cargo.toml` | Add `tauri-plugin-shell` with `sidecar` feature |
| `hifimule-ui/src-tauri/src/lib.rs` | Register `tauri_plugin_shell::init()` plugin |
| `hifimule-ui/src-tauri/capabilities/*.json` | Add shell/sidecar permissions |
| `hifimule-ui/src/main.ts` (or daemon launch code) | Use sidecar API to spawn daemon |
| Build scripts / `beforeBuildCommand` | Add daemon build + rename step |

### Existing Patterns to Follow

- **Workspace structure:** `Cargo.toml` at root defines workspace members `hifimule-daemon` and `hifimule-ui/src-tauri`
- **Release profile:** Already configured with `lto = true`, `strip = true`, `opt-level = "z"` for size optimization
- **UI dependencies pattern:** Tauri plugins are listed as `tauri-plugin-xxx = "2"` in `[dependencies]`
- **Error handling:** `anyhow` at binary level, `thiserror` for library errors

### What NOT to Do

- Do NOT change the JSON-RPC IPC mechanism — sidecar is packaging only
- Do NOT modify the daemon's `main.rs` or its runtime behavior
- Do NOT hardcode platform-specific paths — use Tauri's sidecar resolution
- Do NOT remove `tray-icon` or `tao` from the daemon — the daemon manages its own tray icon independently
- Do NOT add `tauri-plugin-shell` to the daemon crate — only the UI crate needs it

### Previous Epic Learnings (Epic 5)

- All 102 tests pass — do not introduce regressions
- `ts-rs` used for TypeScript interface generation from Rust structs
- Atomic Write-Temp-Rename pattern for manifest operations
- RPC handlers in `rpc.rs` return success/error envelopes
- UI components use Shoelace web components

### Project Structure Notes

- Workspace: `hifimule-daemon` (standalone Rust binary) + `hifimule-ui/src-tauri` (Tauri Rust backend)
- Frontend: `hifimule-ui/src/` (Vanilla TypeScript + Shoelace)
- Frontend build: `hifimule-ui/dist/` output
- Icons: `hifimule-ui/src-tauri/icons/`
- Rust edition 2021, MSRV 1.93.0

### References

- [Source: planning-artifacts/epics.md#story-61-tauri-bundler-configuration--sidecar-packaging] — Epic Requirements
- [Source: planning-artifacts/architecture.md#structure-patterns] — Packaging & Distribution patterns
- [Source: planning-artifacts/architecture.md#core-architectural-decisions] — Multi-process architecture decision
- [Source: hifimule-ui/src-tauri/tauri.conf.json] — Current Tauri configuration (no sidecar yet)
- [Source: Cargo.toml] — Workspace members and release profile
- [Source: hifimule-daemon/Cargo.toml] — Daemon binary name definition

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

- `tauri-plugin-shell` does not have a `sidecar` Cargo feature in v2 — sidecar support is built-in. Removed the feature flag after initial build failure.
- `@tauri-apps/api` npm package needed updating from 2.9.1 to match tauri crate 2.10.3 before `cargo tauri build` would proceed.

### Completion Notes List

- **T1:** Added `bundle.externalBin: ["sidecars/hifimule-daemon"]` to `tauri.conf.json`. Tauri automatically appends the target triple and `.exe` suffix.
- **T2:** Updated `productName` to `"HifiMule"` and `identifier` to `"hifimule.github.io"`. Verified all icon paths resolve correctly.
- **T3:** Added `tauri-plugin-shell` dependency, registered plugin in `lib.rs`, added sidecar spawn in `setup()` hook using `app.shell().sidecar("hifimule-daemon")`. Added `shell:allow-spawn` and `shell:allow-execute` permissions to capabilities.
- **T4:** Created `scripts/prepare-sidecar.mjs` — cross-platform Node.js script that builds the daemon in release mode and copies the binary to `src-tauri/sidecars/` with the correct target-triple naming. Updated `beforeBuildCommand` to run this script after frontend build. Added `sidecars/` to `.gitignore`.
- **T5:** `cargo tauri build` produces both MSI (4.5 MB) and NSIS (3.3 MB) installers. All 122 existing tests pass with zero regressions.

### File List

- `hifimule-ui/src-tauri/tauri.conf.json` — Modified: added `externalBin`, updated `productName`, `identifier`, `beforeBuildCommand`
- `hifimule-ui/src-tauri/Cargo.toml` — Modified: added `tauri-plugin-shell` dependency
- `hifimule-ui/src-tauri/src/lib.rs` — Modified: registered shell plugin, added sidecar launch in `setup()` and properly wrapped in state with exit handling to avoid zombie processes/crashing on startup.
- `hifimule-ui/src-tauri/capabilities/default.json` — Modified: added `shell:allow-spawn`, `shell:allow-execute` permissions
- `hifimule-ui/src-tauri/.gitignore` — Modified: added `/sidecars/` exclusion
- `scripts/prepare-sidecar.mjs` — New: cross-platform sidecar build and copy script, updated for Node < 20.11 compatibility using `fileURLToPath`
- `Cargo.lock` — Modified: automatically updated during build steps
- `hifimule-ui/package-lock.json` — Modified: automatically updated during frontend build steps

### Change Log

- 2026-03-15: Implemented Story 6.1 — Configured Tauri sidecar packaging for hifimule-daemon, updated app metadata, added sidecar launch via shell plugin, created pre-build script for daemon binary preparation. Build produces MSI and NSIS installers. 122 tests pass, zero regressions.
- 2026-03-15: [AI-Review] Handled `.expect()` crashes by matching on sidecar spawn result. Wrapped child process in Tauri managed state for controlled exit. Changed `prepare-sidecar.mjs` `__dirname` to use `fileURLToPath` for broader cross-platform Node.js compatibility. Documented `Cargo.lock` and `package-lock.json` modifications.
