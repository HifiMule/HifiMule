---
baseline_commit: bd215ee1e65aa2a300fdbe246dcb30f664733984
---

# Story 15.11: Preview a full track without losing the main listening session

Status: done

## Story

As a HifiMule user,
I want to audition a track while keeping my current listening session,
so that I can assess music for a playlist or basket and then return to where I was listening.

**Requirements:** FR62–63; applicable FR58, FR60 and FR64; P-NFR2, P-NFR4–6; session portions of P-AR3–4 and P-AR10; P-UX-DR3, P-UX-DR11 and P-UX-DR13–14.

**Dependencies:** Stories 15.1–15.10. Prepared 2026-09-18 against the baseline above. Sprint tracking marks 15.10 done, while its dedicated story remains in-progress and records missing installed Windows/Linux/macOS x64, native-worker recreation and physical-output evidence. Treat that discrepancy as an inherited evidence limitation; do not rewrite predecessor status or claim unrun acceptance.

**Scope:** One preserved main session, one full-track audition, explicit Return, local audition outcomes, durable recovery and existing-browser controls. No server listening reports (15.21), playlist/basket export (15.23–25), Radio, new loudness policy, editable queue UI (15.13), Playback destination (15.12), floating bar (15.14), quality adaptation or dependency upgrade. Existing playlist/basket actions remain available and retain their meaning.

## Acceptance Criteria

1. **Preserve before audition.** Given a playing or paused main session, when Preview is explicitly invoked for a library track, preserve its complete queue/order/occurrence identities, actual consumed position, playing/paused intent and frozen album gain/representation context before audition audio starts. Preview plays the full selected track from the beginning, uses the captured source-server identity, and is visibly distinct from Play and curation actions.
2. **Replace only the audition.** Given an active or loading audition, when another Preview is accepted, replace only that audition. Preserve the original main snapshot, never nest saved sessions, and keep audio/resource counts bounded.
3. **Natural return.** Given a preserved main session, when audition audio finishes naturally, restore the same main queue, occurrence and position with its saved intent, subject to output safety. Do not insert the audition into the queue, advance the album or report an artificial main completion.
4. **Explicit Return.** Given an audition with a main session, an accessible, clearly labeled Return to session action ends the audition and restores the saved intent/position. Return differs from Stop.
5. **Stop safety.** UI or native Stop ends an audition and restores the main session paused at its saved position. With no main, it leaves idle. It must not execute ordinary main Stop's zero-position behavior or unexpectedly start music.
6. **No-main completion.** A standalone audition finishes to idle without Radio, repeat or a misleading Return action.
7. **Recoverable failures.** Audition loading failure or main-return failure retains recoverable main state and exposes a specific failed operation. Do not silently restart at zero, substitute an output, discard the queue or loop between preview and return. Output loss inhibits automatic resume; reconnect alone never starts audio.
8. **Race safety.** Preview replacement, Stop, Return, ordinary Play, seek and pending provider/decode/handoff work are serialized and generation/epoch fenced. Only the current transport may emit audio or publish state. Successful ordinary Play replaces the old listening context and prevents later audition completion from resurrecting it; failed admission preserves existing durable state.
9. **UI-independent lifecycle.** Closing/reopening the UI shows the same daemon audition and preserved-main state; native controls target the audition. Quit and crash restoration follow the contract below: dismiss the pending audition, restore main paused or idle if absent, and retain distinct interrupted-audition evidence.
10. **Separate local outcomes.** Natural completion, Stop, Return, replacement, ordinary-Play supersession, technical failure and restart interruption remain distinguishable from main-queue outcomes. Preserve enough bounded local consumption evidence to distinguish a fully heard audition from seeking to its end; submit nothing to the server in this story.
11. **Cross-platform and browser evidence.** On Windows, macOS and Linux, verify playing/paused/no-main previews, replacement, completion, Stop, Return, failed return, output loss and restart with one active audio owner. Preserve browser selection behavior, visible focus, accessible names and audition status. Record each actual architecture/runtime tested; unrun rows remain open.

## Tasks / Subtasks

- [x] Add the typed audition contract and owner transitions (AC: 1–10).
  - [x] Implement active-transport projection independently of the durable main queue; settle the wire additions below with strict request validation and serialization tests.
  - [x] Implement Preview admission, replacement, natural return, explicit Return, Stop override, failure recovery and ordinary-session replacement; preserve command deduplication and shutdown admission.
  - [x] Gate successor preparation and Next while auditioning; test direct RPC/native bypasses as well as disabled controls.
- [x] Persist the main checkpoint and bounded audition state atomically (AC: 1–3, 7–10).
  - [x] Migrate playback persistence v3→v4 transactionally; retain v1/v2 migrations and album membership/representation validation.
  - [x] Store one active audition separately from main occurrences and append terminal audition outcomes exactly once locally; page historical records.
  - [x] Apply restart dismissal idempotently in normal restoration and storage Retry; failed/corrupt restoration must preserve evidence.
