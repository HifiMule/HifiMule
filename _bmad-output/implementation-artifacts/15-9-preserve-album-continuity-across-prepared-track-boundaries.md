---
baseline_commit: d4d73f19b7cb3466a1b612a8b22ba5099b3ecc87
---

# Story 15.9: Preserve album continuity across prepared track boundaries

Status: in-progress

## Story

As a HifiMule user,
I want album tracks to join without player-added gaps,
so that live recordings, continuous compositions and intentional pauses sound as recorded.

**Requirements:** Continuity portion of FR61; applicable FR73 and FR75; P-NFR1–4; P-AR5–7 and continuity evidence portion of P-AR14.

**Dependencies:** Stories 15.1–15.8. Prepared 2026-09-18 against the baseline above. Ordered album transport and its review fixes are implemented. Sprint tracking currently marks 15.8 done, while its story header says in-progress and installed-platform acceptance remains open. Preserve that discrepancy as an evidence limitation; do not infer platform qualification or change the predecessor's status in this story.

**Scope:** One prepared album successor, continuous native output, occurrence-aware presentation handoff, verified padding/conversion and reproducible continuity evidence. Gain remains unchanged. Album gain (15.10), Preview (15.11), destination/queue/floating redesign (15.12–14), Radio, reporting, quality adaptation (15.26), sync throttling/stress and extended reliability remain separate. No promise of uninterrupted playback when preparation misses the boundary. No new provider API, browser audio, second daemon or event loop is required.

## Acceptance Criteria

1. **Prepared continuous handoff.** Given supported adjacent album tracks with the successor ready before the boundary, decoded output continues through the same active output stream with no inserted silence, missing valid samples or duplicate boundary samples. Exactly one queue advancement occurs at the audible handoff; current metadata and position follow the correct occurrence, never prefetch start.
2. **Preserve recorded silence.** Given intentional leading/trailing silence, preserve it exactly as source audio. Do not detect/trim silence, crossfade or overlap tracks to hide a boundary.
3. **Use verified padding only.** Given validated encoder-delay/end-padding metadata, remove only known non-musical padding according to tested container/codec behavior. Missing or ambiguous metadata never authorizes guessing, trimming to provider duration, or trimming to fixture lengths. Record continuity limitations explicitly.
4. **Keep a fixed output format.** Given supported differences in sample rate/channel layout, convert into the established output format without reopening the stream between tracks. Compare resampled duration and samples against a reference conversion that accounts for filter delay/drain. Unsupported transitions produce an explicit recoverable state, never corrupt audio.
5. **Bound preparation.** Compressed and decoded lookahead have separate, enforced bounds. Audio callbacks allocate nothing, perform no blocking IO, and acquire no device-sync locks. Preparation never expands into downloading/decoding the album in full.
6. **Fence obsolete work and boundary controls.** Next, seek, Stop, replacement and output loss invalidate obsolete prepared PCM and events. Late work cannot enter the current session. Pause/Stop ordered before boundary activation prevents successor samples; test controls against both queued and submitted backend audio.
7. **Degrade honestly.** If the successor is late or unavailable, expose buffering or the established same-occurrence album pause/retry state, retain all tracks and do not silently skip. The transition is not reported as gapless success or user rejection.
8. **Prove decoded continuity.** Deterministic adjacent fixtures cover validated padding, intentional silence, different rates/layouts and cancellation around boundaries. Same-format output has zero extra silence, missing valid samples or duplicated valid samples relative to the expected reference. Record exact supported combinations and runtime versions, including exclusions.
9. **Measure physical continuity separately.** Windows, macOS and Linux physical-output checks use captured-output comparison or a documented equivalent measurement for player-added gaps. Listening supplements measurement. Label VM, callback-counter and decoder evidence separately. Real-sync sustained stress remains later acceptance; unavailable physical evidence remains explicitly unverified.

## Tasks / Subtasks

- [x] Finalize and document the handoff contract before production changes (AC: 1–9).
  - [x] Record identities, preparation bounds, fixed output policy, supported padding matrix, presentation timestamps, owner/persistence reconciliation and backend cancellation semantics in the existing API/runtime/test documents.
  - [x] Add deterministic contract tests for boundary authorization, presentation acknowledgment, failure precedence and exactly-once owner advancement.
