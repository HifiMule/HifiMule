---
baseline_commit: f60a93c
---

# Story 16.13: Protect playback during sync without unnecessary throttling

Status: review

<!-- Ultimate context engine analysis completed - comprehensive developer guide created -->

## Story

As a HifiMule user,
I want music playback and device synchronization to run together,
so that preparing a device does not interrupt listening or slow down unnecessarily when resources are sufficient.

## Scope

Conditionally reduce **provider/network staging demand** when current, attributable playback-health evidence shows sustained risk during device sync, then restore normal sync demand after sustained recovery. Healthy playback must use the existing sync policy unchanged. Slow physical-device writes alone must never trigger playback protection.

This story reuses Story 16.12's bounded delivery and compressed/PCM buffer observations, adds a narrow session/generation-scoped scheduler-facing health snapshot, and applies a versioned protection policy only at safe sync admission boundaries. It includes deterministic policy/scheduler tests and reproducible coexistence evidence.

It does **not** replace playback quality adaptation, switch representations mid-track, interrupt an admitted device write or manifest transaction, weaken verification/managed-zone integrity, cancel sync silently, create a second scheduler/player, claim uninterrupted playback, or complete Story 16.14's sustained installed-platform certification.

### Normative Protection Policy v1

These values are the implementation baseline and acceptance oracle. A measured change requires an explicit reviewed policy-version change plus updated deterministic/evidence expectations; implementation must not silently tune them after seeing final results.

| Concern | v1 contract |
|---|---|
| Eligible evidence | Reuse Story 16.12 eligibility exactly: complete origin requests only; exclude startup/cache/seek/range/partial/failed and `<250 ms` requests. Retain at most 16 samples per source/representation and 64 scopes. |
| Risk classification | The 60-second value is **maximum sample age**, not a requirement to observe depletion for 60 seconds. Require at least 3 eligible samples within that age horizon. Risk is actionable when at least 2 eligible samples report compressed **or** PCM low-water `<100 ms`, or the current representation is unsustainable using the minimum eligible delivery rate with the implemented 125% delivery headroom. |
| Recovery classification | Require at least 3 eligible non-depleted samples whose evidence spans the full 120 seconds and whose minimum delivery rate meets the implemented 175% recovery headroom. Cache/startup bursts remain ineligible. |
| Snapshot publication | The playback session owner, not the global adaptation mutex, atomically publishes an immutable admitted snapshot only after validating current session/generation, transport state and active occurrence source/representation. Preview may publish its own occurrence source but remains explicitly distinct from the main session identity. |
| Snapshot validity | Valid only for the same active session/generation and while its newest eligible observation is no older than 60 seconds. Stop/end/session replacement/seek generation/output replacement or loss invalidates immediately. Pause retains the snapshot for at most 5 seconds, then publishes no-active-playback. Closed publisher or older evidence is `stale` and cannot restrict sync. |
| Normal | `healthy`, `unknown`, `stale`, no active playback, or playback source not present in the sync set uses the exact current scheduler without added delay/cap. |
| Attributable protection | Before each future item is resolved or any provider request is opened, delay only the producer matching the at-risk portable server by 500 ms. Re-check after the delay and before every retry. This is a cancellable admission delay, not a staging-permit resize and not an interruption of in-flight provider/device work. |
| Unattributable CPU/output risk | When actual PCM/compressed depletion is current but no shared source can be identified, apply one weakest global producer-admission delay of 100 ms per future item; do not escalate globally without a separately reviewed evidence rule. Device-write slowness alone is never such evidence. |
| Fairness/minimum progress | A protected producer is delayed at most once per item/retry admission and remains eligible immediately after the bounded delay; unrelated producers are never delayed by source-attributable risk. The policy must not hold either global staging permit while waiting. Existing non-AutoFill priority remains, and cancellation preempts every wait. Thus protection adds at most 500 ms (attributable) or 100 ms (unattributable) before each new attempt and cannot starve a source. |
| Ineffective protection | After 120 continuous seconds of current risk while protection is applied, mark it `ineffective`, retain only the same bounded per-attempt delay/minimum progress, and rely on adaptation/buffering. Never escalate, pause or cancel sync. Re-evaluate normally on new evidence. |
| Release | On valid recovery, remove the admission delay for the next not-yet-resolved item. Because no permits are resized, release cannot oversubscribe or leak capacity. |
| Healthy impact gate | In the fixed healthy coexistence profile, median sync throughput must remain within 5% of the sync-only baseline and no protection decision may be emitted. Report variance and raw runs; do not tune the threshold post hoc. |

