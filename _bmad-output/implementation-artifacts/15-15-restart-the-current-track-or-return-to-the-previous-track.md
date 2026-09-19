---
baseline_commit: 3efa4529c56c94a009d5ed02a64562a97b957c9a
---

# Story 15.15: Restart the current track or return to the previous track

Status: review

## Story

As a HifiMule user,
I want a Back button in the playing bar and equivalent supported keyboard/media control,
so that I can restart a track or return to the previous track without rebuilding my queue.

Requirements: amended FR58; FR59, FR60 and FR61; P-NFR3–6; P-AR3–4, P-AR6 and P-AR10; P-UX-DR2 and P-UX-DR12–14. Existing output safety (FR64) and Preview contracts (FR62–63) remain binding.

Dependencies: delivered Stories 15.1–15.14. No Epic 16 prerequisite. Story 15.16 owns compact library browsing; Story 15.17 owns installed manual-release validation. This story prepares Back, not Radio, reporting, export, new shortcuts or dependency upgrades.

## Acceptance Criteria

1. **Restart after three seconds.** Given a main track with authoritative position greater than 3,000 ms, when Back is accepted, then the current track returns to its beginning. The decision uses daemon position rather than interpolated UI time. Playing remains playing and paused remains paused, subject to existing output/error inhibition.
2. **Previous near the beginning.** Given a main track at 0–3,000 ms inclusive and an available preceding occurrence in the retained main-session playback order, when Back is accepted, then that preceding track becomes current at its beginning. Preserve its source identity, accepted forward order and deliberate repeated entries. Back followed by forward progression returns through the same sequence without silently dropping or duplicating queued selections.
3. **Beginning and unavailable-source boundaries.** Given no preceding occurrence, Back restarts the current track where supported, without wrapping to the queue end. With no current track, it is unavailable. If the requested restart/previous source cannot be prepared, preserve recoverable state and expose an error; do not silently choose a different track or output.
4. **Preview isolation.** Given an active audition, Back restarts that audition and never navigates the preserved main history. The saved main occurrence, queue, position and return intent remain intact. Existing Return and Stop semantics are unchanged.
5. **Shared UI/native command.** Given an available Back action, clicking its bar button, activating the focused button with Enter/Space, or receiving the OS Previous/media-key command invokes the same daemon operation. Support native Previous/keyboard delivery wherever the platform integration provides it, including with the UI closed. Advertise truthful native availability; document unsupported delivery. Do not introduce an arbitrary global key combination or hijack text-editing, seek-slider or browser-navigation keys.
6. **Accessible bar integration.** Given any library/Playing/device view and supported layout, Back remains reachable beside transport controls with a localized accessible name, tooltip/help explaining restart versus previous behavior, visible focus and truthful disabled state. Preserve the current two-row layout, timeline, bottom-row reachability and focus during updates.
7. **Ordering and history integrity.** Given repeated commands or a race with automatic advance, queue edits, seek, preview, source replacement or Quit, the daemon serializes decisions, deduplicates retried command identities and fences obsolete preparation. Stale UI commands cannot rewind a newly selected session. Replaying earlier music must not overwrite already-recorded outcomes; implement the replay identity and forward-cursor contract below.
8. **Persistence and regression evidence.** Given a successful Back followed by orderly Quit/relaunch, restore the accepted current track, queue and position paused. Verify the 0, 3,000 and 3,001 ms boundaries; first-track fallback; repeated source IDs; manual and album queues; paused state; Preview with/without a main session; unavailable sources; output loss; concurrent commands; and restoration. Record UI/native API and physical keyboard/media-key results separately on shipping platforms.

## Tasks / Subtasks

- [x] **1. Add replay-safe durable attempts without changing queue identity** (AC 2, 7, 8).
  - [x] Implement the version-5-to-6 playback migration and attempt lifecycle specified below; verify rollback/idempotence and preservation of existing outcomes, album policy and saved Preview.
  - [x] Replace destructive outcome reset for replay, including Resume/Retry, and make ordinary terminal handling and prepared-handoff terminal handling attempt-aware.
  - [x] Test stable forward order, repeated source IDs, migration, terminal persistence failure/retry and paused restoration.