- [x] Prepare one successor through existing provider/streaming boundaries (AC: 1, 5–7).
  - [x] Resolve by portable source identity and indexed next occurrence; retain repeated source tracks as distinct occurrences.
  - [x] Hold bounded metadata, compressed input and PCM for at most one successor; propagate cancellation/deadlines and measure aggregate high-water use.
  - [x] Fence metadata, qualification, errors and PCM before promotion; release retired resources before allocating a replacement slot.
- [x] Refactor native output lifetime and PCM consumption (AC: 1, 4–6).
  - [x] Separate continuous output ownership from per-occurrence decode ownership in both CPAL and Pulse implementations.
  - [x] Consume predecessor tail and successor head in one callback/write span without refill silence, overlap, per-track drain or reopen.
  - [x] Add bounded occurrence-aware presentation accounting; preserve Pulse startup priming, selected endpoint pinning, cork/flush/retirement and CPAL loss/sleep detection.
- [x] Integrate owner transitions and durable recovery (AC: 1, 6–7).
  - [x] Add token/occurrence-aware handoff events and terminal precedence; prevent Failed being overwritten by Completed.
  - [x] Reuse atomic disposition/current/cursor commits, bounded recovery and daemon effect bridge without restarting an already adopted stream.
  - [x] Reset per-occurrence progress, seek qualification and metadata at presentation; preserve final-album completion, explicit Next/Retry, stopped/paused intent and restart-paused behavior.
- [x] Validate conversion and padding through the production decoder (AC: 2–4, 8).
  - [x] Retain worker-owned FFmpeg objects, decoder EOF drain and resampler drain; establish the precise same-rate and mixed-rate references below.
  - [x] Add valid/absent/ambiguous padding, intentional-zero, short-track, mono/stereo and unsupported-layout fixtures.
  - [x] Test large metadata, sequential and verified Range sources, cancelled full queues and slow/malformed successor sources.
- [ ] Run regression and platform evidence checks (AC: 1–9).
  - [ ] Run focused playback/owner/decoder/streaming/effect tests, then relevant full daemon and runtime/evidence tests; compile both cfg-selected output paths. (macOS and vendored WASAPI compile/tests pass; Pulse cross-check is blocked on this host by the absent Linux OpenSSL/sysroot.)
  - [x] Extend installed evidence/checklist with a distinct continuity schema and malformed/contradictory-record validation tests.
  - [ ] Record production-decoder output and native physical measurements per OS/architecture/runtime. Leave unmet acceptance tasks open rather than equating source tests with physical proof.

## Dev Notes

### Existing system and implementation gate

The feature requires a change in output lifetime. Today `AudioEngine::start_at_epoch_kind` retires the current pipeline, and each `run_output` owns one decoder, occurrence and native stream. CPAL completes after decoder EOF, empty PCM and `PresentationClock` drain. Linux drains and retires its pinned Pulse stream before completion. Calling the existing start method sooner or fetching successor metadata alone cannot satisfy AC1.

The owner currently advances in `complete_occurrence`/`commit_terminal`: the database transition commits, generation/control epoch rotate, audio is stopped and a bounded effect starts the successor. Preserve this as the fallback/manual-navigation path. Add a distinct prepared-handoff path that changes the occurrence without destroying the authorized output stream. Do not suppress all generation changes globally to keep audio alive.

**Contract decisions for this story (validate with executable tests before integrating):**

