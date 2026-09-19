---
baseline_commit: 36add0b4801503d8abdaf124bb0c015ea859e0e7
prepared: 2026-09-19
---

# Story 15.13: Edit the upcoming listening queue

Status: ready-for-dev

## Story

As a HifiMule user,
I want to add, reorder and remove upcoming tracks in Playback,
so that I can shape what I hear next without interrupting the current track.

**Requirements:** Manual queue portion of FR55 and FR68; occurrence-preservation foundation for FR78; P-NFR2, P-NFR4–6; queue portions of P-AR3–4 and P-AR10; P-UX-DR5, P-UX-DR11–14.

**Dependencies:** Stories 15.1–15.12. Reuse the daemon session owner, SQLite queue, prepared successor, album gain, preview overlay and Playback destination. Sprint tracking and the 15.12 header say done, but its review completion explicitly retains deferred R16 (physical-basket mutation guards) and says acceptance is incomplete. Carry that known limitation; this story does not resolve or certify it. New queue actions must have no basket side effects. Installed physical-output/platform evidence inherited from earlier stories remains unverified where recorded as such.

**Scope:** Local append, upcoming-only move/remove, authoritative conflicts, durable order, preview-safe editing and bounded history/queue presentation. Stories 15.14–15.29 own the floating bar, selection configuration, Radio replenishment/exclusions/deduplication/normalization, reporting, feedback, exports, adaptation and soak/package certification. Do not introduce those features here.

## Acceptance Criteria

1. **Given** tracks are selected in the library,
**When** the user explicitly adds them to the Playback queue,
**Then** the daemon appends occurrences in the specified selection order, retaining their source identities,
**And** deliberately adding the same recording again creates a distinct occurrence rather than silently deduplicating it. Adding tracks does not interrupt current playback or automatically start an idle session.

2. **Given** upcoming occurrences are displayed,
**When** the user reorders them through an accessible queue action,
**Then** their new order becomes authoritative and persists across reconnection,
**And** the currently playing occurrence, its position and past history are unchanged. Keyboard operation must provide an alternative to drag-and-drop.

3. **Given** an upcoming occurrence is selected,
**When** the user removes it,
**Then** only that occurrence is removed and playback continues,
**And** removal of one repeated entry does not remove every occurrence of the recording or alter the source server's library or playlists.

4. **Given** a queue edit invalidates a prefetched successor,
**When** pending preparation completes,
**Then** generation/revision checks prevent the obsolete successor from playing,
**And** the next boundary uses the latest accepted queue order with bounded preparation buffers.

5. **Given** playback advances while a queue edit is being submitted,
**When** the edit reaches the daemon with a stale revision or targets an occurrence that is no longer upcoming,
**Then** it is rejected with current authoritative state rather than silently removing/reordering the active track,
**And** the UI explains the conflict and refreshes without blindly replaying the mutation. Progress-only updates do not cause such conflicts.

6. **Given** an album queue is manually changed,
**When** the edit is accepted,
**Then** subsequent playback follows the explicit user order,
**And** unchanged album playback still follows disc/track order. The session's album-versus-manual mode and gain policy are resolved explicitly before implementation rather than silently applying album assumptions to mixed material.

7. **Given** a preview is active with a preserved main queue,
**When** the user edits upcoming main-session occurrences,
**Then** the edits apply to that preserved queue without replacing the audition or its saved current occurrence,
**And** returning from preview exposes the accepted edited queue.

8. **Given** a long listening history and queue,
**When** the destination is opened or scrolled,
**Then** history is paged and queue rendering is bounded or virtualized with stable occurrence identities,
**And** local loading, empty and mutation-error states remain accessible without resetting unrelated browser multi-selection. Manual-queue limits are documented separately from later Radio lookahead limits.

9. **Given** the UI closes or the application restarts after accepted edits,
**When** state is restored,
**Then** the accepted queue order and repeated occurrences are retained, with application relaunch paused,
**And** no remote playlist or device basket has changed as a side effect.

10. **Given** Windows, macOS and Linux builds,
**When** append/reorder/remove, repeated occurrences, edit-versus-advance races, preview editing, keyboard operations and long-history rendering are tested,
**Then** results verify occurrence identity, preserved current playback and bounded display behavior,
**And** the existing provider capability and source-identity rules remain intact.

## Tasks / Subtasks

