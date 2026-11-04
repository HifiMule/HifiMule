---
title: 'Consume a prebuilt FFmpeg runtime from a pinned submodule'
type: 'chore'
created: '2026-11-04'
status: 'done'
baseline_commit: 'b0e0141440d96739f84ca1294726687f4eabfa58'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Fresh HifiMule runners repeatedly compile unchanged FFmpeg and codec dependencies.

**Approach:** Use the existing `HifiMule/ffmpeg-runtime` submodule at `dependencies/ffmpeg-runtime`, and publish archives for the four shipping targets. HifiMule consumes checksum-pinned archives during ordinary builds.

## Boundaries & Constraints

**Always:** Preserve FFmpeg version/capabilities, shared linking, Windows MSVC CRT, Ubuntu 22.04 baseline and macOS minimums. Include headers, linking/shared libraries, executable, receipts and licenses. Keep playback policy and Rust binding pins in HifiMule. Version recipes independently; use existing Node/platform tools.

**Ask First:** New targets, capability/license changes, or private visibility. Spec approval authorizes publishing the public dependency repository and initial dependency release, excluding application releases.

**Never:** Commit binaries, consume `latest`/branch tips, silently compile after download failure, remove package checks, or switch CI before all four assets validate.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|---------------------------|----------------|
| Fresh | Supported target | Fetch, checksum, extract, relocate, validate, export environment | Fail before Cargo |
| Reuse | Same release prefix | No fetch/compilation | Reject stale receipt |
| Override | Explicit prefix | Validate and reuse | Explain mismatch |
| Bad archive | Missing, corrupt, unsafe or interrupted | Preserve installed prefix; no partial installation | Clean staging; fail |
| Missing submodule | Uninitialized clone | Explain initialization command | Never fetch branch tip |
| Unsupported | No locked target | Explain supported targets/source-build route | Never substitute host asset |

</frozen-after-approval>

## Code Map

- `scripts/{linux,macos,windows}-audio-runtime.mjs`: provisioning, validation and app packaging.
- `scripts/{encoding-dependencies,runtime-lock}.mjs`: codec builds and locking.
- `scripts/{build-daemon,prepare-sidecar}.mjs`: build/staging callers.
- `hifimule-daemon/audio-runtime.json`: mixed dependency/app manifest, also packaged evidence.
- `.github/workflows/{build,release}.yml`: consumers.

## Tasks & Acceptance

**Execution:**
- [x] `dependencies/ffmpeg-runtime/manifest.json`, `scripts/`, `tests/` — extract standalone source builders and codec recipes with a build-only manifest and explicit source-build command. Preserve upstream checksum/signature checks and configure settings; leave app bundling in HifiMule.
- [x] Producer `scripts/package-runtime.mjs`, `.github/workflows/release.yml` — build Windows x64, Linux x64, macOS x64/ARM64 on versioned tags; support manual validation. Record commit, target, build identity and toolchain. Preserve symlinks/modes; extract elsewhere and verify ABI/capabilities plus an encode/decode smoke. Publish assets/checksums only after all targets pass; recipe/toolchain changes require a new release revision.
- [x] `.gitmodules`, `dependencies/ffmpeg-runtime` — use HTTPS and pin producer commit. Bootstrap/publish `ffmpeg-9.0.2-r2`; record actual checksums after building.
- [x] `hifimule-daemon/audio-runtime-lock.json`, `scripts/prebuilt-audio-runtime.mjs` — pin release, producer commit, build identity and asset SHA-256 per target. Check submodule HEAD and build-relevant app requirements against the lock/producer manifest. Verify checksum before safe extraction; use locking, staging and atomic installation. Key reuse by release/target.
- [x] `scripts/{linux,macos,windows}-audio-runtime.mjs`, `scripts/encoding-dependencies.mjs` — replace default source builds, preserving packaging exports and overrides. Separate compiler preflight from validation. Relocate Unix pkg-config metadata and macOS Mach-O paths, including executable dependencies; verify no producer paths remain and Linux ffmpeg resolves its shared libraries.
- [x] `.github/workflows/{build,release}.yml` — initialize submodules; consume locked assets. Remove FFmpeg-only build tools while retaining Rust, bindgen, relocation and packaging prerequisites. Preserve older candidate refs without submodules.
- [x] `scripts/tests/{prebuilt-audio-runtime,release-runtime-contract,build-daemon,linux-audio-runtime,macos-audio-runtime,windows-audio-runtime}.test.mjs`, producer `tests/` — cover matrix errors, relocation, identity mismatch, concurrency and absence of ordinary source compilation. Move builder tests to producer ownership.
- [x] `README.md`, `docs/development-guide.md`, `docs/release-guide.md`, producer `README.md` — document recursive cloning, explicit source builds, producer releases, consumer pin updates and package verification.