- [x] **2. Add the shared daemon Back command and capability** (AC 1–5, 7).
  - [x] Extend `ControlAction`, owner dispatch and `PlaybackState`; retain the existing strict control envelope and add daemon-derived `canGoBack`/`backUnavailableReason`.
  - [x] Sample authoritative position, reconcile presented handoffs, validate current identity, select restart/previous and persist the accepted transition before publishing it.
  - [x] Reuse qualified seek or bounded provider reopen at zero; fence previous preparation, PCM and prepared successors. Preserve explicit playing/paused intent and all output inhibition.
  - [x] Restart Preview in isolation, preserving its open audition identity and saved main state while marking discontinuous heard coverage conservatively; allocate a fresh audition identity after an already-recorded terminal failure.
  - [x] Suppress every audio side effect on dedup cache hits; cover admission/failure/race cases.
- [x] **3. Enable supported native Previous delivery** (AC 5, 7, 8).
  - [x] Extend the daemon native mask and map `MediaControlEvent::Previous` to the shared Back command.
  - [x] Publish dynamic macOS/Windows/MPRIS availability through the existing patched Souvlaki adapters. Test headless command routing and metadata/position refresh.
  - [x] Record API-level and physical-key results separately, including unsupported or untested environments.
- [x] **4. Integrate Back into the retained bar** (AC 5, 6).
  - [x] Extend `playbackControl` and snapshot types; reuse the existing control click/refresh path, not UI time or client history.
  - [x] Add a stable Shoelace Back button beside the primary transport, using `skip-start-fill`, localized name and explanatory hover/focus help in EN/FR/ES/DE.
  - [x] Apply daemon availability, busy/freshness gates and scoped error handling. Preserve focus and clear superseded interaction state/errors when identity changes.
  - [x] Verify the two-row bar, full-width timeline, output chooser, Library/Playing switch, guidance overlay and final-content reachability at narrow width/zoom.
- [x] **5. Validate end-to-end and record evidence** (AC 1–8).
  - [x] Extend real production-module Rust/Node test harnesses with the matrix below; run focused tests, relevant broader regressions and the UI build.
  - [x] Verify actual UI keyboard activation and daemon playback behavior; do not treat DOM fixtures or mock native events as physical-key evidence.
  - [x] Document commands/results/limitations in the Dev Agent Record. Preserve existing done statuses and explicit release follow-ups.

## Dev Notes

### Frozen transport and replay contract

These decisions close the implementation gate in Epic 15.15. They are implementation requirements, not claims that the code already supports them.

**Command and admission.** Extend `playback.control` with `action: "back"`, keeping wire `schemaVersion: 1`, `instanceId`, `sessionId`, UUID `commandId`, `expectedGenerationId` and `occurrenceId`. Do not add a second RPC or transmit a UI-selected target/position. Preserve current envelope validation: stale instance/session yields `SESSION_MISMATCH`, stale generation/current yields `GENERATION_CONFLICT`, conflicting reuse yields `COMMAND_ID_REUSED`; malformed envelopes use existing validation errors. Owner/lifecycle fencing and pending terminal persistence errors remain authoritative. Add `BACK_UNAVAILABLE` for an otherwise valid command that cannot restart/navigate; include a safe structured reason, not a URL or raw provider error. Existing source/output/persistence failure codes remain applicable to asynchronous preparation. UI refreshes conflicts once and never automatically replays an intent against a new current track.

**Decision point.** The serialized owner samples consumed audio progress and reconciles any already-presented prepared handoff before checking the command's generation/occurrence and choosing the target. Back must not inherit the special Pause/Stop behavior that retargets a stale command to a just-advanced track. Main position `>3000` chooses current; position `<=3000` chooses the nearest retained preceding queue occurrence, or current if none. Resolve by session/ordinal and occurrence identity, never by track ID or an incomplete UI history page. A removed row is not a predecessor. A retained predecessor with an unavailable source is still the requested target: fail explicitly rather than skipping to another predecessor.

**Intent and edge states.** Playing/buffering preserves intent to play when the selected output remains eligible; paused stays paused. Stopped/completed/error with a retained current uses the same threshold rule but settles at zero paused, with explicit Resume required. No-current and shutdown are unavailable. A pending terminal storage failure must be resolved using existing Retry/Stop handling before Back. Output loss, pending output changes and saved Preview resume inhibition cannot be cleared by Back. An available source may be selected/restarted while inhibited, but no PCM may become audible; output reconnection alone does not resume. Back may cancel superseded seek/preparation, but it must never accept an uncommitted slider/seek target as its threshold position.

