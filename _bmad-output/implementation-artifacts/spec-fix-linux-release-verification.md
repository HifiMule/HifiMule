---
title: 'Fix Linux release verification for DEB, AppImage and Fedora RPM'
type: 'bugfix'
created: '2026-10-05'
status: 'done'
baseline_commit: '4c8b6f9ce5a032b60fa0a7d91d1c24b917cf2a3b'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The Ubuntu 0.17.0 release builds and uploads DEB, AppImage and RPM, but its installed DEB verifier selects the autostart desktop file as the daemon and fails readelf. RPM support already exists; the same selection bug affects its extracted layout. Several workflow artifact searches also search nonexistent target directories under strict shell error handling.

**Approach:** Select the installed daemon by its exact executable filename and make artifact discovery search existing target directories. Preserve existing Fedora RPM generation, extraction, dependency metadata and private audio-runtime verification.

## Boundaries & Constraints

**Always:** Preserve the controlled FFmpeg linkage and private closure checks, all existing bundle targets, Fedora dependency names, and existing signing behavior. Keep actual executable ELF validation strict. Cover the observed desktop-file collision and missing-directory discovery with regressions.

**Ask First:** Publishing releases or adding a separate Fedora runner/container qualification job.

**Never:** Suppress readelf failures, silently skip missing packages, add a duplicate RPM build, or claim native Fedora installation qualification from an Ubuntu build.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|----------------------------|----------------|
| Desktop-file collision | Autostart desktop file precedes usr/bin/hifimule-daemon | Inspect actual daemon executable | Desktop metadata is ignored |
| Missing executable | Only desktop file or similarly prefixed filenames | Report missing daemon | Fail verification |
| Invalid executable | Exact daemon name contains non-ELF data | ELF verifier rejects file | Preserve existing readelf failure |
| Build directory discovery | Only workspace target or only Tauri target exists | Find the requested package and checksum inputs | Missing alternative directory does not fail discovery |
| Missing package | Existing target directories lack requested bundle | Explicit package-not-found diagnostic | Fail verification |
| No build directory | Both candidate target directories absent | Explicit target-directory diagnostic | Fail verification |
| Fedora RPM | Default Linux release build | Existing RPM build and extraction verification continue | Closure failures remain blocking |

</frozen-after-approval>

## Code Map

- `scripts/linux-audio-runtime.mjs` — verifyInstalledLinuxBundle currently matches any filename starting with hifimule-daemon.
- `.github/workflows/release.yml` — DEB/RPM discovery and candidate checksum searches currently include nonexistent directories; AppImage uses an existing-directory array already.
- `scripts/tests/linux-audio-runtime.test.mjs` — installed bundle fixtures and injectable ELF inspector support focused daemon-selection regressions.
- `scripts/tests/release-contract.test.mjs` — existing RPM packaging contract and workflow checks; add executable shell fixture checks for artifact discovery.
- `hifimule-ui/src-tauri/tauri.linux.conf.json` — already targets deb, appimage and rpm with Fedora package dependencies and autostart resource mappings.
- `docs/release-guide.md` — existing guidance states Ubuntu produces and verifies RPM; no additional build step needed.

## Tasks & Acceptance

**Execution:**
- [x] `scripts/linux-audio-runtime.mjs` — require exact installed daemon filename; retain all ELF and dependency validation.
- [x] `.github/workflows/release.yml` — use existing-directory discovery for DEB/RPM and immutable candidate checksums, preserving strict failure behavior.
- [x] `scripts/tests/linux-audio-runtime.test.mjs` — add desktop-file-first, metadata-only and invalid-executable regression cases.
- [x] `scripts/tests/release-contract.test.mjs` — execute discovery fixtures with either target directory present, missing packages and no target directories; retain RPM contract checks.

**Acceptance Criteria:**
- Given extracted DEB or RPM contents containing an autostart desktop file, when bundle verification runs, then only the actual daemon is passed to ELF inspection.
- Given a release with one target directory present, when package verification and candidate checksum collection run, then the absent alternative directory does not abort the job.
- Given the existing default Linux build configuration, when release packaging runs, then DEB, AppImage and Fedora RPM remain selected and RPM verification remains mandatory.

## Spec Change Log

## Verification

**Commands:**
- `rtk node --test scripts/tests/linux-audio-runtime.test.mjs scripts/tests/release-contract.test.mjs scripts/tests/release-runtime-contract.test.mjs scripts/tests/tauri-platform-config.test.mjs` — focused runtime and release regressions pass.
- `rtk git diff --check` — no whitespace errors.

The current host is macOS. Real Linux ELF/package extraction and Fedora installation require a Linux environment or the subsequent CI run; report that limit separately from local fixture results.

**Implementation evidence (2026-10-05):** The focused command completed with 77 passing tests, zero failures and one existing Linux-only native ELF staging test skipped on macOS. New shell fixtures execute the workflow's actual DEB/RPM discovery and candidate checksum scripts with each target directory present alone, package filenames containing spaces, missing packages and no target directories. The checksum script rejects an empty artifact list before invoking xargs. Desktop metadata never reaches the injected ELF inspector, a missing exact daemon fails explicitly, and an exact-name invalid daemon's ELF error propagates unchanged. Existing Fedora RPM resource/dependency and required extraction-verification contracts pass. `rtk git diff --check` completed successfully. Real Linux extraction and Fedora installation remain unverified locally.

**Review evidence:** Independent blind, edge-case and acceptance reviewers found no actionable defects. The acceptance reviewer independently reproduced the focused test results. No deferred findings.

## Suggested Review Order

**Daemon identification**

- Select the installed executable exactly, excluding autostart metadata while retaining strict ELF validation.
  [linux-audio-runtime.mjs:305](../../scripts/linux-audio-runtime.mjs#L305)

**Artifact discovery**

- Search existing build directories so DEB and RPM verification survive either target layout.
  [release.yml:372](../../.github/workflows/release.yml#L372)

- Collect checksums only after confirming bundle files exist; preserve strict pipeline failures.
  [release.yml:500](../../.github/workflows/release.yml#L500)

**Regression coverage**

- Cover desktop-file collisions, missing executables and propagated ELF failures.
  [linux-audio-runtime.test.mjs:319](../../scripts/tests/linux-audio-runtime.test.mjs#L319)

- Execute workflow discovery and checksum scripts against temporary build layouts.
  [release-contract.test.mjs:153](../../scripts/tests/release-contract.test.mjs#L153)
