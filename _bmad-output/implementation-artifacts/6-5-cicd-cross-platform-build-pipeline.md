# Story 6.5: CI/CD Cross-Platform Build Pipeline

Status: done

Completion Note: T7 review patches applied - workflow now installs explicit pkg-config tooling, verifies macOS libmtp architecture coverage before universal sidecar builds, and keeps runner execution validation pending until CI runs.

## Story

As a **System Admin (Alexis)**,
I want an automated GitHub Actions workflow that builds and publishes installers for all three platforms,
so that every release produces verified, downloadable artifacts without manual per-platform builds.

## Acceptance Criteria

1. **Tag triggers pipeline**: Given a tagged release commit (e.g., `v0.1.0`) is pushed, the GitHub Actions workflow triggers automatically.
2. **Parallel platform builds**: The workflow builds JellyfinSync on Windows, macOS, and Linux runners in parallel (matrix strategy).
3. **All installers produced**: Each build produces the platform-native installer — MSI (Windows), DMG (macOS), AppImage and .deb (Linux).
4. **Artifacts uploaded to GitHub Release**: All artifacts are uploaded to a GitHub Release draft tied to the tag.
5. **Failure is clear**: If any platform build fails, the workflow fails clearly with actionable output identifying which platform and step failed.
6. **Sidecar staged before build**: The `prepare-sidecar.mjs` pre-build script runs before `cargo tauri build` on each runner, correctly staging the daemon sidecar for the target platform.
7. **MTP build dependency (Sprint Change 2026-04-30):** Given the Linux and macOS build runners, when the workflow runs, `libmtp` and its development headers are installed before `cargo build` (`sudo apt-get install -y libmtp-dev` on Ubuntu; `brew install libmtp` on macOS). `pkg-config` can resolve `libmtp` for the daemon build script. The Windows runner requires no additional system libraries (`windows-rs` WPD bindings are pure Rust).

## Active Reopen Scope

This story was originally implemented on 2026-04-06 and reopened by the 2026-04-30 MTP sprint change. Treat AC #7 / T7 as the only remaining implementation scope. The release pipeline already exists and was end-to-end validated for MSI, universal DMG, AppImage, and .deb; do not rebuild the workflow from scratch.

## Tasks / Subtasks

