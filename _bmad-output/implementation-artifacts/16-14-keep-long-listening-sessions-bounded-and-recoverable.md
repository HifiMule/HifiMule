---
baseline_commit: 989103c
---

# Story 16.14: Keep long listening sessions bounded and recoverable

Status: in-progress

<!-- Ultimate context engine analysis completed - comprehensive developer guide created -->

## Story

As a HifiMule user,
I want long album and Radio sessions to remain responsive and recoverable,
so that leaving music playing through a workday does not progressively consume memory or lose my session after interruptions.

## Scope

Certify the integrated Epic 16 runtime over sustained album and Radio sessions on every shipping platform/architecture, and fix reproducible bounded-resource or recovery defects discovered by that work. Freeze budgets, workload, fault schedule, sampling and pass/fail rules before the acceptance runs. Reuse the Story 15.17 packaging/evidence process and the Story 16.12/16.13 measurement formats.

This story validates P-NFR1–4, the applicable installed accessibility/responsive evidence from P-NFR6 and P-UX-DR13–14, FR60 and FR65–75 regressions, and the resource portions of P-AR12/P-AR14. It owns fresh installed evidence for Stories 16.1–16.13: automatic selection, Radio, reporting/feedback, snapshots, playlist/basket exports, quality adaptation and conditional sync protection. Its evidence index must map those requirement families and every applicable story/scenario to each required artifact row.

It does **not** redesign playback, add a second player/scheduler, promise uninterrupted audio, infer audible continuity from callback counters, silently tune budgets after observing results, upgrade dependencies opportunistically, certify an untested platform, publish a release, or absorb larger unrelated defects. Record unavailable/failed scenarios as explicit blockers or limitations.

### Pre-Acceptance Measurement Contract

Task 1 must commit a versioned Story 16.14 evidence contract before acceptance runs. Values must be derived from earlier measurements and stated in native units; the table below is the minimum contract, not permission to invent numbers.

| Concern | Required predeclared contract |
|---|---|
| Soak | Preserve the inherited minimum 30-minute playback-plus-sync profile after a 5-minute warm-up, sampled at least every 5 seconds, then define a separate longer album/Radio workday soak, repetitions, idle baseline and active baseline. A short smoke run cannot satisfy the sustained claim. |
| Workload | Queue/history/exclusion sizes, Radio refill/edit cadence, seeks/previews/replacements/output changes, provider/server mix, reporting/feedback/export operations and representative real-device sync workload. |
| Memory | Preserve or formally revise with evidence the inherited gates: active RSS p95 <= idle p95 + 128 MiB; retained RSS growth over the final 20 minutes <= 16 MiB; same-fixture cross-OS p95 active-RSS delta and normalized p95 CPU delta each <= 15%. Also declare separate budgets/high-water/slopes for native heap where measurable, compressed prefetch, decoded PCM, automatic upcoming entries, candidate/adaptation caches and in-memory history. Active decoder memory is compared with an active baseline, not the idle target. |
| Runtime resources | Task/thread count, file descriptors or Windows handles, decoder/output instances, open requests, bounded channels/operation queues, SQLite page/query behavior and cleanup latency. State the platform-specific collector and uncertainty. |
| Fault schedule | Slow/outage source, ambiguous reporting/export failure, seek, Preview, queue/session replacement, output loss/switch, sleep/wake, orderly Quit, abrupt daemon exit and interrupted checkpoint. |
| Recovery | Expected paused/restored state, session/generation fences, Radio identity/exclusions, Preview/main behavior, durable ambiguous operations, corruption/unsupported-schema diagnosis and maximum cleanup/reconciliation times. |
| Coexistence | Playback-only, sync-only and combined runs; physical-output observation, underruns/buffering, source and device throughput, protection decisions/recovery, and Story 16.13's healthy combined median within 5% of sync-only with zero protection decisions. |
| Platform matrix | Separate rows for Windows x64 MSI and NSIS, Linux x64 deb and AppImage, macOS x64 DMG and macOS ARM64 DMG unless the release contract explicitly changes shipping artifacts. Require clean install and upgrade for MSI/NSIS/deb/DMGs and clean launch for AppImage. Record physical/VM status, hardware, package type, artifact hash, application/daemon/Rust/FFmpeg/runtime versions; no format inherits another format's result. |
| Result policy | Preserve raw sanitized time series. A failed budget, missing physical observation, unavailable provider/device or untested architecture is not a pass. Fix in-scope reproducible defects and rerun the affected scenario using the frozen contract. |

