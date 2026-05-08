# Story 7.4: Packaging & CI/CD Hardening

Status: review

## Story

As a **System Admin (Alexis)**,
I want the CI/CD pipeline and installers to be reproducible, supply-chain-safe, and properly declare runtime dependencies,
so that every release artifact is verifiable, installable on clean machines, and not broken by upstream floating versions.

## Acceptance Criteria

1. **Given** the Linux `.deb` package **When** installed on a machine without `libmtp` pre-installed **Then** the package declares `libmtp9` (or the appropriate soname) as a runtime `Depends` entry, so `apt` installs it automatically.

2. **Given** the Linux AppImage **When** built **Then** the `libmtp.so` shared library is bundled inside the AppImage, so it runs on distros without `libmtp` in the system library path.

3. **Given** the macOS DMG **When** distributed to a machine without Homebrew or `libmtp` **Then** the `.app` bundle includes `libmtp.dylib` and its transitive deps via `@rpath` / `otool -L` fixup, so the app launches on a clean macOS install.

4. **Given** `.github/workflows/release.yml` **When** reviewed **Then** `pnpm/action-setup` is pinned to a specific version, `node-version` is pinned to `"20"`, and `tauri-apps/tauri-action` is pinned to a commit SHA.

5. **Given** `scripts/prepare-sidecar.mjs` **When** `copyFileSync` fails mid-execution **Then** any partially-written sidecar is removed before the script exits with a non-zero code.

6. **Given** `scripts/prepare-sidecar.mjs` **When** a build for a new architecture runs **Then** stale sidecar binaries from previous architectures in `sidecars/` are removed before the new copy is written.

7. **Given** `scripts/prepare-sidecar.mjs` **When** `rustc -vV` output format changes **Then** the parsing logic degrades gracefully with a clear error instead of silently producing an `undefined` triple.

8. **Given** a CI runner or fresh clone **When** `prepare-sidecar.mjs` executes **Then** it verifies `node_modules` is populated, or runs the existing project install command before invoking `npm run build`.

9. **Given** `beforeBuildCommand` in `tauri.conf.json` **When** triggered by `cargo tauri build` on Windows and Linux **Then** the command resolves relative paths correctly whether the CWD is the workspace root or `jellyfinsync-ui`.

10. **Given** the boot-volume exclusion guard in `get_mounts` **When** unit tests run **Then** at least one test covers the root-device check with a mocked or factored filesystem path.

11. **Given** the installation smoke tests **When** run in CI **Then** the macOS step discovers the mounted `.app` with `find "${MOUNT_POINT}" -maxdepth 1 -name "*.app"` rather than hardcoding `JellyfinSync.app`.

12. **Given** the smoke test workflow **When** a release is published **Then** the workflow also has a `workflow_call` trigger so it can be invoked programmatically from the release pipeline.

13. **Given** `tauri.conf.json` sets `minimumSystemVersion` **When** a macOS-specific dependency is added that raises the minimum OS floor **Then** `minimumSystemVersion` is updated in the same PR and a CI lint step or PR checklist reminds contributors to verify the value.

14. **Given** the Linux `.deb` package **When** installed via `sudo dpkg -i` on a clean VM and launched from the application menu **Then** the daemon starts and responds to `daemon.health` at `localhost:19140`.

15. **Given** the Xvfb display server is started in the Linux smoke test **When** another process already occupies `:99` **Then** the script auto-selects an available display instead of failing silently with a wrong display.

16. **Given** the Windows smoke test searches for the installed executable **When** the MSI `INSTALLDIR` is customized or a NSIS target is added **Then** the smoke test resolves the install path via the registry rather than hardcoding `C:\Program Files\JellyfinSync`.

## Tasks / Subtasks

