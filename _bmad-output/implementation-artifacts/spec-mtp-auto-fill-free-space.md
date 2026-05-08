---
title: 'MTP auto-fill free space lookup'
type: 'bugfix'
created: '2026-05-03'
status: 'done'
baseline_commit: 'NO_VCS'
context: []
---

<frozen-after-approval reason="human-owned intent - do not modify unless human renegotiates">

## Intent

**Problem:** Starting sync on an initialized MTP device can fail with `Failed to start sync: Auto-fill: could not determine device free space` when auto-fill is enabled and no explicit fill limit is saved.

**Approach:** Make daemon storage lookup fall back to the selected `DeviceIO.free_space()` backend when filesystem disk APIs cannot inspect the selected path, which is expected for `mtp://...` virtual paths.

## Boundaries & Constraints

**Always:** Preserve existing filesystem storage reporting for MSC devices. Preserve auto-fill budget behavior when `maxBytes` is explicitly supplied.

**Ask First:** Any change that disables auto-fill automatically or changes user preferences.

**Never:** Do not fake a huge capacity, start an unbounded auto-fill, or bypass capacity checks silently.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| MTP device | Selected path is `mtp://...`; filesystem capacity API returns `None`; backend reports free bytes | `device_get_storage_info` returns non-null storage info and auto-fill can calculate delta | If backend free-space fails, existing `None` behavior remains |
| MSC device | Selected path is a mounted filesystem | Existing total/free/used values are returned | No behavior change |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/device/mod.rs` -- owns `DeviceManager.get_device_storage()` and `StorageInfo`.
- `hifimule-daemon/src/device_io.rs` -- defines backend `free_space()` used by MTP and MSC implementations.
- `hifimule-daemon/src/rpc.rs` -- `sync_calculate_delta` and `basket.autoFill` consume `get_device_storage()`.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/device/mod.rs` -- add `DeviceIO.free_space()` fallback in `get_device_storage()` -- allows MTP capacity lookup.

**Acceptance Criteria:**
- Given a selected MTP device whose backend reports free space, when `sync_calculate_delta` expands auto-fill without `maxBytes`, then it does not fail with `could not determine device free space`.
- Given a mounted MSC device, when `device_get_storage_info` is called, then total/free/used values still come from filesystem storage info.

## Verification

**Commands:**
- `rtk cargo check -p hifimule-daemon` -- expected: daemon compiles.

## Suggested Review Order

- Keep filesystem storage reporting first for mounted devices.
  [`mod.rs:467`](../../hifimule-daemon/src/device/mod.rs#L467)

- Fall back to the active backend for MTP-style virtual paths.
  [`mod.rs:471`](../../hifimule-daemon/src/device/mod.rs#L471)

- Return bounded free-space data when total capacity is unavailable.
  [`mod.rs:479`](../../hifimule-daemon/src/device/mod.rs#L479)
