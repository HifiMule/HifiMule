---
baseline_commit: 78d963ed5efee7fdaed6d291fcd120700d3432ee
---

# Story 15.7: Seek within a track and see the actual playback position

Status: done

## Story

As a HifiMule user,
I want to move to another point in a track when its source supports seeking,
so that I can replay a passage or continue from a chosen position without restarting the whole track.

**Requirements:** Position-control portion of FR58; seeking integrity portion of FR75; P-NFR4 and P-NFR6; seek portions of P-AR3, P-AR6, P-AR9 and P-AR10; applicable P-UX-DR7 and P-UX-DR13.

**Dependencies:** Stories 15.1–15.6 are marked done in the current sprint. Extend their daemon-owned single-track player, provider resolution, selected-output safety, native controls and checkpointed session. Preparation date: 2026-09-18; repository baseline inspected: `21e1800`.

**Scope:** Current-track seeking and elapsed/duration presentation in the existing playback controls, shared native seeking, and truthful committed-position restoration. Preserve the occurrence, source and queue. Queue advancement/Next (15.8), gapless continuity (15.9), previews (15.11), full Playback destination/bar redesign (15.12/15.14), server reports (16.7) and adaptation (16.12) remain separate. Seeking does not authorize a representation switch, a browser player, a new event loop, or sync throttling.

## Acceptance Criteria

1. **Seek accurately and preserve transport intent.** Given a loaded track with positive known duration and validated seeking for its provider/representation, a valid UI seek moves to the chosen media time within the documented fixture tolerance. It preserves playing or paused intent after completion, except the explicit end-of-track rule below. The committed display comes from the actual decoded/presented result, never an unconfirmed requested target.
2. **Expose honest capabilities.** A source without reliable media-time seeking or usable duration has disabled seek controls and an accessible, localized explanation. Ordinary playback remains usable. Neither `range_supported`, an extension, an `Accept-Ranges` header nor a successful byte-range request alone establishes time-seek support. Zero/missing duration is unavailable, not a zero-length seekable track.
3. **Validate boundaries.** Invalid, non-finite, negative, fractional, unsafe-integer or beyond-duration absolute targets are rejected without changing transport, queue or committed position. Zero is valid; exactly duration follows the explicit completion contract and never automatically repeats the track. Native protocol-specific boundary normalization is documented and tested before entering the common owner path.
4. **Fence superseded work and bound resources.** A newer admitted seek, track replacement or Stop invalidates pending seek fetch/decode/output work. Old work cannot emit old-position audio, commit position, publish a failure over the new operation, or restart playback. Repeated seeking preserves bounded compressed/PCM storage and bounded workers/requests. Pause, output changes/loss and Quit remain responsive and obey the same generation/control-epoch rules.
5. **Recover truthfully from seek failure.** Disconnects, malformed range replies, decode failures and preparation timeout retain a recoverable current occurrence and last committed actual position. Expose the real paused/buffering/playing outcome and sanitized retryable error; never claim the requested target was reached or record a skip/dislike. No automatic output substitution occurs.
6. **Use native/UI parity.** Supported native relative/absolute seek commands use the same capability checks, checked units, owner admission and audio effect as UI seeking. Native position updates reflect authoritative state and acknowledge actual discontinuity only after success. Omit unsupported native seek actions where supported by the OS.
7. **Render authoritative progress.** Playing, paused, buffering and restored positions use daemon updates. Interpolate only while authoritative playback is advancing; correct on pause, seek completion, reconnect, generation/session/occurrence changes and source changes. Routine progress does not increment `queueRevision`, and the UI does not become a second transport owner.
8. **Restore only committed results.** After successful seek, checkpoint and restart, restore paused at the last committed actual position. Failed, pending or superseded targets never enter durable position. Seeking itself sends no completed-listen report; skipped-over duration must not become evidence of listening.
9. **Validate each enabled combination.** Deterministic decoded-position fixtures and integration tests cover forward/backward, paused, exact-end, repeated and failed seeks. Record numeric accuracy and actual transport outcomes for supported provider/format combinations on Windows, macOS and Linux. Untested combinations remain disabled; ordinary-playback smoke evidence is not seek evidence.

## Tasks / Subtasks

- [x] Close and record the seek contract before implementation (AC: 1–9).
  - [x] Record the initial provider/representation/platform capability matrix, supported mechanism, timestamp origin, decoded landing tolerance and presentation-latency allowance in the API/test documentation. Implement the initial Jellyfin original PCM-in-WAV path described below; add other combinations only after qualification, never by blanket provider-wide capability.
  - [x] Adopt the boundary, failure, exact-end and identity contracts below; resolve native normalization and transport-race tables in tests.
- [x] Extend authoritative model, owner and shared service (AC: 1–5, 8).
  - [x] Add a strict seek request, additive capability/pending-outcome projection and a generation-fenced commit event; route RPC/native admission through one `PlaybackCommandService` effect.
  - [x] Keep pending target separate from committed position, preserve queue/occurrence identity, and implement replay/conflict behavior across the existing command namespace.
  - [x] Reset progress ingress for new generations; checkpoint confirmed seek commits, including paused seeks, with existing persistence failure handling.
- [x] Implement bounded media-time seeking in the existing pipeline (AC: 1–5, 9).
  - [x] Use validated demuxer seeking, decoder flush/recreation and bounded pre-roll trimming; preserve request authentication, representation identity and byte-range validation.
  - [x] Retire old decode/output buffers before activation, retain latest-seek supersession and selected-output safety, and prevent any audible output during paused preparation.
  - [x] Measure actual landing/presentation position on CPAL and Pulse paths; exclude pre-roll, buffering silence and queued-but-unpresented audio from actual advancement.
- [x] Wire native seek and actual-position publication (AC: 3, 6–7).
  - [x] Extend bounded native intents and actual platform capability masks. Implement checked Windows/macOS conversions and MPRIS track-ID fencing plus correctly typed, success-driven `Seeked`.
  - [x] Preserve publication coalescing, correlated failures, native registration lifetime, nonfatal registration errors and priority teardown.
- [x] Add accessible seek/elapsed UI using the current controls (AC: 1–3, 5, 7).
  - [x] Add elapsed/duration, keyboard-operable slider, local scrub preview, pending/error/unavailable feedback and bounded interpolation.
  - [x] Preserve mounted focus, independent output selection, responsive compact output dropdown and polling cleanup. Add English/French/Spanish/German catalog strings.
