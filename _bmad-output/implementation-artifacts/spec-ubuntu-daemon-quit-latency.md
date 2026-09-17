---
title: 'Avoid Ubuntu daemon quit latency from MTP discovery'
type: 'bugfix'
created: '2026-09-17'
status: 'done'
route: 'dispatch'
review_loop_iteration: 0
context: []
baseline_commit: aa2a27bd685f3f35353814dd6cace36efb444dee
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** On Ubuntu, quitting the daemon can take several seconds longer than on other platforms. An in-flight Linux `libmtp` discovery call is currently scheduled on the daemon Tokio runtime's blocking pool, and runtime teardown waits for that non-cancellable FFI call even when no device sync is active.

**Approach:** Isolate passive MTP discovery from the core runtime shutdown path, so an in-flight enumeration cannot delay process exit. Retain the existing orderly drain for active sync and any device-mutating work.

## Boundaries & Constraints

**Always:** Preserve lifecycle ownership, shutdown fencing, authenticated health until core teardown, and safe cancellation/draining of active sync work. Discovery must still poll at its existing cadence while the daemon is running, and no discovery result may mutate state after shutdown has started.

**Never:** Do not force-cancel or abandon active MTP sync I/O, weaken the existing runtime drain for device writes, change Windows/macOS device behavior, or add a user-visible timeout that claims an unfinished sync completed.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Idle quit during enumeration | Linux MTP enumeration is blocked in `LIBMTP_Detect_Raw_Devices` | Core shutdown completes without waiting for the passive scan | Late scan result is discarded as the process exits; no device work is admitted |
| Quit during active MTP sync | A device read/write is executing | Existing cancellation, drain, recovery, and ownership-release ordering remains intact | Shutdown remains visibly pending until safe work completes |
| Normal discovery | Daemon remains running | MTP discovery continues to emit device events at the established interval | Enumeration failure remains non-fatal and is retried on the next poll |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/device/mod.rs` -- `run_mtp_observer` currently polls every two seconds and calls `tokio::task::spawn_blocking(mtp::enumerate_mtp_devices)` on the core runtime; separate passive enumeration lifetime from that runtime without changing event semantics.
- `hifimule-daemon/src/device/mtp.rs` -- Unix `enumerate_mtp_devices` calls synchronous `LIBMTP_Detect_Raw_Devices`; it is the platform-specific non-cancellable operation to keep off the core shutdown join path.
- `hifimule-daemon/src/main.rs` -- aborts observers then drops the core runtime, which joins its blocking pool; retain this behavior for actual sync/device I/O and add focused lifecycle coverage for the passive-observer exception.
- `hifimule-daemon/src/device_io.rs` -- active MTP operations deliberately use the core blocking pool and must not be moved or relaxed by this fix.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/device/mod.rs` -- run passive MTP enumeration with an independently owned, non-blocking-shutdown execution path; provide a test seam so a deliberately stalled scan can be exercised deterministically.
- [x] `hifimule-daemon/src/main.rs` -- ensure observer shutdown stops accepting its events while preserving the existing join-and-drain behavior for active sync/device writes; add a regression test proving a blocked passive scan does not delay core completion.
- [x] `hifimule-daemon/src/device/mod.rs` and/or its tests -- cover normal polling, shutdown event suppression, and error/retry behavior using the seam rather than real `libmtp` hardware.

**Acceptance Criteria:**
- Given an idle Linux daemon with MTP discovery blocked, when Quit is committed, then the core completion signal is not held by that scan and lifecycle ownership can be released without waiting seconds for `libmtp`.
- Given a Linux MTP sync has admitted device-mutating work, when Quit is committed, then it retains the existing cancellation-and-draining behavior and never exits before the work reaches a safe terminal outcome.
- Given a passive enumeration completes after observer shutdown begins, when it produces a result, then it cannot initiate device handling, auto-sync, or other new work.
- Given a running daemon, when enumeration succeeds or fails, then normal periodic discovery and non-fatal retry behavior are preserved.

## Implementation Notes

- Passive MTP enumeration now runs on a named native thread and returns its result through a oneshot receiver. The core Tokio runtime continues to own and join active MTP sync operations.
- Added observer retry, late-result suppression, and core-teardown regressions. The retry test records invocation timestamps to prove the configured delay is retained.

## Spec Change Log

## Review Triage Log

| Finding | Verdict | Evidence |
| --- | --- | --- |
| Blind Hunter: native thread creation could panic and stop discovery | false | The implementation now uses `std::thread::Builder::spawn` and logs a failed launch as a non-fatal empty poll, so the observer reaches its normal retry cadence. |
| Blind Hunter: late-result test could not emit device work | false | The regression now returns `dummy_mtp_device_info("late-device")` after observer abort and asserts the event receiver stays empty. |
| Blind Hunter: teardown test did not abort an observer | false | The regression starts `run_mtp_observer_with_enumerator`, waits until its scan blocks, aborts the observer, then drops the runtime before releasing the scan. |
| Edge Case Hunter: OS thread-creation failure panics the observer | false | Same resolved condition as the Blind Hunter finding: `Builder::spawn` returns an error handled as a non-fatal poll failure. |
| Verification Gap Reviewer: retry test did not prove poll cadence | low | Fixed in the same test by recording both enumerator invocation times and asserting the second begins no sooner than the supplied 20 ms interval. |

## Design Notes

The narrow split is intentional: `LIBMTP_Detect_Raw_Devices` is observational and has no recoverable state to commit, while the same treatment would be unsafe for the `MtpBackend` read/write operations used by sync. The core runtime may therefore continue joining its blocking pool; only discovery must cease to contribute non-cancellable work to that pool.

## Verification

**Commands:**
- `rtk cargo test -p hifimule-daemon lifecycle_shutdown_tests` -- expected: lifecycle regression tests pass, including a blocked-discovery quit.
- `rtk cargo test -p hifimule-daemon device` -- expected: device observer tests pass without physical MTP hardware.
- `rtk cargo check -p hifimule-daemon` -- expected: daemon compiles on the host target.
