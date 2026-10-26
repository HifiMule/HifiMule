---
title: Upgrade platform and dependencies to current stable releases
type: chore
created: '2026-10-26'
status: done
baseline_commit: db9ac3d79e4a1f39a2f3996c04bc7d793e6ab54d
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Platform pins and dependency constraints lag current stable releases, and GitHub reports dependency vulnerabilities. The committed frontend lockfile currently produces two high-severity npm audit findings, affecting Vite and source-map-js.

**Approach:** Upgrade the supported application toolchains, direct dependencies, and resolved transitive dependencies to verified stable releases, adapting affected APIs. Keep build, release, local installation, runtime contracts, and documentation consistent and validate the resulting application.

## Boundaries & Constraints

**Always:** Revalidate stable versions against official registries. Include major upgrades; document compatibility exceptions imposed by upstream frameworks. Preserve behavior, platform support, playback patches, security controls, and reproducible installs. Record unresolved advisories and validation limits. Use RTK and the controlled daemon build wrapper.

**Ask First:** Any necessary product behavior change, reduction in supported operating systems, replacement of a framework, or removal of a locally maintained playback patch that cannot be shown redundant upstream.

**Never:** Use prereleases, suppress advisories, blindly replace vendors, rewrite historical evidence, publish, push, or change unrelated functionality. Keep application version 0.20.0.

</frozen-after-approval>

## Code Map

- Root and member `Cargo.toml`, `Cargo.lock` — Rust dependencies and Windows/GTK bindings.
- `hifimule-ui/package{,-lock}.json` — authoritative frontend dependency resolution.
- Root npm metadata, new `.nvmrc` and `rust-toolchain.toml` — developer toolchains.
- `.github/workflows/{build,release}.yml`, `scripts/prepare-sidecar.mjs` — CI/install commands. Release uses pnpm without a committed pnpm lockfile; build CI uses npm ci.
- `hifimule-ui/{vite.config.ts,tsconfig.json}`, affected application sources — required migrations.
- `hifimule-daemon/audio-runtime.json`, `third_party/{cpal,souvlaki}`, runtime scripts/tests, notices — native versions, ABI contracts, patches.
- `README.md`, `docs/development-guide.md` — setup instructions.
- `experiments/playback-probe/**/Cargo.{toml,lock}` — independent historical proof projects.

## Tasks & Acceptance

**Execution:**
- [x] Toolchain files, root npm metadata, CI, documentation — align on Rust 1.99.1, Node.js 26.12.1, npm 12.2.1; avoid changing machine defaults unnecessarily.
- [x] Frontend manifests/lockfile — upgrade Vite to 8.3.5, TypeScript to 7.0.2, Tauri API/CLI to 2.12.4, and opener to 2.7.2; retain Shoelace 2.20.1 if still latest. Adapt bundler/compiler configuration and source types as required.
- [x] Cargo manifests/lockfile and sources — upgrade direct/transitive dependencies. Major migration candidates: rusqlite 0.41.0, reqwest 0.13.5, tower-http 0.7.1, tray-icon 0.26.1, tao 0.37.2, rand 0.10.3, secrecy 0.10.3, sha2/md-5 0.11, chacha20poly1305 0.11.0. Tauri latest is 2.12.5. Remove confirmed unused declarations.
- [x] Platform dependency families — maintain coherent Windows and GTK types across framework boundaries; select versions from upstream dependency requirements rather than independently taking every package's maximum version.
- [x] Native runtime, patches, tests, notices — review stable native releases and update coupled metadata/checksums when necessary. CPAL 0.18.2, Souvlaki 0.8.3, ffmpeg-next 9.0.0, libpulse-binding 2.30.1 are current; ffmpeg-sys-next 9.0.1 is available.
- [x] Release workflow and sidecar installer — use npm ci and the checked-in npm lockfile consistently, updating command-contract tests affected by that change.
- [x] Lockfiles and verification record — run advisory checks, inspect remaining constraints, separately document experimental/vendor findings.