- [x] **T1: Create GitHub Actions workflow file** (AC: #1, #2, #3, #4, #5)
  - [x] T1.1: Create `.github/workflows/release.yml` with trigger on `push` to `tags: ['v*']`
  - [x] T1.2: Define a matrix strategy with three runners: `windows-latest`, `macos-latest`, `ubuntu-22.04`
  - [x] T1.3: Each job checks out the repo, installs Rust stable, sets up Node.js (LTS), and installs pnpm
  - [x] T1.4: Each job installs Linux system dependencies (only on `ubuntu-22.04`) before building
  - [x] T1.5: Each job runs `node scripts/prepare-sidecar.mjs` to stage the daemon sidecar before `cargo tauri build`
  - [x] T1.6: Use `tauri-apps/tauri-action@v0` to build and upload artifacts to a GitHub Release draft

- [x] **T2: Validate Rust + Node toolchain setup on all runners** (AC: #3, #6)
  - [x] T2.1: Confirm that `rustup` is pre-installed on all three runner types and `rust-toolchain.toml` (if present) or `stable` channel is used
  - [x] T2.2: Confirm `pnpm` install works via `pnpm/action-setup` before `actions/setup-node`
  - [x] T2.3: Verify `scripts/prepare-sidecar.mjs` runs correctly on each platform — it uses `rustc -vV` to detect the target triple, which requires Rust to be installed first

- [x] **T3: Install Linux system dependencies** (AC: #3)
  - [x] T3.1: On `ubuntu-22.04`, install required apt packages via `apt-get`:
    - `libgtk-3-dev`, `libwebkit2gtk-4.1-dev` (Tauri v2 WebKit), `libappindicator3-dev`, `librsvg2-dev`, `patchelf`, `libxdo-dev` (daemon transitive dep via `tray-icon` → `libxdo`)
  - [x] T3.2: Verify package names match Ubuntu 22.04 apt registry (Tauri v2 requires `webkit2gtk-4.1`, not `webkit2gtk-4.0`)

- [x] **T4: Configure tauri-action for release upload** (AC: #4, #5)
  - [x] T4.1: Pass `tagName`, `releaseName`, `releaseBody`, `releaseDraft: true` to `tauri-apps/tauri-action`
  - [x] T4.2: Set `GITHUB_TOKEN` from `secrets.GITHUB_TOKEN` — no additional secrets needed for MVP (no code signing)
  - [x] T4.3: Verify the action auto-discovers the Tauri project under `jellyfinsync-ui/` via `projectPath` parameter

- [x] **T5: Verify no code signing configuration is needed** (AC: #3)
  - [x] T5.1: Confirm architecture.md: code signing (Windows Authenticode, macOS notarization) is deferred to post-MVP
  - [x] T5.2: Ensure no `APPLE_CERTIFICATE`, `APPLE_ID`, or `WINDOWS_CERTIFICATE` secrets are set — build must succeed without them
  - [x] T5.3: Document in completion notes that code signing is post-MVP

- [x] **T7: Add libmtp dependency to Linux and macOS build steps (AC: #7 — Sprint Change 2026-04-30)**
  - [x] In `.github/workflows/release.yml`, add `libmtp-dev` to the Ubuntu apt-get install step (alongside existing Tauri deps)
  - [x] Add a macOS-only step: `brew install libmtp` before `cargo tauri build`
  - [x] Add CI verification that `pkg-config --libs libmtp` succeeds on both Unix runners after install
  - [x] Confirm Windows matrix job needs no changes (windows-rs WPD is pure Rust)
  - **Depends on:** Story 4.0 (DeviceIO abstraction — introduces libmtp dependency in Cargo.toml)

- [x] **T6: Test the workflow end-to-end** (AC: #1–#5)
  - [x] T6.1: Push a test tag (e.g., `v0.0.1-test`) to trigger the workflow
  - [x] T6.2: Verify all three matrix jobs complete without errors
  - [x] T6.3: Verify a GitHub Release draft is created with MSI, DMG, AppImage, and .deb attached
  - [x] T6.4: Delete the test tag and draft release after verification

## Dev Notes

### Architecture & Technical Requirements

- **Trigger**: `push` event scoped to tags matching `v*` (e.g., `v0.1.0`, `v1.0.0`)
- **Current implementation state**: `.github/workflows/release.yml` already exists and is the UPDATE target. It has `permissions: contents: write`, `fail-fast: false`, an include-matrix for `macos-latest`, `ubuntu-22.04`, and `windows-latest`, `FORCE_JAVASCRIPT_ACTIONS_TO_NODE24: true`, pnpm + Node LTS setup, Rust stable setup, macOS universal daemon sidecar staging, non-macOS `node scripts/prepare-sidecar.mjs`, and `tauri-apps/tauri-action@v0`.
- **Current missing dependency**: `jellyfinsync-daemon/build.rs` now calls `pkg_config::probe_library("libmtp")` for all Unix targets. Any Linux or macOS runner that builds `jellyfinsync-daemon` without libmtp available through `pkg-config` will fail before packaging.
- **Runner matrix**:
  | Platform | Runner | Installer Output |
  |----------|--------|-----------------|
  | Windows | `windows-latest` | `.msi` |
  | macOS | `macos-latest` | `.dmg` |
  | Linux | `ubuntu-22.04` | `.AppImage` + `.deb` |
- **Build tool**: `tauri-apps/tauri-action@v0` — handles `cargo tauri build`, sidecar bundling, and artifact upload
- **Rust channel**: `stable` (pinned — do NOT use `nightly`; project uses Rust edition 2021, MSRV 1.93.0)
- **Node.js version**: LTS (20.x)
- **Package manager**: `pnpm` (matches the project's local toolchain)
- **No code signing for MVP** (per architecture.md — deferred to post-MVP)

### Active Implementation Requirements for T7

- Update only `.github/workflows/release.yml` unless verification proves another file is required.
- Add `libmtp-dev` to the existing Ubuntu dependency install step, keeping the existing Tauri and `libxdo-dev` packages.
- Add a macOS-only `brew install libmtp` step before `Stage daemon sidecars (macOS universal)`, because that step runs `cargo build --release -p jellyfinsync-daemon` twice and `build.rs` links libmtp on Unix.
- Add `pkg-config --libs libmtp` verification on both Linux and macOS after installation. This should fail the platform job immediately with an actionable dependency error if libmtp is missing.
- Do not change the Windows job for libmtp; Windows uses WPD through `windows-rs` and does not need the C library.
- Preserve macOS universal sidecar behavior: the inline macOS block stages `aarch64-apple-darwin`, `x86_64-apple-darwin`, and `universal-apple-darwin`. Do not replace it with `prepare-sidecar.mjs`.
- Preserve `ubuntu-22.04`; do not switch to `ubuntu-latest` while the Linux package list is intentionally pinned.

### Reference Workflow Structure

```yaml
name: Release

on:
  push:
    tags:
      - 'v*'

jobs:
  release:
    permissions:
      contents: write
    strategy:
      fail-fast: false
      matrix:
        platform: [macos-latest, ubuntu-22.04, windows-latest]

    runs-on: ${{ matrix.platform }}

    steps:
      - uses: actions/checkout@v4

      - name: Setup Node
        uses: actions/setup-node@v4
        with:
          node-version: lts/*

      - name: Install pnpm
        uses: pnpm/action-setup@v4
        with:
          version: latest

      - name: Install Rust stable
        uses: dtolnay/rust-toolchain@stable

      - name: Install Linux dependencies
        if: matrix.platform == 'ubuntu-22.04'
        run: |
          sudo apt-get update
          sudo apt-get install -y libgtk-3-dev libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf

      - name: Install frontend dependencies
        run: pnpm install
        working-directory: jellyfinsync-ui

      - name: Stage daemon sidecar
        run: node scripts/prepare-sidecar.mjs

      - name: Build and release
        uses: tauri-apps/tauri-action@v0
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        with:
          tagName: ${{ github.ref_name }}
          releaseName: 'JellyfinSync ${{ github.ref_name }}'
          releaseBody: 'See the release notes for details.'
          releaseDraft: true
          prerelease: false
          projectPath: jellyfinsync-ui
```

> **Note:** The above is a starting reference — adapt as needed based on actual project structure. If `pnpm install` is run from the workspace root (not `jellyfinsync-ui`), adjust `working-directory` accordingly. Check if a `pnpm-workspace.yaml` or `package.json` exists at the repo root.

### prepare-sidecar.mjs — Pre-Build Requirement

`scripts/prepare-sidecar.mjs` **must run before** `tauri-apps/tauri-action` on every runner:
- It calls `rustc -vV` to detect the host target triple (requires Rust to be installed first)
- It copies the compiled daemon binary to `jellyfinsync-ui/src-tauri/sidecars/jellyfinsync-daemon-{target-triple}`
- The sidecar dir is gitignored — the workflow must stage it on every run

**Problem**: On CI, the daemon binary must exist before `prepare-sidecar.mjs` runs. Either:
1. `tauri-apps/tauri-action` builds the entire workspace (including `jellyfinsync-daemon`) automatically via `cargo tauri build`, OR
2. The workflow must explicitly `cargo build --release -p jellyfinsync-daemon` before calling `prepare-sidecar.mjs`

**Resolution**: `cargo tauri build` is called by `tauri-apps/tauri-action` internally and builds the entire workspace. However, `prepare-sidecar.mjs` needs the daemon built first to copy it. The safest approach is to run `cargo build --release -p jellyfinsync-daemon` explicitly before `node scripts/prepare-sidecar.mjs`, then let `tauri-action` handle the Tauri build.

Alternatively: check if `tauri-action` supports a `beforeBuildCommand` to run preparation steps — use that if available to keep the workflow clean.

### Linux System Dependencies (Ubuntu 22.04 — Tauri v2)

Tauri v2 uses WebKit2GTK 4.1 (not 4.0). The exact package list for Ubuntu 22.04:

```bash
sudo apt-get update
sudo apt-get install -y \
  libgtk-3-dev \
  libwebkit2gtk-4.1-dev \
  libappindicator3-dev \
  librsvg2-dev \
  patchelf
```

> **Critical**: `libwebkit2gtk-4.1-dev` is the correct package for Tauri v2. Using `libwebkit2gtk-4.0-dev` (Tauri v1) will fail.

### File Location

```
.github/
  workflows/
    release.yml     ← UPDATE existing workflow; do not recreate
```

Existing workflow file present:
- `.github/workflows/release.yml`

### tauri.conf.json — No Changes Expected

The existing config (`jellyfinsync-ui/src-tauri/tauri.conf.json`) already has:
- `"targets": "all"` — produces all platform-native installers
- `"externalBin": ["sidecars/jellyfinsync-daemon"]` — sidecar bundling configured
- Correct `productName`, `identifier`, `version`, and icons

No changes to `tauri.conf.json` are expected for this story.

### permissions: contents: write

The release job requires `permissions: contents: write` so `GITHUB_TOKEN` can create and upload to GitHub Releases. Either set at job level or workflow level.

### fail-fast: false

Use `fail-fast: false` in the matrix strategy so that if one platform fails, the other platforms continue to build. This gives more diagnostic information when debugging multi-platform failures.

### What NOT to Do

- Do NOT implement code signing (Windows Authenticode, macOS notarization) — deferred to post-MVP
- Do NOT use `ubuntu-latest` for Linux — use `ubuntu-22.04` for stability; `ubuntu-latest` may shift to 24.04 and break apt packages
- Do NOT use `actions-rs/toolchain` (deprecated) — use `dtolnay/rust-toolchain@stable` instead
- Do NOT run `cargo tauri build` manually — `tauri-apps/tauri-action` handles this
- Do NOT skip `fail-fast: false` — each platform failure should be independently visible
- Do NOT omit `permissions: contents: write` — the workflow cannot create releases without it
- Do NOT hardcode the tag name in `releaseName` — use `${{ github.ref_name }}` dynamically
- Do NOT add non-MVP features (auto-update, Sparkle, WiX custom dialogs) in this story

### Latest Technical Information (checked 2026-05-03)

- GitHub-hosted runner labels have shifted since this workflow was first created: current public runner image docs map `macos-latest` to macOS 15 arm64, `ubuntu-22.04` remains x64, and `windows-latest` maps to Windows Server 2025. The existing matrix already accounts for Apple Silicon macOS by building `universal-apple-darwin`. [Source: GitHub Actions runner-images README]
- The current Tauri action README examples use `tauri-apps/tauri-action@v1`, while this project currently uses `@v0` with an existing review deferral to pin/harden action versions later. Do not migrate action major versions as part of T7 unless the release workflow fails specifically because of the existing action version. [Source: tauri-apps/tauri-action README and releases]
- GitHub Actions workflow syntax stores workflow files under `.github/workflows`, supports matrix `include`, and `fail-fast: false` keeps other matrix jobs running after one platform fails. Preserve that behavior for diagnostics. [Source: GitHub Docs workflow syntax]
- `permissions: contents: write` is still required for workflows that create/upload GitHub Release assets with `GITHUB_TOKEN`; keep it at job level. [Source: tauri-apps/tauri-action README; GitHub Docs workflow syntax]

### Previous Story Intelligence (6.4 — Linux Packages)

- `cargo tauri build` on Linux produces both AppImage and .deb — confirmed working with `"targets": "all"`
- `prepare-sidecar.mjs` is cross-platform and handles Linux target triples correctly — no changes needed
- **123 tests pass** — do not regress this; workflow should not skip tests (though running tests in CI is out of scope for this story — just don't break them)
- No code signing on Linux — GPG signing deferred to post-MVP
- `"bundle.linux"` section is not required in `tauri.conf.json` for MVP
- The `sc start` Windows service fallback in `lib.rs` fails silently on Linux — this is expected behavior
- CI note from 6.4 review: Linux AppImage builds may need FUSE2 support or `APPIMAGE_EXTRACT_AND_RUN=1` in environments where the Tauri-cached `linuxdeploy` AppImage cannot mount. Do not address this unless the release workflow fails at AppImage bundling after libmtp is fixed.

### Project Context Reference

- Project principle: Managed Sync Mode, Jellyfin-first workflow, speed-focused buffered streaming, and Rockbox scrobble bridge future-proofing remain foundational. This CI story must preserve cross-platform parity and packaging reliability; it should not alter runtime sync behavior.
- Platform support is mandatory across Windows, Linux, and macOS; CI must keep all three platform outputs visible even on partial failure.

### Project Structure Notes

```
c:\Workspaces\JellyfinSync\
├── .github/
│   └── workflows/         ← UPDATE: release.yml exists
├── scripts/
│   └── prepare-sidecar.mjs   ← MUST run before tauri build
├── jellyfinsync-daemon/       ← daemon crate (built by cargo)
├── jellyfinsync-ui/
│   ├── src/                   ← Vanilla TypeScript + Shoelace
│   ├── src-tauri/
│   │   ├── tauri.conf.json    ← no changes expected
│   │   ├── sidecars/          ← gitignored; staged by prepare-sidecar.mjs
│   │   └── src/lib.rs         ← no changes expected
│   └── package.json
└── Cargo.toml                 ← workspace root
```

### Key Files to Create/Modify

| File | Action |
|------|--------|
| `.github/workflows/release.yml` | **UPDATE** — add Unix libmtp install/verification steps only |
| `jellyfinsync-daemon/build.rs` | **READ/PRESERVE** — explains why Unix CI needs libmtp via `pkg-config` |
| `scripts/prepare-sidecar.mjs` | **READ/PRESERVE** — non-macOS sidecar build path already works |
| `jellyfinsync-ui/src-tauri/tauri.conf.json` | **READ/PRESERVE** — bundler sidecar config already exists |
| All other files | No changes expected |

### Existing UPDATE File Behavior to Preserve

- `.github/workflows/release.yml` creates a draft GitHub Release for `v*` tags and uploads Tauri artifacts through `tauri-apps/tauri-action`.
- The matrix is intentionally `include`-based so macOS can carry `args: --target universal-apple-darwin` and extra Rust targets while Linux/Windows stay simple.
- The macOS sidecar step does not call `prepare-sidecar.mjs`; it manually cross-compiles and merges the daemon because a universal Tauri bundle needs arch-specific and universal sidecars.
- The Linux dependency step already installs Tauri v2 WebKit/appindicator/SVG/patchelf dependencies plus `libxdo-dev` for the daemon's `tray-icon` transitive dependency. Add `libmtp-dev` alongside these; do not remove existing packages.
- `FORCE_JAVASCRIPT_ACTIONS_TO_NODE24: true` is already present for current GitHub hosted runner/action compatibility. Leave it in place unless a verified action update makes it unnecessary.

### Testing Requirements for T7

- Minimum local/static verification: inspect `.github/workflows/release.yml` after edit and confirm Linux has `libmtp-dev`, macOS has `brew install libmtp`, both Unix paths run `pkg-config --libs libmtp`, and Windows has no libmtp step.
- Preferred CI verification: push a disposable test tag and confirm all three release matrix jobs reach the build stage; Linux/macOS must not fail with `libmtp not found`.
- If full release validation is run, confirm the draft release still receives MSI, DMG, AppImage, and .deb artifacts, then clean up the test tag and draft release.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#story-65-cicd-cross-platform-build-pipeline] — Story 6.5 ACs including libmtp build dependency
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-04-30.md#46-epic-6-changes-epicsmd] — MTP sprint change adding libmtp CI dependency
- [Source: _bmad-output/planning-artifacts/architecture.md#packaging--distribution] — Tauri v2 bundler, GitHub Actions matrix, code signing deferred
- [Source: _bmad-output/planning-artifacts/prd.md#functional-requirements] — FR27/FR28 packaging and CI/CD requirements
- [Source: _bmad-output/implementation-artifacts/6-4-linux-packages-appimage-deb.md] — Previous story: Linux packaging, libxdo/FUSE/AppImage learnings
- [Source: _bmad-output/implementation-artifacts/deferred-work.md] — Deferred packaging/CI risks
- [Source: .github/workflows/release.yml] — Existing release workflow to update
- [Source: jellyfinsync-daemon/build.rs] — Unix `pkg-config` libmtp probe
- [Source: jellyfinsync-daemon/Cargo.toml] — `pkg-config` build dependency and Unix libc dependency
- [Source: scripts/prepare-sidecar.mjs] — Cross-platform sidecar build script for non-macOS runners
- [Source: jellyfinsync-ui/src-tauri/tauri.conf.json] — Current Tauri sidecar/bundle config
- [Source: https://github.com/actions/runner-images] — Current GitHub-hosted runner image labels
- [Source: https://github.com/tauri-apps/tauri-action] — Current Tauri action usage examples
- [Source: https://docs.github.com/actions/reference/workflows-and-actions/workflow-syntax] — GitHub Actions matrix, permissions, and workflow syntax

## Dev Agent Record

### Agent Model Used

Claude Sonnet 4.6

### Completion Notes List

- **T7 complete**: Updated `.github/workflows/release.yml` so Ubuntu installs `pkgconf` and `libmtp-dev` alongside the existing Tauri/libxdo packages and immediately verifies `pkg-config --libs libmtp`.
- **macOS libmtp setup added**: Added a macOS-only dependency step before universal sidecar staging that runs `brew install pkg-config libmtp`, configures target-specific `PKG_CONFIG_PATH` values for the Rust build, verifies `pkg-config --libs libmtp`, and fails early if the installed `libmtp` dylib is not usable for both `arm64` and `x86_64` universal slices.
- **Windows intentionally unchanged for libmtp**: Confirmed the Windows matrix entry has no libmtp dependency step because Windows uses pure Rust `windows-rs` WPD bindings.
- **Static validation passed**: Inspected the workflow after edit; CI runner execution remains pending for the updated libmtp dependency checks.
- **T1-T5 complete**: Created `.github/workflows/release.yml` implementing all workflow structure, matrix strategy, toolchain setup, Linux dependencies, sidecar staging, and tauri-action release upload.
- **`prepare-sidecar.mjs` handles daemon build internally**: The script calls `cargo build --release -p jellyfinsync-daemon` itself before copying the sidecar — no separate build step needed in the workflow.
- **`pnpm install` scoped to `jellyfinsync-ui/`**: No root-level `package.json` or `pnpm-workspace.yaml` exists; pnpm install must run in `jellyfinsync-ui/` with `working-directory`.
- **pnpm installed before setup-node**: `pnpm/action-setup@v4` runs first so `pnpm` is on PATH when `setup-node` resolves it.
- **No pnpm-lock.yaml in repo**: Cache configuration omitted from `setup-node` to avoid a missing-file error; can be added once lock file is committed.
- **No cache for node_modules** in pnpm-lock.yaml: lock file not present in repo; removed `cache: pnpm` from setup-node to prevent failure.
- **macOS universal binary**: `macos-latest` is Apple Silicon — the default build only produces `aarch64-apple-darwin`. Switched matrix to `include` format with `args: --target universal-apple-darwin` for macOS and `rust-targets: aarch64-apple-darwin,x86_64-apple-darwin` passed to `dtolnay/rust-toolchain`. Added a dedicated macOS sidecar step that cross-compiles the daemon for both targets and stages all three sidecar variants: `aarch64-apple-darwin`, `x86_64-apple-darwin` (checked during each arch compilation slice), and `universal-apple-darwin` (lipo-merged, checked during final bundling). `prepare-sidecar.mjs` only runs on non-macOS platforms.
- **`libxdo-dev` required on Ubuntu**: The daemon transitively depends on the `libxdo` crate (via `tray-icon` → `libxdo`), which requires `libxdo-dev` on Ubuntu 22.04. Added to apt-get install step.
- **Code signing deferred to post-MVP**: No `APPLE_CERTIFICATE`, `APPLE_ID`, or `WINDOWS_CERTIFICATE` secrets required. The workflow succeeds without them.
- **`fail-fast: false`** ensures independent platform failure visibility.
- **`permissions: contents: write`** set at job level for GitHub Release creation.
- **T6 validated by Alexis**: All three matrix jobs completed successfully. GitHub Release draft confirmed with MSI (Windows), universal DMG (macOS), AppImage and .deb (Linux). Test tag and draft release cleaned up.

### Change Log

- 2026-05-03: Completed reopened T7 scope - added Linux/macOS libmtp install, explicit pkg-config tooling, macOS universal dylib architecture verification, and `pkg-config --libs libmtp` checks to the release workflow; Windows path unchanged. CI runner execution pending.
- 2026-04-30: Reopened — MTP support (Sprint Change 2026-04-30). AC #7 and T7 added. Requires Story 4.0 (DeviceIO abstraction) to be completed first so libmtp dependency exists in Cargo.toml.
- 2026-04-06: Created `.github/workflows/release.yml` — CI/CD cross-platform release pipeline for Windows (MSI), macOS universal DMG, Linux (AppImage + .deb). All tasks complete; end-to-end validated.
- 2026-04-06: Fixed Ubuntu build failure — added `libxdo-dev` to apt-get step (daemon transitively depends on `libxdo` crate).
- 2026-04-06: Added macOS universal binary support — matrix restructured to `include` format, daemon cross-compiled for both architectures; all three sidecar variants staged (aarch64, x86_64 for per-slice compilation checks, plus lipo-merged universal for bundling), `tauri-action` called with `--target universal-apple-darwin`.

### File List

- `.github/workflows/release.yml` (updated for T7: Unix libmtp install and pkg-config verification)
- `_bmad-output/implementation-artifacts/6-5-cicd-cross-platform-build-pipeline.md` (story status, T7 checklist, Dev Agent Record updates)
- `_bmad-output/implementation-artifacts/sprint-status.yaml` (story status tracking)
- `_bmad-output/implementation-artifacts/deferred-work.md` (runtime libmtp packaging follow-up)

### Review Findings

- [x] [Review][Decision] macOS universal build only installs/verifies host-arch `libmtp` - resolved by configuring target-specific macOS pkg-config paths and adding early `lipo -verify_arch` checks before the universal daemon build.
- [x] [Review][Patch] Install `pkg-config`/`pkgconf` explicitly before verifying `libmtp` [.github/workflows/release.yml:50]
- [x] [Review][Patch] Do not mark runner `pkg-config --libs libmtp` verification complete until CI has actually executed it [_bmad-output/implementation-artifacts/6-5-cicd-cross-platform-build-pipeline.md:58]
- [x] [Review][Patch] Replace unrelated completion note with a T7-specific completion note [_bmad-output/implementation-artifacts/6-5-cicd-cross-platform-build-pipeline.md:5]
- [x] [Review][Defer] Unix release artifacts may depend on CI-only `libmtp` runtime libraries [.github/workflows/release.yml:61] - deferred, runtime packaging was outside the reopened AC #7 build-dependency scope; add Linux package dependencies/AppImage inclusion checks and macOS dylib bundling or static/vendor strategy in packaging hardening.
- [x] [Review][Decision] AC6 interpretation: `prepare-sidecar.mjs` not invoked on macOS — closed as done; inline shell block satisfies AC6 intent, universal binary cross-compilation requirement made script deviation necessary — AC6 requires the script to run on each runner. macOS uses an inline shell block instead because `prepare-sidecar.mjs` only handles the host target and cannot cross-compile for both architectures needed for a universal binary. The inline block is functionally equivalent and was end-to-end validated. Decision: is this a necessary implementation adaptation (close AC6 as done) or does it require back-porting cross-compilation support into the script?
- [x] [Review][Defer] `pnpm/action-setup@v4` uses `version: latest` [.github/workflows/release.yml:34] — deferred, pre-existing convention; pin to a specific pnpm version when hardening for deterministic releases
- [x] [Review][Defer] `node-version: lts/*` is a floating Node.js version [.github/workflows/release.yml:40] — deferred, idiomatic for Tauri workflows; pin to `20` when hardening
- [x] [Review][Defer] `tauri-apps/tauri-action@v0` is a floating major-version tag [.github/workflows/release.yml:87] — deferred, supply chain hardening; pin to a commit SHA for production releases
- [x] [Review][Defer] `rustc -vV` output parsing in `prepare-sidecar.mjs` is fragile to format changes — deferred, pre-existing in the script; not introduced by this story