- [ ] Extend the existing queue contract and read model (AC: 1–3, 5, 8).
  - [ ] Add strict occurrence-based remove/move operations to `playback.applySession`, typed frontend wrappers, limits, stable error codes and authoritative conflict data.
  - [ ] Add canonical main-current and album/manual queue policy projection independently of active Preview; support bounded upcoming/history reads and cursor validation.
  - [ ] Document the closed implementation decisions below in the daemon API contract; retain command deduplication and decimal-string revisions.
- [ ] Implement atomic queue editing and migration in the serialized owner (AC: 1–3, 5–7, 9).
  - [ ] Validate target/anchor eligibility against the latest main current, preserve repeated source occurrences and enforce current-plus-upcoming capacity.
  - [ ] Persist edited order, revision, album/manual transition and frozen current gain atomically; preserve historical outcomes and current identity/position.
  - [ ] Keep empty-main append paused; make preview edits preserve audition and handle the empty-main baseline explicitly.
  - [ ] Exercise rollback, migration, receipt retry, shutdown admission and paused restoration.
- [ ] Make successor invalidation safe before audio emission (AC: 2, 4–7).
  - [ ] Introduce or extend a successor-only preparation fence; revoke pending/ready/staged successor material without stopping the current stream.
  - [ ] Serialize boundary authorization with edit admission; reconcile already presented boundaries before deciding whether a target is upcoming.
  - [ ] Preserve existing buffer bounds, output-loss behavior, unsupported-boundary fallback and callback constraints.
- [ ] Add library queue actions using existing track selection (AC: 1, 3, 8).
  - [ ] Provide per-track Add to queue in grid/list and Tracks surfaces plus selected-track bulk Add in existing selection bars.
  - [ ] Capture ordered portable sources before awaits; keep queue actions independent of basket and playlist-write capability.
  - [ ] Retain browser selection and expose pending/success/conflict/limit errors through localized accessible status; narrow the existing PREVIEW_ACTIVE copy that currently tells users to return before editing the main queue.
- [ ] Make the Playback destination editable and bounded (AC: 2–3, 5, 7–8).
  - [ ] Show current, upcoming and paged history with occurrence identity, metadata and source labels; keep Preview separate.
  - [ ] Provide keyboard-operable Move up/Move down/Remove, including page-boundary movement; preserve focus after accepted edits and conflicts.
  - [ ] Retain the shared poll store, request fencing, metadata cache, stable paging controls, Retry and Browse library navigation.
- [ ] Validate behavior and document evidence (AC: 1–10).
  - [ ] Add owner/persistence/audio race tests and production-component UI tests described below.
  - [ ] Update API contracts and installed-test checklist; run focused checks, shared regressions and available platform checks.
  - [ ] Record actual OS/architecture/commit results and leave unavailable hardware/platform evidence explicitly pending.

## Dev Notes

### Closed implementation decisions

These are story-level design decisions completing the epic's implementation gate, not claims that the baseline already implements them.

**1. Ordered explicit track admission.** A single-track action submits one frozen `{serverId, trackId}`. Bulk Add captures selected Audio rows in the current displayed data order, before the first await; use the existing ordered selection resolvers, not Set insertion/click order. Preserve duplicates supplied in the payload and allocate a new UUID occurrence for each. Do not expand artists/albums/genres/playlists silently, route through basket conversion, or deduplicate by source/recording. Mixed/container selection cannot invoke the track-only bulk action and receives an explanatory disabled state. Preserve existing album Play as its own command. A new explicit Add of the same selection is a new command; a transport retry of the same command must not add again.

**2. Reuse the mutation envelope.** Extend `SessionOperation` under authenticated `playback.applySession` rather than adding a second mutation owner:

```ts
type QueueEditOperation =
  | { type: 'appendQueue'; sources: TrackSource[] }
  | { type: 'removeUpcoming'; occurrenceIds: string[] }
  | { type: 'moveUpcoming'; occurrenceId: string; beforeOccurrenceId: string | null };
// Existing envelope:
// { schemaVersion: 1, instanceId, sessionId, commandId, expectedQueueRevision, operation }
```

Move means remove that occurrence from its current upcoming position, then insert immediately before the identified upcoming anchor; null moves to the end. It never means a page index or source ID. Remove is atomic for the whole batch; duplicate IDs, absent IDs, current/history targets, or invalid anchors reject the entire request. Treat a move before itself, an already-adjacent move, or an already-last move-to-end as a successful no-op after identity/revision/eligibility validation. Empty append is a compatible no-op; empty removal is invalid. No-ops do not change queue policy/revision or audio state. Receipts remain bounded and keyed by complete payload; command-ID reuse with different payload fails. Keep retained receipt recognition before stale revision validation, and use `currentMetadata` on replay rather than regressing the UI to an old result revision.