**Queue occurrence versus listening attempt.** Keep each accepted queue `occurrenceId`, source and ordinal stable. Introduce a durable UUID attempt identity referencing the queue occurrence, separate from runtime `generationId`. Rewinding does not insert clones or reorder rows. Example: retained `A₁, B, A₂, C` (A₁ and A₂ may have the same source) with C at zero goes Back to A₂, Back to B, then forward through A₂ and C in that exact order. Each visit has its own attempt identity; there is still one row per accepted queue selection. Back/current restart does not increment queue revision; accepted structural edits still do. Increment state sequence and invalidate generation/control epochs for every accepted Back, even same-occurrence restart, so old seek replies, decoders, terminal events and handoffs cannot mutate the new attempt.

**Durable attempt records.** Add playback persistence version 6 using the migration authority in `playback/persistence.rs` (`Database::init_playback`), not an independent database. Store the active main attempt reference with the recoverable session and an indexed attempt ledger (`playback_attempts`) keyed by attempt UUID, referencing session and occurrence. Store source identity with historical attempt evidence so a later queue edit cannot orphan its meaning. Each attempt's terminal disposition is written once. Preserve/migrate every existing non-null occurrence outcome as legacy attempt evidence without inventing duration/heard coverage. Retain existing occurrence outcome/failure fields as immutable legacy evidence while migrating consumers to attempt-aware semantics; do not clear or rewrite them to make replay possible. Existing open current gets an active attempt; a legacy terminal current gets a fresh attempt only when explicitly replayed. Migration must retain current cursor, position, ordered queue, album context/gain/qualified-suffix policy and Preview data, and commit schema version atomically. Unknown versions remain rejected.

**Outcome semantics.** Every accepted main Back starts a fresh attempt at its target. Close an open prior attempt as `restarted` (same occurrence) or `backNavigation` (previous); neither means skip, dislike or completion. Already-terminal attempts remain unchanged. Natural end, explicit Next and technical failures terminate the active attempt once; replay Resume/Retry creates a new attempt when the previous one is terminal, instead of erasing it. Pause/Resume of an open attempt, ordinary seek, checkpoint and Preview suspension retain that attempt. Accepted session/queue replacement or Clear closes an abandoned open attempt atomically as a neutral `superseded`/`interrupted` outcome and retains its ledger evidence; a paused restore continues an open attempt without claiming a completion. Prepared album handoff must finalize the outgoing attempt and establish the successor attempt atomically with cursor advancement. Never rewrite a technical failure to an explicit skip; if Next follows a failed attempt, move forward without reclassifying that failed evidence. Future 16.7 reporting must use actual attempt/heard evidence, never cursor movement or migration as proof of a completed listen. Keep history/ledger queries paged and resident memory bounded.

**Forward progression and editing after rewind.** Next/natural completion selects the immediate retained successor even if that queue occurrence has an earlier terminal attempt. Use a fresh attempt for the new visit. Preserve ordinal-based order, deliberate repetitions and album membership/gain. Upcoming/history sections describe position relative to the current cursor; after rewind an earlier-played retained successor is upcoming again. Existing current-occurrence page fences must invalidate those sections even if queue revision is unchanged. A user may explicitly edit the now-upcoming section under existing revision/current checks; such accepted edits take precedence over the old forward sequence. Do not resurrect removed entries or lose their attempt evidence.

**Long retained history.** Back is not restricted to the loaded 200-row window or the 10,000-entry active admission cap. Rewind may legitimately make the cursor-relative forward span exceed that cap; preserve every retained row. Reject new append operations while the resulting active span exceeds the existing cap, but keep Remove/Move usable through bounded indexed SQL/transaction operations rather than loading the whole span. The existing capped fetch in those edit paths must not truncate, drop or silently ignore distant rows. Cover a fixture with more than 10,000 history entries plus a full active range. Removing an earlier-played upcoming row, Clear and queue replacement must retain its immutable attempt evidence: no cascading ledger deletion or requirement that every historical attempt still has a live queue row. Navigation can only use rows still retained in the current queue.

**Preparation, commit and recovery.** Reuse qualified seek for a same-source restart only when it can honor the new attempt/generation and zero landing safely; otherwise use the existing bounded provider-resolved reopen pipeline at zero. Reopen at the beginning need not require a known duration or arbitrary byte-range seeking. Do not claim seek support merely because restart can reopen. Persist an accepted target cursor/attempt and zero resume target atomically before publishing that target; label audio loading/pending until it is actually prepared/committed. If preparation fails, retain that selected target at zero in recoverable paused/error state with its queue intact; explicit Retry retries that source. No alternate track/output or automatic rollback playback. If persistence fails, retain the prior committed cursor/position, inhibit unsafe audio, and expose failure. Quit during preparation restores only the committed target, paused; a successful audible restart checkpoints the committed position. Preserve gain policy and clear obsolete prepared-successor tokens before restarting.