## Acceptance Criteria

1. **Long album and Radio sessions stay independently bounded**
   - Given the committed resource contract and representative workload, album and Radio soaks keep compressed prefetch, decoded PCM, automatic upcoming entries, candidate/adaptation caches and in-memory history within their separate bounds.
   - Evidence records high-water values and time-series growth after warm-up, distinguishing idle from active memory. A bounded container alone is not evidence that the installed process remains bounded.
   - Existing bounds remain regression anchors: per-source compressed capacity 8 MiB, two-source aggregate 16 MiB, 64 KiB reader chunks, upstream network chunk <= 1 MiB and a 7,274,496-byte retained seek window; PCM queue 1 MiB per source / 2 MiB aggregate; Radio at most five automatic upcoming entries with refill threshold two; adaptation at most 16 samples/scope and 64 scopes; sync staging at most two tracks / 2 GiB.

2. **History and logical-session exclusions remain complete without whole-history loading**
   - Radio cycles, skips, removals and manual edits preserve all required current logical-session exclusions through bounded/keyset-paged access.
   - Neither daemon restart nor reopened UI materializes the entire history. Page-size/query-count and retained-memory evidence accompany behavioral assertions.
   - Storage growth and retention/cleanup policy are explicit. Cleanup cannot erase exclusions needed by the active logical session or destroy an immutable snapshot/export audit trail still required for reconciliation.

3. **Obsolete work releases resources and cannot publish late results**
   - Repeated seeks, Previews, track/session replacements and output changes cancel obsolete work and release decoder tasks, CPAL streams, file descriptors/handles and provider requests within declared limits.
   - Every asynchronous completion validates its applicable authoritative identity/fence: occurrence ID for occurrence-scoped work; session/generation for playback pipeline work; command/operation/source identity for durable remote work; and the owning lifecycle/output generation where applicable. Late work cannot emit audio, mutate queue/session state, change output, submit reports or revive protection.
   - The audio callback remains allocation-, blocking-, I/O- and session/sync-lock-free.

4. **Retry and remote-operation recovery stay bounded and truthful**
   - Source outages/slow responses and reporting, feedback, playlist or basket export failures keep retry work and all in-memory operation queues bounded.
   - Durable unresolved/ambiguous state is retained according to a documented policy. Reconciliation does not blindly repeat remote writes, claim provider exactly-once semantics, or convert a technical failure into a user rejection.
   - Every provider URL/HTTP retry remains gated by current cancellation/session state and, where applicable, the Story 16.13 protection admission decision.

5. **Sleep/wake and output loss preserve intent on each OS**
   - Installed tests define and exercise sleep/wake behavior on Windows, macOS and Linux, reconcile actual output/source availability, and preserve recoverable session state.
   - Output loss pauses without automatic rerouting or unrequested restart. Wake/reconnect does not auto-resume paused audio.
   - Platform APIs or CI limitations are recorded per target; one OS result is never generalized to another.

6. **Quit, crash and interrupted checkpoints restore only committed state**
   - Orderly Quit checkpoints playback paused and preserves safe sync cancellation/manifest truth. Abrupt daemon exit and killed-mid-transaction checkpoint tests restore the last valid committed state paused.
   - Restore retains Radio identity/exclusions and the applicable Preview/main-session recovery semantics.
   - Corrupt or unsupported state remains recoverable and visibly diagnosed. It is quarantined/preserved as required by policy rather than silently overwritten or treated as a valid empty session.

7. **Real playback and real-device sync coexistence is measured**
   - Prepared album playback runs with a representative physical-device sync workload and records physical/audible-output continuity observations, underruns/buffering, source staging rate, device write rate, total sync throughput, protection decisions and recovery.
   - Playback-only and sync-only baselines use the same environment and workload. Slow device writes alone never manufacture playback risk.
   - Callback counters or simulated policy tests alone cannot certify audible continuity or close Story 16.13's live 5% healthy-throughput gate.

8. **Cross-platform evidence is reproducible and honest**
   - The same versioned workload and fault schedule run on installed Windows, Linux and macOS packages for each shipping architecture, with deviations explicitly justified.
   - Each record includes duration, hardware/VM status, package/artifact identity, runtime/dependency versions, resource time series, immutable non-secret raw-evidence locations and recovery outcomes against budgets fixed before acceptance.
   - Failed budgets, unavailable equipment and untested scenarios remain blockers/limitations; they are not hidden by shorter passing runs or deterministic mocks.