Use existing conflict/error plumbing; add stable `OCCURRENCE_NOT_UPCOMING` and `QUEUE_LIMIT_EXCEEDED` codes if no equivalent exists. Existing mutation conflicts use 409 with flat authoritative metadata; occurrence-description conflicts use -7 with `data.code = QUEUE_CONFLICT` and a nested authoritative snapshot. Recognize both shapes during refresh rather than matching one HTTP/JSON-RPC code only. Conflict data carries authoritative instance/session/queue revision/state sequence/generation; the UI immediately refreshes `getSession` and affected pages. It announces that the queue changed and never blindly retries with a new revision. A user may deliberately submit a new action after refresh. Offline source entries remain valid local identities with unavailable metadata; append does not require remote playlist permission or live provider enumeration. Validate nonempty source strings and the existing 1024 UTF-8 byte identity limit.

**3. Upcoming and revision semantics.** Upcoming means ordinal strictly after the canonical main current occurrence, irrespective of playing/paused/stopped/completed state. Historical rows before current and current itself cannot be removed/moved here, even when their outcome is NULL. During Preview use the preserved main cursor, not audition `snapshot.current`. Transport, historical rejection and clear/replace retain their existing separate meanings.

Persist one queue revision increment for each successful structural edit, atomically with order/policy changes. Progress never increments it. Existing ordinary/presented-boundary advancement changes current/state sequence while preserving queue revision; retain that documented contract and independently recheck both target and anchor against the latest current at execution. A same-revision edit can therefore still fail eligibility after an advance. Compare decimal strings via BigInt on the UI; enforce checked database integer arithmetic. Concurrent structural edits with the same expected revision cannot both win. Do not use `stateSequence` as the expected edit revision.

**4. Explicit manual bounds.** Set `MAX_MANUAL_ACTIVE_OCCURRENCES = 10_000`, counting current plus upcoming only, not prior history or the separate audition. This aligns with the existing album admission ceiling without allowing accumulated history to block new listening. Retain `MAX_INSERT_BATCH = 200`; remove accepts at most 200 distinct IDs; move affects one occurrence. Reject an oversized/over-capacity batch atomically with no truncation or silent multi-request splitting. The bulk UI explains when more than 200 selected tracks must be narrowed. Retain read pages of 100 by default, at most 200; mount/cache at most one 200-row page per displayed region, with a fixed number of regions (current, upcoming, history), and evict old metadata rather than accumulating every page. Source metadata resolution retains its process-wide concurrency maximum of eight. These are manual queue/display bounds, not later Radio lookahead/candidate/history-retention policy. No history compaction/deletion is introduced.

**5. Empty and completed sessions.** First append to an empty main queue selects the first inserted occurrence at zero in Paused with no audio start, preserving the existing append behavior and making explicit Resume usable. Appending to a nonempty paused/stopped/completed session preserves its current and cursor; never auto-advance or resume. Explicit Next must be usable after appending a successor to a completed final current. The existing terminal writer rejects an already-consumed naturalCompletion: add an atomic cursor-only departure path for a current whose terminal outcome is already final, preserving its recorded outcome rather than rewriting it as a skip or freezing PERSISTENCE_FAILED. Resume replay remains its separate intentional behavior. Recompute authoritative Next availability after edits, including removing the final successor or appending after the previous end; the UI/native surfaces must not retain stale capability flags.

**6. Album-to-manual policy.** Add `queueKind: 'album' | 'manual'` orthogonal to existing `mode: 'main' | 'preview'`. Successful non-no-op manual append/remove/move converts the main queue to manual. Preserve current occurrence's exact frozen gain and qualified representation by occurrence ID, including when main is suspended for Preview. Future manual occurrences use unity gain (1.0), not album gain or future Radio normalization. Persist the current policy through pause/resume, seek, retry, preview return and relaunch. A new current occurrence in manual mode uses unity; a later explicit album Play establishes a new album policy. Unedited albums retain admitted disc/track order and frozen relative loudness. Failed/no-op edits cannot change policy. Update the existing API documentation statement that original album members retain policy after append: this story intentionally replaces that rule for future occurrences after any manual edit; do not leave contradictory contracts.