Preparation must be an explicit effect independent of `resume_audio`, carrying whether playback may become audible. Existing dispatch returns early when `resume_audio` is false; do not fake it true to prepare a paused target. A paused/inhibited restart can resolve and prepare while the output gate remains closed, and later Resume uses that selected open attempt. Add nullable `playback.pendingBack: { operationId: string }` and `playback.backOutcome: { operationId: string, status: "committed" | "failed", error?: PlaybackFailure | null }` to snapshots, where `PlaybackFailure` uses the existing `{code, retryable}` shape. The operation ID is the accepted command UUID. Pending is runtime-only, not success; completion/failure must match current generation/operation, clear pending and advance state sequence. A newer Back/seek/replacement cancels pending work; old completions cannot repaint it. On daemon restart restore the committed cursor paused without resurrecting a runtime pending operation. Suppress the new preparation effect on dedup hits.

**Preview.** Back always targets the audition regardless of position and whether a main session exists. Retain `auditionId` while its attempt is open, plus saved main occurrence/position/intent, queue edits made during Preview and `resume_inhibited`. If the audition already has a terminal failure outcome, reuse the existing `retry_audition` pattern to allocate a fresh audition UUID before Back restarts; preserve the previous immutable outcome and the same saved-main baseline. The existing unique outcome per audition must not suppress the new attempt’s eventual completion. Invalidate the audition generation and restart from zero; do not create/finish a main attempt, insert a queue row or emit a terminal audition outcome merely for Back. Reset/mark coverage conservatively (`seek_discontinuous`/unknown coverage as appropriate) so replayed audio cannot inflate fully-heard evidence. Natural completion/Return restore the approved main intent subject to inhibition; Stop restores main paused; Preview without main still finishes idle. Failed restart leaves that audition recoverable and Return available where it was available before.

**Dedup and effects.** Reuse payload-checked control dedup (bounded to 1,024 control identities during the current owner lifetime; do not promise durable exactly-once semantics). Identical retries return the cached result without restarting, navigating, incrementing counters or writing outcomes twice. Cache hits must clear both `resume_audio` and `seek_audio`, plus any new Back effect. Different fresh native key presses remain separate commands. After eviction, old generation/current preconditions still protect against stale re-execution. Preserve common mutation admission and bounded command/source-preparation behavior.

### Native capability and frontend contract

Add `playback.canGoBack: boolean` and nullable `playback.backUnavailableReason` to the version-1 snapshot and TypeScript contract. Calculate them in the daemon from current ownership, restart/previous feasibility and blockers; empty, shutting down or blocked persistence states must not advertise a usable command. No predecessor alone does not disable Back because restart is allowed. Unknown source resolution may be attempted; known unsupported behavior is disabled with explanation. Native and UI use the same availability policy, with the UI additionally disabling during stale/disconnected/busy interaction. Capability means a command can be accepted, not that an unavailable physical device will produce sound.

| Surface | Existing integration | Required mapping |
| --- | --- | --- |
| macOS | Patched Souvlaki `previousTrackCommand` | Previous event → shared Back; publish daemon-derived enabled state on existing daemon event loop. |
| Windows | Patched Souvlaki SMTC Previous | Same action; `IsPreviousEnabled` follows capability; preserve the existing native window/event ownership. |
| Linux | Active Souvlaki D-Bus MPRIS backend | `Previous` → Back; `CanGoPrevious` and change publication follow capability. Preserve disabled-command no-op behavior. |
| Focused UI | Existing Shoelace `sl-button` | Normal Enter/Space activation dispatches one Back click. No global listeners or interception of typing/slider/navigation keys. |

HifiMule deliberately maps native Previous to its approved three-second Back behavior, including first-track restart; MPRIS's generic previous-at-boundary description is not permission to replace this product rule. Test/document this mapping. The optional zbus backend is not enabled; do not claim it was validated. Keep native feedback, sanitized errors and headless operation. No vendored adapter edit should be necessary unless a concrete uncovered capability defect is demonstrated.

Native feedback must track a pending paused restart/reopen until preparation actually succeeds or fails; the current receipt check only recognizes Loading/pending seek and must be extended if Back uses another pending representation. A committed same-track restart publishes the corresponding MPRIS position discontinuity (`Seeked`) once, just as successful seek does; admission/pending/failed restart is not a committed seek event. Metadata must refresh after a previous-occurrence change without publishing stale source details.

