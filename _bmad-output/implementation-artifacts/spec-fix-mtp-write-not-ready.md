---
title: 'Fix MTP write_file: 0x80070015 after device.Open() succeeds'
type: 'bugfix'
created: '2026-05-03'
status: 'done'
baseline_commit: '5e1e5a1'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** After applying the COM threading fix (`session()` per-call), `device.Open()` now succeeds, but the subsequent WPD write operations in `WpdHandle::write_file` still fail with `0x80070015 (ERROR_NOT_READY)`. Two root causes are implicated: (1) `write_with_verify` writes the dirty-marker sentinel as `b""` (0 bytes); several WPD drivers, including Garmin's, reject `CreateObjectWithPropertiesAndData` when `WPD_OBJECT_SIZE = 0`. (2) `write_file` calls `device.Content()` three separate times in one session (inside `ensure_dir_chain`, inline, and inside `find_child_object_id`), which can confuse drivers that don't tolerate concurrent content-interface references.

**Approach:** (1) Fix `MtpBackend::write_with_verify` to write a 1-byte dirty sentinel instead of empty bytes. (2) Refactor `write_file` to obtain `IPortableDeviceContent` once and pass it into `ensure_dir_chain` and `find_child_object_id` instead of letting each helper re-acquire it. (3) Add per-call `daemon_log!` around each WPD call in `write_file` so future failures have a precise call-site in the log.

## Boundaries & Constraints

**Always:**
- The dirty-marker strategy (write sentinel → write payload → delete sentinel) is preserved in `write_with_verify`; only the sentinel payload changes from `b""` to `b"\x00"`.
- `ensure_dir_chain` and `find_child_object_id` keep their same logic; only their signature changes from `device: &IPortableDevice` to `content: &IPortableDeviceContent`.
- Logging uses `crate::daemon_log!` (no new dependencies).
- Only `jellyfinsync-daemon/src/device/mtp.rs` and `jellyfinsync-daemon/src/device_io.rs` are modified.

**Ask First:**
- If `read_file`, `delete_file`, or `list_files` also fail to compile after the content-threading refactor (they call helpers that now require a content ref), halt and ask before extending the refactor to those methods.

**Never:**
- Do not add `daemon_log!` to `enumerate()` or helpers that are on hot paths (called thousands of times per sync).
- Do not skip or redesign the dirty-marker resilience mechanism.
- Do not change the `MtpHandle` trait or `DeviceIO` trait.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Write dirty marker (new design) | `write_with_verify(".json", bytes)` | `.json.dirty` created with 1 byte, `.json` written, `.json.dirty` deleted | Error on any step propagates |
| `write_file` with empty parent (root write) | `path=".jellyfinsync.json.dirty"`, `data=b"\x00"` | Single `device.Content()` call; `CreateObjectWithPropertiesAndData` with `WPD_OBJECT_SIZE=1` | `?` on each WPD call; log identifies failing call |
| `write_file` with nested path | `path="Music/track.mp3"` | `ensure_dir_chain` creates `Music/` if missing; single content ref reused | Error propagates; intermediate dir creation logged |

</frozen-after-approval>

## Code Map

- `../../jellyfinsync-daemon/src/device_io.rs:246-252` — `MtpBackend::write_with_verify`; dirty sentinel written as `b""` here
- `../../jellyfinsync-daemon/src/device/mtp.rs:574-648` — `WpdHandle::write_file`; calls `ensure_dir_chain` and `find_child_object_id`; gets `device.Content()` inline
- `../../jellyfinsync-daemon/src/device/mtp.rs:396-451` — `ensure_dir_chain(device: &IPortableDevice, ...)`; gets its own `device.Content()` — to be refactored
- `../../jellyfinsync-daemon/src/device/mtp.rs:310-363` — `find_child_object_id(device: &IPortableDevice, ...)`; gets its own `device.Content()` — to be refactored

## Tasks & Acceptance

**Execution:**

- [x] `jellyfinsync-daemon/src/device_io.rs` — In `MtpBackend::write_with_verify` (line ~248), change `self.write_file(&dirty_marker, b"").await?` to `self.write_file(&dirty_marker, b"\x00").await?`. One character change; no logic change.

- [x] `jellyfinsync-daemon/src/device/mtp.rs` — Change `ensure_dir_chain` signature from `(device: &IPortableDevice, components: &[&str])` to `(content: &IPortableDeviceContent, components: &[&str])`. Remove the `let content = device.Content()?;` line at the top of the function body — the caller now passes `content` in. `IPortableDeviceContent` must be imported in scope (it already is via `windows::Win32::Devices::PortableDevices::*`).