`FrozenAlbumContext` currently indexes gain/representation by ordinal and verifies an ordered membership digest on restoration. Merely retaining it after a reorder, or merely clearing it while losing the current policy, is incorrect. Store an explicit current-occurrence policy independently, clear obsolete future album membership atomically, and migrate persistence schema v4 to the next version. Derive legacy queue kind from validated existing album context, preserve legacy gain on the current occurrence, and retain corrupt/future-version restoration failures rather than overwriting storage. Keep source URLs, credentials and private gain internals off the public DTO.

**7. Preview-safe nontransport edits.** A dedicated manual-edit path must preserve active audition ID, source, position, transport intent, output gate, generation/control epoch, gain and outcome bookkeeping. Nonempty main edits preserve its saved current, cursor and intent. Existing `apply_inner` rejects append during Preview, and its generic tail derives the output gate from suspended main state; removing the guard alone can stop the audition. Update only the preserved queue/policy transaction, then publish the new queue revision.

Exception for Preview begun with no main: first append atomically establishes the first occurrence as the new paused main baseline at zero in both persisted session and audition saved-main fields; audition continues unchanged and returning/natural completion exposes that main paused. This initializes an absent saved current, never replaces an existing one. Subsequent edits use normal upcoming eligibility. `finish_preview` currently copies saved fields back unconditionally, so both records must agree. Restart dismisses the audition as interrupted exactly once and restores the edited main paused. Persistence failure leaves queue, preview and saved baseline unchanged.

**8. Read model and accessible editing.** Add a canonical `mainCurrent: Occurrence | null` snapshot field (active `current` remains audition during Preview). Extend `listOccurrences` with optional `section: 'all' | 'upcoming' | 'history'`, default `all` for compatibility. Use indexed ordinal queries, not loading every occurrence. Current is displayed separately; history is strictly before main current, upcoming strictly after it. Section cursors bind section, session, revision and main-current anchor; a changed anchor invalidates a scoped cursor even when queue revision is unchanged. Scoped requests, including the first page, carry `expectedMainOccurrenceId: string | null`; validate it under the owner with the queue revision. Responses echo `section` and `mainCurrentOccurrenceId`, and the UI discards a response if either differs from its latest snapshot/request. Preserve `totalOccurrenceCount` as the whole-session count and add `sectionCount` for the scoped count. Return authoritative cursor errors and refresh on main-current changes. Keep legacy all-page semantics. Update every snapshot projection, including `commit_terminal` responses that currently build a predecessor snapshot and patch `current` afterward: `mainCurrent`, current gain/qualified suffix and successor policy must all describe the newly committed occurrence immediately, not only after the next poll. This avoids paging through the entire history to reach what plays next.

Each scoped page supplies the immediately preceding upcoming occurrence ID and up to two following upcoming occurrence IDs, with an explicit end-of-section indicator. These are bounded edge neighbors, not extra mounted rows. Move down on the final visible row needs the second following occurrence as its insertion anchor; distinguish that from end-of-queue. Never infer neighbors from only the visible page or issue an unbounded fetch. Support a mutually exclusive `aroundOccurrenceId` page locator in place of a cursor, validated against the same session/revision/main anchor, that uses the occurrence index to return a bounded page containing that occurrence. After move, reload around the moved ID; after removal use a captured next surviving occurrence, then previous surviving occurrence, falling back to the region heading. This allows deep-page focus recovery without walking all earlier pages. Reject locators outside the requested region; if an advance invalidates the chosen survivor, refresh and use the accessible current-region fallback without replaying the edit. Move up inserts before the preceding upcoming occurrence; Move down inserts before the occurrence after the next one, or at end. UI metadata is joined to occurrence records, not used instead of them. Key interactive rows by occurrence ID. Keep focus on the moved row/action; after removal focus the next surviving row, then preceding row or region heading when empty. Expose pending state, accessible names and localized polite success/conflict/error status without recreating live regions on progress. Native button controls provide the required keyboard alternative; drag-and-drop is optional. Do not add listbox roles to rows containing buttons without implementing a valid composite pattern.

### Successor and persistence invariants