- Maintain a continuous **output epoch** tied to the selected endpoint/configuration, and separate per-occurrence **preparation identities**. Bind a handoff token to daemon instance, session, predecessor and successor occurrence IDs, queue revision, control epoch, preparation generation and output epoch. Track source IDs alone are insufficient for repeated entries.
- The owner authorizes preparation/adoption under its serialized state. Authorization does not make the successor current, record a natural completion, publish its metadata or change the durable cursor. Provider acquisition, decoder work and allocation run outside owner/DB locks.
- A bounded preallocated boundary descriptor identifies the exact output-frame offset between predecessor and successor. The consumer may switch at that offset within the same callback/write buffer; it must not enter the normal 100 ms refill path merely because the predecessor ends. All frame operations preserve channel alignment. A short fully decoded successor is ready even if shorter than refill target.
- Establish readiness from a valid decoded head plus an authorized, current token, not HTTP success or metadata. Prepare as soon as the active stream is established; do not wait for an inaccurate provider duration to signal imminent EOF. Ready means the normal 100 ms decoded head or clean EOF with at least one complete frame. Zero-frame or failed decodes must follow an explicit bounded failure path, not automatic silent completion loops.
- Stamp the boundary using backend presentation timing, then acknowledge it outside the callback. CPAL currently has only a final drain deadline; extend this to per-boundary presentation. Linux maps submitted offsets through `SubmissionCursor` and `PresentationLedger` to server `played_frames`. Queue pop, `output.write`, decode EOF and UI interpolation are not proof of presentation.
- On matching presentation acknowledgment, the owner commits the predecessor natural outcome and successor current occurrence/cursor atomically exactly once, updates authoritative metadata/seek capability and rebinds progress to the successor. Queue order/membership did not change: preserve `queueRevision`, advance `stateSequence`. Use the measured successor offset if some frames have already presented when the owner processes the acknowledgment; do not replay them or treat them as predecessor progress.
- Keep at most one unacknowledged handoff. While it is pending, do not prepare/authorize a third occurrence. Bound retained descriptors/events and define backpressure; never silently drop the only boundary receipt. Slow-owner cases may buffer at a later boundary and must not be labeled successful gapless transitions.
- The audio callback cannot wait for SQLite. A backend may already have presented a bounded successor prefix before the owner's commit finishes. Document this explicitly: on persistence failure close the gate, retire/cork the stream, preserve the last coherent committed occurrence and one frozen terminal recovery record, and expose `PERSISTENCE_FAILED`. Do not claim rollback of sound already presented or persist a guessed successor early. Recovery retries the frozen transaction silently and leaves its resulting occurrence paused; a separate Resume starts audio. Crash recovery uses the last coherent commit. If implementation cannot preserve these invariants, stop at the contract gate instead of introducing callback DB work or prefetch-time advancement.
- A prepared-handoff acknowledgment must suppress the ordinary successor start effect; duplicate acknowledgments cannot start another worker or advance twice. Final EOF still drains once and preserves the completed album/current terminal cursor. The unprepared path keeps normal atomic advancement and buffering/retry.

### Control races, ingress and recovery

Use owner order and explicit gates, with deterministic barriers in tests. Before resolving a control against the current occurrence, reconcile the bounded backend presentation receipt: if the successor boundary has already presented, adopt/commit that receipt exactly once before targeting Stop, seek, Next or Pause at the audible occurrence. Do not discard an already-presented receipt merely because its owner event was queued. If that reconciliation cannot persist, enter the frozen terminal recovery path rather than pretending the predecessor is still audible. A Pause accepted before successor activation must close its consumption gate; Stop/output loss/seek/replacement revoke unpresented tokens and cancel preparation. Replacement may supersede the old session, but must not let old receipts mutate the replacement. Next remains intentional navigation with explicitSkip; seek retains its existing occurrence semantics, including exact-end seek not being natural completion. Native/UI actions share `PlaybackCommandService`. Never automatically retarget a stale Next at a successor after an EOF race.

Native buffers complicate controls: a successor may be submitted ahead of its audible boundary. Test Pause/Stop between submission and presentation, with backend cork/flush/retirement as required; define and measure the backend's cancellation point rather than claiming that already-presented audio can be withdrawn. Preserve frozen logical position during pause, and ensure discarded prefetched/submitted samples are neither skipped nor duplicated after Resume. Output loss pauses and never selects different speakers or auto-resumes on reconnect.

Current `PlaybackEvent::Completed` contains only position, and terminal ingress coalesces Failed/Completed under the same event kind. Story 15.8 review R13 identifies failure erasure. Harden touched ingress here: terminal failure dominates a delayed completion for the same attempt; distinct occurrence/token receipts cannot replace one another. Metadata/SeekQualified/Active/progress from a prepared successor must remain private until adoption; late predecessor events cannot overwrite the new occurrence. Reset progress sequences/anchors on handoff and qualify every event with its owning occurrence. Preserve bounded owner command/event ingress and command deduplication.

Retain 15.8 terminal recovery: outcome, sanitized reason, current occurrence, cursor and checkpoint commit together. Next/seek reject unresolved persistence recovery; Stop/replacement may invalidate it. Storage Retry retries the exact frozen transaction without audio; source Retry starts the same occurrence using the committed cursor and resets attempt disposition. Technical prefetch failure must not pause the still-playing predecessor early: retain the error in the successor slot, then expose it at the failed occurrence when the predecessor completes. No server scrobble, skip/dislike or future reporting eligibility is implied by preparation or local completion.

### Bounded resource policy

Existing `hifimule-daemon/audio-runtime.json` describes a single active source: 8 MiB compressed capacity (including 1 MiB retained network chunk and 64 KiB read scratch), PCM target 500 ms capped at 1 MiB, 100 ms startup/refill, 60 s preparation deadline. These are existing measured-policy inputs, not an active RSS promise.