- [ ] Add integration, fixture and installed evidence coverage (AC: 1–9).
  - [x] Exercise the real RPC/native ingress → service → owner → audio seam, not only enum mappings; add deterministic race barriers and mock HTTP failures.
  - [x] Extend UI clock tests, persistence/restart tests and installed collector validation to reject no-op, stale or contradictory success evidence.
  - [ ] Run required installed provider/format/backend/OS/architecture qualification; local automated checks pass as recorded below, but installed evidence remains unavailable.

### Review Findings

Review date: 2026-09-18. Scope: `78d963e..c03a5af`; full Blind Hunter, Edge Case Hunter and Acceptance Auditor review. The user authorized applying every patch. Ten findings are fixed; the qualification gate is patched, but its installed-evidence/usable-combination requirement remains open. The pre-existing macOS conversion issue is unchanged.

- [x] [Review][Patch] **P1 — Reset progress admission to the committed backward-seek cursor.** Admission initializes the new generation's ingress position from the old cursor; commit changes the session cursor, but `refresh_ingress` only resets on a generation change. Subsequent positions below the old cursor are rejected as `STALE_PROGRESS`, freezing progress/checkpoints until playback catches up. Reset the ingress baseline on the matching commit and test backward commit followed by real progress. AC7–8. [hifimule-daemon/src/playback/session.rs:1927]
- [x] [Review][Patch] **P1 — Publish ordinary pipeline failures after seek completion.** The worker retains `worker_seek_failure` for its entire lifetime and maps later output/HTTP/decode errors to `SeekFailed`; the owner ignores that event once `pending_seek` is cleared. Audio can stop while the snapshot remains active. Distinguish preparation failure from failure after commit. AC5. [hifimule-daemon/src/playback/audio.rs:807]
- [x] [Review][Patch] **P1 — Supersede pending seeks on output generation changes.** Output selection/loss rotates generation without clearing `pending_seek`; the old preparation is fenced out, but later Resume enters the pending-seek branch and never dispatches replacement audio. Clear the abandoned operation with a superseded outcome on these transitions. AC4–5. [hifimule-daemon/src/playback/session.rs:1518; hifimule-daemon/src/playback/session/output_selection.rs:355]
- [x] [Review][Patch] **P1 — Reconcile duration with decoded media before admitting exact-end success.** Duration still comes exclusively from integer provider seconds. The exact-end branch checkpoints that value without validating actual media end; a 10.5-second WAV advertised as 10 seconds completes early, while overstated metadata can persist a cursor beyond EOF. Derive/validate the qualified duration and reject discrepancies. AC1–3. [hifimule-daemon/src/playback/audio.rs:649; hifimule-daemon/src/playback/session.rs:1687]
- [ ] [Review][Patch] **P1 — Gate enablement on the required provider/format/platform qualification.** Runtime container/codec recognition enables selected original formats, although the installed checklist explicitly leaves the required numeric platform evidence unverified. Keep unqualified combinations disabled and record numeric landing/transport evidence before enabling them; retain at least one genuinely qualified usable path as required by the story. AC9. [hifimule-daemon/src/playback/decoder.rs:161; hifimule-daemon/src/playback/audio.rs:1110] **Partial resolution:** the per-track runtime gate exposes Jellyfin original PCM-WAV, AAC/ALAC-in-M4A, Opus-in-Ogg, MP3 and FLAC only after FFmpeg verification. Navidrome now exposes the same raw original formats only after a fresh authenticated ping reports `type=navidrome`; other Subsonic/OpenSubsonic servers remain disabled. All enabled paths reconcile duration and enforce the 50 ms landing budget. Platform qualification and accepted numeric installed rows remain required, so this item is intentionally unchecked.
- [x] [Review][Patch] **P2 — Keep resolution and qualification in separate event slots.** `Resolved` and `SeekQualified` have the same coalescing kind. If qualification arrives before the owner's next drain, it removes the metadata/duration event, leaving duration absent and seeking unavailable. Preserve both events or merge their payloads. AC2, AC7. [hifimule-daemon/src/playback/session.rs:1862]
- [x] [Review][Patch] **P2 — Preserve an active scrub across polls and separate preview from elapsed time.** A newer ordinary snapshot clears `previewMs` before release, and the preview also replaces the committed elapsed label. Reproduction: a 7000 ms drag becomes 1500 ms after one progress poll. Track active scrubbing separately and keep the elapsed display authoritative. AC1, AC7 and UI contract. [hifimule-ui/src/components/PlaybackControls.ts:121; hifimule-ui/src/components/PlaybackControls.ts:201]
- [x] [Review][Patch] **P2 — Fence queued scrubs by session and occurrence identity.** `queuedSeekMs` stores only a number and dispatches against the latest snapshot. Reproduction: queue 7000 ms behind a pending seek, replace the track, then resolve the first RPC; the queued request targets the replacement occurrence. Clear or reject stale queued intent on identity changes. AC4, AC7. [hifimule-ui/src/components/PlaybackControls.ts:245]
- [x] [Review][Patch] **P2 — Render rejected seek errors immediately.** The catch stores `commandError`, but finally calls only `renderTimeline`; unchanged snapshots do not trigger `render`. Reproduction confirms `INVALID_SEEK` is stored but absent from the alert. Render the error and clear it when a new command begins. AC3, AC5. [hifimule-ui/src/components/PlaybackControls.ts:237]
- [x] [Review][Patch] **P2 — Deduplicate native discontinuities by committed operation identity.** Comparing only the last landed position suppresses a second successful seek to the same position when publication misses its intermediate pending state. Preserve operation identity through coalesced native projection and emit once per successful discontinuity. AC6. [hifimule-daemon/src/playback/native.rs:560]
- [x] [Review][Patch] **P2 — Reject seek evidence that never reaches the requested target.** The validator compares landing with the oracle but never ties either to the requested target; forward/backward coverage uses requests alone. Reproduction with both actual/oracle/committed positions fixed at the prior 3000 ms and requests at 4000/2000 ms returns no errors. Validate target accuracy and actual movement, including transport preservation. AC9 and evidence contract. [scripts/playback-installed-evidence.py:519]
- [x] [Review][Defer] **P2 — Reject finite macOS time values outside Duration's range.** `Duration::from_secs_f64` can panic for oversized finite values; use the checked conversion. This conversion already existed before the reviewed diff, so classified deferred as pre-existing rather than an introduced defect. [third_party/souvlaki/src/platform/macos/mod.rs:300] — deferred, pre-existing

Validation: daemon playback selection passed **183 tests, 6 ignored** after rerunning outside sandbox restrictions; UI suite **17 passed**; playback evidence suites **20 passed**. Additional scratch reproductions confirmed scrub reset, invisible rejection, cross-occurrence queued seek and accepted no-op evidence. Installed audio/hardware qualification was not performed. One preliminary Linux argument-count concern was dismissed after confirming the cfg-selected function alias; no review layer failed.