- [x] Integrate existing audio/command/native paths (AC: 1–9, 11).
  - [x] Capture actual quiesced main position, invalidate prepared successor/presentation work, retire old audio and use existing bounded provider/start-at-position machinery.
  - [x] Reopen main with the original frozen scalar and admitted suffix; preserve decoder samples, padding, replay and output pinning on CPAL and Pulse.
  - [x] Dispatch return effects from natural completion as well as explicit RPC/native commands; do not strand an owner state change without starting the admitted effect.
  - [x] Publish active audition metadata/capabilities and enforce disabled Next in the owner.
- [x] Add browser Preview and transport Return/status controls (AC: 1–2, 4–9, 11).
  - [x] Wire distinct per-track Preview actions in cards, ordinary list rows and dedicated Tracks rows, independent of device or playlist-write capability.
  - [x] Extend shared RPC/snapshot state and existing PlaybackControls; preserve polling/reconnect ordering, focus, selection and all existing curation controls.
  - [x] Add matching EN/FR/ES/DE labels and actionable preview/return errors.
- [x] Verify behavior and document evidence (AC: 1–11).
  - [x] Add owner/persistence/RPC/native regressions, production decoder return tests, failure/race injection and local outcome tests described below.
  - [x] Build daemon and UI; run controlled-runtime suites and installed-platform checks; record missing prerequisites honestly.
  - [x] Update API contracts and installed playback checklist with preview semantics, v4 recovery, evidence and limitations.

### Review Findings

- [x] [Review][Patch] Route seek validation through the active Preview occurrence instead of the suspended main occurrence [hifimule-daemon/src/playback/session.rs:2802]
- [x] [Review][Patch] Build native transport commands from the active Preview identity, position and state [hifimule-daemon/src/playback/session.rs:2913]
- [x] [Review][Patch] Retain the pending-return cursor marker after return preparation fails so Stop cannot reset it to zero [hifimule-daemon/src/playback/session.rs:973]
- [x] [Review][Patch] Surface and preserve persistence failures from natural completion and failed-return checkpointing [hifimule-daemon/src/playback/session.rs:988]
- [x] [Review][Patch] Commit Preview admission or replacement before retiring the currently valid transport state [hifimule-daemon/src/playback/session.rs:2069]
- [x] [Review][Patch] Quiesce and capture audition audio before committing Return or Stop terminal evidence [hifimule-daemon/src/playback/session.rs:2681]
- [x] [Review][Patch] Persist technical failure outcomes even when the next action is Return, Stop, supersession or restart [hifimule-daemon/src/playback/session.rs:946]
- [x] [Review][Patch] Derive fully-heard evidence from continuous presentation rather than progress or EOF endpoints [hifimule-daemon/src/playback/session.rs:1025]
- [x] [Review][Patch] Mark Preview failure and output-loss state dirty and checkpoint its inhibition and failure evidence [hifimule-daemon/src/playback/session.rs:946]
- [x] [Review][Patch] Mark Preview Pause state dirty so its captured position and paused state survive restart [hifimule-daemon/src/playback/session.rs:2695]
- [x] [Review][Patch] Reject Retry and error-state Resume when the Preview failure is non-retryable [hifimule-daemon/src/playback/session.rs:2713]
- [x] [Review][Patch] Apply the shared ten-minute receipt expiry and cross-command ID collision policy to Preview commands [hifimule-daemon/src/playback/session.rs:1207]
- [x] [Review][Patch] Reject an orphan playback_audition row instead of initializing a clean session over it [hifimule-daemon/src/playback/persistence.rs:128]
- [x] [Review][Patch] Pause the active Preview projection when output-preference persistence fails [hifimule-daemon/src/playback/session/output_selection.rs:438]
- [x] [Review][Patch] Validate audition outcome cursors before converting u64 values to SQLite i64 [hifimule-daemon/src/playback/persistence.rs:620]
- [x] [Review][Patch] Validate audition duration before converting it to SQLite i64 and exposing it on the safe-integer wire contract [hifimule-daemon/src/playback/persistence.rs:748]
- [x] [Review][Patch] Reject invalid persisted audition and saved-main transport-state combinations during restoration [hifimule-daemon/src/playback/persistence.rs:726]
- [x] [Review][Patch] Replace source-regex Preview surface checks with behavioral DOM tests for invocation, selection and focus [scripts/tests/playback-ui.test.mjs:177]

## Dev Notes

### State and transition contract

The following are deliberate implementation decisions closing Story 15.11's gate.

**Keep the main queue canonical.** Leave its SQLite occurrence rows, IDs, ordering, outcomes and frozen album context intact during auditions. Add one bounded audition overlay with a fresh audition UUID/occurrence identity, portable TrackSource, position and active state. Keep the original saved main cursor/intent once, not per replacement. A main session exists when the canonical queue has a current occurrence; stopped/completed main states are preserved as non-playing. Buffering preserves admitted play intent unless already inhibited by Pause/output loss. A no-main audition does not manufacture a main queue entry.

