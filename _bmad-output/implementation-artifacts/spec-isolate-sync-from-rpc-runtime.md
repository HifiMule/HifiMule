---
title: Keep daemon health responsive during sync work
type: bugfix
created: 2026-10-20
status: done
baseline_commit: d9cb298e4c8f0831ee0325e3062ea5211baac066
context:
  - investigations/sync-monitoring-disconnection-investigation.md
---

<frozen-after-approval reason="User approved moving blocking filesystem calls and heavy sync work off the RPC thread">

## Intent

**Problem:** Manual sync preparation/execution currently shares a single-thread runtime with daemon health requests. Synchronous filesystem and planning work can prevent that runtime from answering health, even when core workers are available. This is a confirmed architectural risk; attribution to the observed disconnections remains unconfirmed.

**Approach:** Dispatch heavy sync requests onto the existing core runtime, and move MSC free-space system calls onto the blocking pool. Keep health, operation status, cancellation and shutdown control responsive on the existing separate RPC runtime.

## Boundaries & Constraints

**Always:** Preserve authentication, owner identity, JSON-RPC IDs and response contracts, mutation admission, sync pipeline exclusivity, cancellation, device connection fencing, checkpoint durability, task-owned guards and terminal reconciliation. Keep health available while the core runtime drains during shutdown. Use existing worker and staging limits. A filesystem error must still propagate as an error.

**Ask First:** Changing synchronization behavior, RPC/health deadlines, worker/queue limits, installed binaries or active machine settings.