9. **The complete Epic 16 installed workflow matrix is freshly verified**
   - Installed builds exercise automatic selection, bounded Radio and edits, relationship/fallback explanation, recording deduplication, gain, headless Play something, reporting, Like/Dislike, immutable snapshots, source playlists, device basket Add/Replace, quality adaptation and conditional sync protection.
   - Evidence verifies occurrence/source routing, capability-gated UI, keyboard/focus/accessible names and announcements, narrow/medium/wide layouts, failure/recovery and actual provider effects without exposing credentials or unsafe identities.
   - Fresh checks cover changed dependencies and affected workflows. Story 15.17 evidence establishes the process only; it does not certify Epic 16. Actual publication remains separate.

## Tasks / Subtasks

- [x] **Task 1: Freeze the versioned Story 16.14 measurement and evidence contract** (AC: 1–9)
  - [x] Inventory earlier measurements and open handoffs from Stories 15.17 and 16.1–16.13. Commit soak duration, warm-up, sample interval, repetitions, workloads, faults, budgets, acceptable slopes/growth, collectors, recovery deadlines and raw-evidence layout before acceptance runs.
  - [x] Extend `docs/playback-installed-test-checklist.md` and add focused sanitized `docs/playback-evidence/16-14-*` template(s). Preserve artifact/runtime identity, physical/VM status and explicit `PASS`/`FAIL`/`BLOCKED`/`NOT RUN` semantics.
  - [x] Define collectors for RSS/native memory where available, tasks/threads, file descriptors/handles, compressed/PCM high water, upcoming/candidate/history counts, requests/operations and SQLite page/query behavior. Document platform uncertainty rather than comparing unlike metrics as equal.
  - [x] Freeze explicit artifact rows for Windows x64 MSI and NSIS, Linux x64 deb and AppImage, macOS x64 DMG and macOS ARM64 DMG unless the release contract proves a different shipping set. Require clean install/upgrade for every installer and clean launch for AppImage; no architecture- or format-level result may stand in for another row.
  - [x] Do not start final runs until the contract is reviewable. Any later threshold change requires a versioned rationale and rerun; never move a limit merely to turn a failure green.

- [x] **Task 2: Build deterministic bounded-state and fault orchestration** (AC: 1–6)
  - [x] Prefer extending `scripts/playback-session-evidence.py`, `scripts/playback-installed-evidence.py` and their tests; add a focused soak runner only where existing collectors cannot express the frozen contract.
  - [x] Add fake-time/table tests for long history paging, exclusions, Radio upcoming/candidate/adaptation bounds, retry/operation queues, cleanup deadlines and storage retention. Do not use workday wall-clock sleeps in default unit suites.
  - [x] Script seeks, Preview, track/session replacement, output switch/loss, slow/outage sources, ambiguous remote writes, sleep/wake hooks, orderly Quit, abrupt exit and killed-mid-checkpoint recovery with generation/command IDs captured.
  - [x] Ensure evidence is sanitized: no tokens, auth headers, credentials, authenticated/provider URLs, usernames, local profile/home paths, raw portable server IDs or provider response bodies. Immutable non-secret artifact/evidence URIs and stable pseudonymous/hashed IDs are allowed when required for auditability.

- [x] **Task 3: Prove paged history and exclusion continuity** (AC: 2, 6)
  - [x] Exercise existing keyset/page APIs and 200-row restore batches through a history much larger than one page; assert stable ordering, no omissions/duplicates and bounded retained rows in daemon and reopened UI.
  - [x] Prove skips/removals/manual edits/deliberate repeats keep the correct logical-session exclusions across checkpoint/restart without loading all occurrences.
  - [x] Test cleanup at retention boundaries and during active sessions; never delete active exclusions or durable ambiguous-operation state.
  - [x] Fix only measured defects in `playback/persistence.rs`, `radio.rs`, session/UI paging or related schema/query code; migration changes require rollback/corruption tests and documentation.