The durable main checkpoint stores its saved position and paused transport while suspended; the overlay stores the prior intent for live return. Playback progress, seek and checkpoints during an audition update the audition only. The saved queue must never be reconstructed from the first 100-entry snapshot page. Do not use `SessionOperation::PlayTrack` to implement Preview: it replaces queue structure.

| Event | Main preservation / active result |
|---|---|
| First Preview | Quiesce main output, capture committed presented position, atomically checkpoint main and admit audition; start audition at 0. |
| Further Preview | Retire previous audition, record replacement once, retain original main checkpoint/intent, start replacement at 0. |
| Natural clean presented EOF | Record audition outcome; restore saved main and saved intent once, or idle without main. |
| Return to session | Record early return unless already terminal; restore saved main intent once. Unavailable when no main. |
| Stop while auditioning/return pending | Invalidate audition/return preparation, restore main paused at saved position, or idle. |
| Pause / Resume / Retry / seek | Target active audition; do not alter saved main position or original intent. Retry restarts failed audition preparation only on request. |
| Seek exactly to audition end | Keep a completed, non-playing audition available for Return/Stop (also without main); do not synthesize natural completion or automatic return. Resume follows existing explicit replay behavior. |
| Native or direct Next | Unavailable/no state effect; structured capability error for direct RPC. Never skip the main track or interpret Next as Return. |
| Preview failure | Stay in a recoverable paused audition error with main intact; offer Retry, Return when main exists, and Stop. No automatic fallback loop. |
| Return preparation failure | Main is selected at saved position, paused with `PREVIEW_RETURN_FAILED`; audition cannot reappear. Retry/Resume retries this main source. Preserve return-operation context long enough to attribute failure. |
| Ordinary Play / successful PlayAlbum / ReplaceQueue / Clear | Atomically end overlay with superseded disposition and apply established main replacement/clear behavior. Late audition work is invalid. |
| AppendQueue / SelectCurrent while auditioning | Reject with `PREVIEW_ACTIVE` and authoritative state; do not silently mutate preserved main. Future queue editing can extend this explicitly. |
| UI close/reopen | No transport mutation; return authoritative active and preserved-main summary. |
| Quit/crash restart | Persist main safely; dismiss active audition once as interrupted, restore main paused or no-main idle. Never resume either automatically. |

**Output safety overrides saved play intent.** Record a resume-inhibited condition when output loss occurs during audition or return. Natural completion and Return restore paused in that case, even if the saved main was playing. Reconnection or choosing a replacement while paused does not lift it implicitly. Explicit Resume after return may do so through existing output selection rules. Keep the user's latest deliberate output selection; do not restore an obsolete output preference saved before Preview.

Admission failure before durable commit must not replace the main or existing audition. If capture has already paused audio, leave coherent recoverable paused state with an actionable persistence error rather than guessing how to resume. Duplicate command IDs must not capture a new main, create another audition/outcome or dispatch audio twice. Return/EOF/Stop races need one owner-committed terminal transition; later stale events cannot win merely because they reference the same track ID.

### Public command and snapshot contract

Extend the existing authenticated JSON-RPC/Tauri bridge; no UI audio element, stream URL or new transport endpoint outside it.

- Add `playback.previewTrack` with `schemaVersion`, `instanceId`, `sessionId`, UUID `commandId`, decimal-string `expectedQueueRevision`, `expectedGenerationId`, and portable `source: {serverId, trackId}`. Use current schemaVersion 1 with additive snapshot fields/new method; preserve strict unknown-field and source-length checks. Return the authoritative snapshot after admission, with loading represented honestly.
- Add `returnToSession` to `ControlAction`, using existing `playback.control` envelope and the active audition occurrence/generation. Return a structured capability error when no main exists; no-op duplicate delivery through existing receipts.
- Add `mode: main | preview` and a nullable `preview` summary containing audition ID, `hasMainSession`, saved main occurrence/position/intent, and resume-inhibited state. Expose no private gain payload or URLs. `current`, `positionMs`, `state`, `playback` and native metadata describe the **active transport**, which can be the audition; queue count/pages/`queueRevision` continue to describe the **main queue**. Document explicitly that preview `current` is not a member of `occurrences`. Keep the summary bounded; never serialize a second whole queue.
- Keep logical main session identity stable throughout Preview/Return. Rotate transport generation on entry, replacement, return and session replacement; advance state sequence on visible transitions. Preview does not increment queue revision because queue membership/order is unchanged. Existing main mutations still do. Control epochs fence asynchronous effects independently of queue revision.
- Reuse 10-minute/1024-command bounded receipt policy with payload identity checks; never interpret transport retries as fresh Preview. Clients refresh on stale instance/session/generation/revision and do not blindly replay destructive actions. UI reopening only reads state.
- Preserve milliseconds as safe nonnegative integer positions and string-encoded revisions/generations. Metadata must match active source before rendering. Add UI mappings for `PREVIEW_ACTIVE`, preview preparation failure, `PREVIEW_RETURN_FAILED`, and unavailable Return; preserve existing sanitized output/source failure causes and retryability.

### Persistence and local audition evidence