- **Fence before emission.** Baseline successor preparation checks active generation/control epoch, but owner queue-revision validation in `adopt_presented_handoff` happens after presentation. That alone cannot stop obsolete audio. Use a successor-only preparation/revocation epoch or equivalent bounded authorization state checked after fetch/decode, at readiness publication and before successor PCM consumption/submission. Queue edits must not change active `generation_serial` merely to invalidate a successor: that stops the current pipeline. Active and successor decoder/preparation currently share the pipeline cancellation Arc; give successor network/decode work its own revocation token while still observing parent shutdown cancellation. Dropping/replacing a successor must not cancel the active decoder through its destructor/join path. Drain or replace the bounded `sync_channel(1)` result and prepared PCM without allowing old results to republish.
- **Order edit admission against boundary commitment.** Reconcile backend-presented/submitted boundary state before checking upcoming eligibility. If successor presentation already won, adopt the audible current and reject edits targeting it. Otherwise revoke the old successor before committing the accepted edit and prevent a late worker from resurrecting it. A submission that can no longer be safely revoked must be resolved by the existing backend acknowledgment protocol before acceptance; defer/reject admission with a bounded retryable busy result rather than promise removal of already irrevocable audio. A generic post-presentation conflict is insufficient. Preserve current audio/cursor and existing platform-specific cancellation guarantees; never claim rollback of sound already heard.
- Reprepare only the latest accepted successor. Preserve active metadata, seek state, position, output identity and native transport. Edits while loading/paused/buffering/output-unavailable cannot accidentally open the output gate. Failure to prepare the new successor uses existing visible buffering/retry behavior; it never falls back to a removed entry. Unsupported boundaries remain honest fallback, not newly continuity-certified.
- Retain at most two decoder/source slots: active plus one successor; 8 MiB compressed per slot, aggregate 16 MiB; each PCM slot `min(500 ms, 1 MiB)`, aggregate 2 MiB. Retired workers/buffers count until released; cancellation must not transiently accumulate replacements. No network, decode, persistence, allocation or blocking synchronization in the output callback.
- Do not implement edits with `persist_playback_structure`: its delete/reinsert path is for replacement and loses occurrence outcomes. Add transactional in-place remove/reorder preserving current identity/ordinal, past rows, dispositions/failure codes, retained occurrence IDs and source identity. Renumber only affected upcoming rows with collision-safe temporary ordinals or an equivalent strategy respecting `(session_id, ordinal)` uniqueness and checked integer bounds.
- Commit order, queue revision, main policy, necessary audition baseline and checkpoint sequence together before acknowledging success. Rollback must leave the previous authoritative state coherent. Gate persistence during pending terminal recovery and committed shutdown with existing admission rules. Do not hold owner/DB locks over provider or backend waits. If successor work was revoked before a failed transaction, reprepare the unchanged authoritative successor; do not restart current audio.
- Queue edits never invoke provider playlist writes, preference/reporting APIs, basket mutations, physical manifests, sync or source-server selection. Preserve source IDs independently of the server currently being browsed.

### Current implementation map