- [x] **Task 4: Prove cancellation, generation fencing and resource release** (AC: 3)
  - [x] Stress seek/Preview/replacement/output transitions and track decoder tasks, output streams, requests, file descriptors/handles and bounded-channel occupancy before/after cleanup.
  - [x] Inject late fetch/decode/output/report completions for retired session/generation/occurrence IDs and assert no audio/state/report/protection publication.
  - [x] Preserve one daemon session manager, one output pipeline and callback isolation. Do not add blocking callback instrumentation; collect high-water/resource state owner-side.
  - [x] If defects are found, keep fixes local to the owning playback module and add deterministic regression tests before rerunning the affected soak.

- [ ] **Task 5: Prove bounded retry, durable reconciliation and lifecycle restore** (AC: 4–6)
  - [x] Exercise source stalls/outages and reporting/feedback/playlist/basket failures, including ambiguous responses. Assert bounded live queues, durable unresolved state, command-id/source identity and no blind duplicate write.
  - [x] Test orderly Quit with active playback and sync, abrupt daemon exit and interrupted SQLite checkpoint. Verify last committed state restores paused and managed-device manifest truth remains honest.
  - [x] Test corrupt and unsupported state diagnosis/recovery without silent overwrite.
  - [ ] Exercise sleep/wake and output disconnect/reconnect on each installed OS; verify actual device/source reconciliation, no auto-reroute and no unrequested resume.

- [ ] **Task 6: Run sustained album, Radio and real-sync profiles** (AC: 1–8)
  - [ ] Run frozen idle, album and Radio profiles with fixed edit/fault cadence and collect all resource time series/high-water/slopes. Investigate reproducible post-warm-up growth.
  - [ ] Run playback-only, sync-only and combined real-provider/physical-output/physical-device profiles, including healthy resources, slow device, constrained source and recovery.
  - [ ] Close or truthfully retain Story 16.13's healthy gate: combined median sync throughput within 5% of sync-only and zero protection decisions. Preserve raw runs and variance.
  - [ ] Rerun affected profiles unchanged after an in-scope fix. Link larger unrelated defects as blockers rather than broadening the story.

- [ ] **Task 7: Execute the fresh installed Epic 16 workflow matrix** (AC: 5, 8, 9)
  - [ ] Install and upgrade every MSI, NSIS, deb, macOS x64 DMG and macOS ARM64 DMG row, and clean-launch AppImage. Capture package/hash plus application, daemon, Rust, FFmpeg and native runtime identity. Verify the actually loaded FFmpeg library, not only the build manifest.
  - [ ] Exercise Stories 16.1–16.13 end to end with supported and unsupported provider capabilities, correct occurrence/source routing, remote effects, recoverable failures and partial outcomes. The evidence index must map every story/scenario to every applicable artifact row; record a reason when a capability is inapplicable rather than inheriting another row's result.
  - [ ] Verify keyboard operation, visible focus, accessible names/status/ARIA live, non-obscuring playback controls and narrow `<600`, medium `600–1000`, wide `>1000` layouts under relevant OS themes.
  - [ ] Keep physical media-key routing separate from API invocation evidence. Record platform/provider limitations independently; do not infer missing targets.

- [ ] **Task 8: Run regressions and finalize an auditable result** (AC: 1–9)
  - [ ] Run targeted playback/radio/persistence/reporting/export/adaptation/protection/sync/lifecycle tests, evidence-script tests, full daemon/workspace tests, normal all-target Clippy and affected UI/i18n checks.
  - [ ] Preserve Story 16.12/16.13 regression anchors: real buffer facts, 16-sample/64-scope bounds, rolling 120-second recovery, weak-owner closure, every-retry gating, no duplicate observation and the slow-device negative control.
  - [ ] Update `docs/playback.md` / `docs/architecture-hifimule-daemon.md` only for measured final budgets, cleanup policy or contract changes. Never backfill aspirational values as measured facts.
  - [ ] Publish an evidence index mapping every AC/platform/scenario to raw sanitized data, result and open blocker. Do not mark the story done while mandatory installed evidence is missing.

## Dev Notes

### Current Implementation — Read Before Editing