For this story explicitly extend the policy to **one active plus one successor**: keep each source's compressed limit at 8 MiB and each PCM slot at `min(500 ms, 1 MiB)`, aligned down to complete output frames. Enforce aggregate compressed capacity **16 MiB** and aggregate PCM capacity **2 MiB**, with at most two decoder/source slots; lower normal-rate PCM allocation remains duration-based. Update the manifest, diagnostics and budget validation together; do not silently reinterpret the old 8 MiB aggregate as unchanged. This is a bounded design ceiling to measure, not a measured resource result.

Keep 60 s preparation timeout per admitted preparation and existing network progress timeouts. Capacity backpressure after readiness is not a stalled-source failure. Pausing retains at most the bounded slots; invalidation cancels and joins workers, including ones waiting to enqueue. Never accumulate retired workers/source responses during rapid Next/seek. Native Pulse server buffering, AVIO/decoder/resampler scratch and metadata must be accounted separately with explicit bounded behavior and measured high-water values; they are not included in the PCM ring ceiling and must not be hidden in an RSS claim. No unbounded album download, task-per-track spawn, full-album decode or whole-library materialization.

### Fixed output conversion and padding policy

Open the selected shared output once; retain its negotiated rate, mono/stereo channel layout and native sample format until explicit output change/loss or session teardown. Decode to packed f32 in that established rate/layout, then retain existing native sample conversion. Linux uses its existing pinned Pulse configuration; do not route it through a new CPAL Linux path. Support mono/stereo input conversion using FFmpeg's explicit channel layouts; untagged one/two-channel sources follow existing mono/stereo normalization. Other/ambiguous layouts must fail visibly unless separately implemented and verified.

Each successor gets its own decoder and conversion state using the fixed output target. Preserve all decoder-drain and resampler-flush samples exactly once before its boundary marker. Define the reference as independently decoding each source with the controlled runtime and converting it to the identical output target/policy, draining each conversion, then concatenating in album order. This makes rounding and filter tails explicit; never compare 44.1 kHz input counts directly with 48 kHz output counts. Test markers and boundary windows as well as full lengths; if this conversion policy creates an artifact outside its reference tolerance, report/fix it rather than masking it with overlap or silence trimming. Keep seek preroll/discard separate from encoder-padding handling.

Use FFmpeg's validated demuxer/decoder padding interpretation for the exact tested representations. Do not add a second generic trim over decoder output. Baseline matrix: PCM WAV, FLAC, ALAC/M4A, MP3 with valid delay/padding metadata, AAC/M4A with validated container timing, and Opus/Ogg pre-skip/end trimming. Missing/ambiguous metadata means no guessed correction. Raw AAC is not automatically equivalent to AAC/M4A. Existing AIFF/Vorbis/WMA playback support is not blanket continuity certification: test and list exact additional combinations; WMA's current test allows 2048 frames of padding tolerance and cannot establish gaplessness. Preserve ordinary playback for uncertified representations and document the limitation.

### Source files: current behavior, change and preservation

Paths beginning with `playback/` in the table are relative to `hifimule-daemon/src/`; other paths are repository-relative. NEW names are suggested scoped helpers; reuse current modules before adding abstractions.