## Acceptance Criteria

1. **Healthy playback preserves the current sync policy**
   - Given healthy or unknown-but-not-risky playback buffers while device sync runs, when the scheduler admits work, existing producer-per-server concurrency, source priority, queue/staging bounds, retry behavior and writer throughput remain unchanged.
   - Starting playback alone does not impose a standing cap, pause sync, or change device-write behavior.
   - Low physical-device write speed, writer queueing, or producer blockage behind the writer is not classified as a server-delivery bottleneck.

2. **Sustained, current, attributable risk causes bounded backoff**
   - Backoff requires the documented minimum eligible evidence and risk observation window derived from Story 16.12's actual complete-origin-request and compressed/PCM low-water measurements.
   - A typed snapshot identifies at least the active playback session/generation, source server, representation scope, monotonic observation/expiry time, eligible sample count and `healthy`/`risk`/`unknown`/`stale` classification. It contains no URL, credentials, headers or raw provider request.
   - The sync owner samples this state outside the audio callback and reduces only applicable provider/network staging at an existing safe boundary before a later item/request or permit admission.
   - No audio callback path allocates, blocks, performs I/O, acquires session/sync locks, or executes the protection policy.
   - Backoff never interrupts `write_with_verify_from_path`, an active device metadata/manifest update, cleanup/M3U work, or another atomic managed-device operation.

3. **Recovery is hysteretic and stale restrictions expire**
   - Protection relaxes only after the versioned recovery observation policy is satisfied; brief fast bursts, cache hits and one good sample do not release it.
   - Recovery moves sync demand toward the exact normal policy without oversubscription or permit leakage.
   - Paused/stopped/ended playback, session replacement, output change, seek generation change, closed observation channel, or expired evidence clears/invalidates the old restriction within a documented bound.
   - Late observations for an obsolete session/generation cannot reapply protection.

4. **Metrics remain correctly attributed**
   - Source delivery capacity, compressed-buffer health, PCM-buffer health, decoder/output state, provider staging throughput and physical-device writer throughput remain distinct diagnostic domains.
   - Device write slowness alone never causes playback-driven backoff when playback is healthy.
   - Cached/startup/seek/range/partial/failed/short requests remain ineligible under Story 16.12's policy and cannot manufacture risk or recovery evidence.
   - Diagnostics state the observed facts and selected policy reason without claiming causal certainty unsupported by the measurements.

5. **Intrinsically slow playback sources cannot suppress sync indefinitely**
   - If risk continues while applicable sync demand is already reduced, the policy reaches a documented minimum-progress state and expires or re-evaluates ineffective suppression within a fixed measured bound.
   - Playback retains Story 16.12 quality-adaptation and existing buffering/retry behavior; this story does not promise continuity or manufacture a viable lower representation.
   - Sync is not silently cancelled, starved indefinitely, or reported as complete while work remains.

6. **Multi-source sync remains targeted and fair**
   - When risk is attributable to a source server shared by playback and sync, the policy first reduces that source's future staging admissions rather than blindly stopping unrelated producers.
   - Unrelated healthy-source jobs and the device writer continue under existing bounds where doing so does not worsen measured playback risk.
   - Every affected producer retains the documented minimum progress/fairness guarantee, subject to cancellation and source availability.
   - Existing non-AutoFill-before-AutoFill priority, portable server routing, cancellation, retry, staging cleanup and managed-file integrity contracts remain intact.

7. **Playback/lifecycle transitions cannot leave or revive stale protection**
   - Seek, output replacement/loss, Stop, Preview/session replacement and Radio replacement obey existing generation, control, queue and paused-state fences.
   - Protection state never restarts audio, changes output routing, mutates the queue/session, or submits an extra listening report.
   - Quit still checkpoints playback and requests orderly sync cancellation; admitted writes and manifest truth retain their current shutdown semantics.