Use playback schema **v4**. Keep `playback_sessions` and `playback_occurrences` as the canonical main session; introduce a singleton audition table and separate indexed audition-outcome table. Do not place preview rows in `playback_occurrences`: current `validate_playback_session` checks global row count against the single main session and would reject them as foreign/orphan rows. Do not weaken that invariant to accommodate accidental queue pollution.

The active record needs its UUID, parent logical-session identity, source IDs, current audition position/state, preserved-main occurrence/position/intent, resume inhibition, and bounded consumption evidence. Validate consistency with the existing main session and frozen album membership; use typed state/disposition values and checked integer bounds. Preserve album policy, membership digest and per-member admitted representations from v3 unchanged. Do not persist credentials, resolved URLs, decoder handles, buffers or raw error payloads.

Checkpoint main + first audition admission together. Replace/end audition + terminal outcome + restored main state in one transaction. Commit normal Play/album replacement and overlay removal atomically. Failed writes leave the previous coherent record. Pause audio safely on terminal persistence failure; retry must not emit duplicate outcomes or auto-run an obsolete return. Normal restore and storage Retry share the same v4 validation/dismissal path. Unsupported/corrupt versions retain stored evidence and surface restoration errors without initializing over them.

Terminal dispositions: `naturalCompletion`, `stopped`, `returned`, `replaced`, `superseded`, `technicalFailure`, `interrupted`. A UUID identifies one audition lifecycle; Retry of a terminal failed attempt creates a new attempt identity with the same original main checkpoint, so prior failure is not overwritten. Store source, audition/attempt identity, terminal position, known duration if available, sanitized failure code, and conservative consumption evidence separately from main outcomes. No server reporting, scrobble calls, favorite changes or dislike inference.

A natural EOF alone is not proof of full listening after a seek. Track presentation-driven continuous coverage from the start with fixed-size state (contiguous heard prefix, coverage completeness/unknown flag, and seek/discontinuity flag); pauses/buffering do not add listening time, replay cannot double-count, and forward holes/unknown presentation evidence prevent `fullyHeard=true`. EOF with known full contiguous coverage is distinguishable from seek-to-end, stop, replacement or decoder failure. Keep uncertain eligibility explicit for 15.21; do not invent remote reporting thresholds here. Page disk outcomes through internal storage/query helpers only in this story; no history UI or new public history RPC is required (default 100, max 200) and retain only bounded active evidence in memory; no ever-growing vector of sample ranges or full history. On restart, finalization of an active row as interrupted and clearing it is idempotent. If an attempt already failed, do not append a second contradictory terminal record.

### Audio return and concurrency guardrails

Reuse `PlaybackCommandService`, `PlaybackSession`, global `AudioEngine`, provider resolution by source server, and the existing 60-second cancellable preparation deadline. Preserve the single serialized audio-start path. Suspended main means metadata/SQLite state, not a third retained decoder/stream. Retire its active and prepared-successor slots before installing an audition; disable `successor_candidate` for preview. Rebuild normal successor preparation only after successful main return.

Capture the backend's actual played position at the pause boundary, including acknowledged Pulse cork / Windows submitted-tail handling and current CoreAudio limitations. Do not use the UI interpolated position or last periodic checkpoint. Reconcile any already-presented album handoff receipt **before** freezing main occurrence/position/gain; a pending unheard successor cannot become the saved current track. Fence both generation and control epoch on resolved events, PCM install, seek commits, terminal events, output switches and prepared handoffs.

Natural EOF currently drives the owner's normal album terminal path. Add an audition branch before that path and route its returned preparation effect through the common command service. Merely setting `resume_audio` in an internal snapshot is insufficient if no caller dispatches it. Retain a private return-operation marker carrying the saved main cursor until return activation succeeds or the operation is explicitly cancelled. Stop must consult this marker even after the overlay has been removed, cancel pending preparation and keep the saved cursor; ordinary main Stop would incorrectly reset it to zero. Clear the marker on successful activation or subsequent unrelated main transport, and retain failure attribution for explicit recovery. No lock may be held across provider/network work or native worker retirement.

Reopen the main at its saved position with its **destination** `gain_bits` and `qualified_suffix`, never audition unity/format or a predecessor's metadata. Standalone Preview uses the existing unity policy; previewing an album member does not inherit or recompute album gain. Preserve one gain application after conversion and before PCM enqueue, including resampler tail and replay. Normal codec gain remains intact.

Reuse current bounded start-at-position resume behavior: normal Resume uses sequential decode/discard (`seek_operation_id=None`), including on sources qualified for interactive seeking. Optimizing return through qualified media-time seek is optional only with separate equivalence/landing tests; it is not how normal Resume currently works. Do not equate unavailable interactive seek with automatic inability to resume. Validate actual returned audio against deterministic source markers. Slow/unsupported/changed-source return may time out or fail recoverably, never fall back silently to zero or a different recording. Completed/stopped main contexts remain non-playing on return; avoid the existing explicit-Resume terminal-cursor restart rule accidentally restarting them. Preview boundaries do not claim gapless switching, but subsequent prepared main album boundaries retain 15.9 behavior.