In `PlaybackControls`, create the node once and update its attributes; do not rebuild controls on progress. Use a short accessible name (“Back”) and separate localized help (“Restart the track; near the beginning, go to the previous track. During Preview, restart the preview.”). Preserve Shoelace focus and tooltip behavior. Use the existing command-then-authoritative-refresh flow: `PlaybackStore.acceptCommand` intentionally rejects cross-occurrence command results and must not be weakened to accept a previous-track replacement. The 100 ms paint clock, bounded 750 ms interpolation and shared 500 ms poll are presentation only. Adding Back must not create another poller.

`setIcon()` currently overwrites tooltip content with the short accessible label, and `hint()` initializes from that label. Preserve the separate explanatory Back help across rerenders explicitly. Show a localized unavailable reason through existing guidance or reachable help, since a disabled button cannot receive keyboard focus. Extend the generic control-error handler for safe `BACK_UNAVAILABLE` reasons; do not expose raw error messages or assume the existing persistence-only special case renders them. Test name/help separation after several snapshots and accessible disabled-state explanation.

### Existing files: current state, intended change, preserved behavior

Paths below are relative to the repository root. UPDATE means an existing implementation seam, not permission for a broad rewrite.

| File / status | Current state | Change and preservation |
| --- | --- | --- |
| `hifimule-daemon/src/playback/model.rs` — UPDATE | Wire v1 typed controls/snapshots and persistence models; no Back. | Add Back/capability/attempt types. Preserve camelCase, strict envelopes, string revisions, portable source IDs and millisecond units. |
| `hifimule-daemon/src/playback/session.rs` — UPDATE | Serialized owner, progress sample, generation checks, command dedup, Preview, terminal commits and prepared handoff. | Implement the frozen contract; preserve owner/lifecycle fences, output inhibition, ordering and nonblocking callback boundary. |
| `hifimule-daemon/src/playback/persistence.rs` — UPDATE | Schema v5; ordinal queue; predecessor/successor queries; outcomes attached to occurrence; saved audition. | Migrate attempt ledger and cursor transactions. Reuse `playback_predecessor_in_range`/successor queries with correct whole-session bounds. Preserve rollback and paused restoration. |
| `hifimule-daemon/src/playback/commands.rs` — UPDATE | Shared RPC/native service, detached source preparation, generation/epoch cancellation and bounded execution. | Dispatch Back effects through existing provider/audio paths, including paused preparation; preserve source routing and command side-effect ordering. |
| `hifimule-daemon/src/playback/native.rs` — UPDATE | Dynamic masks/receipts; Previous ignored and advertised false. | Add Previous mask/intent/capability and tests. Preserve event-loop ownership, bounded ingress and metadata publication. |
| `hifimule-ui/src/rpc.ts` — UPDATE | Tauri-proxied controls with snapshot identity, UUID and generation. | Add Back action and capability fields; preserve release proxy and conflict behavior. |
| `hifimule-ui/src/components/PlaybackControls.ts` — UPDATE | Retained two-row transport, scoped async epochs, timeline/output/metadata and shared store. | Add stable Back node/help/state using existing control path; preserve all current controls and layout. |
| `hifimule-i18n/catalog.json` — UPDATE | Shared EN/FR/ES/DE catalog. | Add Back/help/unavailability text in all four locales with parity checks. |
| `scripts/tests/playback-ui.test.mjs` and existing playback Rust tests — UPDATE | Real component/store/RPC harnesses and owner/persistence/native/audio tests. | Add behavior-focused cases; preserve existing regression assertions and avoid testing a duplicate state machine. |

Read these adjacent seams before any necessary change: `playback/audio.rs`, `audio/handoff.rs`, `continuity.rs`, `album.rs`, `commands_tests.rs`, `hifimule-ui/src/state/playback.ts`, `components/PlaybackDestination.ts`, `state/queue.ts`, `styles.css`, daemon `rpc.rs`, and the patched Souvlaki platform adapters. Propagate new enum variants through exhaustive matches, but keep transport selection in the owner. Existing daemon RPC already forwards `playback.control`; no new Tauri endpoint is required. `PlaybackDestination` must refresh cursor-relative pages after Back with unchanged queue revision; change it only if the regression test exposes a missing current-identity invalidation. If adding a helper module, keep it under `playback/`, not a new crate/application. Read every newly selected UPDATE file completely before editing.

