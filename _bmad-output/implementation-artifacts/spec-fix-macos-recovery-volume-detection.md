---
title: 'Exclude hidden macOS volumes from device discovery'
type: 'bugfix'
created: '2026-09-17'
status: 'done'
baseline_commit: '643d3c096ae13f8fb2e2717aa331787e98168354'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** HifiMule offers to configure `/Volumes/Recovery` as a music device on this machine. The macOS scanner treats writable mounts under `/Volumes` as candidates after excluding the boot filesystem. Recovery is a separate writable APFS mount, so those checks do not exclude it.

**Approach:** Respect the macOS `MNT_DONTBROWSE` mount flag during automatic discovery. Preserve the existing exclusion of read-only mounts and the root filesystem, and skip candidates when mount metadata cannot be obtained reliably. Apply this before probing manifests or emitting device events.

## Boundaries & Constraints

**Always:** Use filesystem metadata rather than a volume-name blacklist. Preserve discovery of ordinary writable, browsable music devices, including a user device named Recovery. Keep the change within macOS mass-storage discovery. Retain existing boot-volume and actual-mount checks. Treat metadata failures as a skipped candidate for the current scan; the normal polling loop can reconsider it later.

**Ask First:** Broader product changes, such as adding a manual override for hidden volumes or changing which internal/external storage classes are eligible.

**Never:** Write to, initialize, rename, or unmount Recovery during diagnosis or verification. Change Linux, Windows, or MTP discovery behavior. Add a dependency or launch a subprocess on every scan merely to obtain flags already exposed by libc.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Recovery on this Mac | Writable APFS mount with `MNT_DONTBROWSE` | Excluded before manifest probing | No device event |
| Ordinary player | Writable mount without excluded flags | Remains eligible for normal probing | Existing behavior |
| Player named Recovery | Ordinary writable, browsable mount named Recovery | Remains eligible | No name blacklist |
| Read-only image | `MNT_RDONLY`, with or without `MNT_DONTBROWSE` | Excluded | No initialization prompt |
| Hidden managed volume | `MNT_DONTBROWSE`, existing manifest | Excluded from automatic discovery | No automatic device event |
| Metadata race | Failed `statfs` or invalid C path | Skip current candidate | Retry through subsequent scans |
| Boot filesystem | Candidate device ID equals root device ID | Still excluded | Preserve fail-safe behavior |
| Unrelated flags | Writable mount with other flags | Remains eligible | Do not reject all nonzero flags |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/device/mod.rs` — `get_mounts()` enumerates `/Volumes`; `is_boot_volume_device()` and `is_mount_point()` provide current identity checks. `is_readonly_mount()` currently uses `statvfs` and returns false on errors. Discovery candidates eventually feed the observer's manifest probing and unrecognized-device events.
- `hifimule-daemon/src/device/tests.rs` — device regression tests, including root-device identity and metadata-failure cases near the Story 7.4 tests.
- `_bmad-output/implementation-artifacts/spec-fix-macos-readonly-volume-filter.md` — rationale for the existing read-only DMG exclusion; that behavior must survive.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/device/mod.rs` — replace the read-only-only helper with a macOS mount-eligibility check using `libc::statfs` and its `f_flags`; reject `MNT_RDONLY` and `MNT_DONTBROWSE`, and fail closed on unavailable metadata. Integrate at the existing filter location.
- [x] `hifimule-daemon/src/device/tests.rs` — add macOS regression coverage for the matrix, including combined flags, unrelated flags, and invalid/missing paths. Exercise the production predicate; keep any pure flag helper small and testable. Preserve existing boot-volume tests.
- [x] `hifimule-daemon/src/device/tests.rs` — add a read-only discovery check against this machine's Recovery mount when it is present with the relevant flag, verifying its absence from candidates without requiring that mount on other test machines.

**Acceptance Criteria:**
- Given this Mac's writable Recovery mount marked `nobrowse`, when the scanner enumerates candidates, then `/Volumes/Recovery` is absent and cannot trigger a configuration prompt through discovery.
- Given an ordinary writable browsable music volume, when the scanner evaluates it, then the new filter preserves its eligibility regardless of its name.
- Given a candidate whose flags cannot be read, when discovery runs, then it is skipped without terminating the scan or admitting an unknown mount.
- Given a macOS build, when device regression tests run, then existing boot-volume protections and new mount-filter tests pass.

## Spec Change Log

## Design Notes

Read-only local evidence from `/sbin/mount` on 2026-09-17 reports `/dev/disk3s3 on /Volumes/Recovery (apfs, local, journaled, nobrowse)`. Root is `/dev/disk3s1s1` and read-only. This explains why the existing root identity and read-only checks miss Recovery. Local installed libc sources expose both `statfs` and `MNT_DONTBROWSE`; use the native flag constants, not hard-coded numeric values. `diskutil info` was unavailable in the current execution environment, but mount output is sufficient to establish the relevant flags.

Respecting `nobrowse` intentionally also excludes user-created mounts explicitly hidden with that flag. This is an automatic-discovery policy, not a universal detector for every macOS system volume. Do not claim it covers system volumes mounted without that flag.

## Verification

**Commands:**
- `rtk cargo test -p hifimule-daemon device::tests` — existing and new device tests pass on macOS.
- `rtk cargo fmt --all -- --check` — no new formatting violations.
- `rtk git diff --check` — no whitespace errors.

Record any environment or pre-existing build blockers separately. Live verification must only read mount metadata and enumerate candidate paths; it must not invoke initialization or run a daemon that could automatically synchronize attached devices.

**Implementation results (2026-09-17):**
- The raw Cargo test command is blocked by the repository build guard requiring controlled FFmpeg verification. The supported equivalent, `rtk npm run build:daemon -- test -p hifimule-daemon device::tests`, passed all 93 device tests on macOS, including four new mount-discovery regressions and existing boot-volume tests.
- `rtk cargo fmt --all -- --check` and `rtk git diff --check` passed.
- Read-only `/sbin/mount` output still reports `/Volumes/Recovery` as APFS with `nobrowse`; the conditional live discovery regression passed. No initialization or daemon synchronization was invoked.
- Existing unrelated dead-code warnings in API/vault code and a future-incompatibility warning for `block v0.1.6` remain.

## Review results

Adversarial, edge-case, and acceptance reviews found no production defects or acceptance violations. Test-strength suggestions were considered: the independent flag matrix already establishes eligibility policy; temporary-directory tests deliberately isolate name/manifest independence; conditional live coverage is intentional across machines and corroborated here by mount output showing the required Recovery mount and flag. No scope changes or deferred defects.

## Suggested Review Order

- Exclude unsafe candidates before probing manifests or offering initialization.
  [mod.rs:2357](../../hifimule-daemon/src/device/mod.rs#L2357)

- Read native mount flags and skip candidates whose metadata is unavailable.
  [mod.rs:2311](../../hifimule-daemon/src/device/mod.rs#L2311)

- Preserve read-only filtering and exclude macOS hidden volumes.
  [mod.rs:2327](../../hifimule-daemon/src/device/mod.rs#L2327)

- Verify flags, metadata failures, name independence, and live Recovery exclusion.
  [tests.rs:2351](../../hifimule-daemon/src/device/tests.rs#L2351)