### Review Patch Validation (2026-09-18)

- Reset ingress on committed backward seeks and restored terminal-cursor restart; tests verify subsequent progress and persisted position before reaching the old cursor.
- Preserve independent resolution, qualification, commit and pipeline-failure event slots. The owner distinguishes preparation failure from post-commit failure, and output changes supersede abandoned seek state.
- Reconcile FFmpeg PCM stream duration with whole-second provider metadata; exact-end uses verified milliseconds, and contradictory durations do not qualify. Ordinary unsupported-WAV terminal resume remains covered.
- Preserve scrub previews through polling while displaying authoritative elapsed time, fence queued scrubs by instance/session/occurrence, render RPC rejections immediately, and acknowledge native discontinuities by operation identity.
- Reject no-op/wrong-target evidence, invalid numeric fields, transport mismatches and entirely disabled acceptance matrices.
- Playback suite after restoring the runtime-qualified PCM-WAV path: **193 passed, 6 ignored**. All new review regressions passed, including qualification, decoded duration, seek landing, backward progress and race coverage.
- Provider suite **125 passed**; UI suite **20 passed**; evidence suites **23 passed**; i18n **7 passed**; UI TypeScript/Vite build passed. Daemon Clippy passed with existing warnings outside the touched playback code. Targeted Rust formatting and `git diff --check` passed.
- Installed Windows/Linux/macOS seek qualification was not available in this run. Runtime-verified Jellyfin original PCM-WAV seeking is enabled to collect that evidence; other providers/formats remain unavailable. This does not complete AC9. Story and sprint status remain `in-progress`; ten patch findings are closed and one is partially resolved.
- Field follow-up: the user confirmed that Jellyfin WAV seeking works on macOS and Linux after the runtime-gate correction. The reports are qualitative and lack the architecture, PCM depth and numeric landing data required for AC9-installed rows.
- First compressed extension batch: Jellyfin original AAC/ALAC-in-M4A and Opus-in-Ogg now use distinct provider candidates plus post-open FFmpeg codec/container verification. Non-periodic chirp fixtures correlate decoded landings within 50 ms; MP3, FLAC and Navidrome remain unchanged pending later batches.
- Compressed field follow-up: the user confirmed that Jellyfin original AAC-in-M4A, ALAC-in-M4A and Opus-in-Ogg all seek successfully on macOS. The report is qualitative and does not complete the numeric AC9 evidence row.
- Second compressed extension batch: Jellyfin original MP3 and FLAC use distinct provider candidates plus post-open verification. A bounded 50 ms compressed pre-roll reconstructs MP3 bit-reservoir state before target trimming; deterministic chirp correlation verifies both formats.
- Second-batch field follow-up: Jellyfin FLAC seeking works on macOS. MP3 exposed a duration-gate mismatch because its sequential probe lacked FFmpeg duration while Jellyfin supplied a positive duration; the MP3-specific fallback now uses that provider duration without relaxing codec/container or landing verification.
- Navidrome extension: authenticated raw streams for WAV PCM, M4A AAC/ALAC, Ogg Opus, MP3 and FLAC now reuse the verified decoder matrix. Qualification requires an exact Navidrome server identity from a fresh ping and validated byte-range responses; generic Subsonic/OpenSubsonic remains disabled.

## Dev Notes

### Implementation contract

These are preparation decisions for this story, not claims that seeking already exists or has passed validation.

- **Wire API:** Add `playback.seek` alongside `playback.control`. Reuse strict schema-v1 envelope fields `schemaVersion`, `instanceId`, `sessionId`, `commandId`, `expectedGenerationId`, `occurrenceId`; add absolute `positionMs`. Use nonnegative integer milliseconds bounded by JavaScript's safe-integer limit and the validated positive duration. Reject malformed/unknown fields through the existing RPC error convention. NaN/Infinity cannot be valid JSON: reject them in UI/native ingress too, and test raw malformed JSON at RPC.
- **Identity and deduplication:** Validate instance/session/current occurrence/generation inside the owner; share the existing bounded command-ID namespace and payload-reuse conflict checks. A replay must not issue audio work twice. Return authoritative metadata on conflicts; an old acknowledged response must not roll the UI back. Seeking changes generation/state sequence as needed, never queue revision, session ID, queue order or occurrence ID.
- **Admission versus success:** Admission returns a snapshot with pending seek information, not a fulfilled target. Define capability availability during initial loading, stopped/completed/error states, pending seek and restored-but-unresolved sessions; allow superseding an admitted seek for the same qualified representation without inheriting another track's capability. Project safe target/operation identity and outcome without provider URLs. Commit only the matching generation/epoch's verified decoded landing; for paused seek, the next decoded frame's verified cursor is commit evidence even though it is not yet audible. It is a media cursor, not listened duration. The request target is not an acceptable stand-in for this evidence.
- **Transport ordering:** A valid seek rotates generation, silences/retires old output and starts one bounded preparation for the same representation and selected endpoint. Preserve the latest owner-approved play/pause intent. A newer admitted seek wins; Pause must not be undone by completion; Stop resets to its established zero-position stopped state and cancels preparation. Replacement, output switching/loss and Quit invalidate stale activation. Do not launch unbounded detached decode tasks while old workers are retiring; retain/coalesce the latest request or use the existing explicit busy/conflict outcome.
- **Boundary behavior:** Absolute UI/RPC targets satisfy `0 <= positionMs <= durationMs`; do not silently clamp invalid absolute input. Exactly duration completes the current single-track transport once, retains occurrence/queue and the end cursor, silences output and checkpoints that terminal cursor. It does not decode an empty stream and accidentally Resume/loop. A later explicit Resume retains the existing completed-track restart-from-zero behavior. Also handle the restored end cursor explicitly: durable state currently stores transport/cursor, not the full transient Completed metadata; after resolving the same duration, explicit Resume from that terminal cursor must restart at zero, not decode to EOF repeatedly. No automatic Next is introduced. Treat any discrepancy between advertised duration and decoded media end as an explicit validation/failure condition rather than fabricating a successful landing.
- **Failure policy:** Before invalidating audio, capture the last confirmed actual cursor. If preparation then fails, retain that cursor and current occurrence, remain paused with a sanitized retryable seek failure, and require an explicit recovery action. Do not auto-restart the old stream or fall back to another output/representation. Distinguish rejection before admission, admitted preparation, successful commit, supersession and failure; persistent/native messages must correlate to the operation. Invalid input must leave a healthy currently playing stream untouched.
- **Accuracy budget:** Initial enablement requires decoded landing error no greater than **50 ms** against a known media-time fixture oracle, including timestamp origin, codec delay and resampling. This is a chosen acceptance budget, not measured evidence. Record first retained sample/time and error for each enabled pair; keep a combination disabled if it cannot meet the budget. Measure output latency separately rather than counting request duration as landing error. Document per-backend presentation uncertainty; the UI budget must include that uncertainty, the owner's 250 ms sampling interval, backend progress-report cadence (CPAL approximately 25 ms; Pulse approximately 5 ms), one 500 ms snapshot interval and one 100 ms local repaint interval. Prefer an authoritative sample-age/observation anchor so interpolation can account for sampling delay; do not mistake RPC receipt time for when audio position was measured. No claim of sample-exact hardware presentation follows from a callback counter.
- **Deadlines and bounds:** Retain the existing 60-second total preparation deadline and cancellation checks; do not reset the deadline for each byte or seek stage. The 8 MiB compressed total includes the 1 MiB network chunk allowance, 64 KiB scratch and retained reader window; PCM targets 500 ms and is capped at 1 MiB, with 100 ms startup/refill. Preserve Linux server-buffer bounds as well. Repeated seeks must respect aggregate ownership, including retiring workers, not just each new ring individually.
- **Persistence:** `positionMs` is the current confirmed cursor; `checkpointedPositionMs` remains the last successful durable cursor. A successful paused seek must be checkpointable even though no frames advance. Preserve transactional save, retry and shutdown blocker semantics; a failed save cannot be labeled durable. Pending seeks restore to the previous committed cursor; successful commits restore paused. Avoid a DB migration unless the chosen durable shape actually requires one; ephemeral capability and pending operation state belong in memory.