Preserve current limits: at most two source/decoder slots globally; 8 MiB compressed per slot/16 MiB aggregate; PCM min(500 ms, 1 MiB) per slot/2 MiB aggregate. Audition replacement must cancel and retire obsolete source work instead of temporarily accumulating slots. No callback allocation, blocking IO, SQLite/provider work or sync locks. Existing managed-device sync and orderly shutdown protection are unchanged.

### Browser and native integration

Add a separate named **Preview track** button beside existing Play in `MediaCard.ts` audio cards, `library.ts` list rows, and `TracksBrowseView.ts` track rows. Existing context menus are conditional on playlist-write capability; Preview must remain available for read-only providers and without a physical device. Do not make a context-menu rewrite a prerequisite. Capture immutable item `{serverId, trackId}` at render/activation, never consult the currently browsed server after an asynchronous delay. Missing source disables the action.

Prevent pointer actions from triggering row navigation/selection or dragging, while preserving native keyboard activation, Ctrl/Cmd/Shift selection and existing basket/playlist behavior. Use localized accessible names, visible focus and existing Shoelace/tokens. Add visible “Preview” status and Return to session to the existing `PlaybackControls.ts`; hide Return without main, and display loading/return failure honestly. Its `previewMs` variable means seek scrubbing, not audio audition: use distinct names. Preserve stable DOM nodes/focus and bounded status announcements during background updates.

`NativePlaybackView::from_snapshot` must show the active audition's metadata/source/position and return to main metadata when restored. Set `canGoNext=false`, but also enforce the restriction in shared owner handlers because `event_to_intent` can still deliver Next. Previous remains unsupported. Native relative seek beyond the end must not route into main advancement. Pause/Play/Toggle/seek target the audition; Stop uses the paused-return rule. Retain a single existing native registration/event loop when windows close. No native Return binding is required.

### Existing files: current behavior, changes and preservation

Paths below are repository-relative. Read complete touched files before implementing; large modules may be split into focused children without a broad refactor.

| File | Current behavior | Story change / behavior to preserve |
|---|---|---|
| `hifimule-daemon/src/playback/model.rs` | Schema v1 DTOs; active snapshot derived from main; `PersistedSession` and `FrozenAlbumContext` | Add audition DTOs/state/control and explicit projection contract. Preserve identity bounds, serde naming, private gain fields, album validation and existing wire units. |
| `hifimule-daemon/src/playback/session.rs` | Serialized queue/transport owner, progress ingress, seek, native control, terminal/handoff and restoration | Centralize active-transport access across `snapshot`, `sample_progress`, `refresh_ingress`, `reset_progress`, `control_inner`, `seek_inner`, `native_control_inner`, `apply_playback_event`, `complete_occurrence`. Intercept audition terminal events before main advancement; preserve paging, receipts, actual-position capture and shutdown checkpoint. |
| `hifimule-daemon/src/playback/session/album_admission.rs` | Latest-request-wins ordered album reservation/commit | Preview invalidates obsolete reservations; successful album commit removes overlay atomically. Preserve old state on failed/superseded commit and frozen-policy admission. |
| `hifimule-daemon/src/playback/session/output_selection.rs` | Output selection/loss copies active pipeline position/state into main | Route changes through active transport, latch main return inhibition on output loss, preserve output revisions and explicit selection/no-reroute rules. |
| `hifimule-daemon/src/playback/persistence.rs` | v3 singleton session, queue/outcome validation, transactional commits, album context | Add v4 overlay/outcomes transactions and restart dismissal; keep canonical main validation, migration rollback and evidence retention. |
| `hifimule-daemon/src/playback/commands.rs` | Common RPC/native service, captured effects, 60-second source preparation, gain-aware seek/resume | Dispatch audition and natural/explicit return through the same owner-captured source/generation/epoch/gain/suffix contract. Preserve cancellation and commit/dispatch even when RPC future is dropped. |
| `hifimule-daemon/src/playback/audio.rs` | Serialized engine, generation gates, captured position, worker retirement, initial/successor preparation | Distinguish return from terminal explicit Resume; reuse retirement and start-at-position. Do not keep suspended main decoder slots. Preserve gain qualification, output pinning, submitted-tail replay and pause authorization. |
| `hifimule-daemon/src/playback/audio/pulse_output.rs` | Linux native output, cork/played-position, same decoder path | Validate audition teardown/return and gain/position on Linux; edit only where shared effect changes require it. Preserve acknowledged cork and no hidden fallback output. |
| `hifimule-daemon/src/playback/decoder.rs` | Bounded decode, padding, seek or decode/discard, terminal-resume shortcut, gain after conversion | Carry restore disposition if needed to suppress implicit terminal restart. Preserve normal explicit Resume semantics, exact frame counts, silence, tails and one gain application. |
| `hifimule-daemon/src/playback/native.rs` | Active metadata/capability projection and OS command forwarding | Publish audition mode/capabilities and destination main metadata; preserve stale receipt and source matching, registration lifetime and seek units. |
| `hifimule-daemon/src/rpc.rs` | RPC validation, album provider work, output-effect dispatcher | Register preview command and share captured effect dispatch. Current `output_dispatcher` calls `audio.start` (unity/no suffix, epoch captured late): do not send automatic return down that path unchanged. Preserve authenticated boundary, mutation guards and sanitized errors. |
| `hifimule-daemon/src/playback/mod.rs` | Playback module/handle exports | Register focused preview helper/test modules if introduced; keep one public command owner. |
| `hifimule-ui/src/rpc.ts` | Playback types, snapshot refresh, command IDs/fences/error handling | Add Preview/Return and additive snapshot state, preserve response ordering/reconnect and portable source routing. |
| `hifimule-ui/src/components/PlaybackControls.ts` | Existing shared controls, timeline, output selector and status | Show audition/Return, target active occurrence, keep persistent controls/focus and output safety. |
| `hifimule-ui/src/components/MediaCard.ts`, `hifimule-ui/src/library.ts`, `hifimule-ui/src/components/TracksBrowseView.ts` | Three browser Play surfaces and selection/curation interactions | Add independent Preview beside Play without changing selection, playlist menus, source identity or virtualization. |
| `hifimule-i18n/catalog.json` | Central EN/FR/ES/DE translations | Add complete key parity for Preview/Return/status/errors; do not invent UI-local locale files. |
| `scripts/tests/playback-ui.test.mjs`, playback Rust tests and `rpc/album_tests.rs` | Real transpiled UI boundary tests and owner/admission/native/audio regressions | Extend production-path tests; do not claim helper-only fixtures validate audible return or installed platforms. |
| `docs/api-contracts-hifimule-daemon.md`, `docs/playback-installed-test-checklist.md` | Current playback RPC, persistence, native and installed evidence contracts | Document snapshot projection, preview transitions/Next/restart/outcomes and actual evidence. |

