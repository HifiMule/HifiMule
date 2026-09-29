# Story 16.13 Playback/Sync Coexistence Evidence

## Immutable run identity

- Commit/build: `6682109` plus the uncommitted Story 16.13 review patches; controlled daemon test build at 2026-09-29T19:52:01Z
- OS/architecture/install type: Darwin 27.0.0, ARM64 development checkout (not an installed build)
- Provider/server version and sanitized portable-server hash: local `mockito` HTTP providers only; no external server identity retained
- Device/transport/output (no credentials or authenticated URLs): temporary MSC filesystem backend; no physical DAP and no physical audio-output observation available in this run
- Fixed workload item count/bytes and observation interval: deterministic unit cases plus 15 provider-sync integration cases; individual fixtures define 1–3 items and bounded mock payloads
- Policy: v1 (`500 ms` attributable, `100 ms` unattributable, `60 s` max evidence age, `120 s` recovery/ineffective bound)
- Raw evidence location and hashes: test names below; source-controlled assertions in `hifimule-daemon/src/{playback/adaptation.rs,sync.rs,sync/protection.rs}`

## Raw runs

Record every run, not only the median. Do not tune thresholds after observing results.

| Profile | Mode | Run | Playback underruns / buffering duration | Physical output observation | Source staging MiB/s | Device writer MiB/s | Sync total/throughput | Decisions: time, reason, scope, delay | Recovery ms | Outcome |
|---|---|---:|---|---|---:|---:|---|---|---:|---|
| Healthy sufficient resources | controlled normal-policy cases | 1 | Not observable without live output | No physical output attached | Fixture-dependent | Temporary MSC | No protection delay in `stale_unknown_and_healthy_are_exactly_normal` | zero-delay `healthy` / `unknown` / `stale` | 0 | PASS for deterministic policy; throughput median unavailable |
| Constrained shared server | controlled policy | 1 | Scripted low-water evidence | Not a physical output run | N/A | N/A | 500 ms attributable admission | `attributableSharedSource`, shared source only | N/A | PASS |
| Intrinsically slow source | controlled adaptation | 1 | Scripted eligible delivery samples | Not a physical output run | Scripted minimum rate | N/A | No escalation after ineffective bound | `unsustainableDelivery` / `ineffectiveProtection` | N/A | PASS |
| Slow physical device | controlled temporary MSC | 1 | Not observable without live playback | First write deliberately blocked | Local mock provider | Deliberately blocked | 3 items; staging stayed within two-track bound | zero playback-protection decisions | 0 | PASS for writer-only negative-control mechanics |
| CPU/output contention | controlled policy | 1 | Scripted PCM low-water | Not a physical output run | N/A | N/A | 100 ms global admission | `pcmDepletion` / `unattributableOutputRisk` | N/A | PASS |
| Multiple source jobs | controlled policy | 1 | Scripted shared/unshared identities | Not a physical output run | N/A | N/A | Matching source 500 ms; unrelated source 0 ms | attributable source only | N/A | PASS |
| Recovery | controlled fake-time policy | 1 | Non-depleted, 175%-headroom windows | Not a physical output run | Scripted | N/A | Restriction retained pending a 120 s qualified episode | `recoveryPending` then `healthy` | 120000 | PASS for deterministic policy |
| Seek/output/session/stop/preview stale fences | controlled lifecycle policy | 1 | N/A | Not a physical output run | N/A | N/A | Closed publisher immediately returns no snapshot; stale/healthy use zero delay | `noActivePlayback` / `stale` | 0 | PASS for owner-close and stale decisions |
| HTTP retry and cancellation | controlled localhost provider + temporary MSC | 1 | N/A | Not a physical output run | Two HTTP 500 attempts | Temporary MSC | Initial attempt and retry each observed the 100 ms gate; cancellation test completed under 100 ms | `unattributableOutputRisk` | N/A | PASS |

## Fixed gates

- Healthy combined median vs sync-only median variance: **NOT RUN**. This requires a configured live provider, audible output observation, and physical target device; the development checkout has none of those identities available. Do not infer the 5% gate from mocks.
- Healthy protection decisions: **PASS (deterministic)** — healthy/unknown/stale decisions are exactly zero delay.
- Slow-device-only protection decisions: **PASS (controlled negative-control mechanics)** — blocking the temporary MSC writer did not create playback evidence or invoke protection; physical-device confirmation remains unavailable.
- Shared-source targeting and unrelated-source progress: **PASS (deterministic)** — only the matching source receives 500 ms; unrelated delivery risk receives zero, while unattributable depletion receives 100 ms.
- Recovery to the exact normal scheduler after valid 120-second evidence: **PASS (fake-time policy)** — rolling qualified windows retain an episode anchor and release only at 120,000 ms; a live throughput recovery run remains unavailable.
- Cancellation wait latency, queued staging cleanup, manifest truth: **PASS (controlled integration)** — cancellation preempted the bounded wait under 100 ms, the 15-case provider-sync suite passed cleanup/manifest assertions, and the full daemon suite passed.

Commands and results:

- `cargo test -p hifimule-daemon protection --no-fail-fast`: 11 passed.
- `cargo test -p hifimule-daemon playback::adaptation::tests --no-fail-fast`: 7 passed before the mixed-domain regression was added; that regression is included in the full suite below.
- `cargo test -p hifimule-daemon test_execute_provider_sync --no-fail-fast`: 15 passed.
- `cargo test -p hifimule-daemon --no-fail-fast`: 1,355 daemon tests passed, 8 intentionally ignored; 5 Audiobookshelf contract tests passed.
- `cargo clippy -p hifimule-daemon --all-targets`: completed with the repository's existing warning debt; the review helper's argument-count lint was subsequently suppressed locally.

## Evidence status and handoff

Controlled policy and repository regression results are recorded above. The required healthy live-provider/playback/physical-device median comparison could not be executed from this checkout because no such configured environment was available, so the 5% gate remains open. No installed Windows x64, Linux x64, macOS x64, or macOS ARM64 coexistence run is inferred from these tests. Sustained installed-platform soak and release certification remain an explicit Story 16.14 handoff.