| File | Current behavior | Required change and preservation |
|---|---|---|
| `hifimule-daemon/src/playback/model.rs` | Strict apply DTO, append/replace/select/clear/play operations; occurrence IDs and ordinals; snapshot active Preview projection; ordinal-based frozen album context. | Add remove/move, bounds, canonical main-current/queue kind and scoped paging DTOs; preserve camelCase field renaming, strict unknown-field rejection and existing Main/Preview contract. |
| `hifimule-daemon/src/playback/session.rs` | Serialized owner, bounded receipts, generation/control fencing, append, advancement, preview checkpoint/return, gain snapshots and terminal recovery. | Add nontransport manual edits, eligibility, independent current policy and successor invalidation coordination. Preserve active audio and preview; do not copy generic replacement side effects. |
| `hifimule-daemon/src/playback/persistence.rs` | Schema v4, indexed occurrence order, session/preview/outcome transactions and bounded reads; full replacement helper recreates rows. | Add schema migration, atomic in-place editing, current-policy persistence and scoped indexed reads. Preserve historical dispositions, rollback and corruption detection. |
| `hifimule-daemon/src/playback/audio.rs`, `audio/pulse_output.rs`, `audio/handoff.rs`, `continuity.rs` | Active/successor bounded decode pipeline and handoff tokens; current queue check at owner adoption is too late for stale PCM. | Add successor-only revocation/readiness/consumption fences and reconciliation with edit admission. Keep active generation/output epoch and buffer bounds. |
| `hifimule-daemon/src/playback/output.rs` and affected native callback paths | Callback-safe gates and platform presentation/acknowledgment evidence. | Extend only where needed to authorize/revoke successor consumption; preserve backend-specific acknowledgment safety and current samples. Linux uses the separate `audio/pulse_output.rs` output loop: fence both startup-primer and steady-state writes, including the gap between scratch-PCM rendering and submission, as well as the non-Linux path; inspect any additional native worker paths changed. |
| `hifimule-daemon/src/playback/commands.rs` | One command service dispatches owner-admitted audio effects outside owner locks. | Dispatch refreshed successor work if needed; never treat a queue edit as Play/Stop/seek. |
| `hifimule-daemon/src/rpc.rs` | Authenticated apply/list/describe handlers; shared eight-request metadata limiter; structured errors. | Wire additive paging/edit DTOs and contract tests; preserve privacy, lifecycle admission and source routing. |
| `hifimule-ui/src/rpc.ts` | Typed source/snapshot/page DTOs and structured RpcError; Play replaces session. | Add exact edit/read wrappers and preserve captured command envelope/error metadata. Do not implement Add using Play. |
| `hifimule-ui/src/state/playback.ts` | Shared 500 ms poll, BigInt state ordering, unchanged-snapshot heartbeat. | Refresh/accept authoritative mutation results without a second state owner or poller; preserve output discovery heartbeat. |
| `hifimule-ui/src/components/PlaybackDestination.ts` | Read-only 100-row page, separate audition banner, page request fencing, retry, bounded metadata reuse and mounted paging controls; rows currently rebuilt. | Add current/upcoming/history actions and keyed focus-preserving rows; retain page cache, explicit equal-snapshot recovery, labels and Browse library path. |
| `hifimule-ui/src/library.ts` | Display-order multi-selection includes containers and Audio; virtualized list plus card grid. | Add Audio-only bulk/per-track queue action, capture source/order before await; preserve browser selection and 56px/overscan patterns. |
| `hifimule-ui/src/components/TracksBrowseView.ts` | Track selection resolves in displayed item order; per-track and bulk basket actions. | Add separate queue actions, preserve Cmd/Ctrl/Shift selection and request lifetimes; never inherit basket dedupe/guards. |
| `hifimule-ui/src/components/MediaCard.ts` | Audio grid Play/Preview; grid selection means basket membership. | Add independent single-track queue action with propagation stopped; do not invent grid batch-selection semantics. |
| `hifimule-ui/src/styles.css`, `hifimule-i18n/catalog.json` | Record Crate design tokens and four-locale shared catalog. | Add focused responsive queue states and complete locale key parity; update obsolete read-only/Preview-blocked copy. Wrap bulk bars for translated labels without changing 56px virtual-row geometry; preserve one outer Playback scroller, authoritative hidden rules, forced-color focus and reduced-motion styles. |
| `scripts/tests/destination-ui.test.mjs`, `scripts/tests/playback-ui.test.mjs` | Production TypeScript DOM harnesses covering store/destination/control behavior. | Extend behavior coverage and keep prior cache/race/retry/focus fixes green. Strengthen the destination harness removal/replaceChildren behavior to clear detached descendant focus, matching the playback harness; assert focused nodes remain connected. |
| `docs/api-contracts-hifimule-daemon.md`, `docs/playback-installed-test-checklist.md` | Versioned playback/reconnect/continuity contracts and evidence checklist. | Record exact queue edit/policy/paging semantics and actual verification limitations. |

A focused new `TrackQueueButton.ts` can mirror `TrackPreviewButton.ts`; an extracted `PlaybackQueue.ts` may keep destination rendering manageable. Reuse existing selection and bounded-list utilities. `main.ts` already preserves the library DOM while showing Playback and normally needs no change. No new provider adapter, playlist resolver, browser audio engine or physical-device state is needed. Read any additional implementation file fully before modifying it.

### Testing requirements

