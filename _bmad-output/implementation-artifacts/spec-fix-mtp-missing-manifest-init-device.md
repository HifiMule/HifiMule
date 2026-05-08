---
title: 'Fix MTP Missing Manifest Init Device'
type: 'bugfix'
created: '2026-05-08'
status: 'done'
context: []
baseline_commit: '33de8e186efb615b7ce07b860d8c4ee6e20bfe6e'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** When an MTP device opens successfully but has no `.hifimule.json`, discovery logs `Manifest read failed` and returns without emitting an unrecognized-device event. The UI therefore never receives a pending device path/friendly name and cannot list the device as available for initialization.

**Approach:** Treat an MTP root missing `.hifimule.json` the same way the mass-storage observer treats a removable drive with no manifest: emit `DeviceEvent::Unrecognized` and allow the observer to mark the MTP device as known until removal. Keep genuine transient MTP read/open failures retryable so a temporarily unavailable device is not hidden.

## Boundaries & Constraints

**Always:** Preserve the existing managed-device path for valid manifests, including cached `storage_id` re-open behavior. Preserve the parse-error behavior that emits `Unrecognized`. Preserve retry behavior for non-missing read failures that may succeed on the next poll.

**Ask First:** If the implementation requires changing the daemon/UI JSON shape beyond the existing `pendingDevicePath` and `pendingDeviceFriendlyName` fields, halt and confirm the new contract.

**Never:** Do not create a placeholder manifest during discovery. Do not add the missing-manifest MTP device to `connected_devices` before `device_initialize` succeeds. Do not change MSC discovery semantics.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| MTP no manifest | `emit_mtp_probe_event` receives a backend whose `.hifimule.json` read fails because the component/file is missing | Sends `DeviceEvent::Unrecognized` with synthetic `mtp://...` path, stored backend, and friendly name; returns `true` so the observer does not spam repeated events | Existing init UI can call `device_initialize` against pending MTP backend |
| MTP transient read failure | Backend fails reading `.hifimule.json` for a reason other than not found/missing path component | Logs the read failure and returns `false` | Device remains retryable on the next observer poll |
| MTP malformed manifest | `.hifimule.json` exists but cannot parse as `DeviceManifest` | Existing parse-error path emits `DeviceEvent::Unrecognized` and returns `true` | No regression |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/device/mod.rs` -- Defines `DeviceEvent`, MTP observer polling, and `emit_mtp_probe_event`; this is where the missing-manifest branch currently returns without emitting an event.
- `hifimule-daemon/src/device/tests.rs` -- Contains MTP probe unit tests for retryable read failures and parse failures; add coverage for missing-manifest detection.
- `hifimule-daemon/src/rpc.rs` -- `get_daemon_state` exposes pending unrecognized devices through `pendingDevicePath` and `pendingDeviceFriendlyName`; no contract change expected.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/device/mod.rs` -- Distinguish missing-manifest MTP read errors from other read failures and emit `DeviceEvent::Unrecognized` for the missing-manifest case -- restores the init path for fresh MTP devices.
- [x] `hifimule-daemon/src/device/tests.rs` -- Add/adjust tests so missing `.hifimule.json` on MTP emits `Unrecognized` and returns `true`, while non-missing read failures still return `false` -- prevents regression in retry semantics.

**Acceptance Criteria:**
- Given an MTP device opens successfully and has no `.hifimule.json`, when discovery probes the manifest, then the daemon stores it as the pending unrecognized device and `get_daemon_state` can expose it to the UI for initialization.
- Given an MTP device has a valid manifest, when discovery probes it, then it is still emitted as a managed `Detected` device.
- Given an MTP manifest read fails for a non-missing/transient reason, when discovery probes it, then the device remains retryable and no unrecognized event is emitted.

## Spec Change Log

## Verification

**Commands:**
- `rtk cargo test -p hifimule-daemon test_mtp_probe_missing_manifest_emits_unrecognized_and_marks_known` -- expected: new focused test passes.
- `rtk cargo test -p hifimule-daemon test_mtp_probe_read_failure_does_not_mark_known` -- expected: existing retry behavior still passes.
- `rtk cargo test -p hifimule-daemon test_mtp_probe_parse_failure_emits_unrecognized_and_marks_known` -- expected: existing malformed-manifest behavior still passes.

## Suggested Review Order

**Discovery Behavior**

- Missing manifest classification keeps fresh MTP devices init-ready.
  [`mod.rs:1402`](../../hifimule-daemon/src/device/mod.rs#L1402)

- The probe now emits pending initialization instead of disappearing.
  [`mod.rs:1464`](../../hifimule-daemon/src/device/mod.rs#L1464)

**Regression Tests**

- Focused coverage for a fresh MTP device without `.hifimule.json`.
  [`tests.rs:1881`](../../hifimule-daemon/src/device/tests.rs#L1881)

- Fake transient backend preserves retry semantics for real read failures.
  [`tests.rs:43`](../../hifimule-daemon/src/device/tests.rs#L43)

- Existing retry test now exercises a non-missing failure.
  [`tests.rs:1914`](../../hifimule-daemon/src/device/tests.rs#L1914)