**Acceptance Criteria:**
- Given initialized submodules, when a fresh build runs, then all four targets consume locked binaries without FFmpeg/codec compilation.
- Given changed producer configuration, when its release and consumer pins update, then matching assets pass and incompatible old assets fail.
- Given extracted archives, when daemon/mediacheck packages build, then existing ABI/capability/package checks pass without producer paths.

## Spec Change Log

- 2026-11-04: User approved the plan and supplied the existing submodule. Reuse its repository and staged gitlink rather than creating a duplicate.
- 2026-11-04: First real Linux/macOS archive checks exposed absolute pkg-config library/include paths surviving relocation. Rewrite all producer-prefix references and cover realistic FFmpeg metadata in regression tests. Publish revision 2 rather than moving the failed revision-1 tag; preserve cross-platform verification before app activation.

## Design Notes

Submodule source and release assets have separate pins: checksums cannot live in the commit that produced their archive. Hash only build fields; changing app buffer policy must not require rebuilding FFmpeg.

## Verification

- `rtk node --test scripts/tests/*.test.mjs` — existing packaging regressions and new consumer cases pass.
- `rtk node --test dependencies/ffmpeg-runtime/tests/*.test.mjs` — standalone producer checks pass.
- Dependency CI: validate all four relocated assets and retain logs/metadata.
- Candidate CI: build/verify all four packages using real assets. Distinguish CI evidence from local checks.

Local evidence: producer suite passed under Node 24 (41 tests); parent script suite passes with one Linux-native skip on macOS. All four published revision-2 assets are pinned to recreated producer commit 32b2e45088281409c40122350cec07f62717dd33 and their actual SHA-256 values. Real ARM64 consumer download, installation, ABI/capability checks, encode/decode smoke and cached reuse pass. Final relocation runs at the destination under the runtime lock, avoiding an otool heading false positive, with failure cleanup covered. Application candidate package CI on all four targets remains a pre-shipping verification step; it has not been run for this uncommitted parent change.

## Review Triage

- Patched consumer lock disappearance race, interrupted-install recovery, final relocation cleanup, and bounded Windows rename retries; regressions pass.
- Confirmed all four archive checksums, embedded producer commits, targets and ABI receipts against the release lock. Actual ARM64 installation, encode/decode and cached reuse pass.
- Recorded inherited source-cache concurrency and whitespace-path parser hardening in deferred-work.md. Controlled producer CI uses isolated checkouts. Windows alias attacks require replacing a trusted checksum-pinned producer asset and are outside the reachable artifact contract.
- Application candidate package CI remains required before shipping; this local implementation does not publish an application release.

## Suggested Review Order

**Runtime selection**

- Provision the locked SDK once and retain existing platform verification.
  [prebuilt-audio-runtime.mjs:38](../../scripts/prebuilt-audio-runtime.mjs#L38)

- Bind every archive to the submodule commit and build configuration.
  [prebuilt-audio-runtime.mjs:15](../../scripts/prebuilt-audio-runtime.mjs#L15)

- Pin the published release and real checksums for all four targets.
  [audio-runtime-lock.json:2](../../hifimule-daemon/audio-runtime-lock.json#L2)

**Concurrency and recovery**

- Retry disappearing locks while preserving exclusive installation ownership.
  [runtime-lock.mjs:4](../../scripts/runtime-lock.mjs#L4)

- Recover interrupted installs and retain clear errors for damaged completed caches.
  [prebuilt-audio-runtime.mjs:56](../../scripts/prebuilt-audio-runtime.mjs#L56)

**Platform integration**

- Keep overrides and package validation while selecting prebuilt provisioning.
  [macos-audio-runtime.mjs:75](../../scripts/macos-audio-runtime.mjs#L75)

- Delegate deliberate source builds to the canonical submodule recipes.
  [source-audio-runtime.mjs:7](../../scripts/source-audio-runtime.mjs#L7)

**Verification and workflow**

- Initialize pinned recipes and skip source-only tools for locked runtimes.
  [build.yml:37](../../.github/workflows/build.yml#L37)

- Cover recovery and Windows publication retries without compiling dependencies.
  [prebuilt-audio-runtime.test.mjs:71](../../scripts/tests/prebuilt-audio-runtime.test.mjs#L71)

- Reproduce the lock-release race deterministically.
  [runtime-lock.test.mjs:38](../../scripts/tests/runtime-lock.test.mjs#L38)

**Documentation**

- Document dependency releases and the corresponding consumer pin update.
  [release-guide.md:5](../../docs/release-guide.md#L5)