1. **Ordered append and identity:** multiple servers, same track repeated within/across batches, display order distinct from selection click order; fresh UUIDs; exact command retry does not duplicate; changed-payload ID reuse rejects. Idle append stays silent and explicit Resume works; nonempty current position/source/gain stays unchanged.
2. **Eligibility and races:** remove one duplicate only; move before/end/self/adjacent; page-edge move (page size one, with two off-page successors); reject current/history/missing target or anchor and duplicate remove IDs atomically. Competing revisions, progress-only success, same-revision advance race, preview start/return during admission and command receipt replay with newer metadata.
3. **Persistence:** repeated occurrence order after reconnect/relaunch paused; no changed historical outcomes/failure codes; rollback injection at every edit/policy/audition write; v4 migration with and without album context, future/corrupt schema, checked overflow and no-op revision stability. Pending terminal recovery/shutdown reject edits coherently.
4. **Audio:** deterministic barriers for delayed provider/decode, ready successor and callback submission; removed/reordered old successor emits no new samples after an accepted edit. Boundary-winning successor is adopted and cannot be removed. Assert current generation/output epoch/position continuity and unchanged active samples. Successive edits/failed commit do not accumulate retired buffers or stop audio. Exercise paused/loading/output-loss cases and preserve unsupported-boundary fallback.
5. **Gain:** edit an album while main is audible and while Preview is audible; current main policy survives return, pause/resume, seek, retry and restart; future manual gain is unity; unchanged album remains ordered and uses frozen album gain. Failed/no-op edit changes nothing. No restored membership-digest corruption after reorder.
6. **Preview:** nonempty preserved main append/move/remove leave audition ID/state/output untouched; empty-main first append updates both saved baseline records and returns paused; rollback leaves no half-created main; repeat Preview, explicit Return, natural completion, Stop, output loss and relaunch preserve accepted queue semantics.
7. **Bounds:** 200 vs 201 batch/metadata, 10,000 active capacity and over-limit rollback, history larger than capacity still permits append, scoped stale cursors and cursorless first-page races on advance/edit, correct whole-session versus scoped counts, bounded DOM/cache across many pages, global metadata concurrency ≤8 and unchanged progress does not refetch metadata. Removing last successor updates Next immediately; append after final natural completion then explicit Next advances without overwriting its outcome or producing PERSISTENCE_FAILED.
8. **UI/accessibility:** execute production components, not just source-regex tests. Verify per-row/bulk/grid add, captured source during server switch, mixed-container disabled action, no-device and read-only-provider queue admission, loading/error/empty/limit status, keyboard moves/focus after page-edge and deep-page relocation/removal without sequentially fetching intervening pages, no blind conflict replay, unchanged browser selection, request fencing/disposal, source-label retry and equal-snapshot retry. Spy on RPC to prove zero remote playlist/basket/sync calls. Reuse daemon `FakePlaylistProvider` mutation logs, `FakeBrowseProvider` with playlist writes disabled, and the existing authenticated production-router applySession test pattern; exercise actual serialized new DTOs, not only direct owner methods.
9. **Cross-platform evidence:** Windows/macOS/Linux append/move/remove, repeats, boundary races, Preview editing, keyboard/screen-reader focus and long-history rendering. Review <600px, 600–1000px and >1000px layouts, final-row reachability, contrast and OS themes; use accessibility tooling alongside actual keyboard checks. Separate mock harness results from installed audio/hardware evidence and document unavailable runs.

Run focused tests first, then appropriate shared regressions:

```sh
rtk npm run build:daemon -- test -p hifimule-daemon playback::
rtk npm run build:daemon -- test -p hifimule-daemon rpc::
rtk proxy node --test scripts/tests/destination-ui.test.mjs scripts/tests/playback-ui.test.mjs
rtk npm --prefix hifimule-ui run build
rtk npm run build:daemon -- test -p hifimule-daemon
rtk proxy python3 -m unittest discover -s scripts/tests -p 'test_playback_installed_evidence.py'
rtk git diff --check
```

Use the controlled daemon build wrapper for native FFmpeg. Do not claim platform certification from compilation or mocks. Story preparation itself runs no implementation/runtime acceptance tests.

### Previous-story and git intelligence

The five latest commits are `36add0b Review 15.12`, `912af9f Dev 15.12`, `cf57ba9 fix(ui): restore library access from Playback`, `15b5896 fix(ui): repair Playback destination layout`, and `830e02d fix(tests): isolate credential test state`.

Story 15.12 fixed metadata refetch on progress, stale page completion, paused equal-snapshot recovery, lost paging focus, output-inventory heartbeat, delayed source labels and multiplied polling timers. Preserve these while making rows interactive. Keep the library accessible through the existing local Browse library callback, without changing the selected destination or rebuilding library DOM. Retain isolated credential fixtures in tests. Its recorded 1,000 daemon tests/6 ignored, 48 UI tests and 33 evidence-validator tests are historical results, not verification of 15.13; deferred R16 and missing installed evidence remain limitations.

### Architecture, libraries and external technical context

Stay within Rust daemon + SQLite + authenticated JSON-RPC/Tauri + vanilla TypeScript/Shoelace. Repository pins override old planning examples: Rust edition 2024/MSRV 1.93.0, Tokio ~1.49, rusqlite ~0.38 (bundled SQLite), Tauri API ~2.10, TypeScript ~5.6.2, Vite ^6.0.3, Shoelace ^2.19.1; existing audio pins CPAL 0.18.2 (local patch), ffmpeg-next/sys 9.0.0, crossbeam-queue 0.3.12 and Souvlaki 0.8.3 (local patch). These are manifest requirements, not claims about latest upstream releases. No dependency upgrade or new queue/drag library is required.