8. **Coexistence is demonstrated with outcomes and baselines**
   - Reproducible profiles cover sufficient resources, constrained shared-server delivery, intrinsically slow playback without sync, slow physical-device writes, CPU contention, multiple source jobs, recovery and stale lifecycle transitions.
   - Evidence records playback underruns/buffering and physical-output observations, source staging and physical writer rates, sync throughput, policy decisions/reasons, affected scope, recovery time and playback-only/sync-only baselines.
   - A healthy device-limited profile proves that playback does not trigger unnecessary throttling. A constrained shared-source profile proves that protection changes only future safe admissions and recovers.
   - Deterministic and available real-environment results are reported honestly. Story 16.14 owns sustained installed Windows/macOS/Linux soak and release certification; unavailable platform evidence remains an explicit handoff, not an inferred pass.

## Tasks / Subtasks

- [x] **Task 1: Freeze the protection contract and evidence thresholds before production integration** (AC: 1–8)
  - [x] Inventory the existing scheduler controls in `sync.rs`, the exact Story 16.12 observation semantics in `playback/adaptation.rs`, and all manual/automatic sync call paths. Record the safe admission seam(s); do not start with callback or device-writer changes.
  - [x] Copy the normative v1 table into owned documentation before enabling it, including units, decision reason codes and an injected monotonic clock. Any deviation requires a reviewed v2 decision and corresponding tests/evidence.
  - [x] Preserve the exact 16.12 evidence semantics: 60 seconds is maximum sample age, not sustained-depletion duration; risk needs 3 eligible samples and either 2 actual low-water samples or failed 125% sustainability; recovery needs 3 non-depleted samples spanning 120 seconds and 175% headroom.
  - [x] Use the fixed healthy-impact gate (within 5% median of sync-only, with zero protection decisions) and record raw variance. Do not select thresholds after observing final results or invent platform guarantees.
  - [x] Implement the table's explicit `unknown`, `stale`, no-playback, paused/stopped, unattributable and ineffective behavior. Default healthy/unknown behavior must not reduce sync demand.

- [x] **Task 2: Expose one bounded playback-owned sync-protection snapshot** (AC: 2–5, 7)
  - [x] Extend `hifimule-daemon/src/playback/adaptation.rs` or add a focused `playback/sync_protection.rs` pure bridge. Do not expose the private global history map or make `sync.rs` infer health from raw observations.
  - [x] Have the authoritative playback session owner atomically publish the cheap immutable snapshot/watch/atomic view after it validates the current session/generation, transport state and active occurrence source/representation. The global adaptation mutex must not independently emit scheduler restrictions. Include only scheduler-required identity, monotonic age, classification, evidence summary and sanitized reason.
  - [x] Add session/generation identity and explicit expiry at the bridge. Story 16.12's current global evidence is server/representation-scoped and is **not** sufficient by itself to fence scheduler restrictions.
  - [x] Keep the public `PlaybackHealth` restoration/persistence model separate from high-frequency sync-protection state unless a reviewed contract demonstrates otherwise. Preview may publish health for its own occurrence source, but its identity remains distinct and cannot overwrite or revive the main session.
  - [x] Preserve the actual observation flow: `BoundedHttpReader` supplies compressed facts, `HttpSource` records complete/partial request outcomes and PCM atomics, and the owner worker supplies PCM health. Do not fabricate depletion from chunk latency or double-count a response and its `Drop` record.
  - [x] Add bounded diagnostics/counters; every retained map/history must have age/count eviction. Never expose authenticated URLs, headers, tokens or unsafe raw identifiers.

- [x] **Task 3: Implement a pure deterministic protection state machine** (AC: 1–6)
  - [x] Prefer a focused `hifimule-daemon/src/sync/protection.rs` for scheduler policy, or an equivalently clear module. Keep playback evidence production separate from sync admission mechanics.
  - [x] Model normal, protected, recovering, ineffective/minimum-progress and expired states using the normative table, typed decisions and an injected monotonic clock. Separate provisional evidence from the state admitted by the current scheduler/session fence.
  - [x] Healthy/unknown input must return the exact existing scheduler policy. Risk must require sustained eligible evidence; recovery must require the complete recovery window.
  - [x] Scope a decision to the attributable portable server. For current unattributable CPU/output depletion, use only the normative 100 ms weakest global per-item admission delay; never escalate it from inference alone.
  - [x] Implement the fixed 500 ms attributable delay, 100 ms unattributable delay, 120-second ineffective transition and per-item/per-retry minimum-progress guarantee. Do not hold global staging permits during a wait.
  - [x] Emit deterministic sanitized reasons such as risk observed, attributable shared source, recovery pending, stale generation, device-limited/no action and ineffective protection. Reasons are diagnostics, not unsupported causal claims.