Recommended new focused modules: `playback/session/preview.rs`, `playback/session/preview_tests.rs`, and persistence/RPC preview tests adjacent to existing modules. These are implementation locations, not a second session service. No new provider API is needed: reuse `resolve_playback` and existing portable server lookup. Do not edit sync manifests/transcoding or introduce `reporting.rs` merely for future server reporting.

### Testing requirements

1. **Owner transition matrix:** playing, paused, buffering, stopped/completed and no-main entry; same-track Preview still has separate audition identity; A→B→C preserves one main; natural EOF/Return/Stop each produce correct intent/cursor/outcome; Pause/Resume/Retry/seek target audition; direct/native Next and seek-past-end do not advance main. Preserve deliberate duplicate main occurrences, queue revision, all paged entries and original outcomes. Test main at exact terminal cursor without restart-at-zero, and audition seek-to-end remaining non-playing until explicit Return/Stop/Resume.
2. **Actual audio/position:** use deterministic ramps/markers and nonzero main suspension/return, both qualified media seek and existing sequential decode/discard paths. Compare consumed sample positions within the established mechanism tolerance (not UI interpolation or fetched bytes). Test pending seek capture of committed position and Preview at a prepared album handoff; reconcile receipt before capture. Verify returned frozen non-unity gain and admitted suffix, unity audition, no second scaling during replay, and continued main boundary frame counts. Use real provider adapter → command owner → decoder tests, not manually supplied snapshots only.
3. **Async and safety:** delay provider resolve, compressed fetch, decoder start, output activation, seek completion and handoff independently. Race Preview/Preview, Preview/album resolution, Return/Stop (including after overlay removal but before main activation), natural EOF/Return, Return/ordinary Play and output switch/loss. Assert stale work emits no audio/state, pending reservations are cancelled, selected output never falls back, and maximum slot/buffer limits hold during repeated replacement. Test output loss then reconnection before return, and explicit replacement selection while paused.
4. **Persistence:** migrate v1/v2/v3 and validate unchanged album membership/suffix; reopen SQLite after each transition; test no-main overlay, duplicate UUID/source identities, invalid/future overlay schema, orphan evidence, interrupted migrations, checkpoint/admission/terminal writes and storage Retry. Crash/quit during loading, active preview, replacement, return preparation and after outcome commit. Restart always paused/idle; interrupted terminal outcome exactly once; no credentials/URLs stored. Test more than 200 main entries and page-boundary occurrences without queue cloning.
5. **Local consumption evidence:** full natural presentation, Pause/Resume, buffering, forward/backward seeks, replayed submitted tail, stop just before EOF, short decoder failure and unknown-duration input. An EOF after seeking must not become a fully heard audition. Replacement/failure never becomes a main skip/dislike; assert zero provider report/scrobble calls.
6. **UI/native:** exercise all three Preview surfaces, read-only provider, no physical device, immutable source after browse changes, unchanged selection/basket, focus retention and all locale labels. Verify audition state after reconnect, Return absent without main, Stop versus Return command distinction, stale seek responses after replacement, visible return failure. Inject native commands with UI closed and despite disabled Next mask; confirm active and restored metadata/capabilities.
7. **Installed evidence:** real Windows/macOS/Linux builds with actual loaded controlled FFmpeg ABI and selected backend; run the AC11 matrix, loss/reconnect and quit during sync. Record commit, OS/architecture, provider/version/format, captured/returned position, gain, active pipeline count, buffer high-water marks and outcome. Distinguish digital fixtures, native API delivery, physical media keys and physical audio. Missing builds/devices remain unchecked with reasons, never copied as passing from 15.10.

