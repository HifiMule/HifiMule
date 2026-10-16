---
title: 'Initialize mass-storage devices without scanning protected children'
type: 'bugfix'
created: '2026-10-16'
status: 'done'
baseline_commit: 'a2195e7d1a8c15464cd8f00ef4e03633628634a3'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Initializing FLASHDISK fails with EPERM because the mass-storage liveness check recursively enumerates all device contents. macOS grants removable-volume access, but protected system directories can require additional permissions; failure in a child is incorrectly presented as a disconnected device.

**Approach:** Check accessibility of the mass-storage root without descending into children. Preserve early rejection of genuinely unavailable devices and make root-access errors informative. The user authorized this fix with “go” following the investigation and proposed shallow-root approach.

## Boundaries & Constraints

**Always:** Route device operations through DeviceIO and implement filesystem behavior inside MscBackend. Preserve managed-folder validation, manifest persistence, initialization reservation/cancellation, and MTP storage-root checks. Root-check success establishes accessibility only; subsequent writes still propagate their own failures. Preserve existing staged playback/main/RPC work.

**Ask First:** Installing the modified application or changing macOS privacy permissions requires a separate user request.

**Never:** Recursively scan a drive as a liveness check, broadly suppress filesystem errors, change general recursive media listing semantics, grant Full Disk Access, touch device contents during automated verification, or commit unrelated edits.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Empty root | Accessible directory with no children | Initialization proceeds and writes manifest | Normal write failures remain visible |
| Protected child | Accessible root containing unreadable child directory | Initialization proceeds without entering child | Child permission denial cannot fail root probe |
| Missing root | Device disappears after discovery | Initialization fails before managed-folder/manifest creation | Preserve underlying cause with device path |
| Unreadable root | Directory itself denies enumeration | Initialization fails without registering device | Explain root-access failure rather than asserting disconnection |
| Invalid root | Root resolves to a file or disallowed symlink | Fail before writes | Preserve existing directory safety expectations |
| MTP device | Existing object transport | Continue existing storage_id probe | Existing stale-object failure behavior retained |
| Media listing | Explicit recursive list_files request | Keep current recursive semantics | Do not silently omit inaccessible media folders |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/device_io.rs` — DeviceIO trait, MscBackend filesystem implementation, backend tests.
- `hifimule-daemon/src/device/mod.rs` — initialize_reserved_pending performs the failing root probe before writes.
- `hifimule-daemon/src/device/tests.rs` — initialization tests, filesystem fixtures and transport doubles.
- Investigation: [device initialization evidence](investigations/device-initialization-inaccessible-investigation.md).

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/device_io.rs` — introduce a root-access probe with a shallow MSC implementation; retain conservative behavior for test/other backends. Read root directory entries without opening children or querying their metadata, preserve root-directory validation, and attach root context to failures.
- [x] `hifimule-daemon/src/device/mod.rs` — replace MSC list_files("") liveness invocation with the root probe, retaining MTP path and logging; expose an accurate access error with the cause.
- [x] `hifimule-daemon/src/device/tests.rs` and backend tests — verify protected descendants cannot prevent initialization, missing/unreadable/invalid roots fail, and normal root/subfolder and MTP initialization remain intact. Restore temporary permission changes even on assertion failure.
- [x] This spec and investigation — record validation, review results, and any remaining physical-device verification gap.

**Acceptance Criteria:**
- Given an accessible device root with protected system folders, when initialization runs, then the manifest and selected managed folder are created without enumerating those protected folders.
- Given a root removed or made unreadable after discovery, when initialization runs, then no recognized device is published and no device root is recreated by later writes.
- Given an access error, when the UI receives the existing initialization RPC error, then its message describes access failure with useful path/cause context rather than asserting disconnection.
- Given an MTP device, when initialization runs, then the existing storage-ID probe and caching behavior are preserved.

## Spec Change Log

## Design Notes

Keep this check separate from list_files: recursive listing remains necessary for intentional media discovery and sync. A default root-probe implementation may preserve the previous list_files behavior for existing doubles/backends, while MscBackend must explicitly override it. MTP initialization does not call the new method and continues its existing bounded storage-ID probe.

Avoid a permissive no-op default or a storage_id-based default, because optional/default storage IDs do not prove accessibility. Do not use DeviceManager filesystem calls: the architecture requires device access through DeviceIO. Root readability does not imply writability, so manifest and managed-folder writes remain responsible for write errors. Permission-based tests should establish the denied condition and restore permissions before assertions; tests under privileged users must not claim coverage for a denial they cannot reproduce.

## Verification

**Executed validation:**
- `rtk cargo test -p hifimule-daemon initialize` — blocked by the repository's controlled FFmpeg build guard. Retried using its required build wrapper below; no environment guard was bypassed.
- `rtk npm run build:daemon -- test -p hifimule-daemon initialize -- --nocapture` — 16 passed, including root/subfolder initialization, missing and unreadable roots, protected descendants, existing MTP cache behavior, and initialization RPC tests.
- `rtk test npm run build:daemon -- test -p hifimule-daemon probe_root -- --nocapture` — all 4 focused probe tests passed.
- `rtk npm run build:daemon -- test -p hifimule-daemon device_io::tests -- --nocapture` — 32 passed. Explicit recursive media listing still reports protected-child access failure.
- Permission tests reproduced actual `PermissionDenied` on this machine; no privileged-user skips occurred. Permissions were restored by an RAII guard before assertions (also restored on unwinding).
- `rtk git diff --check` reports existing CRLF style on added lines in `device/mod.rs`; `rtk git -c core.whitespace=cr-at-eol diff --check` passes. Original file line endings were preserved to avoid whole-file churn.

Independent review: completed with three reviewers (blind adversarial, edge-case, acceptance). Edge-case and acceptance reviews found no actionable defects. Blind review raised portability/coverage and residual race concerns; triage found no new production defect. Symlink rejection matches the prior implementation, the actual permission-denial paths ran on this machine, and mount-identity/post-probe races predate this change. The conservative trait fallback is intentional; production MSC overrides it. Cosmetic duplicate error context and hypothetical test-cleanup failure do not block this fix.

Physical FLASHDISK retry requires an installed build and remains separate from source/test verification. Automated tests use temporary directories only. No application installation or physical-device writes were performed.

## Suggested Review Order

- Use shallow MSC probing and preserve actionable failures.
  [mod.rs:2379](../../hifimule-daemon/src/device/mod.rs#L2379)

- Validate the root and enumerate entries without accessing descendants.
  [device_io.rs:512](../../hifimule-daemon/src/device_io.rs#L512)

- Verify protected children and missing or unreadable roots through initialization.
  [tests.rs:2859](../../hifimule-daemon/src/device/tests.rs#L2859)

- Check backend boundaries while preserving recursive media-listing behavior.
  [device_io.rs:893](../../hifimule-daemon/src/device_io.rs#L893)