- [x] **Task 4: Apply protection only at existing provider-staging admission boundaries** (AC: 1–7)
  - [x] Integrate with `execute_provider_sync` without replacing its producer/writer pipeline. The required seam is **before** `resolve_sync_media`/`download_url` and before opening/sending the provider HTTP request for every future item; re-check before retries. The existing staging count/byte permits remain memory bounds and are not the protection control.
  - [x] Preserve current healthy behavior: one sequential producer per server, non-AutoFill priority barrier, at most 2 staged tracks / 2 GiB staged bytes, one verified writer and current retry/cancellation semantics.
  - [x] Do not delay or revoke a staging unit after it has entered an atomic device write/verify/manifest operation. Protection affects later admission, not completed or admitted work.
  - [x] Never derive protection from `average_writing_speed_mb_s`, device writer duration, queue depth caused by a slow writer, or sync progress alone.
  - [x] With several server producers, delay only the attributable server by at most 500 ms per future attempt and prove unrelated producers progress without that delay. Preserve existing priority/fairness and bounded queue memory.
  - [x] Ensure cancellation interrupts any protection wait/backoff promptly and still deletes queued staging files and reports truthful incomplete work.
  - [x] Pass the same read-only protection observer into manual single-server, manual multi-server and daemon-initiated auto-sync paths. `rpc.rs` and `main.rs` wire ownership; they do not own the policy.

- [x] **Task 5: Fence lifecycle changes and shutdown** (AC: 3, 7)
  - [x] Bind each admitted restriction to current playback session/generation and a monotonic expiry. Session replacement, Stop/end, seek generation change and output replacement/loss make older state ineligible.
  - [x] Treat Pause according to the frozen policy and clear a restriction within its documented bound; paused playback must not leave a permanent cap.
  - [x] Prove late observations and closed/stale watch state cannot recreate a retired restriction.
  - [x] Preserve `SyncOperationManager` cancellation/admission, safe Quit drain ordering, playback checkpointing, managed-device job begin/end and current partial-write/manifest truth.
  - [x] Keep Preview and main-session identities distinct; Preview activity cannot corrupt Radio/album state or create a global permanent throttle.

- [x] **Task 6: Add deterministic scheduler and regression tests** (AC: 1–7)
  - [x] Add pure fake-time table tests for healthy/no action, exact risk threshold, insufficient samples, full recovery window, jitter/hysteresis, stale snapshot, session/generation replacement, pause/stop/end, closed observer and maximum ineffective suppression.
  - [x] Test source attribution and multi-source fairness: only the relevant producer backs off, unrelated work progresses, and every affected source receives documented minimum progress.
  - [x] Extend `sync.rs` tests around producer/writer overlap: healthy policy remains unchanged; protection applies only before the next staging unit; an active write/verification/manifest update completes; slow writer alone never triggers protection; queue/count/byte bounds remain intact.
  - [x] Test cancellation during a protection wait, retry/error propagation, cleanup of staged files, non-AutoFill priority and portable-source routing.
  - [x] Test manual single/multi-server and daemon-initiated auto-sync wiring plus Quit/checkpoint/cancellation regressions in `rpc.rs`/`main.rs` ownership tests.
  - [x] Preserve Story 16.12 tests for eligible sample rules, bounded histories, adaptation decisions, album continuity, quality state and callback isolation.

- [x] **Task 7: Record reproducible coexistence evidence without overclaiming** (AC: 8)
  - [x] Extend `docs/playback-evidence/16-12-network-profile-template.md` conventions or add a focused 16.13 template with sanitized environment/build/runtime versions, fixed thresholds and raw evidence locations.
  - [x] Run playback-only, sync-only and combined baselines for healthy resources, a constrained shared source, an intrinsically slow source, a slow device, CPU contention, several sources and recovery. Record the same workload sizes and observation intervals.
  - [x] Capture physical-output/underrun or buffering outcomes, not only callback counters or a protection-enabled flag. Record staging and physical writer rates separately.
  - [x] Demonstrate that healthy device-limited sync has no playback-driven reduction and that shared-source risk recovers to normal demand after the declared window.
  - [x] Mark absent Windows/macOS/Linux installed evidence explicitly and hand sustained/shipping-platform execution to 16.14. Do not promote ARM64 development results or deterministic tests to release certification.

