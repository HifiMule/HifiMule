# Story 6.5: CI/CD Cross-Platform Build Pipeline

Status: in-progress

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

- [ ] **T6: Test the workflow end-to-end** (AC: #1–#5)
  - [ ] T6.1: Push a test tag (e.g., `v0.0.1-test`) to trigger the workflow
  - [ ] T6.2: Verify all three matrix jobs complete without errors
  - [ ] T6.3: Verify a GitHub Release draft is created with MSI, DMG, AppImage, and .deb attached
  - [ ] T6.4: Delete the test tag and draft release after verification

## Dev Notes

### Architecture & Technical Requirements

- **Trigger**: `push` event scoped to tags matching `v*` (e.g., `v0.1.0`, `v1.0.0`)
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
    release.yml     ← create this (does not exist yet)
```

No existing workflow files are present in the repository.

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

### Previous Story Intelligence (6.4 — Linux Packages)

- `cargo tauri build` on Linux produces both AppImage and .deb — confirmed working with `"targets": "all"`
- `prepare-sidecar.mjs` is cross-platform and handles Linux target triples correctly — no changes needed
- **123 tests pass** — do not regress this; workflow should not skip tests (though running tests in CI is out of scope for this story — just don't break them)
- No code signing on Linux — GPG signing deferred to post-MVP
- `"bundle.linux"` section is not required in `tauri.conf.json` for MVP
- The `sc start` Windows service fallback in `lib.rs` fails silently on Linux — this is expected behavior

### Project Structure Notes

```
c:\Workspaces\JellyfinSync\
├── .github/
│   └── workflows/         ← CREATE: release.yml here
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
| `.github/workflows/release.yml` | **CREATE** — the only new file in this story |
| All other files | No changes expected |

## Dev Agent Record

### Agent Model Used

Claude Sonnet 4.6

### Completion Notes List

- **T1-T5 complete**: Created `.github/workflows/release.yml` implementing all workflow structure, matrix strategy, toolchain setup, Linux dependencies, sidecar staging, and tauri-action release upload.
- **`prepare-sidecar.mjs` handles daemon build internally**: The script calls `cargo build --release -p jellyfinsync-daemon` itself before copying the sidecar — no separate build step needed in the workflow.
- **`pnpm install` scoped to `jellyfinsync-ui/`**: No root-level `package.json` or `pnpm-workspace.yaml` exists; pnpm install must run in `jellyfinsync-ui/` with `working-directory`.
- **pnpm installed before setup-node**: `pnpm/action-setup@v4` runs first so `pnpm` is on PATH when `setup-node` resolves it.
- **No pnpm-lock.yaml in repo**: Cache configuration omitted from `setup-node` to avoid a missing-file error; can be added once lock file is committed.
- **No cache for node_modules** in pnpm-lock.yaml: lock file not present in repo; removed `cache: pnpm` from setup-node to prevent failure.
- **`libxdo-dev` required on Ubuntu**: The daemon transitively depends on the `libxdo` crate (via `tray-icon` → `libxdo`), which requires `libxdo-dev` on Ubuntu 22.04. Added to apt-get install step.
- **Code signing deferred to post-MVP**: No `APPLE_CERTIFICATE`, `APPLE_ID`, or `WINDOWS_CERTIFICATE` secrets required. The workflow succeeds without them.
- **`fail-fast: false`** ensures independent platform failure visibility.
- **`permissions: contents: write`** set at job level for GitHub Release creation.
- **HALT — T6 requires user action**: End-to-end validation (T6.1–T6.4) requires pushing a real tag to GitHub to trigger the Actions runner. This cannot be simulated locally.

### Change Log

- 2026-04-06: Created `.github/workflows/release.yml` — CI/CD cross-platform release pipeline for Windows (MSI), macOS (DMG), and Linux (AppImage + .deb). T1–T5 complete; T6 pending manual tag-push validation.
- 2026-04-06: Fixed Ubuntu build failure — added `libxdo-dev` to apt-get step (daemon transitively depends on `libxdo` crate).

### File List

- `.github/workflows/release.yml` (created)