### Provider and decoder guardrails

**Initial delivery matrix:** Implement Jellyfin original WAV containing PCM s16le/s24le/s32le via the existing authenticated download route, validated HTTP byte ranges, FFmpeg post-open media-time seek and decoded-frame verification. Qualify this path on Windows, macOS and Linux before claiming story completion; shipping architecture evidence remains explicit. At least one end-to-end usable seek combination is mandatory: leaving every control disabled does not satisfy AC1. Initially keep Subsonic/OpenSubsonic raw streams and FLAC/MP3/AAC/ALAC/Opus/Vorbis/AIFF/WMA combinations disabled with explanations until their exact provider/representation mechanism passes the same tests. This is an implementation/qualification matrix, not a claim of present support. Retain ordinary playback for all existing formats. Expand the matrix within this story only with evidence, without changing representation or dependencies.

- `MediaProvider::resolve_playback` returns daemon-private `PlaybackDescription`/`PlaybackRepresentation`/`PlaybackRequest`. Keep credentials, URLs and headers there. Jellyfin currently selects original `/Items/{id}/Download` with `MediaBrowser` header and `range_supported: true`; Subsonic uses authenticated raw streaming and `range_supported: false`. These are byte-transport hints, not a media-time capability matrix. Both adapters currently populate `codec` from `song.suffix` and leave sample rate/bit depth unset: inspect actual FFmpeg codec/container/time base for qualification, not that suffix-derived field. Subsonic `server_version` comes from the ping API version, not necessarily the server implementation version; collect the latter separately in installed evidence.
- Introduce an explicit seek capability tied to the resolved original representation, validated codec/container and relevant server/runtime evidence. Unknown, zero-duration, changed-source, unvalidated transcode and unsupported format cases remain unavailable with reasons. Re-resolve credentials without silently re-ranking into a different representation. Clear capability when current source/generation changes; restored snapshots must not inherit stale capability claims.
- `HttpSource` already validates range status 206, exact `Content-Range` offset/total and identity encoding, sends `If-Range` using a strong ETag or Last-Modified, rejects redirects and checks cancellation. It does not currently compare returned validators; ensure changed-representation handling is tested rather than claiming that comparison already exists. Reuse it; do not hand URLs to FFmpeg or implement a second HTTP stack. A changed/truncated representation or a server ignoring Range is a typed failure, not a reason to guess byte-to-time offsets. Preserve length probes that do not destroy the original response.
- `decode_stream` currently implements restored start by decoding from the beginning and discarding frames. This is not random-access transport seeking. FLAC/MP3 intentionally use non-seekable probe IO because seeking during probing previously corrupted early audio. Do not flip that switch globally. Separate safe probing from validated post-open seek; if a sequential fallback is retained, explicitly validate its bounded completion/accuracy and disclose its mechanism rather than presenting all sources as seekable.
- Convert media timestamps through the stream time base with checked arithmetic, account for nonzero stream start and decoder delay, flush/recreate decoder and resampler state, discard only necessary pre-roll, and return the verified landed cursor. A successful demuxer seek or HTTP 206 is insufficient. Do not trim intentional recorded silence; full gapless padding work remains 15.9.
- The current duration comes from `song.duration_seconds * 1000` and can be `Some(0)`. Normalize unavailable duration and define the validated seek duration from reliable media evidence; reconcile coarse provider duration rather than using `Option::is_some()` as the gate.

### Existing files: current behavior, changes and preservation

Paths below are repository-relative implementation references; read them again if the baseline changes.