| File | Current behavior | Change / preserve |
| --- | --- | --- |
| `hifimule-daemon/src/playback/audio.rs` | One `Pipeline`, serialized starts/retirement, provider resolution, CPAL output, generation gate, runtime diagnostics | Own continuous stream and active/successor slots; retain selected-output validation, sanitized failures, deadline/cancellation, no competing streams. |
| `playback/audio/pulse_output.rs` | Pinned stream per track, startup server priming + 200 ms local reserve, presentation ledger, drain/retire at EOF | Keep one stream across prepared boundaries; preserve priming only at true startup, writable-capacity pacing, pause/cork and actual server presentation. |
| `playback/output.rs` | Frame-aligned `PcmConsumer`, refill policy, final `PresentationClock`, bounded 256-span Pulse ledger, joining `DecoderWorker` | Add boundary-aware consumption/presentation with fixed-capacity callback-safe storage. Existing VecDeque ledger is outside the callback; do not move allocating containers into it. |
| `playback/decoder.rs` | Worker-owned FFmpeg custom IO, sequential FLAC/MP3 probing, verified media-time seek, mono/stereo resampling, EOF + bounded flush | Produce exact tail/head and token-bound completion; preserve large metadata/Range fallback, seek qualification/preroll and cancellation while queues are full. |
| `playback/streaming.rs`, `playback/http_source.rs` | 8 MiB bounded compressed reader, deferred seek, typed timeout/source failure, verified HTTP access/deadlines | Reuse for each bounded slot; preserve advisory unknown-size probes, provider-controlled request/privacy and actual retained-byte accounting. |
| `playback/model.rs`, `playback/mod.rs` | Strict wire models, owner handle, event ingress and shared identities | Add private handoff/token commands/events; keep internal preparation off public RPC, camelCase and decimal-string wire counters. |
| `playback/session.rs` | Serialized command owner, generation/epoch fencing, terminal outcome/recovery, paged successor and bounded effects | Adopt presentation once without stopping continuous output; preserve transactions, dedup, stale-event protection and final/failed state invariants. |
| `playback/persistence.rs` | v2 local occurrence outcomes, atomic advancement/failure/checkpoint, indexed successor lookup | Reuse current transactions; add only narrowly needed handoff persistence support. Never persist decoder objects, URLs or transient PCM. No schema bump unless durable data genuinely changes. |
| `playback/commands.rs`, `playback/commands_tests.rs` | Shared provider/source resolution, transport and seek effects, daemon-owned successor dispatch | Reuse preparation routing and ensure adoption does not trigger duplicate `start`; preserve UI/native parity and shutdown ordering. |
| `hifimule-daemon/audio-runtime.json`, runtime verifier/tests | Controlled FFmpeg build and single-source budgets | Document/enforce active+successor aggregate policy and actual linked runtime. Preserve version/build verification and network-disabled FFmpeg. |
| `experiments/playback-probe/{generate-fixtures.py,verify.py,ffmpeg_decode.py}` | Deterministic six-format 48 kHz fixtures/reference verifier; CLI adapter rejects mixed layouts/rates | Reuse algorithms/reference data; extend a production-path harness for mixed conversion/silence/padding. Probe-only success cannot satisfy production output acceptance. |
| `docs/api-contracts-hifimule-daemon.md`, `docs/playback-installed-test-checklist.md` | Existing transport and installed evidence contracts | Record boundary tokens/presentation, error fallback, support matrix and physical measurement procedure. |
| `scripts/playback-installed-evidence.py`, `scripts/tests/test_playback_installed_evidence.py` | Sanitized versioned evidence with strict album/seek assertions | Add distinct continuity evidence; reject missing runtime/capture, wrong occurrence sequence, malformed types and contradictory success. |

Read these complete files before editing, including backend-specific implementations and their tests. Add a small private `playback/continuity.rs` and adjacent tests if it reduces audio/session complexity. Inspect `hifimule-daemon/src/playback/devices/pulse_stream.rs` if cancellation requires changes to its cork/flush API. UI/native consumers should continue to read the authoritative snapshot without a redesign; only add localization/UI files if a new visible recoverable reason is necessary, with EN/FR/ES/DE and existing focus/output controls preserved.

### Testing and evidence requirements

Use injected clocks, barriers and fake output sinks to test actual production PCM consumers/owner effects without audio hardware. Do not assert only cursor movement or a positive decoded-frame count.

- **PCM boundary oracle:** use the original non-round 96,017 / 72,011 / 120,013 frame fixtures (288,041 total at 48 kHz stereo) and production decoding. Same-rate lossless fixtures require exact sequence/count; lossy fixtures require exact validated lengths plus full-signal/boundary comparisons with documented codec tolerance (existing probe RMS 0.02 is fixture-specific). Comparing production assembly against independently decoded reference catches extra/missing/duplicated frames even for lossy formats. Never use a large RMS tolerance to excuse a time shift.
- **Preserved audio:** zeros at both edges and internally, impulse/unique boundary markers, tracks shorter than callback/refill, tail shorter than one callback, two different sources with identical track IDs, repeated occurrence of one source, mixed codecs, 44.1↔48 kHz and mono↔stereo. Unsupported layouts and padding ambiguity get explicit non-success records.
- **Race matrix:** Pause/Resume/Stop/Next/seek/replacement/output loss/Quit before preparation completion, after readiness, while tail/head share a callback, and after submission before presentation. Include the explicit presentation → control → queued acknowledgment ordering, duplicate acknowledgments, stale metadata/progress, Failed→Completed and Completed→Failed, owner ingress saturation, DB failure at handoff, storage Retry, short successors and slow owner. Show no leaked head after a winning cancellation and no duplicate effect/advancement.
- **Resource behavior:** delayed/failed next source, long streams, large metadata, full PCM rings, cancelled blocked writers and rapid replacements. Assert per-slot and aggregate high-water limits, number of live decoders/responses, bounded pending events and orderly cleanup. Real device sync must remain independent; no sync lock on the callback path.
- **Regression:** preserve 15.7 seek exact-end/qualified landing, 15.8 source Retry/replay and atomic terminal recovery, paused/stopped Next, no-device playback, same output identity, UI close/reopen and offline restart-paused. Query the persisted current/outcome after reopen rather than trusting in-memory projection alone.
- **Physical output:** for each tested OS/architecture record build commit, loaded FFmpeg libraries/configuration, binding/backend versions, endpoint/rate/layout, fixture hashes, preparation status, occurrence boundary offsets, open/close counts, underruns, capture method and raw capture location/hash. Align capture globally, account for hardware latency/clock drift and compare boundary windows; do not independently realign each side and thereby hide a gap. Document timing resolution/tolerance and listening result. A volume-zero run or VM counter is not physical proof.