### Specific regression traps

- `reset_playback_attempt` currently clears `playback_occurrences.outcome`; Resume/Retry calls it for terminal occurrences. Reusing it for Back would destroy evidence. Replace that path as part of attempt integration.
- `persist_playback_terminal` can rewrite technical failure to explicit skip. Preserve the old evidence instead; changing the cursor is not changing what happened.
- `adopt_presented_handoff` writes a terminal outcome directly, bypassing the ordinary terminal plan. It must understand replay attempts or replaying a completed album occurrence can stall on a duplicate-outcome error.
- The control dedup branch currently suppresses only `resume_audio`. Reusing a seek effect without suppressing `seek_audio` can execute a duplicate restart.
- Seek requires a qualified mechanism and known duration today. Back needs an independently truthful reopen-from-zero fallback; do not falsely advertise full seeking.
- Existing queue occurrences, album digest/member count, pinned gain and qualified suffix encode accepted membership/order. Cloning replay rows breaks those assumptions.
- Album-to-manual edits retain the current occurrence's frozen gain/suffix. Same-track Back preserves that policy; selecting a different occurrence follows the existing target/queue policy and must not copy another track's frozen gain blindly.
- Existing replay tests currently assert cleared occurrence outcomes. Replace those expectations with preserved evidence and new attempt records; retain their coverage of Retry's committed-position behavior.
- Keep server selection/device destination independent of the playback source. Never invoke provider-specific URLs directly or put credentials/resolved stream URLs in attempt rows, snapshots, logs or UI messages.
- No network, SQLite, allocation or blocking synchronization in the realtime audio callback. Use existing generation/epoch and prepared-handoff boundaries.

### Architecture, versions and current technical information

Use repository manifests/lockfiles, not historical architecture placeholders: Rust edition 2024 / MSRV 1.93.0; Tokio `~1.49`; rusqlite `~0.38`; CPAL `=0.18.2` with local patch; FFmpeg wrappers `=9.0.0`; Souvlaki `=0.8.3` with local patch; crossbeam-queue `=0.3.12`. Frontend uses vanilla TypeScript `~5.6.2`, Vite `^6.0.3`, Tauri 2.10 and Shoelace `^2.19.1`. Keep the existing controlled FFmpeg packaging and lockfile resolutions. This feature needs no dependency upgrade.