| File / area | Current state | Story change and preservation requirement |
| --- | --- | --- |
| `hifimule-daemon/src/playback/model.rs` | Schema-v1 snapshots, millisecond positions, duration, strict control envelopes; only Pause/Resume/Stop. | Add seek request/capability/pending/commit contracts without widening ordinary control payloads or exposing requests. Preserve canonical decimal revisions, bounded occurrence pages and safe metadata. |
| `hifimule-daemon/src/playback/session.rs` | Serialized owner, bounded command cache, generation/control-epoch fences, transactional checkpoints; progress is monotonic within a generation. | Admit and commit seeks, reset progress ingress for backward jumps, checkpoint actual cursor. Preserve mutation guards through owner processing, idempotence, shutdown admission and failure recovery. `PendingEvents` currently coalesces into three classes (Resolved, Active/Buffering, Completed/Failed), and only activity events get epoch checks in the owner loop: introduce a bounded commit slot/acknowledgment with explicit operation and epoch fencing so seek commits cannot be evicted or stale terminal events applied. |
| `hifimule-daemon/src/playback/commands.rs`, `commands_tests.rs`, `mod.rs` | Shared RPC/native Resume service detaches cancellable provider/audio effects; checks epoch before activation. | Add one shared seek effect and public exports/tests. Preserve responsiveness to later Pause/Stop and do not duplicate effect logic in RPC/native handlers. |
| `hifimule-daemon/src/playback/audio.rs` | Single active pipeline, worker retirement, selected endpoint, bounded decode and CPAL output; CPAL cursor counts submitted PCM while `PresentationClock` protects final drain. | Reposition the same representation, retire old output safely, reset clocks and commit verified cursor. Account for queued output latency; preserve callback real-time restrictions and concrete output binding. |
| `hifimule-daemon/src/playback/output.rs` | Whole-frame PCM rendering, refill/startup gating, presentation clocks/ledger and joined decoder ownership. | Reset seek position origin, consumer and presentation accounting together; preserve silence exclusion, tail drain and callback completion fences. |
| `hifimule-daemon/src/playback/audio/pulse_output.rs` | Linux `PresentationLedger` uses played-frame reports and excludes inserted silence/queued frames. | Reset seek ledger/cursor and flush/retire old server-buffered PCM. Preserve corking for paused state, no automatic movement and bounded Pulse buffer. |
| `hifimule-daemon/src/playback/decoder.rs` | FFmpeg custom IO and sequential start-frame discard; format-specific probing protection. | Add validated time-seek/flush/pre-roll handling with actual landing evidence. Preserve all existing beginning/resume/large-metadata fixtures and recorded silence. |
| `hifimule-daemon/src/playback/streaming.rs`, `http_source.rs` | Bounded reader/window, range validation, EOF/size probes, PCM ring and cancellation. | Extend only where needed for seek/reset and regression coverage. Preserve aggregate bounds, redaction, no redirects and correct cached-size probes. |
| `hifimule-daemon/src/providers/mod.rs` and relevant provider adapters | Original representation selection and daemon-private authenticated requests; no verified time-seek capability. | Add representation-specific capability evidence, never a blanket byte-range flag. Preserve provider routing, original-first ranking and ordinary nonseekable playback. |
| `hifimule-daemon/src/rpc.rs` | Authenticated RPC and lifecycle mutation admission. | Add `playback.seek` to both dispatch and `is_mutating_method`; transfer `mutation_guard.take()` into the shared service/owner. Define seek rejection versus asynchronous retryable failure mapping (`playback_error` currently treats only PLAYBACK_BUSY as retryable). Preserve shutdown allowlist/source privacy and test the separate output-effect dispatcher racing seek activation. |
| `hifimule-daemon/src/playback/native.rs` | Bounded native ingress, authoritative snapshots and correlated failure receipts; seek disabled. | Add guarded seek intents/capability and committed discontinuity projection. Preserve latest-value metadata publication, owner-only transport authority and registration lifecycle. |
| `third_party/souvlaki/src/lib.rs`, `publication.rs`, platform adapters | Patched native backend; bounded publication, command masks and cleanup. Existing seek hooks are incomplete. | Correct platform mappings below; retain patched version and teardown priority. Update `HIFIMULE_PATCH.md` and tests with any vendor changes. |
| `hifimule-ui/src/rpc.ts` | Typed schema-v1 playback helpers and identity-bearing mutation calls. | Add typed seek/capability fields and helper; preserve conflict refresh and no command replay on reconnect. |
| `hifimule-ui/src/components/PlaybackControls.ts` | 500 ms snapshot polling, state-sequence filtering, mounted transport/output nodes; no position display or interpolation. | Add timeline, pending scrub state and bounded local clock. Preserve focused nodes, output discovery at its existing cadence, transport during output selection, sanitized errors and destroy/pagehide cleanup. |
| `hifimule-ui/src/styles.css`, `hifimule-i18n/catalog.json` | Compact output dropdown plus existing responsive styles and four-locale messages. | Add timeline/focus styling and localized statuses; preserve latest dropdown width/hoist/alignment and readable controls at narrow widths. |
| `scripts/tests/playback-ui.test.mjs` | Runs real component with fake RPC/DOM/clock; focus/output/lifecycle tests. | Extend fake clock/slider events and seek RPC seam for behavioral tests. Keep existing output-selection and timer cleanup assertions. |
| `docs/api-contracts-hifimule-daemon.md`, `docs/playback-installed-test-checklist.md` | Existing schema-v1 and installed playback/output/native evidence contracts. | Document actual seek protocol, matrix, tolerances and evidence procedures without rewriting historical observations as seek passes. |
| `scripts/playback-installed-evidence.py`, `scripts/tests/test_playback_installed_evidence.py` | Sanitized collector and strict action-specific evidence validation. | Add seek evidence requiring request/landed position, identity, final state, accuracy and capability; add `seekEvidenceVersion: 1` with a Story 15.7 validation mode that requires it. Historical records may remain readable but cannot pass seek acceptance. Preserve all provider/representation/backend combinations: current matrix indexing by OS target overwrites duplicate provider records, so extend the matrix key or require complete nested combination records. |

`persistence.rs`, output-selection/device modules and `main.rs` are reuse/integration references, not invitations for unrelated rewrites. Extend only if the finalized seek integration needs them. Keep tests beside existing playback modules; deterministic audio fixtures and their generation provenance belong under `hifimule-daemon/tests/fixtures/`.

### Native protocol requirements

- Resolve a relative native target from the current authoritative cursor inside the serialized owner; retain signed offsets until checked conversion. Strict UI occurrence/generation validation and native owner-time targeting have different ingress semantics but share capability, effects and commit rules.
- **Linux/default D-Bus backend:** `platform/mpris/dbus/controls.rs` currently publishes a constant `mpris:trackid` of `/`; `interfaces.rs` ignores SetPosition's track ID and emits Seeked before actual completion with an incorrect string signature. Supply an occurrence-based valid object path through native metadata; reject stale SetPosition at owner admission as well as adapter checks. Use signed microseconds and checked conversions; absolute out-of-range SetPosition is ignored per MPRIS, relative negative overshoot clamps to zero. Relative beyond-end maps to terminal completion while this story has no next-track transport. Document that single-track boundary and keep CanGoNext false. Emit `Seeked(i64)` only for a confirmed discontinuity, never an accepted/failed request or routine position sample; Position does not emit PropertiesChanged. Preserve bounded publication and ensure coalescing cannot lose the committed discontinuity for the final generation.
- **macOS:** Use the public position-change event accessor, finite/nonnegative seconds validation and checked conversion before `Duration` construction. The current private `_positionTime` access is not a safe implementation contract. Enable only implemented commands; retain balanced Objective-C ownership and handler cleanup.
- **Windows:** Validate signed TimeSpan values before conversion to milliseconds; do not cast a negative value into unsigned duration. Update timeline from confirmed state. If FastForward/Rewind directional events are enabled, define a fixed relative step (10 seconds for this story) and bounds; omit unsupported commands instead of advertising dead controls.
- Preserve the confirmed terminal cursor in native projection: current Completed maps to native Stopped, whose Windows/Linux paths reset position to zero and whose macOS path can retain stale elapsed time. Extend the stopped-position/discontinuity representation or define a consistent terminal projection; test live exact-end and subsequent explicit Resume rather than losing the authoritative end position.
- Native callbacks perform no provider, database or decoder work. Retain the existing bounded ingress and persistent request-correlated outcomes; registration failures must remain nonfatal to ordinary UI playback.