- [x] `jellyfinsync-daemon/src/device/mtp.rs` — Change `find_child_object_id` signature from `(device: &IPortableDevice, parent_id: &str, name: &str)` to `(content: &IPortableDeviceContent, parent_id: &str, name: &str)`. Remove `let content = device.Content()?;` from its body.

- [x] `jellyfinsync-daemon/src/device/mtp.rs` — Rewrite `write_file` call sites to use the single-content pattern. Before the `unsafe {}` block, add `let content = device.Content()?;`. Change `ensure_dir_chain(&device, parent_components)?` to `ensure_dir_chain(&content, parent_components)?`. Change `find_child_object_id(&device, &parent_id_str, filename)?` to `find_child_object_id(&content, &parent_id_str, filename)?`. The `content.Delete(...)` and `content.CreateObjectWithPropertiesAndData(...)` calls already use this same `content` and are unchanged.

- [x] `jellyfinsync-daemon/src/device/mtp.rs` — Add `daemon_log!` calls in `write_file` (inside the `unsafe {}` block, before each major WPD call): before `content.CreateObjectWithPropertiesAndData(...)`, log `"[WPD] write_file: CreateObjectWithPropertiesAndData path={} size={}"`. Before `data_stream.Commit(...)`, log `"[WPD] write_file: Commit"`. After each call returns `Ok(...)`, log `"[WPD] write_file: <call> OK"`. Use the pattern `crate::daemon_log!(...)`.

**Acceptance Criteria:**
- Given an unrecognized Garmin MTP device is connected, when the user submits the Initialize form, then `write_with_verify` completes without `0x80070015` and the device transitions to recognized state.
- Given `cargo build --manifest-path jellyfinsync-daemon/Cargo.toml`, then zero errors, zero new warnings.
- Given `cargo test --manifest-path jellyfinsync-daemon/Cargo.toml`, then all existing tests pass (including `mtp_write_with_verify_dirty_marker_sequence`).
- Given any WPD call in `write_file` fails, then the `daemon_log!` immediately before it appears in the log, making the failing call identifiable without a debugger.

## Spec Change Log

## Design Notes

**Why size=0 causes `ERROR_NOT_READY`:** WPD drivers for MTP devices sometimes validate `WPD_OBJECT_SIZE` before allocating the transfer stream. A zero-size value is outside the normal transfer range and some drivers surface the "device not ready" HRESULT rather than a more descriptive error. Using 1 byte sidesteps the validation.

**Why single content handle:** `IPortableDeviceContent` is a view into the device's object tree. Acquiring it three times in one session (via three `device.Content()` calls) creates three reference-counted COM pointers to the same underlying object. On conformant drivers this is harmless, but the Garmin WPD driver is known to behave unexpectedly with concurrent content handles open. Using one handle per session matches the WPD programming model recommended by MSDN.

## Suggested Review Order

**Root cause fix — 1-byte dirty sentinel**

- The change that unblocks initialization: `WPD_OBJECT_SIZE=0` workaround.
  [`device_io.rs:249`](../../jellyfinsync-daemon/src/device_io.rs#L249)

**Single content handle — write_file refactor**

- Entry point: `write_file` acquires one `IPortableDeviceContent` for the whole call.
  [`mtp.rs:581`](../../jellyfinsync-daemon/src/device/mtp.rs#L581)

- `ensure_dir_chain` now takes caller-supplied content; no extra `device.Content()` inside.
  [`mtp.rs:395`](../../jellyfinsync-daemon/src/device/mtp.rs#L395)

- `find_child_object_id` likewise; removes the second redundant `device.Content()` call.
  [`mtp.rs:310`](../../jellyfinsync-daemon/src/device/mtp.rs#L310)

**Diagnostic logging**

- Log before and after `CreateObjectWithPropertiesAndData` to pinpoint future failures.
  [`mtp.rs:612`](../../jellyfinsync-daemon/src/device/mtp.rs#L612)

- Log before and after `Commit` to distinguish stream-write vs. commit failures.
  [`mtp.rs:643`](../../jellyfinsync-daemon/src/device/mtp.rs#L643)

## Verification

**Commands:**
- `rtk cargo build --manifest-path jellyfinsync-daemon/Cargo.toml` — expected: zero errors, zero new warnings
- `rtk cargo test --manifest-path jellyfinsync-daemon/Cargo.toml` — expected: all tests pass
