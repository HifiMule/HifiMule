---
title: 'Sync step lifecycle and timing logs'
type: 'feature'
created: '2026-10-18'
status: 'done'
baseline_commit: '4cccc08'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Sync can spend minutes preparing without useful daemon log output. Existing transfer and reconciliation logs do not consistently identify each step's start, terminal outcome, or duration, making bottlenecks difficult to locate and compare.

**Approach:** Write paired start/end lifecycle records for meaningful sync stages, from preparation through final persistence, using the existing file-backed daemon logger. Include monotonic elapsed time and correlation identifiers so users can identify the active step and compare completed stages when optimizing performance.

## Boundaries & Constraints

**Always:** Cover manual prepared sync, direct execution, and automatic sync, including single-provider, multi-provider, and retained legacy preparation branches. Emit starts before the corresponding expensive work. Record elapsed milliseconds, stable stage names, terminal outcomes, and available aggregate counts. Preserve existing execution, cancellation, storage safety, and optional reconciliation behavior. Keep ordinary logging enabled without additional configuration.

**Ask First:** Changes to sync behavior, scheduling, retry policy, public RPC contracts, or user interface beyond logging.

**Never:** Log credentials, credential-bearing URLs, raw request/config payloads, or manifest fingerprints. Add per-byte or per-track lifecycle records to bulk planning loops. Treat preparation success as completed transfer, or include time waiting for user confirmation in execution duration. Claim an end log can be guaranteed after process termination.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Slow preparation | Provider, auto-fill, or device I/O blocks | Start is already present; last unfinished stage identifies work in progress | Existing cancellation remains effective |
| Normal completion | Preparation and execution succeed | Paired stage records with durations; preparation reports ready and execution reports its final status | None |
| Failure | Early return, provider/device error, failed checkpoint | Started stages terminate with an accurate failure outcome and duration | Preserve original error propagation |
| Cancellation | Preparation, transfer, or finalization cancelled | Terminal cancellation records; no false success | Preserve existing fences and cleanup |
| No work or skipped work | Empty delta, disabled reconciliation, automatic cooldown | Accurate no-op/skip or zero-count outcome; no claim a skipped stage ran | Preserve skip behavior |
| Partial transfer failure | Executor returns a result containing file errors | Final log agrees with the operation status and error count | Do not infer success from outer Result alone |
| Prepared confirmation | Plan waits for user confirmation | Preparation and execution have separate durations and linked identifiers | Expired/rejected plans do not claim execution began |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/main.rs` — `daemon_log!` writes both stdout and `daemon.log`; `run_auto_sync_via_provider` orchestrates automatic sync separately.
- `hifimule-daemon/src/rpc.rs` — preparation entry, provider routing, basket/auto-fill expansion, preflight, plan publication, execution admission, and spawned finalization.
- `hifimule-daemon/src/sync.rs` — shared execution wrapper, transfer pipeline, deletion, cleanup, identity/provenance updates, playlists, and operation outcomes.
- `hifimule-daemon/src/device/checkpoint.rs` — periodic and final manifest persistence.
- `hifimule-daemon/src/device_music_scan.rs` — existing gated reconciliation lifecycle and summary logs.
- `hifimule-daemon/src/sync_plan.rs` — prepared plan ownership and identifiers.
- `docs/sync-performance.md` — existing diagnostic and comparison guidance.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/sync_logging.rs`, `hifimule-daemon/src/main.rs` — introduce a small shared lifecycle/timing utility and register its module. Use `Instant`, stable event fields, and explicit terminal outcomes; handle early exits and dropped scopes without falsely reporting success.
- [x] `hifimule-daemon/src/rpc.rs`, `hifimule-daemon/src/sync_plan.rs` — instrument manual preparation and execution admission with correlation available before operation creation and linked to plan/operation identifiers. Cover target/config validation, provider resolution, basket expansion, auto-fill slots, delta calculation, existence checks, media preflight, reconciliation, plan construction/publication, and dirty-manifest persistence. Avoid duplicate lifecycle pairs around shared helpers.
- [x] `hifimule-daemon/src/main.rs` — apply the same stage vocabulary to automatic sync. Explicitly classify skip, failure, and cancellation branches that currently return `Ok(())`.
- [x] `hifimule-daemon/src/sync.rs`, `hifimule-daemon/src/device/checkpoint.rs`, `hifimule-daemon/src/device_music_scan.rs` — cover transfer, managed deletions, directory cleanup, metadata/identity/provenance updates, playlists, checkpoint writes, and finalization. Preserve existing useful counts/rates and reconciliation summaries; derive overall outcome after checkpoint and operation finalization.
- [x] `hifimule-daemon/src/sync_logging.rs` and focused existing sync/RPC tests — verify lifecycle pairing on success, error, cancellation, and scope drop; assert no duplicate terminal events and no false success for partial failures. Verify preparation logs precede blocked fake-provider work, plus skipped/no-op behavior from the matrix.
- [x] `docs/sync-performance.md` — document event fields, correlation, the unfinished-stage diagnostic, and comparisons of elapsed times including overlapping stages.