### UI and accessibility behavior

- Extend the existing compact controls. Retain the latest `sl-dropdown` output selector (`bottom-end`, hoisted, bounded panel width) and speaker icon label. Avoid introducing the full future Playback destination or changing browse/basket selection. Reuse current implemented tokens (including Signal Cyan and Inter); do not restore obsolete purple/Outfit examples from historical UX planning.
- Show elapsed time and known duration, with unknown duration clearly represented. Use a labeled keyboard-operable slider, visible focus, accessible value text and an accessible explanation adjacent to disabled seeking. Arrow keys, Home/End and pointer interaction must work without stealing focus from browsing.
- Dragging changes a local **preview target**; committed elapsed position remains distinct. Submit on explicit change/release, coalesce repeated intent and allow Stop/Pause while preparation runs. Do not continuously flood the owner during every pointer movement or overwrite the dragged value from a poll. Clear stale preview when identity changes.
- Use a monotonic local clock and authoritative position anchors. Interpolate only `playing`/`active`, never loading, buffering, paused, completed, errored, disconnected or pending-seek states. Re-anchor on accepted newer snapshots and generation/occurrence changes; never reject a valid backward seek because its position decreased. Bound interpolation to known duration and a 750 ms freshness horizon, then freeze until a fresh authoritative update. Equal-sequence responses must not refresh an old position anchor or extend its freshness horizon merely because another RPC arrived; that causes repeated rewind or manufactured progress. Use authoritative observation age/timestamp or a sequence that advances with fresh audio samples. Never accept lower-sequence responses within an unchanged owner/session.
- Keep network polling at 500 ms; a local repaint of at most 100 ms is sufficient. Cancel both on destroy/pagehide/detach. Do not announce every progress tick through ARIA-live; announce meaningful pending, unavailable, failed and completed-seek changes. Preserve locale-switch behavior, focus and responsive layouts below 600 px, 600–1000 px and above 1000 px.

### Architecture and dependency compliance

The daemon owns transport, output, persistence and native registration. Tauri remains the authenticated UI bridge. The architecture's exploratory CPAL 0.16 baseline and project-context's old greenfield status are historical; current source and approved playback amendments control implementation. Keep the provider abstraction and physical managed-sync integrity intact.

Use repository pins: Rust edition 2024 / minimum 1.93.0; CPAL `=0.18.2`; `ffmpeg-next` and `ffmpeg-sys-next` `=9.0.0`; FFmpeg runtime `9.0.1` from `audio-runtime.json`; `crossbeam-queue =0.3.12`; patched Souvlaki `=0.8.3`; Linux `libpulse-binding =2.30.1`. Existing UI uses TypeScript, Vite, Tauri 2 and Shoelace; add no new frontend framework or player dependency. Preserve controlled native packaging, FFmpeg network-disabled custom IO and LGPL distribution records. No allocation, blocking IO, database access or sync locks in the output callback.

### Previous-story and git intelligence

- Story 15.6's shared command service is required for actual audio effects: owner admission alone is not implementation. Generation **and control epoch** fencing prevent late Resume/seek activation after Pause/Stop.
- Its review found unbounded Linux publication, premature/uncorrelated failure feedback, macOS string leaks, fatal Windows registration handling, and tests/evidence that accepted no-op outcomes. Preserve the fixes and test production ingress/effect/lifecycle seams, not just projections.
- `91acf7e` explicitly marked 15.6 done; respect that state. Historical paragraphs still record outstanding installed observations; neither that status commit nor prior audible-format smoke certifies seeking.
- `21e1800` changed the current output selector layout and UI tests; preserve it. `0bcf36f` concerns MTP and is outside scope. `53927fd` isolates first-launch default-output bootstrap in tests and checks relative revisions; do not assume fixed initial output revision. `f598663` tests real owner mutation after caller drop with RAII worker cleanup; preserve guard lifetime and ensure locks drop before joining owner threads.

### Testing requirements

Add a sanitized seek evidence projection distinct from `safe_output_snapshot()` (which lacks occurrence/generation/sequence). Include operation identity, instance/session/generation/occurrence, queue revision/state sequence, duration, requested target, prior committed cursor, independently measured decoded landing and absolute error, transport intent/result, and pending/succeeded/failed/superseded outcome. No titles, authenticated URLs or secrets. Reject missing/empty seek evidence, unsupported evidence versions, no-op claimed forward/backward successes, pending/failed targets presented as committed, identity/queue changes, invalid booleans masquerading as numbers, negative/nonfinite values and conflicting duplicate matrix records. Independently verify fixture landing; equality between request and reported result is insufficient. Keep the existing ordinary-playback `FIXTURES` requirements separate: disabled seek rows need a reason and retained playback success, not an invented seek pass.

| Layer | Required evidence |
| --- | --- |
| Contract/owner | Zero, duration, duration+1, negative, fractional, unsafe integer, unknown/zero duration, malformed JSON; stale instance/session/occurrence/generation; duplicate command and changed-payload replay; queue revision unchanged; backward commit accepted only in the new generation. |
| Decoder/HTTP | Known time markers or a non-periodic deterministic waveform with independent expected sample offsets; forward/backward/near-start/near-end and repeated targets; variable-rate codecs, resampling and nonzero timestamps; 200 instead of 206, wrong range/total, changed representation, truncated body, timeout and cancellation. A uniform sine/count-only test cannot prove where seeking landed. Include offsets beyond the retained 8 MiB window and existing large-metadata regressions. |
| Audio/races | Paused seek emits no samples; only new-position samples after committed switch; old CPAL/Pulse buffered audio cannot leak; slow seek then seek/Pause/Stop/replacement/output switch/output loss/Quit; bounded request/worker count, compressed/PCM high-water and cleanup; actual final state and position asserted. |
| Persistence | Commit then restart paused, crash/checkpoint during pending request, failed/superseded request, save failure/retry, exact-end restart and explicit Resume behavior. Verify persisted content, not only in-memory snapshot. |
| Native | Real shared-service parity, checked platform time conversion, disabled capabilities, stale MPRIS track ID, signal payload/order and no Seeked on rejection/failure, coalesced publication and shutdown priority. Compile/test changed target-specific code; a macOS build alone cannot verify Linux or Windows adapters. |
| UI | Fake monotonic clock: advance only active, freeze pause/buffer/stale/disconnect, backward re-anchor, reconnect/old-response ordering, pending preview distinct from actual, repeated scrub coalescing, keyboard boundaries, focus retained across polls, all timers cleaned up, four-locale key parity. |
| Installed | Windows x64, Linux x64, macOS x64 and macOS ARM64 rows with package/revision, backend, provider/server version, codec/container/representation, requested and landed position, error tolerance, transport before/after, generation/occurrence, buffer peaks and audible outcome. Separately label native API and physical controls; unverified hardware stays unverified. |