Run focused tests first, then shared regressions and the full daemon suite after integration:

```sh
rtk npm run build:daemon -- test -p hifimule-daemon playback::
rtk npm run build:daemon -- test -p hifimule-daemon
rtk proxy node --test scripts/tests/playback-ui.test.mjs
rtk npm --prefix hifimule-ui run build
rtk proxy python3 -m unittest discover -s scripts/tests -p 'test_playback_installed_evidence.py'
rtk git diff --check
```

Use the controlled runtime build wrapper, not bare cargo tests that bypass its guard. Production decoder fixture generation needs the matching FFmpeg executable (`HIFIMULE_TEST_FFMPEG` where configured) as well as linked libraries. Run actual target builds; library-only cross-compilation does not validate daemon callers. No production tests were run during story preparation, which changes documentation only.

### Previous-story and git intelligence

Recent commits: `bd215ee Review 15.10`, `0c18afe fix(playback): make album starts reliable`, `77aa658 Story 15.10`, `66c37bb Review 15.9`, `efab103 Dev 15.9`.

15.10 review persisted canonical admitted representations, validated requested album identity and size before metadata resolution, retained retryability for representation contradictions and added production gain/lifecycle coverage. Its Next correction rebuilt destination gain and suffix after commit; apply that same rule to audition return. Preserve latest-request-wins album cancellation and silent expected supersession in UI.

15.9 established two-slot continuous output, presentation receipts, epoch-authorized retained pipelines and replay-aware completion. Preserve Windows native pause/clock handling, repeated Pause/submitted-tail replay, Pulse played-position and CoreAudio's documented capture limitation. Unknown codec padding must remain unknown; Preview adds no silence trimming, crossfade or guessed priming. Existing test counts are historical, not current acceptance evidence.

### Architecture, library versions and current technical references

Keep repository pins and patches: Rust edition 2024/MSRV 1.93.0; CPAL 0.18.2; ffmpeg-next/ffmpeg-sys-next 9.0.0; controlled FFmpeg 9.0.1; crossbeam-queue 0.3.12; libpulse-binding 2.30.1; Souvlaki 0.8.3. `audio-runtime.json`, Cargo lock/patches and the build wrapper are authoritative. FFmpeg disables network and avfilter; providers/streaming own authenticated IO. No package/runtime upgrade is needed for Preview.

Primary documentation checked 2026-09-18:

- [CPAL API](https://docs.rs/cpal/latest/cpal/) currently documents 0.18.2 and callback/stream lifecycle. Preserve repository native patches and callback constraints.
- [Souvlaki API](https://docs.rs/souvlaki/latest/souvlaki/) currently documents 0.8.3; reuse existing OS/event-loop integration rather than instantiate a second media controller.
- [MPRIS Player](https://specifications.freedesktop.org/mpris/latest/Player_Interface.html) specifies that Next has no effect when `CanGoNext` is false and position operations identify the active track. This supports disabled audition Next; HifiMule's explicit preview Stop→paused-main behavior remains its application contract.
- [FFmpeg demux/seek API](https://ffmpeg.org/doxygen/trunk/group__lavf__decoding.html) is supplementary API context, not proof of provider-specific landing accuracy. Validate the pinned production path with decoded markers. [Release downloads](https://ffmpeg.org/download.html) do not override the controlled runtime manifest.

These checks establish relevant APIs, not a security audit or certification of untested platforms. No migration to an upstream latest release is part of this story.

### Project Structure Notes

Remain within the daemon playback owner, existing SQLite database, provider abstraction, Tauri JSON-RPC bridge and vanilla TypeScript/Shoelace UI. Use Rust/SQL snake_case and JSON camelCase. Playback is allowed without a physical device; older UX no-device locks apply only to physical sync/basket actions. Browsing server/device state never changes the audition source. The project context's greenfield label is stale; its provider boundary and managed-zone safety rules still apply.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 15, Story 15.11; 15.10 gain; 15.12–14 UI/queue scope; 15.19 preview gain extension; 15.21 reporting]
- [Source: `_bmad-output/planning-artifacts/prd.md` — Playback Extension, FR58/60/62–64/76, P-NFR2/4–6, UJ-P3]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Playback Session Model, Audio Pipeline, UI and Session Control, Implementation Contracts, Project Structure and Validation Refinements]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md`; `_bmad-output/planning-artifacts/project-context.md` — existing UI/accessibility and provider/sync boundaries, subject to playback amendments]
- [Source: `_bmad-output/implementation-artifacts/15-10-preserve-an-album-s-relative-loudness-with-consistent-gain.md` — Review Findings/Resolution, policy persistence, platform limitations]
- [Source: file table above; `Cargo.toml`; `hifimule-daemon/audio-runtime.json`; `scripts/tests/playback-ui.test.mjs`]

## Dev Agent Record

### Agent Model Used

GPT-6.

### Implementation Plan

- Establish the strict additive Preview/Return wire contract and a single owner-controlled audition overlay without altering canonical main-queue rows.
- Add transactional v4 audition persistence/outcomes and idempotent restart dismissal before connecting audio effects.
- Route Preview, natural return and explicit Return through existing provider/audio generation and epoch fences, including frozen destination gain/suffix.
- Add independent browser Preview surfaces, persistent transport status/Return controls and four-locale strings.
- Drive the work with focused red/green owner, persistence and UI regressions, then run controlled playback and full repository gates.

### Debug Log References

- Post-review runtime defect: Preview admission correctly opened the shared audio gate, but `output_opened` immediately recomputed authorization from the suspended canonical main state (`Paused` or `Idle`), leaving the audition indefinitely in Loading with no samples delivered.
- Added red/green output-open regressions for a buffering Preview over a paused main session, a buffering standalone Preview over an idle main session, and a paused Preview. Corrected the gate projection to use the generation-fenced active transport state; focused verification passes 16/16 output-selection tests.
- Added failing contract/owner/UI tests first, then implemented typed Preview projection, replacement, return, Stop override, no-main completion and ordinary-Play supersession.
- Extended SQLite v4 with a singleton active audition and indexed, paged outcomes; verified transactional replacement, exact-once interruption and conservative full-heard evidence.
- Added a pending-return marker after testing the failure boundary so failed main reopening remains paused at the saved cursor with `PREVIEW_RETURN_FAILED`.
- Verification after the Preview output-open fix: playback suite 272 passed/6 ignored; full daemon 978 passed/6 ignored; output-selection suite 16 passed; browser 29 passed; evidence validator 33 passed; UI production build, rustfmt and Clippy completed. Clippy retains the repository warning baseline.
- Installed Windows/Linux/macOS physical-player and hardware-output rows were not runnable in this environment and remain explicitly unchecked in the installed checklist.

### Completion Notes List

- Fixed Preview audio startup after output opening by authorizing the callback gate from the active Preview overlay rather than the suspended canonical main session; paused auditions remain gated until explicitly resumed.
- Implemented a strict `playback.previewTrack` contract, additive `mode`/`preview` snapshot projection and `returnToSession` control while keeping the main queue/session canonical.
- Added serialized Preview admission/replacement, main checkpoint capture, natural/explicit return, audition Stop semantics, no-main idle completion, owner-enforced Next rejection and stale-work fencing.
- Added transactional v4 audition persistence, paged local outcomes, conservative continuous-listening evidence, atomic ordinary-Play supersession and idempotent restart interruption.
- Reused the common provider/audio dispatch with destination main gain/suffix and explicit pending-return recovery; output loss inhibits automatic resume.
- Added accessible Preview actions to all three browser track surfaces, Preview status/Return controls and EN/FR/ES/DE error/label parity.
- Updated API and installed-test documentation. Automated coverage is green; physical installed-platform evidence remains open and is not claimed.
- Applied all 18 code-review patches covering Preview transport identity, durable failure/return state, safe persistence bounds, receipt expiry, recovery validation and behavioral browser coverage.

### File List

- `_bmad-output/implementation-artifacts/15-11-preview-a-full-track-without-losing-the-main-listening-session.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `docs/api-contracts-hifimule-daemon.md`
- `docs/playback-installed-test-checklist.md`
- `hifimule-daemon/src/playback/audio.rs`
- `hifimule-daemon/src/playback/commands.rs`
- `hifimule-daemon/src/playback/model.rs`
- `hifimule-daemon/src/playback/native.rs`
- `hifimule-daemon/src/playback/persistence.rs`
- `hifimule-daemon/src/playback/session.rs`
- `hifimule-daemon/src/playback/session/album_admission.rs`
- `hifimule-daemon/src/playback/session/output_selection.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-i18n/catalog.json`
- `hifimule-ui/src/components/MediaCard.ts`
- `hifimule-ui/src/components/TrackPreviewButton.ts`
- `hifimule-ui/src/components/PlaybackControls.ts`
- `hifimule-ui/src/components/TracksBrowseView.ts`
- `hifimule-ui/src/library.ts`
- `hifimule-ui/src/rpc.ts`
- `scripts/tests/playback-ui.test.mjs`

### Change Log

- 2026-09-18: Created Story 15.11 implementation context and marked it ready-for-dev.
- 2026-09-18: Implemented full-track Preview with preserved main-session return, v4 local audition evidence, shared audio/native/RPC integration and browser controls; moved to review with automated gates green and installed physical evidence left open.
- 2026-09-18: Fixed Preview remaining in Loading after output open by projecting audio-gate authorization from the active transport; added main-present, standalone and paused-preview regressions.
- 2026-09-18: Completed adversarial code review, applied all 18 accepted patches, expanded regression coverage and moved the story to done.
