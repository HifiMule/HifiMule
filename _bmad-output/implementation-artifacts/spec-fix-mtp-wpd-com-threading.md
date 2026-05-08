---
title: 'Fix MTP Initialize: COM Not Initialized on spawn_blocking Threads (0x80070015)'
type: 'bugfix'
created: '2026-05-02'
status: 'done'
baseline_commit: '5e1e5a1'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Clicking "Initialize" on a connected MTP device fails with `Failed to initialize device: Le périphérique n'est pas prêt. (0x80070015)`. `0x80070015 = HRESULT_FROM_WIN32(ERROR_NOT_READY)` — WPD's response when the calling thread is not in the COM multi-threaded apartment (MTA). `MtpBackend`'s `DeviceIO` methods run inside `spawn_blocking` on fresh threads that have never called `CoInitializeEx`; those threads are not COM MTA threads, so WPD rejects their calls.

**Approach:** Add a local `CoInitGuard` to each `impl MtpHandle for WpdHandle` method (and to `open()`) so every calling thread joins the MTA for the duration of the call. Remove the stored `_com_guard` from `WpdHandle` (it only ever covered the opening thread, and its `Drop` would call `CoUninitialize` on the wrong thread). Move the `create_mtp_backend` call in `run_mtp_observer` into `spawn_blocking` so the blocking `IPortableDevice::Open` no longer stalls the async worker.

## Boundaries & Constraints

**Always:**
- Local `CoInitGuard` is the **first** statement in each method body, before any WPD call.
- Propagate `CoInitGuard::init()?` — STA conflict is a hard error.
- Remove `_com_guard` from both the struct definition and the `Ok(Self { ... })` return in `open()`.
- Update the `// Safety:` comment on `unsafe impl Send / Sync for WpdHandle`.

**Ask First:**
- If `CoInitGuard` is not visible from within `impl MtpHandle for WpdHandle`, halt before changing visibility.

**Never:**
- Do not add guards to private helpers (`ensure_dir_chain`, `find_child_object_id`, `make_object_id_collection`, `path_to_object_id`, `collect_files_recursive`) — they are always called from a method that already holds a guard.
- Do not change `CoInitGuard`'s `Drop` or `init()` logic.
- Do not modify `MtpHandle` trait, `MtpBackend`, or any UI/RPC code.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| `write_file` from spawn_blocking | Fresh thread, no prior COM | Guard joins MTA → WPD call proceeds → file created on device | `?` if `CoInitGuard::init()` fails (STA conflict) |
| `read_file` from spawn_blocking | Same | Same — resolves the silent read failure in the observer loop | Same |
| `WpdHandle::open()` called | Async or blocking thread | Local guard covers COM for open; guard drops cleanly on return | `?` propagates open errors |
| `WpdHandle` dropped on any thread | Last Arc released | Only `IPortableDevice::Release()` called — no wrong-thread `CoUninitialize` | — |
| `create_mtp_backend` in spawn_blocking | Fresh blocking thread | Guard in `open()` covers it; blocking open off async thread | Log and skip device if open fails |

</frozen-after-approval>

## Code Map

- `../../hifimule-daemon/src/device/mtp.rs:13-26` — `MtpDeviceInfo` / `MtpDeviceInner`; add `#[derive(Clone)]` to both
- `../../hifimule-daemon/src/device/mtp.rs:188-210` — `WpdHandle` struct + `open()`; remove `_com_guard`, local guard in `open()`
- `../../hifimule-daemon/src/device/mtp.rs:194-197` — `unsafe impl Send/Sync for WpdHandle`; update safety comment
- `../../hifimule-daemon/src/device/mtp.rs:545-700` — `impl MtpHandle for WpdHandle` — all five methods; add local guard at entry
- `../../hifimule-daemon/src/device/mod.rs:1096-1145` — `run_mtp_observer`; wrap `create_mtp_backend` in `spawn_blocking`

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/device/mtp.rs` — Add `#[derive(Clone)]` to `MtpDeviceInfo` (line ~12) and `MtpDeviceInner` (line ~20). All fields (`String`, `u32`, `u8`) are `Clone`.

- [x] `hifimule-daemon/src/device/mtp.rs` — Remove `_com_guard: CoInitGuard` from `WpdHandle` struct (line ~191). Struct body becomes `{ device: IPortableDevice }`. Update `// Safety:` comment on `unsafe impl Send / Sync` (lines ~194–197) to: `// Safety: IPortableDevice is a COM free-threaded (MTA) in-proc interface. Every method in impl MtpHandle for WpdHandle initialises COM via a local CoInitGuard before calling any WPD API.`