- [x] **Task 8: Update owned architecture and operational documentation** (AC: 1–8)
  - [x] Update `docs/architecture-hifimule-daemon.md` and `docs/playback.md` with evidence ownership, typed snapshot, safe sync seam, state machine, attribution, lifecycle expiry and failure behavior.
  - [x] Update `docs/playback-installed-test-checklist.md` with 16.13 controlled coexistence cases and the explicit 16.14 installed-soak handoff.
  - [x] Document diagnostic fields/reasons and confirm no secret-bearing data crosses RPC/log boundaries. Add API/data-model docs only if a public contract actually changes.
  - [x] Run targeted protection/adaptation/sync/lifecycle suites, the full daemon/workspace tests, normal all-target Clippy, and any affected UI/i18n checks. Report pre-existing strict-Clippy debt or environment restrictions separately from story failures.

## Dev Notes

### Current Implementation — Read Before Editing

| File / owner | Current state | Story change | Preserve |
|---|---|---|---|
| `hifimule-daemon/src/playback/adaptation.rs` | Bounded v1 global history keyed by `(server_id, representation_id)`: 16 samples/scope, 64 scopes, 60 s risk window, 120 s recovery, minimum 3 eligible complete-origin requests. Records real compressed/PCM low-water facts and boundary quality decisions. No production session/generation-scoped scheduler snapshot exists. | Expose a narrow bounded playback-owned risk snapshot or feed a focused bridge; add identity/expiry without exposing private maps. | Eligible sample rules, truthful actual-buffer evidence, quality adaptation, bounded eviction and fenced representation commit. |
| `hifimule-daemon/src/playback/streaming.rs` | `BoundedHttpReader` updates compressed-buffer facts under existing fixed caps. | At most provide already-owned observation data to the bridge. | 8 MiB/slot compressed cap, chunk/network bounds, cancellation, seek window and no callback policy. |
| `hifimule-daemon/src/playback/http_source.rs` | Converts compressed bytes to time, samples PCM atomics, and records request outcome on completion/`Drop`; observations are not instantaneous callback telemetry. | Preserve latency semantics and expose only eligible, sanitized facts. | Auth secrecy, range/validator rules, no-progress timeout, no fabricated/double-counted samples. |
| `hifimule-daemon/src/playback/audio.rs` | Decoder/output owner updates PCM health; callback remains allocation-/lock-/I/O-free. | Supply session/generation/transport lifetime to an owner-side snapshot if necessary. | Pipeline bounds, prepared successor, session/report identity, album continuity and callback isolation. |
| `hifimule-daemon/src/playback/model.rs` / `session.rs` | Public `PlaybackHealth` describes restoration/persistence, not live buffer health; session owns generation and lifecycle fences. | Prefer a separate cheap internal snapshot; expose only what sync admission needs. | Public snapshot semantics, daemon authority, paused intent and stale-result rejection. |
| `hifimule-daemon/src/sync.rs` | `execute_provider_sync` groups by portable server, spawns one sequential producer/server, prioritizes manual before AutoFill, bounds staging to 2 tracks/2 GiB and uses one verified device writer with per-file manifest updates. Existing diagnostics distinguish source staging from physical writes. | Sample protection before later source staging/permit admission; target source demand and keep minimum progress. | Producer/writer architecture, cancellation, retries, integrity, cleanup, routing, queue bounds and healthy throughput. |
| `hifimule-daemon/src/rpc.rs` | Manual single-/multi-server sync tasks call `execute_provider_sync`; `AppState` owns clonable `PlaybackSession`. | Pass a read-only observer/handle into both paths. | RPC validation/forwarding only; no policy ownership. |
| `hifimule-daemon/src/main.rs` | Playback exists before device observation; daemon-initiated auto-sync reaches the same provider-sync engine. Shutdown coordinates playback and sync. | Pass the same observer into auto-sync and preserve shutdown ordering. | One native event loop, safe Quit/checkpoint/cancellation and headless sync behavior. |