Run commands with the project-required `rtk` prefix. Use the established native-runtime build wrapper rather than bypassing runtime discovery:

```sh
rtk proxy node scripts/build-daemon.mjs test -p hifimule-daemon playback -- --test-threads=1
rtk proxy node scripts/build-daemon.mjs test -p hifimule-daemon providers -- --test-threads=1
rtk proxy node --test scripts/tests/playback-ui.test.mjs
rtk proxy python3 -m unittest discover -s scripts/tests -p 'test_playback*evidence.py'
rtk cargo test -p hifimule-i18n
rtk npm --prefix hifimule-ui run build
rtk cargo fmt --all -- --check
rtk git diff --check
```

Add focused RPC/native-vendor tests and relevant target builds according to the files changed. Run lifecycle regressions if admission/shutdown integration changes. These are implementation checks to execute during dev-story; story preparation has not run audio or installed acceptance tests.

### Current technical research

Checked 2026-09-18 using primary documentation. Keep the pinned runtime; this story does not require an upgrade.

- FFmpeg's current download page lists 9.0.1. Seek timestamps use the selected stream time base (or AV_TIME_BASE when no stream is selected); demuxer seek does not replace decoder-state reset and verified pre-roll. Use the pinned bindings/headers when implementing, since trunk documentation can describe newer APIs. [FFmpeg releases](https://ffmpeg.org/download.html), [demuxing/seek API](https://ffmpeg.org/doxygen/trunk/group__lavf__decoding.html), [decoder flush API](https://ffmpeg.org/doxygen/trunk/group__lavc__misc.html).
- MPRIS specifies signed microseconds, current-track identity checking, distinct absolute/relative bounds and `Seeked` on discontinuity rather than continuous Position property changes. Implement its native boundary adapter without weakening strict RPC validation. [MPRIS Player specification](https://specifications.freedesktop.org/mpris/latest/Player_Interface.html).
- OpenSubsonic documents `format=raw` separately from time-offset support; default `timeOffset` applicability is video unless the relevant extension is available. Do not invent universal audio time-offset support for all Subsonic/Navidrome servers. [OpenSubsonic stream endpoint](https://opensubsonic.netlify.app/docs/endpoints/stream/).

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 15, Story 15.7; stories 15.1–15.14, 15.17 and 16.1–16.14 and coverage/dependency map]
- [Source: `_bmad-output/planning-artifacts/prd.md` — Desktop Playback amendment, FR58, FR75 and playback NFRs]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Desktop Playback ownership, generation, buffers, providers, position and native control contracts]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md` — general accessible controls, responsive layout and Shoelace foundation; playback-specific UX comes from the approved epic/architecture]
- [Source: `_bmad-output/planning-artifacts/project-context.md` — provider abstraction and managed-sync principles; greenfield status is stale]
- [Source: `_bmad-output/implementation-artifacts/15-6-control-playback-through-the-operating-system-with-the-window-closed.md` — review patches, shared service, native lifecycle and evidence caveats]
- [Source: `docs/api-contracts-hifimule-daemon.md` — playback schema-v1, reconnect and explicit output contracts]
- [Source: `hifimule-daemon/src/playback/` and `hifimule-daemon/src/providers/` — current implementation summarized above]
- [Source: `Cargo.toml`, `Cargo.lock`, `hifimule-daemon/audio-runtime.json`, `third_party/souvlaki/HIFIMULE_PATCH.md` — dependency/runtime baseline]
- [Source: `docs/playback-installed-test-checklist.md`, `hifimule-daemon/tests/fixtures/generated-audio.source.md`, `scripts/tests/playback-ui.test.mjs` — current evidence/fixture/test conventions]

## Dev Agent Record

### Agent Model Used

Story preparation: Codex. Implementation: GPT-6 (Codex).

### Implementation Plan

- Extend schema-v1 playback state and the serialized owner with strict seek admission, shared deduplication, generation/epoch fencing, explicit pending/outcome state, exact-end behavior and durable actual-position commits.
- Qualify selected Jellyfin originals and Navidrome raw originals after provider identity and FFmpeg inspection: PCM WAV, AAC/ALAC-in-M4A, Opus-in-Ogg, MP3 and FLAC. Perform bounded demuxer seek, decoder flush, pre-roll trim and measured landing publication through the existing CPAL/Pulse pipeline.
- Route UI and native requests through `PlaybackCommandService`, add accessible timeline behavior and native discontinuity publication, then validate contracts, races, persistence, fixtures and installed-evidence schema.

### Debug Log References

- `rtk proxy node scripts/build-daemon.mjs test -p hifimule-daemon -- --test-threads=1` — 854 passed, 0 failed, 6 explicit hardware/diagnostic ignores.
- `rtk proxy node --test scripts/tests/playback-ui.test.mjs` — 17 passed.
- `rtk proxy python3 -m unittest discover -s scripts/tests -p 'test_playback*evidence.py'` — 20 passed.
- `rtk cargo test -p hifimule-i18n` — 7 passed.
- `rtk cargo test --manifest-path third_party/souvlaki/Cargo.toml --lib` — 2 passed on macOS.
- `rtk npm --prefix hifimule-ui run build` — TypeScript and Vite production build passed; existing bundle-size/dynamic-import warnings remain.
- `rtk proxy node scripts/build-daemon.mjs clippy -p hifimule-daemon` — passed with existing repository warnings; two story-local suggestions were corrected afterward.
- Targeted `rustfmt --check` for every changed Rust file and `rtk git diff --check` passed. The repository-wide `cargo fmt --all -- --check` remains blocked by pre-existing formatting drift in unrelated MTP files.
- Windows cross-check could not run because the active Rust toolchain lacks its target core; Linux and installed Windows/macOS hardware evidence remain explicitly unverified in the installed checklist.
- First compressed seek batch: playback suite **180 passed, 6 ignored**; evidence validator **22 passed**; daemon Clippy passed with existing unrelated warnings.
- Second compressed seek batch: playback suite **180 passed, 6 ignored**; Jellyfin provider suite **43 passed**; evidence validator **22 passed**; daemon Clippy passed with existing unrelated warnings.
- Navidrome extension: playback suite **181 passed, 6 ignored**; Subsonic/Navidrome provider suite **57 passed**; evidence validator **23 passed**; daemon Clippy passed with existing unrelated warnings.

### Completion Notes List

- Added strict `playback.seek` admission with shared command identity, pending/committed separation, supersession fencing, exact-end behavior, pause intent preservation and checkpoint retry semantics.
- Implemented runtime-qualified Jellyfin original WAV seeking for FFmpeg-verified PCM s16le/s24le/s32le. Decoder landing is independently measured and bounded to 50 ms; unsupported WAV codecs retain ordinary playback.
- Extended runtime qualification to Jellyfin original AAC/ALAC-in-M4A and Opus-in-Ogg. Deterministic compressed fixtures use chirp correlation for codec-delay-safe landing evidence; mismatched codecs retain ordinary playback without seek.
- Extended runtime qualification to Jellyfin original MP3 and FLAC. Compressed media seek uses 50 ms pre-roll before exact target trimming so MP3 bit-reservoir state is available.
- Extended the verified format matrix to Navidrome raw original streams. A fresh authenticated `type=navidrome` ping and validated range transport prevent the capability from applying to generic Subsonic servers.
- Added authenticated range identity checks, including ETag/Last-Modified replacement rejection, bounded pre-roll trimming and actual-position progress bases for CPAL and Pulse.
- Added native relative/absolute seek parity, occurrence-based MPRIS track IDs, success-driven typed `Seeked`, checked macOS/Windows conversions and terminal-position projection.
- Added the accessible UI timeline, explicit scrub commit/coalescing, authoritative 750 ms interpolation, four-locale strings, deterministic fixtures, persistence/race tests and strict `seekEvidenceVersion: 1` validation.
- Installed evidence requirements and all unavailable platform/hardware rows remain explicitly unverified; no local source test was promoted to installed evidence.

### File List

- `_bmad-output/implementation-artifacts/15-7-seek-within-a-track-and-see-the-actual-playback-position.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `docs/api-contracts-hifimule-daemon.md`
- `docs/playback-installed-test-checklist.md`
- `hifimule-daemon/src/playback/audio.rs`
- `hifimule-daemon/src/playback/audio/pulse_output.rs`
- `hifimule-daemon/src/playback/commands.rs`
- `hifimule-daemon/src/playback/commands_tests.rs`
- `hifimule-daemon/src/playback/decoder.rs`
- `hifimule-daemon/src/playback/http_source.rs`
- `hifimule-daemon/src/playback/model.rs`
- `hifimule-daemon/src/playback/native.rs`
- `hifimule-daemon/src/playback/session.rs`
- `hifimule-daemon/src/playback/session/output_selection.rs`
- `hifimule-daemon/src/providers/jellyfin.rs`
- `hifimule-daemon/src/providers/mod.rs`
- `hifimule-daemon/src/providers/subsonic.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-daemon/tests/fixtures/generated-audio.source.md`
- `hifimule-daemon/tests/fixtures/generated-seek-aac.m4a`
- `hifimule-daemon/tests/fixtures/generated-seek-alac.m4a`
- `hifimule-daemon/tests/fixtures/generated-seek-flac.flac`
- `hifimule-daemon/tests/fixtures/generated-seek-mp3.mp3`
- `hifimule-daemon/tests/fixtures/generated-seek-opus.oga`
- `hifimule-daemon/tests/fixtures/generated-seek-pcm-f32.wav`
- `hifimule-daemon/tests/fixtures/generated-seek-pcm16.wav`
- `hifimule-daemon/tests/fixtures/generated-seek-pcm24.wav`
- `hifimule-daemon/tests/fixtures/generated-seek-pcm32.wav`
- `hifimule-i18n/catalog.json`
- `hifimule-ui/src/components/PlaybackControls.ts`
- `hifimule-ui/src/rpc.ts`
- `hifimule-ui/src/styles.css`
- `scripts/playback-installed-evidence.py`
- `scripts/tests/playback-ui.test.mjs`
- `scripts/tests/test_playback_installed_evidence.py`
- `third_party/souvlaki/HIFIMULE_PATCH.md`
- `third_party/souvlaki/examples/window.rs`
- `third_party/souvlaki/src/lib.rs`
- `third_party/souvlaki/src/platform/macos/mod.rs`
- `third_party/souvlaki/src/platform/mpris/dbus/controls.rs`
- `third_party/souvlaki/src/platform/mpris/dbus/interfaces.rs`
- `third_party/souvlaki/src/platform/mpris/zbus.rs`
- `third_party/souvlaki/src/platform/windows/mod.rs`
- `third_party/souvlaki/src/publication.rs`