Technical checks on 2026-09-19 focused on the APIs this story needs. The [rusqlite release list](https://github.com/rusqlite/rusqlite/releases) lists 0.40.1 as latest, including a fix for tainted SAVEPOINT names and a bundled SQLite update. Keep the repository 0.38 line for this scoped work; use fixed internal transaction/savepoint names and bound parameters, never source/occurrence strings as SQL identifiers. Do not silently upgrade the workspace. [Shoelace releases](https://github.com/shoelace-style/shoelace/releases) list 2.20.1 as latest in the archived repository, including focus/accessibility fixes; retain the existing installed components rather than migrating to Web Awesome. [SQLite transaction documentation](https://www.sqlite.org/lang_transaction.html) supports atomic editing and notes commit failure/busy behavior; acknowledgment must follow successful commit. [W3C rearrangeable-list guidance](https://www.w3.org/WAI/ARIA/apg/patterns/listbox/examples/listbox-rearrangeable/) illustrates keyboard action buttons, focus retention and live confirmation, and requires assistive-technology testing rather than copying an example as production proof. Prefer semantic HTML with existing buttons; no ARIA/framework migration is needed. The pinned rusqlite documentation page could not be retrieved during research; verify exact API signatures against installed 0.38 source rather than assuming newer APIs.

### Project Structure Notes

Use Rust/SQL snake_case and JSON/TypeScript camelCase, including enum variant fields. Keep source-server routing separate from browse-server/destination selection. `DESIGN.md` and existing CSS tokens govern visual design; the older UX document supplies selection, responsive and accessibility patterns. Playback amendments supersede its physical-device-only add restrictions for local listening only. The project-context greenfield label is stale: this is a brownfield extension with existing sync/playback behavior to preserve.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 15, Story 15.13; Playback UX requirements; Stories 15.14–15.29 scope boundaries]
- [Source: `_bmad-output/planning-artifacts/prd.md` — Listening destinations and controls; Radio and selection FR68; History, preferences and curation FR78; Playback quality requirements]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Playback State and Ownership; Playback UI and Session Control; Playback Implementation Contracts; Playback Project Structure; Playback Validation Refinements]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md` — §§5.1–5.2 selection/list patterns; §6 responsive/accessibility]
- [Source: `_bmad-output/planning-artifacts/project-context.md`; `_bmad-output/implementation-artifacts/epic-15-context.md`]
- [Source: `_bmad-output/implementation-artifacts/15-12-access-playback-as-an-always-available-destination.md` — Review Findings R6–R11/R15/R16; Review Patch Completion; Dev Notes]
- [Source: `docs/api-contracts-hifimule-daemon.md` — Playback session reconnect contract; Full-track preview overlay; Prepared album continuity; Ordered album playback]
- [Source: `hifimule-daemon/src/playback/model.rs`; `session.rs`; `persistence.rs`; `audio.rs`; `audio/handoff.rs`; `continuity.rs`; `commands.rs`; `output.rs`]
- [Source: `hifimule-ui/src/components/PlaybackDestination.ts`; `TracksBrowseView.ts`; `MediaCard.ts`; `TrackPreviewButton.ts`; `hifimule-ui/src/library.ts`; `state/playback.ts`; `rpc.ts`]
- [Source: `DESIGN.md`; `Cargo.toml`; `hifimule-ui/package.json`; `scripts/tests/destination-ui.test.mjs`; `scripts/tests/playback-ui.test.mjs`]

## Dev Agent Record

### Agent Model Used

GPT-6 (story preparation).

### Debug Log References

- Prepared against `36add0b4801503d8abdaf124bb0c015ea859e0e7` using planning, backend and UI research; no implementation changes or runtime acceptance claims.
- Validated against the create-story checklist; applied backend/UI review corrections. Confirmed all ten epic acceptance criteria are preserved verbatim, story/sprint status agree, template placeholders are absent and whitespace checks pass.

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- Closed selection ordering, operation schemas, queue limits, revision behavior, album/manual gain and preview-edit decisions.
- Retained the predecessor's deferred physical-basket guard and unavailable installed-platform evidence as known limitations.

### File List

- `_bmad-output/implementation-artifacts/15-13-edit-the-upcoming-listening-queue.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