### Architecture Compliance

- `sync.rs` owns sync cancellation and conditional resource backoff. Playback owns playback measurements and session/generation truth; RPC/UI must not infer either policy.
- Keep provider APIs and authenticated requests in `providers/`/daemon-private streaming code. Do not expose URLs or credentials in a snapshot, event, log or evidence file.
- Apply backoff before future provider/network work. The writer owns verified device I/O and per-file manifest truth and is never used as a throttle knob.
- One daemon playback session, one sync producer/writer pipeline and one native event loop remain authoritative.
- Every asynchronous signal needs cancellation **and** stale-result rejection. A source/representation observation does not become a scheduler restriction without current session/generation admission.
- Keep independent bounds for compressed prefetch, decoded PCM, candidate/history state, staged track count/bytes and diagnostic histories.
- Preserve JSON/TypeScript `camelCase` and Rust/SQLite `snake_case` if a public contract changes. This story should remain daemon-internal unless user-visible state is demonstrably required.

### Protection Policy Guardrails

- **Healthy/default:** exact current sync scheduling; no protection just because playback exists.
- **Risk evidence:** only Story 16.12-eligible complete-origin observations plus actual compressed/PCM low-water facts; never chunk latency or device speed.
- **Attribution:** shared playback/sync source server first. CPU/output contention requires a documented conservative scope; absence of attribution is not permission for global indefinite suppression.
- **Safe action:** delay/reduce a later source staging admission. Never revoke a writer operation or weaken verification.
- **Recovery:** full declared window and asymmetric hysteresis; cache/startup bursts do not release.
- **Expiry:** session/generation/lifecycle change, stale monotonic age, or closed ownership retires the restriction.
- **Ineffective action:** bounded minimum-progress mode plus truthful playback adaptation/buffering; no infinite starvation.
- **Diagnostics:** facts, selected state/reason, scope and duration; never claim causality that was not measured.

### Library and Framework Requirements

- Use repository pins without dependency upgrades: Rust edition 2024 / MSRV 1.93.0, Tokio `~1.49`, CPAL `0.18.2`, `ffmpeg-next`/`ffmpeg-sys-next` `9.0.0`, crossbeam-queue `0.3.12`, patched Souvlaki `0.8.3`, libpulse-binding `2.30.1`.
- No new crate is justified for the policy. Prefer current Tokio/std atomics, `watch`/bounded channels and existing semaphores with an injected/test monotonic clock.
- If `tokio::sync::watch` is used, keep receiver borrows short and never across `.await`; consume looped changes with `borrow_and_update()` to avoid processing the same unseen state twice. A new subscriber treats the current value as seen, so explicitly read the initial snapshot before awaiting changes.
- Tokio semaphores are fair. Do not resize permit counts in a way that leaks capacity, strands existing permit holders, or accidentally serializes unrelated sources; a separate admission gate/state machine may be clearer than mutating the existing staging-cap semaphore.
- CPAL callback size and timing vary by platform/hardware. Never base source-delivery or scheduler causality on callback size alone, and never compare output-stream instants across replacement streams.

### Testing Requirements

- Prefer pure table tests with injected monotonic time and scripted snapshots; do not use wall-clock sleeps to prove windows/hysteresis.
- Assert exact scheduler decisions, affected source, safe-boundary timing, normal-policy equivalence, high-water/staging bounds and stale expiry—not merely that a `protected` flag changed.
- Use local mock providers and controlled I/O for default integration tests. Public network services and installed hardware are evidence runs, not prerequisites for deterministic unit suites.
- Include the real slow-device negative control. A test that slows only the writer must keep playback protection inactive.
- Verify current active write/verification/manifest completion under newly arriving risk and cancellation during a later protection wait.
- Verify callback isolation through code ownership and existing regression tests; do not add a callback-to-sync lock/channel send that can block.
- Record environment restrictions honestly. Story 16.12 ended with 1,338 daemon tests passing and 8 intentional ignores; strict `clippy -D warnings` remained blocked by 144 pre-existing warnings. Do not misattribute those warnings to this story, but introduce no new touched-code warnings.

### Previous Story Intelligence