### Change Log

- 2026-09-18: Added Navidrome raw-original seeking for the verified WAV, M4A AAC/ALAC, Ogg Opus, MP3 and FLAC matrix, gated by fresh server identity and validated byte ranges.

- 2026-09-18: Recorded user-confirmed Jellyfin FLAC seeking on macOS and fixed MP3 qualification when sequential probing has no FFmpeg duration but Jellyfin supplies a positive duration.

- 2026-09-18: Added the second compressed seek batch for Jellyfin original MP3 and FLAC, including bounded compressed pre-roll and deterministic correlated landing tests. Navidrome remains disabled.

- 2026-09-18: Added the first compressed seek batch for Jellyfin original AAC/ALAC-in-M4A and Opus-in-Ogg with post-open verification and deterministic correlated landing tests. MP3, FLAC and Navidrome remain disabled.

- 2026-09-18: Recorded user-confirmed Jellyfin WAV seeking on Linux; macOS and Linux now have qualitative passes, with formal numeric installed evidence still pending.

- 2026-09-18: Recorded user-confirmed macOS seeking for all three first-batch Jellyfin compressed combinations: AAC-M4A, ALAC-M4A and Opus.

- 2026-09-18: Applied ten review fixes and restored the narrowly implemented Jellyfin original PCM-WAV runtime path after per-track FFmpeg verification. Reopened story to in-progress for installed seek qualification; runtime availability is not recorded as platform acceptance evidence.

- 2026-09-18: Implemented authoritative current-track seeking, actual-position UI/native publication, runtime PCM-WAV qualification, durable commits, race/error handling, fixtures and installed-evidence validation. Marked ready for review.