**Acceptance Criteria:**
- Given a normal daemon configuration, when any supported sync path enters preparation, then `daemon.log` receives a start record before slow provider or device work.
- Given a completed stage, when reading its end record, then its stable name, matching correlation, elapsed milliseconds, and outcome are available without debug logging.
- Given a prepared plan, when execution later starts, then logs link the two phases while measuring each independently.
- Given overlapping nested stages, when comparing timings, then documentation explains that durations are wall-clock measurements and must not simply be summed.

## Spec Change Log

## Design Notes

The daemon uses `daemon_log!`, not a tracing subscriber. Existing operation IDs arrive too late to identify early preparation. Carry a lightweight correlation context through preparation and link it to execution without changing public RPC payload requirements. Optional work should emit a skip marker or omit its execution pair consistently. A fallback scope end must say interrupted/aborted rather than inventing failure details; handled errors and cancellation should have explicit outcomes.

## Verification

- `rtk npm run build:daemon -- test -p hifimule-daemon` — daemon tests pass through the required FFmpeg-aware build wrapper; report environmental restrictions separately.
- `rtk git diff --check` — no whitespace errors.
- Inspect captured logs from fake-provider/device fixtures: stage start precedes blocked work, every normally exited stage has one end, and final outcomes agree with operation state. No real device sync or performance improvement is claimed from fixture tests.

### Implementation evidence (2026-10-18)

- Added private task-local run correlation, UUID-only plan/operation links, monotonic stage durations, idempotent terminal events, and `interrupted` scope-drop fallback. Spawned execution and checkpoint timers retain the same run context explicitly.
- Instrumented all three preparation branches, direct/manual execution admission, automatic sync, bulk transfer/deletion/identity/provenance/playlist phases, checkpoint encoding/persistence and final operation commit. Checkpoint starts now precede lock acquisition and serialization. Automatic early returns classify skipped/no-op/failure/cancellation independently of the outer `Result`.
- Prepared plans retain the run context privately; confirmation wait is outside the separate preparation and execution timers. Existing RPC payloads and synchronization decisions remain unchanged.
- Added blocked fake-provider assertions for preparation ready/failure/cancellation, prepared-plan-to-spawned-execution correlation and automatic skip, dropped/aborted scope pairing, and actual finalization outcome assertions for success, cancellation, partial errors and failed final commits.
- Full FFmpeg-aware wrapper run: **1541 daemon tests passed, 12 ignored; 5 contract tests passed**. Initial sandbox run could not bind localhost mock servers or access macOS system configuration; the approved run with those accesses passed. Existing deprecation/dead-code warnings remain.
- Final focused `rtk npm run build:daemon -- test -p hifimule-daemon sync_step`: **2 passed**, including the final configuration/target validation timing boundaries.
- `rtk git diff --check` passed. No real-device sync or performance improvement is claimed.

Bulk stages may complete with zero counts. Optional work that is never entered is omitted; disabled reconciliation explicitly emits `skipped`. Timings overlap, especially managed deletions and transfer, and are not additive (documented in `docs/sync-performance.md`).

### Review follow-up

- Moved automatic executor-failure termination after the awaited operation status update, so execution duration includes that finalization work.
- Finished each multi-provider basket stage with that group's contributed item count; automatic admission now terminates with `admitted` before dirty-manifest persistence.
- Retained automatic basket-resolution failure counts privately and classify an empty all-failed preparation as `failed`, preserving its existing `Ok(())`, Idle state and no-execution behavior.
- Added all-failed automatic basket and per-provider group-count regressions. Focused `sync_step` tests: **4 passed** after review patches. Reconciliation cancellation precedence remains consistent with its existing summary behavior.
- Existing `auto_sync` filtered tests: **20 passed** after review patches. Final `rtk git diff --check` passed.

## Suggested Review Order

**Run and stage lifecycle**

- Share correlation across tasks and pair each stage with a timed terminal outcome.
  [sync_logging.rs:14](../../hifimule-daemon/src/sync_logging.rs#L14)

**Preparation and execution**

- Log before preparation waits and preserve correlation through prepared-plan execution.
  [rpc.rs:7738](../../hifimule-daemon/src/rpc.rs#L7738)

**Automatic sync**

- Classify automatic skips and failures without changing synchronization decisions.
  [main.rs:1863](../../hifimule-daemon/src/main.rs#L1863)

**Checkpoint persistence**

- Start timing before manifest locking, encoding, and persistence.
  [checkpoint.rs:291](../../hifimule-daemon/src/device/checkpoint.rs#L291)

**Regression coverage**

- Verify logs during blocked preparation, terminal outcomes, correlation, and provider counts.
  [rpc.rs:18270](../../hifimule-daemon/src/rpc.rs#L18270)

**Log interpretation**

- Explain fields, unfinished stages, and overlapping wall-clock measurements.
  [sync-performance.md:62](../../docs/sync-performance.md#L62)