- Story 16.12's review corrected real failure modes that this story must not reintroduce: fabricated chunk-latency depletion, duplicate observation counting, unbounded runtime maps, premature state publication, false recovery, and untruthful behavior when no action can improve playback.
- Its implemented v1 policy uses 60-second risk and 120-second recovery windows, at least 3 eligible complete requests, 16 samples/scope and 64 scopes. The code—not stale prose—is authoritative.
- Evidence is source-server/representation-scoped. It is not session/generation-scoped and therefore cannot be used directly as a durable throttle flag.
- Observations are normally finalized on complete request/`HttpSource::drop`; protection is not instantaneous buffer telemetry. Test whether this cadence is adequate before changing it, and never move blocking coordination into the callback to reduce latency.
- Representation state is committed only after authoritative fenced handoff. Apply the same provisional-versus-admitted discipline to scheduler protection.
- Extend the sanitized 16.12 evidence format with sync throughput, source/device attribution, decisions and recovery; never log URLs, tokens or raw IDs.

### Git Intelligence

- Recent sequence: `c595ff1 Story 16.12` -> `096c70c Dev 16.12` -> `f60a93c Review 16.12`, preceded by Story 16.11 implementation/review. The worktree was clean when Story 16.13 was prepared.
- Review 16.12 is the baseline because it contains eleven correctness patches to measurement, bounded state, admission and evidence. Do not prepare from the pre-review implementation commit.
- Keep implementation and adversarial review distinct. Expect touched playback/sync/lifecycle code, thresholds and evidence claims to receive focused review.

### Latest Technical Information

