# Story 16.13 Playback/Sync Coexistence Evidence

## Immutable run identity

- Commit/build:
- OS/architecture/install type:
- Provider/server version and sanitized portable-server hash:
- Device/transport/output (no credentials or authenticated URLs):
- Fixed workload item count/bytes and observation interval:
- Policy: v1 (`500 ms` attributable, `100 ms` unattributable, `60 s` max evidence age, `120 s` recovery/ineffective bound)
- Raw evidence location and hashes:

## Raw runs

Record every run, not only the median. Do not tune thresholds after observing results.

| Profile | Mode | Run | Playback underruns / buffering duration | Physical output observation | Source staging MiB/s | Device writer MiB/s | Sync total/throughput | Decisions: time, reason, scope, delay | Recovery ms | Outcome |
|---|---|---:|---|---|---:|---:|---|---|---:|---|
| Healthy sufficient resources | playback-only / sync-only / combined | | | | | | | | | |
| Constrained shared server | playback-only / sync-only / combined | | | | | | | | | |
| Intrinsically slow source | playback-only / combined | | | | | | | | | |
| Slow physical device | sync-only / combined | | | | | | | | | |
| CPU/output contention | playback-only / combined | | | | | | | | | |
| Multiple source jobs | sync-only / combined | | | | | | | | | |
| Recovery | combined | | | | | | | | | |
| Seek/output/session/stop/preview stale fences | combined | | | | | | | | | |

## Fixed gates

- Healthy combined median vs sync-only median variance: **unverified** (must be within 5%).
- Healthy protection decisions: **unverified** (must be zero).
- Slow-device-only protection decisions: **unverified** (must be zero).
- Shared-source targeting and unrelated-source progress: **unverified**.
- Recovery to the exact normal scheduler after valid 120-second evidence: **unverified**.
- Cancellation wait latency, queued staging cleanup, manifest truth: **unverified**.

## Evidence status and handoff

Deterministic policy and repository regression tests are recorded in the Story 16.13 Dev Agent Record. No installed Windows x64, Linux x64, macOS x64, or macOS ARM64 coexistence run is inferred from those tests. Sustained installed-platform soak and release certification remain an explicit Story 16.14 handoff.