Start with `rtk cargo test -p hifimule-daemon playback::`, then relevant complete daemon tests with the controlled runtime and service/mock-server access. Use `rtk proxy python3 -m unittest discover -s scripts/tests -p 'test_playback_installed_evidence.py'` for evidence validation. Run the repository's runtime verifier/tests and platform build paths where touched. Existing six-format fixtures must be generated/available; missing fixtures/runtime are explicit test prerequisites, never silent skips reported as passes. Run UI build/behavior tests only if UI/contracts are touched. Do not copy historical test counts into current evidence.

### Previous-story and git intelligence

- `d4d73f1 Review 15.8` fixed album admission/epoch races, receipt replay, attempt outcomes, atomic terminal recovery, stopped Next, provider deadline, inaccessible keyboard action and evidence validation. Preserve these corrections. R13 terminal failure coalescing remains relevant to this change.
- `79a93a7 Dev 15.8` introduced ordered album transport, occurrence outcomes/persistence v2, daemon effect bridge and UI/native Next/Retry. Build on these instead of a second queue owner.
- `b9c2860 Story 15.8` is planning context, not execution evidence. `b54751a Finish 15.7` marks completion but does not supply new physical numeric qualification.
- `9cd69fd fix: make device credentials provider-aware` preserves multi-provider authentication; preparation must use the queued portable source and provider boundary independently of the browsed server.
- Feasibility reports establish six-format decoded continuity on tested Mac/Windows ARM64/Ubuntu ARM64 runtimes, not physical gaplessness. Ubuntu FFmpeg 8.0.1 added 1,751 AAC frames; a controlled FFmpeg 9 build passed the same fixtures. Wrapper version/pkg-config output alone did not ensure the intended loaded libraries.

### Architecture, versions and current technical references

Use current repository pins: Rust edition 2024/MSRV 1.93.0; CPAL `=0.18.2`; ffmpeg-next/sys `=9.0.0`; controlled FFmpeg 9.0.1; crossbeam-queue `=0.3.12`; libpulse-binding `=2.30.1`; locally patched Souvlaki `=0.8.3`. No dependency upgrade is required. Architecture's CPAL 0.16 experiment and earlier generic crate examples are historical. Playback remains daemon-owned; SQLite and provider IO stay outside audio callbacks; authenticated Tauri RPC and shared native command service remain in use.