- The repository intentionally pins Tokio `~1.49`; do not upgrade to the current latest release within this story. Tokio's watch contract retains only the latest value, tracks seen state per receiver, and warns against holding a borrow across `.await`; design the observer around those semantics.
- Tokio's semaphore API provides fair queued acquisition and acquire/release memory ordering, but dynamically forgetting/adding permits changes global capacity. Preserve the staging semaphore's existing memory-bound role and implement resource protection as a separately tested admission policy unless a proof shows capacity mutation is reversible and race-safe.
- CPAL `0.18.2` documents callback size as a request rather than a cross-platform guarantee. Use owner-side buffer health and physical-output evidence, not callback-size assumptions, for protection decisions and acceptance claims.
- Primary references: [Tokio watch](https://docs.rs/tokio/1.49.0/tokio/sync/watch/), [Tokio semaphore](https://docs.rs/tokio/1.49.0/tokio/sync/struct.Semaphore.html), [CPAL 0.18.2 buffer size](https://docs.rs/cpal/0.18.2/cpal/enum.BufferSize.html).

### Project Structure Notes

Preferred new focused file (choose one ownership location after Task 1):

```text
hifimule-daemon/src/sync/protection.rs              # pure scheduler protection policy/state machine
# or
hifimule-daemon/src/playback/sync_protection.rs     # playback-owned snapshot bridge only
```

Likely update surface (confirm during implementation; avoid broad rewrites):

```text
hifimule-daemon/src/playback/adaptation.rs
hifimule-daemon/src/playback/{mod,audio,http_source,model,session,streaming}.rs  # only if bridge identity/ownership requires it
hifimule-daemon/src/sync.rs
hifimule-daemon/src/rpc.rs
hifimule-daemon/src/main.rs
docs/architecture-hifimule-daemon.md
docs/playback.md
docs/playback-installed-test-checklist.md
docs/playback-evidence/16-13-*.md
```

Do not add UI/i18n work unless the implemented product contract adds a user-visible state. The AC requires diagnostics and truthful evidence, not a new playback control.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 16, Stories 16.12–16.14]
- [Source: `_bmad-output/planning-artifacts/prd.md` — FR75 and P-NFR1–4]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — playback/sync coexistence, callback isolation, ownership and generation fencing]
- [Source: `_bmad-output/planning-artifacts/playback-prd-source-extract.md` — approved playback/sync coexistence and evidence intent]
- [Source: `_bmad-output/planning-artifacts/playback-epic-validation.md` — FR/NFR/architecture coverage and validation limits]
- [Source: `_bmad-output/planning-artifacts/sprint-change-proposal-2026-09-19.md` — approved Epic 15/16 split and historical Story 15.27 mapping]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md` — headless sync feedback, accessibility and playback release refinement]
- [Source: `_bmad-output/implementation-artifacts/epic-16-context.md` — Epic 16 sequencing and cross-story contracts]
- [Source: `_bmad-output/implementation-artifacts/16-12-adapt-playback-quality-at-track-boundaries-using-buffer-health.md` — measurements, review fixes and 16.13 handoff]
- [Source: `hifimule-daemon/src/playback/adaptation.rs` — current bounded v1 evidence policy]
- [Source: `hifimule-daemon/src/playback/http_source.rs` — complete-request observation flow]
- [Source: `hifimule-daemon/src/playback/audio.rs` — PCM health, owner worker and callback boundary]
- [Source: `hifimule-daemon/src/sync.rs` — producer/writer scheduler, bounds, cancellation and integrity]
- [Source: `hifimule-daemon/src/rpc.rs` — manual sync call paths]
- [Source: `hifimule-daemon/src/main.rs` — daemon-initiated auto-sync and shutdown ownership]
- [Tokio 1.49 watch documentation](https://docs.rs/tokio/1.49.0/tokio/sync/watch/)
- [Tokio 1.49 semaphore documentation](https://docs.rs/tokio/1.49.0/tokio/sync/struct.Semaphore.html)
- [CPAL 0.18.2 buffer-size documentation](https://docs.rs/cpal/0.18.2/cpal/enum.BufferSize.html)

## Dev Agent Record

### Agent Model Used

GPT-5 Codex

### Debug Log References

- RED: the initial `sync::protection::tests` target failed before the policy types/functions existed; the controlled daemon build wrapper was then used for all Rust validation.
- The first sandboxed full daemon run reported 245 local mock-server failures (`Operation not permitted`). Re-running with local bind permission passed 1,350 daemon tests plus 5 Audiobookshelf contract tests; 8 tests remain intentionally ignored.
- Normal all-target Clippy completed with the repository's existing warning debt and no build failure. No strict `-D warnings` claim is made.

### Implementation Plan

- Freeze policy v1 and evidence gates in owned documentation, then derive scheduler-facing evidence from Story 16.12's bounded observation history.
- Have the serialized playback owner publish a session/generation-fenced sanitized snapshot, preserving Preview and lifecycle identity.
- Keep sync protection as a pure decision module and apply its bounded cancellable delay before future provider work without touching staging permits or the verified writer.
- Wire the same observer through manual single/multi-server and daemon auto-sync ownership paths; validate deterministic policy, cancellation, existing sync regressions, and honest evidence handoff.

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- Implemented playback-owner publication of bounded, sanitized session/generation/source/representation health snapshots using the existing 16.12 eligible evidence and 60 s/120 s hysteresis rules.
- Added deterministic protection decisions: exact normal behavior for healthy/unknown/stale/no-playback, targeted 500 ms shared-source admission delay, conservative 100 ms unattributable delay, and non-escalating 120 s ineffective state.
- Integrated cancellable checks before future provider resolution/request work and retries without holding staging permits or modifying writer/device/manifest semantics; manual single/multi-server and daemon auto-sync share one observer.
- Added seven deterministic policy/lifecycle/cancellation tests. Full daemon validation passed: 1,350 unit tests, 5 contract tests, 8 intentional ignores. All-target Clippy completed with pre-existing warnings only.
- Added policy, architecture, playback, installed-checklist, and coexistence evidence documentation. Installed Windows/Linux/macOS coexistence runs are explicitly unverified and handed to Story 16.14; no release certification is inferred.

### File List

- `_bmad-output/implementation-artifacts/16-13-protect-playback-during-sync-without-unnecessary-throttling.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `docs/architecture-hifimule-daemon.md`
- `docs/playback-installed-test-checklist.md`
- `docs/playback-sync-protection-v1.md`
- `docs/playback.md`
- `docs/playback-evidence/16-13-coexistence-template.md`
- `hifimule-daemon/src/main.rs`
- `hifimule-daemon/src/playback/adaptation.rs`
- `hifimule-daemon/src/playback/session.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-daemon/src/sync.rs`
- `hifimule-daemon/src/sync/protection.rs`

## Change Log

- 2026-09-29: Implemented Story 16.13 playback/sync protection policy v1, owner-fenced health snapshot, targeted cancellable sync admissions, deterministic tests, and controlled coexistence documentation; moved story to review.