Technical documentation checked 2026-09-19: [Souvlaki latest published documentation](https://docs.rs/souvlaki/latest/souvlaki/) reports 0.8.3 and requires platform event-loop/window ownership; keep HifiMule's patched capability implementation. [Apple previousTrackCommand](https://developer.apple.com/documentation/mediaplayer/mpremotecommandcenter/previoustrackcommand) supports registering and disabling Previous. [Windows IsPreviousEnabled](https://learn.microsoft.com/en-us/uwp/api/windows.media.systemmediatransportcontrols.ispreviousenabled) controls Previous availability. [MPRIS Player](https://specifications.freedesktop.org/mpris/latest/Player_Interface.html) specifies Previous, CanGoPrevious and disabled-command behavior. These checks establish the relevant integration contract, not physical-key delivery or a general dependency/security audit. No migration to another native library or optional backend is needed.

### Previous story and git intelligence

Story 15.14 is the nearest predecessor and is marked done. Its implemented layout is the chosen two-row inset bar with full-width timeline, icon controls, one Library/Playing toggle and exceptional guidance overlay. Preserve current `DESIGN.md` navy/cyan/Inter tokens over older UX purple/Outfit examples. Do not add Play something before 16.6.

Recent baseline commits: `f22793a` (Correct course: approved release split), `7188e18` (Fix layout: bar stacking and top-end guidance tooltip), `91cf967` (Fix windows test), `e60c1ab` (Review15.14), `3b74774` (Dev15.14). Reuse retained UI nodes, command freshness and identity guards, shared snapshot store and native routing introduced by those changes.

Historical passing test counts and browser-fixture screenshots are not evidence for this story. Keep explicit inherited follow-ups: 15.14 R8 (seek-epoch interaction invalidation), R9 (old occurrence command error), and 15.12 R16 (basket guard); Story 15.17 owns release reconciliation. Back's new generation must fence its own stale seek/error interactions. If directly modifying an affected path, fix or explicitly disposition that scoped issue with a regression test; do not silently claim all prior findings closed or reopen completed story statuses.

### Testing requirements

| Area | Required evidence |
| --- | --- |
| Threshold / intent | 0, 3000, 3001 ms; UI interpolation deliberately disagrees; first occurrence; no current; playing, buffering, paused, stopped, completed, error and output-inhibited states. |
| Retained order / replay | Manual and album `A₁,B,A₂,C`; repeated Back then Next and natural progression; unchanged queue identities/revision and legacy outcomes; fresh attempt IDs; append/move/remove after rewind; removed predecessor and paged history. |
| Terminal integrity | Restart an already-completed occurrence; Resume/Retry and replayed prepared handoff; duplicate terminal events; injected transaction failure/retry; no technical-failure reclassification; no false completion from Back. |
| Preview | With and without main, playing/paused/inhibited; repeated Back; same open audition identity or fresh identity after terminal failure; untouched main cursor/position/return intent; conservative coverage; Return/Stop/natural completion and failed restart. |
| Source/output | Qualified seek and safe reopen; unknown duration; unavailable predecessor/source/server; source resolution timeout; output loss/change/reconnection during preparation; no fallback or automatic audible resume. |
| Admission races | Duplicate ID and payload collision; both audio effects suppressed on cache hit; fresh presses; stale generation/session; presented handoff; seek commit/failure; queue edit; preview replacement; source/session replacement; orderly Quit. |
| Persistence | v5 data with null/natural/skip/failure outcomes; migration idempotence/rollback; existing older migration fixtures; current/attempt transaction; pending preparation then Quit; paused relaunch with exact cursor/source/order and valid gain policy. |
| UI | Production wrapper sends only Back+identity; refresh after cross-occurrence success; truthful freshness/capability/busy gates; stable focused node; no repeated action on refresh; late error/reply isolation; all four locales; keyboard does not hijack editing/slider/navigation. |
| Native and real app | Shared Previous routing/masks; UI closed; metadata and committed position; API vs physical keys distinguished per Windows/macOS/Linux. Actual Shoelace Enter/Space/focus, narrow width, zoom, tooltip clipping, last-row reachability and all destinations. |

Suggested commands (use current repository toolchain/controlled audio environment):

```sh
rtk cargo test -p hifimule-daemon playback::
rtk node --test scripts/tests/playback-ui.test.mjs
rtk node --test scripts/tests/*.test.mjs
rtk npm --prefix hifimule-ui run build
rtk cargo fmt --all -- --check
rtk cargo clippy -p hifimule-daemon --all-targets -- -D warnings
```

Run required platform-target checks for changed native code and the relevant existing lifecycle/queue/Preview suites. Record actual failures separately from pre-existing/environment failures. Do not mutate live credentials, managed-device files or user listening history for tests; use disposable databases/providers/audio fixtures. Add physical/platform results or explicit unavailable evidence and carry outstanding installed checks to 15.17 without labeling them passed.

### References and precedence

- `_bmad-output/planning-artifacts/epics.md` — Epic 15, Story 15.15, approved 2026-09-19 sequencing and implementation gate.
- `_bmad-output/planning-artifacts/prd.md` — Desktop Playback amendment FR58–64 and P-NFR3–6.
- `_bmad-output/planning-artifacts/architecture.md` — Playback Implementation Contracts, Validation Refinements, Project Structure and Implementation Handoff; these supersede older single-provider/no-device assumptions.
- `_bmad-output/planning-artifacts/ux-design-specification.md` — playback addendum and accessibility requirements; current `DESIGN.md` and delivered 15.14 layout govern visual implementation.
- `_bmad-output/planning-artifacts/project-context.md` — provider abstraction, managed-sync integrity; its historical greenfield status is stale, not the current repository state.
- `_bmad-output/implementation-artifacts/epic-15-context.md`; Stories 15.6, 15.7, 15.9–15.14 — native controls, seek, continuity, gain, Preview, queue and bar foundations.
- `third_party/souvlaki/HIFIMULE_PATCH.md`; workspace `Cargo.toml`; `hifimule-ui/package.json` — current integration/version authority.
- Implementation symbols and file map above are the current-code grounding; verify line locations at implementation time.

## Dev Agent Record

### Agent Model Used

Creation and implementation: Codex.

### Debug Log References

Creation analysis: approved planning/architecture/UX context, preceding story and five recent commits; current UI/control/native/session/persistence implementation; official native API documentation.

Implementation plan and execution: introduced schema-v6 attempt evidence first, migrated terminal paths and bounded queue edits, then added serialized Back admission/preparation, native Previous projection, retained UI integration and regression coverage. Full workspace testing exposed and drove fixes for legacy-v3 ledger reconstruction and byte-equivalent album receipt replay.

Validation evidence (2026-09-19):

- `rtk proxy env HIFIMULE_FFMPEG_RUNTIME_VERIFIED=ffmpeg-9-runtime-verified-v2 cargo test --workspace` — passed: daemon 1,038 passed/6 intentional diagnostic or hardware ignores; i18n 7 passed; lifecycle contract 13 passed; UI Rust 7 passed; all remaining workspace/doc suites passed.
- `rtk proxy env HIFIMULE_FFMPEG_RUNTIME_VERIFIED=ffmpeg-9-runtime-verified-v2 cargo test -p hifimule-daemon playback::` — passed: 317 passed/6 intentional diagnostic or hardware ignores.
- `rtk node --test scripts/tests/*.test.mjs` — passed: 152/152.
- `rtk npm --prefix hifimule-ui run build` — passed; retained pre-existing Vite chunk and mixed static/dynamic import warnings.
- `rtk cargo fmt --all -- --check` and `rtk git diff --check` — passed.
- `rtk cargo clippy -p hifimule-daemon --all-targets -- -D warnings` — blocked by the existing repository lint backlog (108 errors across unrelated API/device/RPC/sync and older playback code); no Clippy-clean baseline exists for this command.
- API/automation evidence: daemon threshold, replay, Preview, output inhibition, migration, long-history, native Previous/mask/position and retained DOM activation/focus/help tests passed. Physical media-key delivery, actual Shoelace Enter/Space in an installed app, narrow-width/zoom clipping and shipping-platform checks were not executed in this environment; they remain explicit Story 15.17 installed manual-release checks. The optional zbus backend remains disabled and unclaimed.

### Completion Notes List

- Added a schema-v6 paged playback-attempt ledger with immutable legacy evidence, atomic migration/recovery, fresh replay attempts and attempt-aware terminal/handoff/replacement behavior.
- Added the shared Back operation with the authoritative 3,000 ms boundary, retained predecessor navigation, first-track restart, Preview isolation, persistence-first commit, dedup and generation/control fencing.
- Fixed replacement-pipeline retirement so an audible Back reopens the matching generation/epoch gate, while paused, inhibited, failed and superseded Back operations remain silent; Back commits only after worker readiness.
- Preserved cursor-relative forward order for manual and album queues, including repeated sources, and replaced cap-truncated Move/Remove paths with bounded indexed SQL suitable for rewound histories over 10,000 rows.
- Enabled truthful native Previous capability/routing and committed zero-position discontinuities through the existing macOS, Windows and MPRIS Souvlaki adapters.
- Added the stable localized Back control and safe unavailable/error presentation through the shared RPC/store refresh path.
- Added Rust and production-module Node regression coverage; the complete workspace and UI build pass. Installed physical keyboard/media-key and layout checks remain assigned to Story 15.17 and are not labeled passed here.

## Change Log

- 2026-09-19: Implemented replay-safe Back across daemon persistence/session/preparation, native controls and retained UI; added migration, long-history and end-to-end regression coverage.
- 2026-09-19: Fixed macOS Back silence by reopening only the admitted replacement pipeline at worker readiness; hardened failure, supersession, native feedback and attempt-position races found during adversarial review.

### File List

- `_bmad-output/implementation-artifacts/15-15-restart-the-current-track-or-return-to-the-previous-track.md`
- `_bmad-output/implementation-artifacts/spec-15-15-back-audio-resume-regression.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `hifimule-daemon/src/playback/audio.rs`
- `hifimule-daemon/src/playback/audio/handoff.rs`
- `hifimule-daemon/src/playback/audio/pulse_output.rs`
- `hifimule-daemon/src/playback/commands.rs`
- `hifimule-daemon/src/playback/commands_tests.rs`
- `hifimule-daemon/src/playback/model.rs`
- `hifimule-daemon/src/playback/native.rs`
- `hifimule-daemon/src/playback/persistence.rs`
- `hifimule-daemon/src/playback/session.rs`
- `hifimule-daemon/src/playback/session/album_admission.rs`
- `hifimule-daemon/src/playback/session/album_admission_tests.rs`
- `hifimule-i18n/catalog.json`
- `hifimule-ui/src/components/PlaybackControls.ts`
- `hifimule-ui/src/rpc.ts`
- `scripts/tests/playback-ui.test.mjs`