- [x] `hifimule-daemon/src/device/mtp.rs` — Rewrite `WpdHandle::open()` (lines ~200–210): replace `let com_guard = CoInitGuard::init()?;` with `let _com = CoInitGuard::init()?;` (local, not stored), and change `Ok(Self { device, _com_guard: com_guard })` to `Ok(Self { device })`.

- [x] `hifimule-daemon/src/device/mtp.rs` — Add `let _com = CoInitGuard::init()?;` as the **first line** of each `impl MtpHandle for WpdHandle` method body: `read_file` (~line 545), `write_file` (~line 580), `delete_file` (~line 655), `list_files` (~line 667), `free_space` (~line 679).

- [x] `hifimule-daemon/src/device/mod.rs` — In `run_mtp_observer` (~line 1108), replace the direct `match mtp::create_mtp_backend(dev)` call. Before the match, capture `dev_clone = dev.clone()`, `dev_id = dev.device_id.clone()`, `friendly_name = dev.friendly_name.clone()`. Wrap the call: `tokio::task::spawn_blocking(move || mtp::create_mtp_backend(&dev_clone)).await.unwrap_or_else(|e| Err(anyhow::anyhow!("spawn_blocking panicked: {}", e)))`. Adjust the match arms to use `dev_id` and `friendly_name` in place of `dev.device_id` and `dev.friendly_name`. Preserve all existing `DeviceEvent` sends verbatim.

**Acceptance Criteria:**
- Given an unrecognized MTP device is connected, when the user submits the Initialize form, then `write_with_verify` completes without error and the device becomes recognized (no `0x80070015`).
- Given `cargo build --manifest-path hifimule-daemon/Cargo.toml`, then zero errors, zero new warnings.
- Given `cargo test --manifest-path hifimule-daemon/Cargo.toml`, then all existing tests pass.
- Given `WpdHandle` is dropped from any thread, then `CoUninitialize` is not called (no `_com_guard` field).
- Given `run_mtp_observer` detects a new MTP device, then `create_mtp_backend` runs in `spawn_blocking`, not on the async worker.

## Spec Change Log

## Design Notes

**Why `_com_guard` in the struct was wrong:** `CoInitGuard` initialises COM on the thread that constructs it. Stored inside `WpdHandle`, it initialised COM on the async Tokio worker that called `open()` — not on the `spawn_blocking` threads that later call `write_file` etc. Worse, its `Drop` calls `CoUninitialize` on whichever thread drops the last `Arc<WpdHandle>`, which is not the thread that called `CoInitializeEx` — undefined behaviour per the COM contract.

**Local guards are safe:** For COM MTA in-proc objects, any thread that has called `CoInitializeEx(COINIT_MULTITHREADED)` can call methods on the object directly (no marshaling). The device session established by `IPortableDevice::Open` is managed by the WPD runtime and persists for the lifetime of the `IPortableDevice` COM reference (kept alive by the `Arc`), not by thread apartment membership.

## Verification

**Commands:**
- `cargo build --manifest-path hifimule-daemon/Cargo.toml` -- expected: zero errors, zero new warnings
- `cargo test --manifest-path hifimule-daemon/Cargo.toml` -- expected: all tests pass

## Suggested Review Order

**COM threading fix — WpdHandle**

- Root of the bug: struct no longer holds `_com_guard`; safety comment explains per-call MTA contract.
  [`mtp.rs:190`](../../hifimule-daemon/src/device/mtp.rs#L190)

- `open()` uses a local guard — COM initialised for the duration of open, then dropped cleanly.
  [`mtp.rs:202`](../../hifimule-daemon/src/device/mtp.rs#L202)

- Pattern applied to all five trait methods; `read_file` is the entry point that was silently failing.
  [`mtp.rs:547`](../../hifimule-daemon/src/device/mtp.rs#L547)

- Remaining four guards: `write_file`, `delete_file`, `list_files`, `free_space`.
  [`mtp.rs:583`](../../hifimule-daemon/src/device/mtp.rs#L583)

**Async thread safety — run_mtp_observer**

- `create_mtp_backend` (blocking COM open) moved into `spawn_blocking`; clones enable the move.
  [`mod.rs:1107`](../../hifimule-daemon/src/device/mod.rs#L1107)

**Clone derivations — prerequisite for spawn_blocking**

- `#[derive(Clone)]` on `MtpDeviceInfo` and `MtpDeviceInner` enables the `dev_clone` move.
  [`mtp.rs:13`](../../hifimule-daemon/src/device/mtp.rs#L13)