- [x] **T1: Declare and verify Linux runtime dependencies** (AC: #1, #14)
  - [x] In `jellyfinsync-ui/src-tauri/tauri.conf.json`, add Linux bundle config for `.deb` runtime deps: `"linux": { "deb": { "depends": ["libmtp9"] } }` under `bundle`.
  - [x] Keep build-time deps in `.github/workflows/release.yml` (`libmtp-dev`, `pkgconf`) separate from runtime deps (`libmtp9`).
  - [x] Update `scripts/smoke-tests/smoke-linux.sh` so the clean install path uses `sudo dpkg -i "$DEB"` followed by `sudo apt-get install -f -y` only for dependency resolution, then launches the installed desktop binary and polls `daemon.health`.
  - [x] Add a CI check or smoke-test assertion that `dpkg-deb -f "$DEB" Depends` contains a `libmtp` runtime package.

- [x] **T2: Bundle `libmtp` into Linux AppImage** (AC: #2)
  - [x] Extend the Linux release step in `.github/workflows/release.yml` to locate `libmtp.so*` after installing `libmtp-dev`.
  - [x] Pass the library to the AppImage packaging path using `linuxdeploy --library` or an equivalent AppDir fixup supported by the Tauri build output.
  - [x] Verify with `ldd` / AppImage extraction that the packaged app resolves `libmtp` from inside the AppImage, not only from `/usr/lib`.

- [x] **T3: Bundle `libmtp.dylib` into macOS `.app`** (AC: #3, #13)
  - [x] Add a macOS post-build fixup step in `.github/workflows/release.yml` or a dedicated script that copies Homebrew `libmtp*.dylib` and transitive dylibs into the `.app` bundle.
  - [x] Use `otool -L` to inspect daemon/UI sidecar linkage and `install_name_tool` or `dylibbundler` to rewrite references to `@rpath` / bundled paths.
  - [x] Preserve the existing universal-build behavior that merges arm64 and x86_64 `libmtp` before packaging.
  - [x] Add a release-step check that fails if `otool -L` still points at a Homebrew path for `libmtp`.
  - [x] Add a CI lint or checklist file reminding maintainers to update `bundle.macOS.minimumSystemVersion` when macOS dependencies raise the floor.

- [x] **T4: Pin release workflow supply-chain inputs** (AC: #4)
  - [x] In `.github/workflows/release.yml`, change `pnpm/action-setup@v4` plus `version: latest` to a specific action version and pnpm version.
  - [x] Change `actions/setup-node` `node-version: lts/*` to `node-version: "20"`.
  - [x] Pin `tauri-apps/tauri-action` to a commit SHA, with a comment naming the upstream version/tag the SHA came from.
  - [x] Do not broaden `GITHUB_TOKEN` permissions beyond `contents: write`.

- [x] **T5: Harden `prepare-sidecar.mjs`** (AC: #5, #6, #7, #8, #9)
  - [x] Replace direct `copyFileSync(sourceBinary, destBinary)` with copy-to-temp plus `renameSync` into the final sidecar path; delete the temp/final partial on failure.
  - [x] Before copying, remove stale `jellyfinsync-daemon-*` sidecars from `jellyfinsync-ui/src-tauri/sidecars`, preserving unrelated files if any are added later.
  - [x] Parse `rustc -vV` with a strict `^host: (\\S+)$` regex; fail with stderr containing the captured `rustc -vV` output when absent.
  - [x] Detect missing `jellyfinsync-ui/node_modules`; run `npm install` in `jellyfinsync-ui` or fail with a clear instruction if package-manager choice is ambiguous.
  - [x] Resolve all paths from `import.meta.url` / repository root so the script works when invoked from either the workspace root or `jellyfinsync-ui`.
  - [x] Update `tauri.conf.json` `build.beforeBuildCommand` to call the script via a path that is valid from `jellyfinsync-ui` (current command is `npm run build && node ../scripts/prepare-sidecar.mjs`).

- [x] **T6: Factor and test macOS boot-volume exclusion** (AC: #10)
  - [x] In `jellyfinsync-daemon/src/device/mod.rs`, factor the macOS root-device decision into a small pure helper that accepts candidate/root device IDs or metadata results.
  - [x] Add tests in `jellyfinsync-daemon/src/device/tests.rs` covering same-device skip, different-device include, and metadata-error fail-safe skip.
  - [x] Preserve current behavior: APFS firmlink-safe device-ID comparison, no direct `canonicalize` dependency.

- [x] **T7: Make smoke tests release-pipeline callable and less brittle** (AC: #11, #12, #15, #16)
  - [x] Add `workflow_call` to `.github/workflows/smoke-test.yml` with `release_tag` as a required string input, while preserving manual `workflow_dispatch`.
  - [x] Update all references from `github.event.inputs.release_tag` to an expression that works for both triggers.
  - [x] Optionally add a release workflow job that calls `./.github/workflows/smoke-test.yml` after artifacts are published to the draft release, passing `${{ github.ref_name }}`.
  - [x] In `scripts/smoke-tests/smoke-macos.sh`, discover the `.app` under the mount point using `find` and derive `APP_NAME` / `APP_PATH` from the result.
  - [x] In `scripts/smoke-tests/smoke-linux.sh`, replace hardcoded `Xvfb :99` with `Xvfb -displayfd` or a deterministic `:99` to `:100` fallback loop.
  - [x] In `scripts/smoke-tests/smoke-windows.ps1`, read install location from registry first, then fall back to common locations. Add/update WiX config to write `HKLM\SOFTWARE\JellyfinSync\InstallDir`.

- [x] **T8: Validate the hardening work** (AC: all)
  - [x] Run `rtk cargo test -p jellyfinsync-daemon`.
  - [x] Run `rtk tsc` or `npm run build` in `jellyfinsync-ui` after script/config edits.
  - [x] Run `node scripts/prepare-sidecar.mjs` from the workspace root and from `jellyfinsync-ui` to prove path handling.
  - [x] If local platform limits prevent full installer verification, document which checks were deferred to GitHub Actions and why.

## Dev Notes

### Current State

- `release.yml` currently builds on `macos-latest`, `ubuntu-22.04`, and `windows-latest`. It installs `pnpm/action-setup@v4` with `version: latest`, uses `actions/setup-node@v4` with `node-version: lts/*`, and uses `tauri-apps/tauri-action@v0`; these are intentionally targeted by AC4.
- Linux release setup installs `libmtp-dev` for build/link, but `tauri.conf.json` has no Linux `.deb` `depends` entry, so clean machines may not receive `libmtp` at install time.
- macOS release setup already builds a universal `libmtp` dylib for the CI build environment, but there is no app-bundle dylib copy/fixup step. Do not remove the existing universal merge; extend it into packaging verification.
- `scripts/prepare-sidecar.mjs` already resolves `projectRoot` from `import.meta.url`, builds `jellyfinsync-daemon`, and copies to `jellyfinsync-ui/src-tauri/sidecars/jellyfinsync-daemon-${targetTriple}`. It does not clean stale sidecars, does not copy atomically, and does not check `node_modules`.
- `tauri.conf.json` currently sets `beforeBuildCommand` to `npm run build && node ../scripts/prepare-sidecar.mjs` and `externalBin` to `sidecars/jellyfinsync-daemon`.
- `smoke-test.yml` currently supports only `workflow_dispatch`. Platform scripts already install, launch, poll `daemon.health`, and uninstall, but Linux hardcodes `DISPLAY=:99`, macOS hardcodes `JellyfinSync.app`, and Windows hardcodes `C:\Program Files\JellyfinSync`.
- `get_mounts` on macOS now excludes the boot volume by comparing device IDs rather than using `canonicalize`; keep that APFS-safe approach while making it testable.

### Architecture & Compliance Guardrails

- Keep the detached Rust daemon + Tauri UI sidecar model. Do not convert the daemon into an always-installed OS service as part of this story.
- Packaging remains Tauri v2 built-in bundling for MSI, DMG, AppImage, and `.deb`.
- Use file-based release diagnostics where possible; release builds do not have dependable stdout/stderr for runtime debugging.
- Do not add Python, Electron, or other runtime-heavy tooling to the shipped app.
- Do not bypass `scripts/prepare-sidecar.mjs` with ad hoc sidecar copies for Windows/Linux; harden the shared script so local and CI builds behave the same.
- Keep all commands in docs/scripts compatible with Windows, macOS, and Linux CI runners. Use platform-specific shell only inside the relevant workflow step or smoke script.

### Latest Technical Notes

- Tauri v2 config supports Linux bundle config under `bundle.linux`, including `deb.depends`, and macOS config under `bundle.macOS.minimumSystemVersion`. Use those schema locations rather than inventing a custom package manifest. [Source: Tauri v2 configuration docs](https://v2.tauri.app/reference/config/)
- GitHub reusable workflows must include `on.workflow_call`; callers invoke same-repo reusable workflows at the job level with `uses: ./.github/workflows/<file>`. [Source: GitHub Actions reusable workflows docs](https://docs.github.com/en/actions/how-tos/reuse-automations/reuse-workflows?learn=getting_started&learnproduct=actions%2F1000)
- GitHub documents commit SHA references as the safest option for stability and security when referencing workflows/actions by ref. Apply that requirement to `tauri-apps/tauri-action`. [Source: GitHub reusable workflow ref guidance](https://docs.github.com/en/actions/how-tos/reuse-automations/reuse-workflows?learn=getting_started&learnProduct=actions)
- `linuxdeploy` supports `--library` / `-l` to bundle a shared `.so` into an AppDir so bundled executables prefer it over a system copy. [Source: AppImage linuxdeploy user guide](https://docs.appimage.org/packaging-guide/from-source/linuxdeploy-user-guide.html)

### Previous Story Intelligence

- Story 7.3 touched `jellyfinsync-daemon/src/device/mod.rs`, `jellyfinsync-daemon/src/device/tests.rs`, `jellyfinsync-daemon/src/rpc.rs`, `jellyfinsync-ui/src/components/BasketSidebar.ts`, and `jellyfinsync-ui/src/components/InitDeviceModal.ts`.
- Story 7.3 added tests around `cleanup_tmp_files`, empty device names, MTP probe retryability, and storage-aware MTP backend creation. Continue this pattern: small focused tests in existing Rust test modules, not broad integration rewrites.
- The Story 7.3 review patched freshly initialized MTP manifests missing `storage_id`. Packaging checks in this story must preserve MTP runtime dependency handling; otherwise the UI can look correct locally while installers fail on clean machines.
- Recent validation baseline from Story 7.3: `rtk cargo test -p jellyfinsync-daemon` passed 198 tests, daemon clippy had 32 pre-existing warnings, and UI TypeScript had no errors. Treat new failures as regressions unless clearly unrelated.

### Project Structure Notes

- Primary files expected to change:
  - `.github/workflows/release.yml`
  - `.github/workflows/smoke-test.yml`
  - `scripts/prepare-sidecar.mjs`
  - `scripts/smoke-tests/smoke-linux.sh`
  - `scripts/smoke-tests/smoke-macos.sh`
  - `scripts/smoke-tests/smoke-windows.ps1`
  - `jellyfinsync-ui/src-tauri/tauri.conf.json`
  - `jellyfinsync-ui/src-tauri/wix/startup-fragment.wxs`
  - `jellyfinsync-daemon/src/device/mod.rs`
  - `jellyfinsync-daemon/src/device/tests.rs`
- Avoid unrelated UI component changes; this story is packaging, CI, installer, and daemon mount-test hardening.

### References

- Story source: `_bmad-output/planning-artifacts/epics.md` — Epic 7, Story 7.4.
- Architecture packaging rules: `_bmad-output/planning-artifacts/architecture.md` — Packaging & Distribution, Logging & Diagnostics, Structure Patterns.
- Prior story context: `_bmad-output/implementation-artifacts/7-3-device-ui-and-identity-polish.md`.
- Current release workflow: `.github/workflows/release.yml`.
- Current smoke workflow and scripts: `.github/workflows/smoke-test.yml`, `scripts/smoke-tests/*`.
- Current Tauri bundler config: `jellyfinsync-ui/src-tauri/tauri.conf.json`.
- Current sidecar staging script: `scripts/prepare-sidecar.mjs`.
- Current WiX fragment: `jellyfinsync-ui/src-tauri/wix/startup-fragment.wxs`.
- Current macOS mount filtering: `jellyfinsync-daemon/src/device/mod.rs`.

## Dev Agent Record

### Agent Model Used

GPT-5

### Debug Log References

- `rtk cargo test -p jellyfinsync-daemon test_boot_volume_device` — 3 boot-volume helper tests passed.
- `rtk cargo test -p jellyfinsync-daemon` — 202 daemon tests passed.
- `rtk cmd /c "set PATH=... && npm.cmd run build"` from `jellyfinsync-ui` — TypeScript and Vite production build passed.
- `rtk cmd /c "set PATH=... && node scripts/prepare-sidecar.mjs"` from repo root — sidecar preparation passed.
- `rtk cmd /c "set PATH=... && node ..\scripts\prepare-sidecar.mjs"` from `jellyfinsync-ui` — sidecar preparation passed outside sandbox after sandboxed cargo hit `.cargo-lock` access denied.
- PowerShell JSON/XML parse checks passed for `tauri.conf.json` and `startup-fragment.wxs`.

### Completion Notes List

- Added Linux `.deb` `libmtp9` runtime dependency and AppImage file bundling config for `libmtp.so.9`.
- Hardened release workflow: pinned Node to `20`, pinned pnpm package-manager version, pinned `tauri-action` to a reviewed commit SHA, kept `GITHUB_TOKEN` scoped to `contents: write`, and added release-to-smoke workflow chaining.
- Added Linux AppImage extraction verification and macOS app-bundle verification so CI fails if `libmtp` is not bundled.
- Added macOS dylib staging/fixup logic that copies Homebrew `libmtp` and transitive Homebrew dylibs, rewrites install names, and verifies no Homebrew `libmtp` path remains in sidecars.
- Hardened `prepare-sidecar.mjs` with strict target-triple parsing, `node_modules` recovery, stale sidecar cleanup, atomic temp-copy/rename, and cwd-independent path resolution.
- Made smoke tests less brittle: Linux verifies `.deb` Depends and auto-selects Xvfb display; macOS discovers the `.app`; Windows resolves install dir from registry with common-location fallback.
- Added WiX registry value for `HKLM\SOFTWARE\JellyfinSync\InstallDir`.
- Factored macOS boot-volume exclusion into a pure helper and added tests for same-device skip, different-device allow, and metadata-error fail-safe skip.
- Local platform limits: full Linux AppImage extraction, macOS dylib `otool` verification, installer smoke tests, and GitHub workflow execution are validated by CI steps added in this story rather than runnable on this Windows workspace.

### File List

- `.github/workflows/release.yml`
- `.github/workflows/smoke-test.yml`
- `jellyfinsync-daemon/src/device/mod.rs`
- `jellyfinsync-daemon/src/device/tests.rs`
- `jellyfinsync-ui/src-tauri/bundled-libs/.gitkeep`
- `jellyfinsync-ui/src-tauri/tauri.conf.json`
- `jellyfinsync-ui/src-tauri/wix/startup-fragment.wxs`
- `scripts/check-macos-minimum-system-version.mjs`
- `scripts/prepare-sidecar.mjs`
- `scripts/smoke-tests/smoke-linux.sh`
- `scripts/smoke-tests/smoke-macos.sh`
- `scripts/smoke-tests/smoke-windows.ps1`
- `_bmad-output/implementation-artifacts/7-4-packaging-and-cicd-hardening.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`

## Change Log

- 2026-05-08: Story context created. Comprehensive packaging, CI/CD, sidecar, smoke-test, and mount-filter test guidance added for dev implementation.
- 2026-05-08: Implemented packaging and CI/CD hardening; story moved to review.