| File / owner | Current state | Story change | Preserve |
|---|---|---|---|
| `hifimule-daemon/src/playback/audio.rs` | PCM queue is bounded to 1 MiB; the serialized owner fences pipeline/session generations; output loss is retryable. | Instrument owner-side high-water/lifetime data or fix demonstrated cleanup defects only. | Callback performs no allocation, blocking, I/O or session/sync locking; album/Preview continuity and occurrence identity. |
| `playback/streaming.rs` / `http_source.rs` | Each source has 8 MiB compressed capacity (16 MiB across two slots), 64 KiB reader chunks, <=1 MiB upstream chunks and a 7,274,496-byte retained seek window; source reads use a 15-second no-progress stall rule and sanitized request outcomes. | Measure high water/open requests and repair reproducible leaks/fence failures. | Cancellation, range/validator behavior, auth secrecy and no fabricated/double-counted observations. |
| `playback/adaptation.rs` | At most 16 samples per source/representation, 64 scopes; 60-second risk age and rolling 120-second recovery. | Treat bounds/recovery as regression anchors; change only with an explicit versioned contract. | Complete-origin eligibility, real compressed/PCM depletion, boundary-only decisions and closed-owner expiry. |
| `playback/radio.rs` | At most five automatic upcoming entries, refill threshold two, max five preparation failures; manual entries remain separate. | Prove long-session exclusion and cache behavior; fix measured growth/continuity defects. | Session-scoped skips/removals, deliberate repeats, source/recording identity and fresh-center semantics. |
| `playback/persistence.rs` | SQLite checkpointing is atomic; occurrence/attempt/audition history uses keyset/page reads and 200-row restore batches; killed-mid-transaction coverage exists. | Exercise large histories, retention, interrupted checkpoints and schema corruption; optimize/fix only from evidence. | Last-valid committed restore paused, version gates, durable ambiguity and no whole-history load. |
| `playback/session.rs` / `output.rs` / `decoder.rs` | One session owner coordinates generation, main/Preview state, output and decoder lifecycle. | Add bounded diagnostics or targeted cleanup/fence fixes found by soak/fault tests. | No auto-reroute/resume, Preview isolation, stale-result rejection and one native output pipeline. |
| `playback/reporting.rs` / `export.rs` / `server_export.rs` / `basket_export.rs` | Source-aware remote operations retain typed outcomes and recovery state. | Measure live queue bounds and ambiguous-failure reconciliation; fix duplicate/unbounded behavior if reproduced. | Occurrence source routing, partial results, truthful provider semantics and no blind retry. |
| `sync.rs` / `sync/protection.rs` | One producer/server and verified writer; staging max two tracks/2 GiB; protected admissions use current 500 ms attributable or 100 ms unattributable delays with cancellation/recheck. | Close the live coexistence gate and fix only measured lifecycle/resource faults. | Every retry gated; no writer/manifest throttling; slow device alone is not playback risk; safe cancellation/cleanup. |
| `rpc.rs` / `main.rs` | RPC wires one daemon session; shutdown/checkpoint/sync cancellation share authoritative owners. | Exercise reopen, Quit/crash and auto/manual paths; ownership changes require focused tests. | RPC/UI do not own policy; one native event loop; truthful incomplete sync state. |
| `hifimule-ui/src/state/playback.ts` and playback components | UI consumes daemon-owned session and paged/detail contracts. | Change only if reopen/paging/a11y tests expose a defect. | Never materialize full history or retain stale async pages/status after target/session changes. |
| evidence/checklist/workflows | Story 15.17 provides packaging collectors; 16.12/16.13 provide network/coexistence formats, but installed sustained evidence is open. | Extend templates/collectors and CI only where needed for the expanded matrix. | Signing/package/runtime identity, sanitized evidence and explicit unverified states. |

### Architecture Compliance

- Keep playback in the Rust daemon/Tokio runtime with one serialized session manager, CPAL shared output, controlled FFmpeg decoding, SQLite persistence, provider abstraction and JSON-RPC/Tauri bridge.
- Every asynchronous operation carries typed session/generation/occurrence identity. Cancellation is necessary but insufficient: reject stale completion before publishing audio or state.
- Keep compressed prefetch, PCM, Radio upcoming, candidate/adaptation state, history and sync staging independently bounded. Do not trade one budget against another without a reviewed contract.
- Authenticated streams, credentials and provider-specific APIs remain daemon-side behind provider adapters. Evidence contains sanitized capability/result data only.
- Output loss pauses; it never auto-reroutes or auto-resumes. Restart restores valid committed state paused.
- Rust/SQLite use `snake_case`; JSON/TypeScript use `camelCase`. Version any changed persisted/wire/evidence schema and test old/unsupported/corrupt input.
- Remote writes are not exactly once unless the provider contract proves idempotency/reconciliation. Preserve durable ambiguity and user-visible partial outcomes.

