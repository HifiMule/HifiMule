---
title: 'Configure synchronization performance in config.json'
type: 'feature'
created: '2026-10-12'
status: 'done'
baseline_commit: 'a9b62826e4b6d5518b148d674c41f0ba1bcb9a27'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Users cannot tune the buffer used to copy staged media to a USB memory stick or the staging and metadata batching limits. These values are fixed in code despite differences between devices and workloads.

**Approach:** Add an optional `sync_performance` object to the existing application `config.json`. Expose bounded controls, preserve existing defaults, log effective settings, and document their resource and throughput tradeoffs.

## Boundaries & Constraints

**Always:** Preserve verified writes, flushing, atomic publication, cancellation cleanup, original-device fences, and final manifest persistence. Keep one device writer and existing per-server scheduling. Apply one validated settings snapshot per synchronization, shared by manual and automatic starts. Preserve settings through login, server switching, device-ID creation, and logout. Changes apply to the next sync; no mid-transfer reload.

**Ask First:** Increasing checkpoint bounds beyond 5000 updates or 900 seconds, relaxing the 2 GiB single-file limit, or changing provider concurrency or playback protection policy.

**Never:** Add UI controls, disable verification or durability, use whole-file RAM buffering, change MTP/WPD transport buffer semantics, alter audio quality, or promise a speedup without measurement.

## I/O & Edge-Case Matrix

All fields below are integers within `sync_performance`; KiB/MiB use binary units.

| Field | Default | Inclusive range | Effect |
| --- | --- | --- | --- |
| `copy_buffer_kib` | 8 | 8–16384 | Buffer for staged-file copies to mass storage |
| `staging_max_tracks` | 2 | 1–32 | Tracks admitted to staging, including the active writer |
| `staging_max_mib` | 2048 | 2048–32768 | Aggregate temporary-file byte budget, not RAM allocation |
| `checkpoint_track_updates` | 1000 | 1–5000 | Metadata mutations before a checkpoint |
| `checkpoint_interval_seconds` | 600 | 1–900 | Maximum scheduled interval between checkpoint attempts |

| Scenario | Expected behavior | Error handling |
| --- | --- | --- |
| Missing file/object/field | Existing defaults | No warning for absence |
| Bad type, null, fraction, negative, zero, overflow, or out-of-range field | Default that field; retain valid neighbors and credentials | Warn with field name and expected range; never dump config contents |
| Non-object tuning section | Default tuning | Sanitized warning; authentication remains usable |
| Malformed/unreadable JSON | Sync uses defaults without rewriting the file | Sanitized diagnostic; retain existing credential error behavior |
| Settings edited during sync | Current snapshot remains stable; next sync reloads | No per-file config reads |
| Staging capacity pressure or inaccurate sizes | Bounded staging that either progresses or returns an actionable error | No deadlock or leaked files/permits; cancellation remains responsive |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/api.rs` — config persistence; session save replaces config and logout deletes it.
- `hifimule-daemon/src/sync.rs` — common executor, staging permits, writer, and integration tests.
- `hifimule-daemon/src/device_io.rs` — MSC streaming copy and safety tests.
- `hifimule-daemon/src/device/checkpoint.rs`, `device/tests.rs` — checkpoint session, timer, and regressions.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/sync_performance.rs` (new), `main.rs` — define and register validated settings with checked unit conversion; test the input matrix.
- [x] `hifimule-daemon/src/api.rs` — expose tolerant tuning reads and preserve preferences across all config mutations; test credential independence and persistence.
- [x] `hifimule-daemon/src/device_io.rs` — add an options-aware MSC copy using bounded read/write chunks, retaining existing callers and transport behavior; test exact bytes, empty files, cancellation, and failure cleanup.
- [x] `hifimule-daemon/src/sync.rs` — snapshot at common execution admission, pass options to writer/checkpoints, replace queue constants, and log effective settings. Test custom caps, both entry paths, cancellation, and underestimated/unknown sizes across multiple producers; prevent unresolvable capacity waits.
- [x] `hifimule-daemon/src/device/checkpoint.rs`, `device/tests.rs` — use configured thresholds; test counts, timer during blocked transfer, finalization, and connection fencing.
- [x] `docs/sync-performance.md` (new), `README.md` — document locations, JSON example, units/defaults/ranges, application timing, diagnostics, tradeoffs, and repeatable comparison procedure; link guide.

**Acceptance Criteria:**
- Given an older config, when synchronization runs, then effective settings match current defaults.
- Given a valid custom buffer and queue limits, when manual or automatic sync copies media, then those limits are used and output bytes remain identical.
- Given retained tuning, when the user logs in, switches servers, or logs out, then tuning survives while credential operations keep their intended behavior.
- Given configured checkpoint thresholds, when a threshold or finalization occurs, then pending metadata is persisted with the existing recovery guarantees.
- Given a completed implementation, when consulting the guide and logs, then a user can edit settings and compare runs without confusing disk staging capacity with memory consumption.