Primary documentation checked 2026-09-18: [CPAL 0.18.2](https://docs.rs/crate/cpal/0.18.2) and [upgrade notes](https://docs.rs/crate/cpal/0.18.2/source/UPGRADING.md) confirm the selected API generation; avoid copying old borrowed-config examples. [ffmpeg-next 9 resampling Context](https://docs.rs/ffmpeg-next/9.0.0/ffmpeg_next/software/resampling/context/struct.Context.html) exposes run/flush/delay; [libswresample](https://www.ffmpeg.org/doxygen/trunk/group__lswr.html) documents delayed output and flushing. Verify behavior against the pinned runtime rather than assuming trunk identity. [FFmpeg releases](https://ffmpeg.org/download.html) lists 9.0.1 as current stable; repository runtime identity/hash remains the packaging authority. These references do not independently certify padding, the packaged build or absence of security defects.

Project context's provider abstraction and managed-zone safety remain foundational; its greenfield label is stale. The January UX specification predates playback. Approved playback amendments override old no-device restrictions, discretionary album ordering and single-provider assumptions. Preserve existing Shoelace components, server-name source badges and focus behavior.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 15, Story 15.9, adjacent 15.8/15.10 and playback requirement inventory]
- [Source: `_bmad-output/planning-artifacts/prd.md` — Desktop Playback extension, FR61/73/75 and playback non-functional requirements]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Playback Audio Pipeline; Implementation Contracts; Validation Refinements; Project Structure]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md`; `_bmad-output/planning-artifacts/project-context.md`]
- [Source: `_bmad-output/implementation-artifacts/15-8-play-an-album-in-order-and-advance-through-its-tracks.md` — Review Findings; Persistence and state consistency; evidence limitations]
- [Source: `_bmad-output/implementation-artifacts/playback-feasibility-results.md`; `playback-linux-vm-results.md`; `playback-windows-vm-results.md` — decoder matrix and runtime-specific limitations]
- [Source: `Cargo.toml`; `hifimule-daemon/Cargo.toml`; `hifimule-daemon/audio-runtime.json` — current versions and resource policy]
- [Source: implementation files listed above — inspected current production behavior, not proposed architecture scaffolding]

## Dev Agent Record

### Agent Model Used

GPT-6 (story preparation).

### Debug Log References

Preparation only: repository/source inspection, previous-story and git analysis, primary documentation research and story checklist review. No implementation, audio capture or runtime acceptance tests were performed for this story preparation.

#### Contract Gate Investigation — 2026-09-18

The first task's contract gate is complete. The user prioritized an effective Pause as soon as practical without sacrificing correctness. The resulting rule closes consumption immediately and lets the acknowledged backend cancellation point decide a submitted-audio race. A presented boundary is reconciled before Pause targets the audible successor; unpresented successor PCM is retained or replayed exactly once. A backend that cannot establish a coherent cutoff retires and resumes from the last coherent commit.

Evidence:

- `rtk cargo test -p hifimule-daemon playback::` is rejected by the build-time runtime-verification guard. The supported command is `rtk npm run build:daemon -- test -p hifimule-daemon playback::`.
- The supported wrapper verified FFmpeg ABI versions avcodec/avformat 63.1.101, avutil 61.1.101 and swresample 7.1.101. Initial sandbox execution had six local HTTP mock-server permission failures. After the retained changes, rerunning with local-server access passed: **214 passed, 0 failed, 6 ignored**. This is regression evidence, not continuity acceptance.
- Existing `AudioEngine::control` closes the consumption gate on Pause, and closes/cancels on Stop. The CPAL callback loads that gate before rendering. Closing it cannot retract a buffer already submitted to the backend.
- A temporary characterization harness (`/tmp/hifimule-15-9-contract-gate.rs`) imports the production `output.rs`: after one frame is rendered, a disabled later render emits zero and retains queued PCM, while the previously submitted frame remains intact. The characterization plus six existing output tests passed. This is consumer-level evidence only, not a native cancellation or physical-output test; the harness is not a committed regression test.
- Locally installed CPAL 0.18.2 `src/host/wasapi/stream.rs` implements `StreamTrait::pause` by enqueueing `Command::PauseStream`; the backend invokes `IAudioClient::Stop` later in `process_commands`. Its return does not acknowledge that cancellation has taken effect. CoreAudio's pause calls AudioUnit stop but does not return an exact consumed-frame cursor. These observations do not prove native extension work is impossible; they identify missing primitives for the stricter interpretation.
- A submission timestamp alone cannot establish the winner of a concurrent native pause. An implementation must reconcile presentation with the actual backend cancellation point, and retain/replay only the unpresented prefix. Treating callback consumption as presentation would violate the story.

Contract implementation: `playback/continuity.rs` defines occurrence-aware authorization, readiness, presentation adoption, pause acknowledgment and terminal precedence. Four deterministic tests cover full-token authorization, exactly-once adoption, both Pause race outcomes and Failed-over-Completed precedence. `BoundaryPcmConsumer` proves a ready tail/head can join inside one callback without silence or duplication and that an unready successor cannot leak. The runtime manifest now records two bounded slots and aggregate compressed/PCM ceilings. API and installed-test documents record output lifetime, padding/conversion support, persistence recovery, evidence separation and cancellation semantics. Terminal ingress now preserves Failed over Completed in either delivery order. Dependencies and predecessor status are unchanged. Physical evidence for all platforms remains unverified.

The user selected the native-extension route. The repository now pins a source patch of CPAL 0.18.2 with `StreamTrait::pause_with_snapshot`: unsupported backends fail without changing stream state; WASAPI acknowledges `Stop`, captures `IAudioClock` position and exact stopped padding, then resets the client. HifiMule now retains a fixed-capacity suffix of submitted WASAPI frames, reconciles the reported pending suffix after Pause, rolls back logical position for its source-audio frames, and replays it exactly once before new PCM on Resume. Two deterministic tests cover exact suffix replay, disabled-gate non-leakage, bounded failure, and logical-audio accounting. CoreAudio callback timestamps remain an estimate rather than an exact consumed cursor, so its experimental snapshot implementation was removed and production macOS keeps the existing synchronous consumption gate. Pulse retains its cork and played-frame ledger. The patched CPAL crate passes an `aarch64-pc-windows-gnullvm` compile check using the rustup toolchain explicitly (the Homebrew Cargo/Rustup target mismatch caused the earlier false missing-target result). Physical/native race evidence remains outstanding.

### Completion Notes List

- Implemented indexed, occurrence-aware successor resolution and one-slot provider preparation with deadline/cancellation fencing, repeated-source preservation, separate compressed readers, aggregate diagnostics, and reusable two-slot decoder ownership.
- CPAL and Pulse now keep the native stream alive across ready boundaries, join tail/head in one render/write span, acknowledge presentation before the atomic owner transition, retire the previous decoder slot, and prepare the following occurrence without rotating the playback generation.
- Pause closes consumption immediately. WASAPI acknowledges Stop and replays the exact unpresented submitted suffix; CoreAudio and Pulse retire a pipeline when a submitted boundary lacks a coherent native cutoff, so Resume restarts from durable state instead of guessing.
- Added strict versioned physical-continuity evidence rows and malformed/contradictory-record tests. Physical captures remain explicitly unverified and therefore keep the final evidence task and story status open.
- Validation on 2026-09-18: playback suite **218 passed, 0 failed, 6 ignored**; full daemon suite **919 passed, 0 failed, 6 ignored**; installed-evidence validator **30 passed**; runtime manifest verifier **8 passed**; `git diff --check` passed. The vendored WASAPI CPAL target compile passed. Pulse cross-compilation reached the host's missing target OpenSSL/sysroot prerequisite before compiling the daemon.
- Contract gate completed with an executable occurrence-bound handoff state machine and documented backend-acknowledgment Pause policy.
- Contract tests passed: 4 passed, 0 failed. Runtime buffer-policy manifest test passed: 1 passed, 0 failed.
- Added callback-safe two-slot boundary oracle tests and hardened terminal ingress so failure dominates delayed completion.
- Added a repository-owned CPAL 0.18.2 patch plus a bounded submitted-tail/replay ledger. WASAPI production Pause now acknowledges and flushes native padding, and Resume replays that suffix once. CoreAudio remains gated without guessed flush/replay.
- Ultimate context engine analysis completed - comprehensive developer guide created.
- Ready for development with explicit handoff/padding/conversion/resource contracts and acceptance evidence requirements. This status does not certify implemented or physical continuity.
- Predecessor status discrepancy and installed-platform evidence limitations retained without altering prior story status.

### File List

- `_bmad-output/implementation-artifacts/15-9-preserve-album-continuity-across-prepared-track-boundaries.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `docs/api-contracts-hifimule-daemon.md`
- `docs/playback-installed-test-checklist.md`
- `hifimule-daemon/audio-runtime.json`
- `hifimule-daemon/src/playback/continuity.rs`
- `hifimule-daemon/src/playback/audio.rs`
- `hifimule-daemon/src/playback/audio/pulse_output.rs`
- `hifimule-daemon/src/playback/commands.rs`
- `hifimule-daemon/src/playback/mod.rs`
- `hifimule-daemon/src/playback/model.rs`
- `hifimule-daemon/src/playback/output.rs`
- `hifimule-daemon/src/playback/session.rs`
- `hifimule-daemon/src/rpc.rs`
- `scripts/playback-installed-evidence.py`
- `scripts/tests/test_playback_installed_evidence.py`
- `scripts/tests/verify-audio-runtime.test.mjs`
- `Cargo.toml`
- `Cargo.lock`
- `third_party/cpal/`

### Change Log

- 2026-09-18: Completed the contract gate and added boundary tests, bounded runtime policy, terminal precedence, a WASAPI acknowledged-pause CPAL patch, and exact pending-tail replay. Production successor preparation, adoption and physical evidence remain pending; CoreAudio exact cutoff remains unresolved.