### Library and Framework Requirements

- Keep repository pins: Rust edition 2024 / MSRV 1.93.0; Tokio `~1.49`; rusqlite `~0.38` bundled; reqwest `~0.12`; CPAL `0.18.2`; `ffmpeg-next`/`ffmpeg-sys-next` bindings `9.0.0`; packaged FFmpeg runtime `9.0.2`; crossbeam-queue `0.3.12`; patched Souvlaki `0.8.3`; libpulse-binding `2.30.1`; Tauri 2.10 family. No dependency upgrade is implied by this story.
- Tokio `JoinSet::abort_all()` does not by itself drain tasks; shutdown must await removals or use `shutdown()`. Prefer existing ownership/cancellation patterns over detached tasks.
- CPAL 0.18 returns newly built streams paused across backends; explicit `play()` and output-generation fencing remain required. Never infer cross-platform timing from callback size.
- The Rust FFmpeg bindings are `9.0.0`, while `hifimule-daemon/audio-runtime.json` pins the packaged controlled runtime to FFmpeg `9.0.2`. Capture the actually loaded library/ABI and compare it with that manifest; a mismatch is evidence, not permission for a silent dependency/runtime upgrade.
- FFmpeg API/ABI compatibility is guaranteed only within a library major version. Preserve owned close/drop paths for input, codec, resampler and output resources; sustained handle evidence supplements—not replaces—those ownership tests.
- rusqlite statement caching is bounded and configurable, but whole-history materialization remains forbidden. Prefer prepared/keyset page queries and short statement/row lifetimes.

### Testing Requirements

- Use injected monotonic/fake time for retention, expiry, retry, hysteresis and long-session state-machine tests. Installed soak duration is evidence, not a reason to make normal CI sleep for hours.
- Assert exact counts/high-water/slopes, page boundaries, cleanup deadlines, state/decision reasons, current identities and durable rows—not merely successful commands or a boolean `bounded` flag.
- Maintain negative controls: idle, playback-only, sync-only, slow device with healthy playback, intrinsically slow source without sync, stale/closed owner and unavailable provider capability.
- Run controlled mocks for deterministic correctness and real provider/output/device environments for certification. Label each evidence class; one cannot substitute for the other.
- Previous baseline: Story 16.13 recorded 1,355 daemon tests passing plus eight intentional ignores. Normal all-target Clippy has pre-existing warnings; introduce no touched-code warnings and do not claim strict `-D warnings` cleanliness.

### Previous Story Intelligence

- Review commit `989103c850cc8456eab4d9ddef269bcb6d73f1e0` is the Story 16.13 baseline. It fixed per-retry protection gating, decision re-sampling after delay, rolling recovery evidence, unattributable depletion, weak-owner closure, separate evidence/decision reasons and mixed-buffer accounting.
- Re-sample protection after every bounded delay and before each retry; never apply a second delay to the same admission. Closed publishers and lifecycle changes invalidate risk immediately.
- Story 16.13's controlled evidence is not installed certification. Its live-provider + audible-output + physical-device healthy 5% median gate remains open and is a direct 16.14 handoff.
- Preserve Story 16.12 review corrections: no chunk-latency-derived depletion, double counting, unbounded maps, premature publication or false recovery.

### Git Intelligence

- `989103c` (Review 16.13) is the implementation baseline; `6682109` introduced the protection feature; `1333dc4` created Story 16.13; `f60a93c` is the reviewed Story 16.12 measurement/adaptation baseline; `096c70c` is its superseded initial implementation.
- Keep evidence, implementation and adversarial review distinct. Do not edit runtime broadly before a frozen scenario reproduces a defect.

### Latest Technical Information

