# HifiMule — Release Guide

This guide covers reproducible `0.15.x` candidate verification and draft release creation. Publishing is a separate, explicit release-manager action.

## 1. Shipping contract

The authoritative machine-readable contract is [`playback-evidence/release-contract-0.15.0.json`](playback-evidence/release-contract-0.15.0.json). The release matrix has four independent rows:

| Row | Supported floor | Packages | Certification boundary |
| --- | --- | --- | --- |
| Windows x64 | Windows 10 and Windows 11 x64 desktop | MSI and NSIS | No Windows ARM claim. Native API injection does not certify physical media keys. |
| Linux x64 | Ubuntu 22.04 x64 desktop | deb and AppImage | Xvfb verifies lifecycle only; it does not certify physical output or media keys. |
| macOS x64 | macOS 10.15+ x64 | x64 app and DMG | Does not certify macOS ARM64. Ad-hoc signing is not notarization. |
| macOS ARM64 | macOS 11+ ARM64 | ARM64 app and DMG | Does not certify macOS x64. |

Every package is its own installation row. Architecture, VM, native-control injection, callback counters and virtual-output observations do not certify another architecture, physical media keys, or audible physical continuity.

The required same-host upgrade baseline is `0.14.0`. Older direct upgrades are unsupported unless separately tested. Machine-bound credentials copied to another host are not upgrade evidence.

## 2. Prepare `0.15.0`

- [ ] All intended changes are merged and the source commit is recorded.
- [ ] `Cargo.toml`, `Cargo.lock` and `hifimule-ui/src-tauri/tauri.conf.json` agree on `0.15.0`.
- [ ] The controlled audio-runtime manifest, source receipt and notices are present.
- [ ] The complete automated suite and platform bundle verifiers pass.
- [ ] If Windows signing is enabled, both Authenticode certificate secrets are available for MSI and NSIS.
- [ ] If macOS Developer ID signing is enabled, all Developer ID signing and notarization secrets are available for both macOS rows.
- [ ] Clean-install, `0.14.0` upgrade, provider, physical-output, accessibility and real-sync fixtures have named owners.

Distribution signing is optional. With no platform signing secrets, the workflow produces unsigned Windows artifacts and ad-hoc signed macOS apps and DMGs, and skips only the corresponding trust verification. Every macOS build must pass strict bundle signature verification, sealed-resource checks, and individual daemon and dylib signature verification. Tauri signs the assembled app before creating its DMG; the same ad-hoc default applies to local builds. When configured, `APPLE_SIGNING_IDENTITY` overrides the ad-hoc fallback and the signed build steps enable the hardened runtime; Developer ID signing and notarization remain required. Ad-hoc builds leave the hardened runtime disabled because its library validation rejects the bundled libraries without a shared Team ID. A partial credential set is a configuration error and fails before packaging. Unsigned Windows builds can trigger SmartScreen warnings, and ad-hoc signed macOS builds are not notarized or Developer ID trusted and can trigger Gatekeeper warnings or require an explicit user override. Missing target hardware, provider fixtures or other required results remain blockers.

For a local Developer ID build, configure the Apple signing and notarization environment variables and explicitly enable the hardened runtime, as the release workflow does. From `hifimule-ui`, run:

```sh
pnpm exec tauri build --config '{"bundle":{"macOS":{"hardenedRuntime":true}}}'
```

The plain local build command uses the ad-hoc runtime settings; setting an Apple identity alone does not enable the hardened runtime.

## 3. Build a non-publishing candidate

Run **Actions → Release → Run workflow** and supply:

- `candidate_ref`: an exact commit SHA or immutable ref.
- `candidate_version`: `0.15.0`.

The workflow checks out that ref, verifies the Cargo/Tauri version, uses Rust 1.93.0, builds all four rows, runs bundle checks, records SHA-256 files and uploads immutable per-row artifacts. It does not create a tag or release. Downstream candidate smoke jobs install the MSI, deb and each DMG; AppImage clean-launch and NSIS installation remain distinct required evidence rows and cannot inherit another package's result.

Candidate artifacts expire from CI storage, so material physical-continuity captures and final evidence must be copied to the approved immutable release-evidence store with URI, SHA-256, capture metadata and retention policy.

## 4. Required gates

Automated smoke proves installation, launch, authenticated daemon health, UI attachment and basic lifecycle behavior. It does not prove decoding, audible playback, physical media keys, output continuity, accessibility, upgrades, migration recovery, or physical-device sync.

Before a release decision, each package row must link:

- artifact filename, SHA-256, exact source revision and lockfile/runtime identity;
- OS/architecture and clean-install or same-host upgrade environment;
- loaded native versions and paths, explicit signed or `not-configured` distribution status, and packaged-license results;
- supported Jellyfin and OpenSubsonic/Subsonic playback outcomes;
- lifecycle, physical output, physical media keys, accessibility, migration, real-sync coexistence and bounded-resource outcomes;
- every limitation/blocker, owner, rationale and final row decision.

A missing or failed required row makes the mechanically derived aggregate decision a blocker. A passing row never hides another row's failure.

## 5. Create the draft release

Only after candidate gates are complete, create and push the matching tag from the verified commit:

```bash
git tag v0.15.0 <verified-commit-sha>
git push origin v0.15.0
```

The tag path rebuilds the same four rows and creates a **draft** release. The release workflow must remain draft-only. Confirm all expected MSI, NSIS, deb, AppImage, x64 DMG and ARM64 DMG artifacts and their checksums are present; a partial matrix is a blocker.

The called smoke workflow installs the release packages. Review its logs as lifecycle evidence only, then link the separately recorded manual and installed-playback evidence.

Draft-release smoke tests require `contents: write` on both the release workflow's reusable-workflow caller and `smoke-test.yml`. GitHub hides draft releases from tokens without push access, so reducing either side to `contents: read` makes an existing draft appear missing. The smoke tests only download and test assets; they do not publish the draft. Candidate calls grant the same permission ceiling because they invoke the same reusable workflow.

## 6. Publish or reject

The release manager derives the aggregate decision from the per-package records. Publish only when every required row passes. A signed row must include a verified signing identity and pass the Windows Authenticode or macOS Developer ID/notarization checks; a row without distribution credentials (including ad-hoc signed macOS) must record signing status `not-configured` without an identity and must not be described as trusted by SmartScreen or Gatekeeper. Otherwise keep the draft unpublished and record `blocker` or a narrowly scoped `unsupported` disposition with owner and rationale.

Downgrade is not automatically rollback-safe. Preserve a backup before migration testing and document the exact supported recovery procedure without claiming erased device state as success.