**Never:** Bypass device verification, report a cancelled/panicked dispatch as successful, block the RPC thread waiting synchronously for core work, or claim the historical machine timeout is proven fixed by a synthetic regression.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected behavior | Error handling |
| --- | --- | --- | --- |
| Heavy request | Planning, estimation, auto-fill or execution | Run handler and spawned descendants on core runtime | Preserve response ID and normal errors |
| Slow filesystem query | MSC free-space call blocks | Await blocking-pool result without occupying async worker | Propagate storage and join errors |
| Concurrent controls | Heavy request held open | Health, status and cancellation remain on RPC runtime | Preserve current authentication and admission |
| Shutdown committed | New heavy request | Reject without new device work | Existing DAEMON_STOPPED contract |
| Core unavailable | Closed runtime or panicked dispatch | Return structured failure with original ID | Never hang or report success |
| Client disconnect | Admitted request already dispatched | Preserve ownership and guard cleanup | No duplicate sync execution |
| Core draining | Blocking device work still active | RPC health remains available | Existing shutdown fencing and completion order |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/main.rs` — separate core/RPC runtimes and ordered shutdown.
- `hifimule-daemon/src/rpc.rs` — server configuration, router, request admission, sync handlers and existing test fixtures.
- `hifimule-daemon/src/device_io.rs` — MSC free-space query and storage backend tests.
- `hifimule-daemon/src/rpc/` — focused RPC regressions if a separate test module improves readability.

## Tasks & Acceptance

**Execution:**
- [x] `main.rs`, `rpc.rs` — capture the existing core runtime handle before creating the RPC thread; pass it through server configuration and a router dispatch wrapper, without storing another runtime owner or modifying unrelated AppState literals.
- [x] `rpc.rs` — dispatch `sync_calculate_delta`, `sync_prepare`, `sync_detect_changes`, `sync_execute`, `basket.estimate` and `basket.autoFill` onto core; leave lightweight sync controls and health on RPC; preserve stopping rejection and map failed joins to structured errors with the original request ID.
- [x] `device_io.rs` — clone the MSC root into spawn_blocking for the native storage query; flatten both join and query errors; test a controlled blocked query without real USB hardware.
- [x] RPC/device tests — verify actual routing and responsiveness while sync work blocks, inherited execution runtime, storage error propagation, unavailable-core errors and shutdown admission. Use synchronization barriers instead of timing races.
- [x] Run the controlled FFmpeg daemon check, focused regressions and full daemon suite; review runtime ownership and shutdown behavior.

**Acceptance Criteria:**
- Given heavy manual sync work, when invoked through production RPC routing, then it and its spawned execution tasks use the existing core runtime.
- Given sync work held on core, when health/status/cancel requests arrive, then they run on the RPC runtime and complete before the held work is released.
- Given an MSC free-space query held open, when another async task runs on a single-thread runtime, then that task remains schedulable until the query is released.
- Given failed storage queries or a failed core dispatch, when the request settles, then the existing error semantics and JSON-RPC ID are preserved.
- Given shutdown admission is closed or the core runtime has stopped, when heavy work arrives, then no new sync begins and the request returns a structured failure.
- Given active core work during teardown, when shutdown drains the core, then authenticated health remains responsive until the existing RPC stop signal is set.

## Spec Change Log

- 2026-10-20: Review patches reserve shutdown admission at the forwarding boundary, including heavy read requests, and keep that guard inside the core task through cancellation. This closes the precommit-fence gap while retaining existing handler admission and detached execution ownership. Blocked test-request and gate deadlines now cover all surrounding control requests.

- 2026-10-20: Implementation authorized by the user's “Let's do it.” The only pre-existing dirty file is the investigation update from this same request; preserve it.

## Design Notes

Use a router dispatch wrapper to avoid adding a runtime field to every AppState test fixture. The core handle is a scheduling handle, not an owner of another Runtime. Await spawned request handlers asynchronously and abort unfinished forwarded handlers if their HTTP future is dropped; detached execution retains its existing pipeline owner. Running the complete sync execution handler on core also moves its descendant tokio::spawn tasks and pending-plan expiry timers onto core. Keep the existing handler reusable by tests and preserve its admission checks; reject stopping requests before dispatch if core may already be unavailable. Reserve shutdown admission at forwarding time, including heavy read requests, and move that guard into the spawned task so queued/running work is included in drain. Keep existing handler admission checks. Keep arbitrary browsing/playback work outside this change.

Regression tests should exercise the routing seam with real request/response handling where practical, rather than asserting a method-name list alone. A controlled synchronous stall must be released from another thread so an in-runtime timeout cannot conceal starvation. Tests must reliably release held workers even after assertion failure.

## Verification

- `rtk npm run build:daemon -- check -p hifimule-daemon --offline` — compiles using the project's controlled FFmpeg runtime.
- `rtk npm run build:daemon -- test -p hifimule-daemon sync_runtime_isolation --offline` — routing, storage and shutdown regressions pass.
- `rtk npm run build:daemon -- test -p hifimule-daemon --offline` — full daemon suite passes.
- `rtk git diff --check` — no whitespace errors.


## Verification Results and Review

- Controlled FFmpeg offline check and final debug build passed.
- Eight production-router isolation regressions and two MSC storage regressions passed.
- Final full daemon suite: 1,600 unit tests and five integration tests passed; 12 ignored; zero failures.
- Authenticated external HTTP probes remain responsive during controlled synchronous preparation/execution stalls. Tests also verify detached execution ownership, core teardown health, precommit fence rejection, admitted heavy-read drain tracking, original-ID errors and interrupted preparation cleanup.
- Three independent review layers completed. Review patches closed the precommit admission gap for heavy reads and increased deliberately blocked test deadlines. Follow-up acceptance and edge reviews found no remaining actionable issues.
- Whitespace check passed. Existing compiler warnings remain.
- Built artifact: target/debug/hifimule-daemon. The installed daemon and active synchronization were left running; replacement/relaunch is required to exercise this backend change.
- Synthetic tests establish the corrected isolation behavior. They do not prove attribution of the earlier machine disconnections.

## Suggested Review Order

**Request scheduling and ownership**

- Admit heavy work and forward it asynchronously without occupying the RPC runtime.
  [rpc.rs:607](../../hifimule-daemon/src/rpc.rs#L607)

- Pass the existing core handle while preserving independent health shutdown ordering.
  [main.rs:470](../../hifimule-daemon/src/main.rs#L470)

- Cancel unfinished forwarded handlers without aborting already-detached transfers.
  [rpc.rs:599](../../hifimule-daemon/src/rpc.rs#L599)

**Filesystem isolation**

- Offload native free-space queries and preserve storage and join errors.
  [device_io.rs:606](../../hifimule-daemon/src/device_io.rs#L606)

**Regression evidence**

- Probe authenticated controls externally while production sync preparation blocks.
  [runtime_tests.rs:189](../../hifimule-daemon/src/rpc/runtime_tests.rs#L189)

- Keep health responsive through core drain and reject fenced work.
  [runtime_tests.rs:291](../../hifimule-daemon/src/rpc/runtime_tests.rs#L291)

- Hold the real backend query while proving its async runtime remains schedulable.
  [device_io.rs:895](../../hifimule-daemon/src/device_io.rs#L895)