- Tokio task groups require abort **and drain** for resource-release proof; official `JoinSet::shutdown()` provides both. Source: [Tokio JoinSet](https://docs.rs/tokio/1.49.0/tokio/task/struct.JoinSet.html).
- CPAL 0.18 standardized newly built streams as paused, reducing callbacks before owner admission; preserve explicit start and current-generation checks. Source: [CPAL 0.18 upgrade notes](https://docs.rs/cpal/0.18.2/cpal/).
- The controlled packaging manifest pins FFmpeg runtime `9.0.2` with 9.0.0 Rust bindings; upstream documents API/ABI compatibility only within library majors. Record the loaded runtime/ABI and manifest match without an implicit upgrade. Sources: [`hifimule-daemon/audio-runtime.json`](../../hifimule-daemon/audio-runtime.json), [FFmpeg 9.0 API](https://ffmpeg.org/doxygen/9.0/index.html).
- rusqlite exposes a bounded prepared-statement cache; use current paged query ownership and do not treat caching as permission for unbounded result retention. Source: [rusqlite Connection](https://docs.rs/rusqlite/0.38.0/rusqlite/struct.Connection.html).

### Project Structure Notes

Likely new evidence assets:

```text
docs/playback-evidence/16-14-soak-contract.md
docs/playback-evidence/16-14-<platform>-<arch>-<scenario>.json
scripts/playback-long-session-evidence.py          # only if current collectors cannot be extended cleanly
scripts/tests/test_playback_long_session_evidence.py
```

Likely updates, driven by evidence rather than pre-emptive redesign:

```text
docs/playback-installed-test-checklist.md
scripts/playback-session-evidence.py
scripts/playback-installed-evidence.py
scripts/tests/test_playback_session_evidence.py
scripts/tests/test_playback_installed_evidence.py
.github/workflows/{build,release,smoke-test}.yml     # only if matrix automation requires it
hifimule-daemon/src/playback/{audio,streaming,http_source,adaptation,radio,persistence,session,output,decoder,reporting,export,server_export,basket_export}.rs
hifimule-daemon/src/{sync,rpc,main}.rs
hifimule-daemon/src/sync/protection.rs
hifimule-ui/src/state/playback.ts
hifimule-ui/src/components/{PlaybackControls,PlaybackDestination,PlaybackSnapshots}.ts
docs/{playback,architecture-hifimule-daemon}.md
```

Do not create all listed files mechanically. The implementation file list must contain only actual contract/evidence changes and reproducible defect fixes.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 16 and Story 16.14]
- [Source: `_bmad-output/planning-artifacts/prd.md` — FR60, FR65–75 and playback success/counter-metrics]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — playback provider integration, Radio, session control, implementation and validation contracts]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md` — installed playback refinement, responsive and accessibility validation]
- [Source: `_bmad-output/planning-artifacts/sprint-change-proposal-2026-09-19.md` — Story 15.28 to 16.14 mapping and expanded installed gate]
- [Source: `_bmad-output/implementation-artifacts/epic-16-context.md` — sequencing, resource constraints and release gate]
- [Source: `_bmad-output/implementation-artifacts/15-17-ship-verified-playback-builds-for-windows-macos-and-linux.md` — packaging/evidence baseline]
- [Source: `_bmad-output/implementation-artifacts/16-13-protect-playback-during-sync-without-unnecessary-throttling.md` — prior-story policy, review fixes and open live evidence]
- [Source: `docs/playback-installed-test-checklist.md` — installed matrix and runtime identity process]
- [Source: `docs/playback-evidence/16-12-network-profile-template.md` — network/adaptation evidence format]
- [Source: `docs/playback-evidence/16-13-coexistence-template.md` — sync coexistence handoff]
- [Source: `hifimule-daemon/src/playback/` — current playback bounds, persistence and lifecycle ownership]
- [Source: `hifimule-daemon/src/sync.rs` and `hifimule-daemon/src/sync/protection.rs` — staging bounds and protection policy]

## Dev Agent Record

### Agent Model Used

GPT-5 Codex

### Debug Log References

- 2026-09-29: Frozen contract and fail-closed evidence schema implemented before acceptance runs.
- 2026-09-29: Controlled evidence runner passed all 14 command groups on macOS ARM64 after granting local RPC socket access; the initial sandboxed run correctly failed two socket-based RPC tests with `Operation not permitted`.
- 2026-09-29: Installed/physical acceptance stopped at Task 5 because the required Windows/Linux/macOS package rows, configured providers, physical audio outputs and physical sync device are unavailable in this checkout.
- 2026-09-29: macOS 27 ARM64 isolated DMG check passed signature, architecture, clean launch, private FFmpeg load/ABI, orderly Quit, 39 packaging/runtime tests, 47 evidence tests, and the full daemon suite (1,355 passed, 8 ignored; 5 contract tests passed). No audio/USB device or provider fixture was available, so installed physical scenarios remain blocked.
- 2026-09-29: Windows local continuation: 47 evidence tests, 284 Node tests (6 skipped), 1,350 daemon tests (7 ignored) plus 5 contract tests, 7 i18n tests and TypeScript type checking passed. The Node suite initially exposed a stale destination-selection test mock; it passed after adding the current basket context method. All-target Clippy could not complete after the local FFmpeg cache disappeared and network fetch was unavailable. The only local MSI is version 0.16.0, older than the current 0.16.1 source, so it cannot be used for Story 16.14 installed certification.
- 2026-09-29: User rebuilt and installed the Windows x64 NSIS 0.16.1 package. The running daemon binary matches the current release build; authenticated health and private FFmpeg library/ABI checks passed. All-target Clippy subsequently completed with pre-existing warnings. A two-minute, 24-sample Radio resource preflight had no collection errors and remained below compressed limits; its 15 paused samples were an intentional user pause. The user reported no audible problem during sync. This short local record is not a frozen sustained/coexistence acceptance run. An isolated UI hydration smoke attempt timed out and needs diagnosis.
- 2026-09-29: Started an eight-hour Windows NSIS Radio resource sampler at 23:06:10 Europe/Paris with five-second sampling and a five-minute warm-up; it is expected to stop at 07:11 on 2026-09-30. The first 19 samples had no collection error. The sampler does not inject the frozen fault schedule or establish audible continuity; this is a resource diagnostic until the full run and companion evidence are reviewed.

### Implementation Plan

- Freeze inherited budgets, workload, faults, sampling and immutable artifact rows.
- Extend deterministic evidence coverage and enforce fail-closed, sanitized installed records.
- Run controlled proofs, then retain unavailable installed/platform work as explicit blockers without promoting mocks.

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- Added the version-1 sustained-session contract, six-row evidence index, and strict evidence template/validator.
- Extended controlled session evidence across Radio, adaptation, output fencing, reporting and immutable export behavior; all 14 command groups passed.
- Verified independent owner budgets, large-history continuity accounting, immutable raw evidence and secret/path rejection with eight Python tests.
- No runtime defect was reproduced by the controlled suite, so no playback runtime code was changed.
- Story remains in progress: installed sleep/wake, sustained soak, physical coexistence and full Epic 16 package matrix are blocked on external platforms/equipment.
- Recorded the macOS ARM64 controlled package result without promoting it to installed or physical certification.
- Repaired the destination-selection regression harness to provide the basket context method used by the current UI; all 284 local Node tests now pass.
- Confirmed the rebuilt NSIS installation matches the current daemon binary and loads the four private FFmpeg libraries with the expected ABI. Preserved the sanitized Windows Radio preflight trace and direct user audio observation; clean-install/upgrade, sustained profiles, matched throughput and the full Windows workflow matrix remain open.
- Added a validated blocked NSIS evidence record and started long Radio resource sampling; no Task 5–8 completion checkbox was changed because the installed acceptance gates remain open.

### File List

- `_bmad-output/implementation-artifacts/16-14-keep-long-listening-sessions-bounded-and-recoverable.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `docs/playback-installed-test-checklist.md`
- `docs/playback-evidence/16-14-soak-contract.md`
- `docs/playback-evidence/16-14-evidence-index.md`
- `scripts/playback-long-session-evidence.py`
- `scripts/playback-session-evidence.py`
- `scripts/tests/test_playback_long_session_evidence.py`
- `scripts/tests/test_playback_session_evidence.py`
- `scripts/tests/destination-ui.test.mjs`
- `docs/playback-evidence/16-14-windows-nsis-radio-preflight.jsonl`
- `docs/playback-evidence/16-14-windows-x64-nsis.json`

## Change Log

- 2026-09-29: Created comprehensive Story 16.14 implementation and certification guide; status set to ready-for-dev.
- 2026-09-29: Froze Story 16.14 contract, added fail-closed evidence validation, expanded controlled proofs, and retained unavailable installed/physical rows as blockers.
- 2026-09-29: Continued Windows local regression verification; repaired a stale UI test mock and retained installed/physical gates as blockers.
- 2026-09-29: Verified the rebuilt Windows NSIS installation and captured a short Radio resource preflight; retained the frozen sustained and cross-platform gates as incomplete.
- 2026-09-29: Started the Windows NSIS Radio resource run and added a fail-closed, validated blocked artifact record.