## Spec Change Log

## Design Notes

Larger buffers/queues do not guarantee improvement or add download workers. Checkpoint defaults remain 1000 updates/600 seconds; the user authorized configurable limits up to 5000 updates and 900 seconds (15 minutes). A checkpoint is triggered when either configured threshold is reached. Tolerant parsing prevents invalid tuning from breaking credentials. Account for collective byte exhaustion by partially staged files when increasing queue counts.

## Verification

- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon` — all daemon regressions pass using the controlled FFmpeg runtime.
- `rtk node scripts/build-daemon.mjs check -p hifimule-daemon` — daemon compiles.
- Review changed-file formatting and the config example. Document a repeatable real-device comparison, but make no hardware throughput claim from temporary-directory tests.


## Implementation Notes

- Added tolerant, independently validated performance settings and one execution snapshot shared by manual and automatic synchronization.
- Preserved raw tuning through authentication/config mutations and preference-only logout; authentication secrets still clear.
- Added bounded MSC copy chunks while keeping native object transport behavior, cleanup guards and durability checks.
- Byte reservations wait cancellably while complete files or active producers can make progress. They fail with actionable tuning guidance only for impossible single-file growth or collective partial-file deadlock, without exceeding the disk budget.
- Added settings/config persistence tests, copy byte/empty/cancellation coverage, custom 1/3-slot pipeline coverage, configured 5000-update/900-second checkpoints, and concurrent underestimated-producer coverage.
- Controlled daemon check passed. Full daemon suite passed 1475 unit tests and 5 integration tests (11 ignored). A final rerun also passed all new regressions, including the added four-producer capacity test, but was stopped after four existing RPC tests stalled beyond 60 seconds. The prior full green run and final successful compile are retained as verification evidence; no device throughput claim was made.


### Review fixes and targeted verification

- Preserved tuning as raw JSON and parsed each field independently, so syntactically valid overflowing numbers such as `1e400` do not invalidate credentials or valid neighboring settings.
- Applied the configured maximum to both Tokio filesystem buffers, including the 4–16 MiB range.
- Added complete/partial/waiting reservation accounting and blocked-writer progress/cancellation coverage while retaining collective-partial deadlock coverage.
- Production-entry integration reads a real config fixture, edits it during a blocked write, and confirms stable in-flight settings plus updated limits for the next synchronization. Test-only task-local paths isolate config fixtures from unrelated parallel tests.
- Targeted controlled-runtime verification: sync group 117 passed (1 ignored), config persistence/overflow 2 passed, validation 3 passed, MSC transport 14 passed. Full suite was not repeated after review fixes. Changed-file whitespace checks pass.

## Review Results

Three independent reviewers examined the diff: blind adversarial review, edge-case review, and acceptance audit. Four findings were classified as patches and resolved: preserve backpressure while completed files can release capacity, configure Tokio file buffers, isolate overflowing JSON numbers from credentials and valid settings, and exercise settings snapshots through the production executor. No unresolved findings remain.

Post-review checks passed: 117 sync tests, 14 MSC tests, two config tests, three validation tests, daemon compilation, and whitespace validation. The earlier complete suite and later stalled rerun remain documented above; the full suite was not repeated after these focused fixes.

## Suggested Review Order

**Execution and safe backpressure**

- Capture settings once for manual and automatic sync, then share them with writer and checkpoints.
  [sync.rs:2703](../../hifimule-daemon/src/sync.rs#L2703)

- Wait when capacity can recover; detect impossible growth without exceeding the staging budget.
  [sync.rs:2433](../../hifimule-daemon/src/sync.rs#L2433)

**Copy and persistence**

- Apply bounded copy buffers to both application chunks and Tokio files.
  [device_io.rs:406](../../hifimule-daemon/src/device_io.rs#L406)

- Use captured checkpoint thresholds while retaining timer ownership and finalization.
  [checkpoint.rs:81](../../hifimule-daemon/src/device/checkpoint.rs#L81)

**Configuration and evidence**

- Parse raw tuning independently, preserving credentials even when a numeric setting overflows.
  [api.rs:2025](../../hifimule-daemon/src/api.rs#L2025)

- Validate individual settings against approved bounds with stable defaults.
  [sync_performance.rs:34](../../hifimule-daemon/src/sync_performance.rs#L34)

- Exercise configured caps and buffer propagation through the production admission path.
  [sync.rs:7382](../../hifimule-daemon/src/sync.rs#L7382)

- Verify 5000-update and 900-second thresholds plus final flush.
  [tests.rs:958](../../hifimule-daemon/src/device/tests.rs#L958)

- Explain configuration, tradeoffs, and repeatable device comparisons.
  [sync-performance.md:1](../../docs/sync-performance.md#L1)
