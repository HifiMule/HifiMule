---
title: 'Log provider pipeline average stage speeds'
type: 'feature'
created: '2026-07-12'
status: 'done'
baseline_commit: '55ede0f8a2d679e5a390a7f21a6454230280a080'
context:
  - '{project-root}/_bmad-output/planning-artifacts/sprint-change-proposal-2026-07-11-sync-throughput-pipeline.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The provider copy pipeline logs duration and throughput per track, but its final summary does not show aggregate source or writer speed. This makes it difficult to measure whether per-server preparation and the serial device writer are using the new pipeline efficiently.

**Approach:** Accumulate successful transfer bytes and active duration independently for each server producer and for the writer, then log weighted average throughput (`total bytes / total active duration`) at the end of provider sync using the existing duration and decimal `MB/s` format.

## Boundaries & Constraints

**Always:** Emit one source-stage metric for every server producer and one writer metric; preserve `None` server IDs as a clearly labelled default source; calculate each server independently because producers overlap; use actual staged byte sizes; include only successfully staged source transfers and successfully verified device writes; include retry time when the retry eventually succeeds; reuse `transfer_timing` so sub-millisecond and zero-duration handling matches current per-track logs; keep metrics log-only and preserve sync, cancellation, retry, cleanup, manifest, and device-centric progress behavior.

**Ask First:** Any change to public return types, RPC/UI contracts, persisted data, stage timing boundaries, or the definition of average throughput.

**Never:** Add dependencies, parallel device writes, global cross-server source averages, arithmetic means of per-track speeds, timing sleeps, or broad pipeline refactors.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|----------------------------|----------------|
| Multi-server success | Successful staged tracks from two server IDs and successful writes | One weighted source metric per server and one combined serial-writer metric, each with duration and `MB/s` | N/A |
| Default source | Adds have no `server_id` | The fallback producer is logged once under a stable default label | N/A |
| Partial failure or cancellation | Some tracks finish a stage and others fail, cancel, or fail verification | Totals contain completed successful stage work only; zero completed work reports zero duration/speed | Existing errors, cancellation, and cleanup remain authoritative |
| Retry succeeds | First source or device attempt fails and retry succeeds | Successful item bytes are counted once and elapsed time includes both attempts | Existing retry policy remains unchanged |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/sync.rs` -- owns `transfer_timing`, per-server producer tasks/outcomes, the single writer loop, final pipeline summary, and provider-sync tests.
- `hifimule-daemon/src/main.rs` and `hifimule-daemon/src/rpc.rs` -- all production callers of `execute_provider_sync`; inspected for compatibility, no changes expected.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/sync.rs` -- retain per-producer successful staged bytes/duration and writer successful bytes/duration, then emit stable per-server and writer aggregate timing lines at the final provider-sync logging point.
- [x] `hifimule-daemon/src/sync.rs` -- add a focused deterministic regression for weighted totals, default/zero behavior, and per-server separation without timing sleeps or log-capture plumbing.

**Acceptance Criteria:**
- Given successful provider adds from one or more server groups, when `execute_provider_sync` finishes, then the log contains exactly one source summary per producer group and one writer summary in the existing `duration(MB/s)` style.
- Given unequal track sizes, when aggregate speed is calculated, then it equals total successful bytes divided by summed active stage duration rather than the arithmetic mean of track speeds.
- Given concurrent server producers, when summaries are emitted, then no server's duration or bytes are combined with another server's source metric.
- Given failed, cancelled, or unverified work, when totals are logged, then incomplete stage work is excluded without altering existing error or cleanup behavior.

## Spec Change Log

## Design Notes

Source duration is the sum of each producer's existing `t_staging.elapsed()` measurements, which excludes queue-capacity blocking. Writer duration is the sum of the existing `t_write.elapsed()` measurements in the serial writer. Reusing those boundaries keeps aggregate and per-track numbers directly comparable. Sort source summaries by their display server key before logging so multi-server output is stable despite `HashMap` producer order.

## Verification

**Commands:**
- `rtk cargo test -p hifimule-daemon transfer_timing` -- expected: aggregate and existing sub-millisecond/zero-duration timing regressions pass.
- `rtk cargo test -p hifimule-daemon test_execute_provider_sync` -- expected: provider pipeline, retry, cancellation, backpressure, and cleanup regressions pass.
- `rtk cargo test -p hifimule-daemon` -- expected: daemon suite passes.
- `rtk cargo check -p hifimule-daemon` -- expected: daemon compiles without new warnings.
- `rtk cargo fmt --check` -- expected: formatting is clean.

## Suggested Review Order

**Metric Model**

- Reuses the existing timing math for weighted stage totals.
  [`sync.rs:46`](../../hifimule-daemon/src/sync.rs#L46)

- Labels legacy/default producer groups without changing sync contracts.
  [`sync.rs:62`](../../hifimule-daemon/src/sync.rs#L62)

**Source Producers**

- Carries per-producer identity and accumulated source timing through joins.
  [`sync.rs:1791`](../../hifimule-daemon/src/sync.rs#L1791)

- Records only successfully staged bytes using the existing staging timer.
  [`sync.rs:2415`](../../hifimule-daemon/src/sync.rs#L2415)

- Emits stable, per-server source summaries after all producers finish.
  [`sync.rs:2733`](../../hifimule-daemon/src/sync.rs#L2733)

**Writer**

- Tracks the serial writer total alongside existing idle timing.
  [`sync.rs:2521`](../../hifimule-daemon/src/sync.rs#L2521)

- Counts only successful verified writes in the writer metric.
  [`sync.rs:2621`](../../hifimule-daemon/src/sync.rs#L2621)

- Logs the aggregate writer speed at the final pipeline summary point.
  [`sync.rs:2741`](../../hifimule-daemon/src/sync.rs#L2741)

**Regression**

- Covers weighted totals, zero/default behavior, and per-server separation.
  [`sync.rs:3622`](../../hifimule-daemon/src/sync.rs#L3622)