**Acceptance Criteria:**
- Given a clean checkout with the declared toolchains, when dependencies are installed from committed lockfiles, then frontend production build and existing script/UI tests pass.
- Given upgraded Rust dependencies, when the controlled daemon build wrapper and relevant workspace checks run, then existing application tests pass and all supported CI matrix targets compile.
- Given refreshed lockfiles, when current npm and Rust advisory databases are queried, then fixable vulnerabilities are removed and any remaining findings have specific dependency paths and explanations.
- Given the native audio contract, when existing runtime/packaging checks run, then binding versions, ABI expectations, notices, and vendor patches agree with the resolved build.

## Spec Change Log

## Verification

- Baseline frontend build passed. npm audit reports two high-severity packages: locked Vite 6.4.3 and source-map-js 1.2.1. Installed Vite is 6.4.5; validate clean locked installs.
- Run `rtk npm ci --prefix hifimule-ui`, frontend build, and `rtk test node --test scripts/tests/*.test.mjs hifimule-ui/tests/*.test.mjs`.
- Run `rtk npm run build:daemon -- check -p hifimule-daemon`, daemon tests, and remaining workspace checks in the controlled native environment.
- Run Python script regressions, npm audit, and cargo-audit against current advisory data. Inspect latest/outdated reports and `rtk git diff --check`.
- Validate Linux, Windows, and both macOS architectures through the existing CI matrix; distinguish pending cross-platform evidence from local macOS results.
- GitHub CLI is absent. Retrieve Dependabot details through an authenticated alternative if available; distinguish local audit results from GitHub alert closure.

## Implementation Record

Implementation and local verification are complete. See [the dependency upgrade report](../../docs/dependency-upgrade-2026-10-26.md) for versions, security findings, compatibility exceptions, and pending cross-platform CI. TypeScript 7 uses the official compiler-API compatibility alias for the existing tests. CPAL’s example ringbuf dependency was also patched. The existing CI matrix was updated but not remotely triggered; its results are not claimed as local evidence.

## Review Results

Three independent reviews completed. Two release invocation boundary issues were patched: duplicate npm argument separation in tauri-action and older candidate refs lacking `.nvmrc`. A release-contract regression covers both. Reviewers verified the live fixes; no remaining actionable findings. Final local totals: 1,761 Rust tests passed (16 ignored), 636 Node tests passed (1 skipped), 47 Python tests passed. Cross-platform CI and installed-package smoke evidence remain pending as documented in the report.

## Suggested Review Order

**Dependency and compatibility policy**

- Pins the stable toolchain and shared dependency families.
  [Cargo.toml:14](../../Cargo.toml#L14)

**Credential and platform boundaries**

- Preserves credential format while adapting cryptography and OS entropy APIs.
  [vault.rs:26](../../hifimule-daemon/src/vault.rs#L26)

- Updates Windows bindings without changing MTP operations.
  [mtp.rs:82](../../hifimule-daemon/src/device/mtp.rs#L82)

**Native audio contract**

- Aligns binding versions, source hashes, and native runtime metadata.
  [audio-runtime.json:22](../../hifimule-daemon/audio-runtime.json#L22)

**Frontend compiler and build**

- Uses TypeScript 7 and preserves compiler API access for tests.
  [package.json:19](../../hifimule-ui/package.json#L19)

- Adapts the production build to Vite 8.
  [vite.config.ts:54](../../hifimule-ui/vite.config.ts#L54)

**Reproducible releases**

- Preserves Tauri flag routing and locked npm installs.
  [release.yml:304](../../.github/workflows/release.yml#L304)

- Uses the committed npm resolution when bootstrapping sidecars.
  [prepare-sidecar.mjs:44](../../scripts/prepare-sidecar.mjs#L44)

**Verification and remaining limits**

- Checks release argument separation and compatibility with older refs.
  [release-contract.test.mjs:266](../../scripts/tests/release-contract.test.mjs#L266)

- Records audit findings, framework constraints, and pending CI evidence.
  [dependency-upgrade-2026-10-26.md:61](../../docs/dependency-upgrade-2026-10-26.md#L61)

**Toolchain setup**

- Selects the exact stable Rust release through rustup.
  [rust-toolchain.toml:2](../../rust-toolchain.toml#L2)

- Selects the exact stable Node.js release through nvm.
  [.nvmrc:1](../../.nvmrc#L1)
