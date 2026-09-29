---
baseline_commit: 7be8cb52458212e5071fed31632aa787bbda7171
---

# Story 16.9: Save an immutable local listening snapshot

Status: done

## Story

As a HifiMule user,
I want to save the accepted history and upcoming queue from my listening session locally,
so that I can keep what I discovered without ongoing playback changing the saved selection.

Requirements: FR78; local cross-server order portion of FR79; P-NFR2 and P-NFR4–6; snapshot portion of P-AR11; P-UX-DR15.

Dependencies: Epic 15 playback foundation and Stories 16.1–16.8. The current checkout includes 16.8 and its review fixes. Story 15.17 remains `in-progress` in sprint tracking: its installed-release evidence is not complete, and this story must not imply otherwise. This feature applies to manual, album, and Radio main sessions.

## Acceptance Criteria

1. **Given** accepted played occurrences, a main current occurrence, and upcoming entries, **when** the user explicitly saves, **then** the daemon persists accepted played occurrences in order, the current occurrence once unless rejected, and upcoming occurrences in accepted order. Explicitly skipped/disliked occurrences are omitted. Inclusion follows the precise policy below, including the history/current boundary.
2. **Given** deliberately repeated tracks or matching recordings from different sources, **when** saving, **then** distinct occurrence IDs remain distinct and ordered. Neither source-track nor recording-level deduplication may collapse them.
3. **Given** concurrent playback advancement, feedback, or queue edits, **when** capture is admitted, **then** one owner/SQLite boundary determines the entire sequence and local disposition. Subsequent changes never mutate it. Repeating the same successful save operation returns the same snapshot under the retention and request-identity contract below.
4. **Given** Preview over a preserved main session, **when** saving, **then** capture that main session's history/current/upcoming, excluding all audition occurrences and outcomes. The UI identifies the main session as the target. Preview without a main selection returns an explained empty result.
5. **Given** mixed sources, **when** saving or inspecting, **then** global order, occurrence IDs, and the chosen portable server ID/provider track ID remain intact. No playlist capability, online provider, selected browse server, physical device, or replacement source copy is required.
6. **Given** incomplete listening, technical failure, or unresolved reporting/feedback, **when** evaluating inclusion, **then** apply local occurrence acceptance independently of remote success. Technical failure alone is not rejection. Explicit local Dislike remains rejection even when its remote journal/write failed; Like clears only that occurrence's dislike, and neutral does not.
7. **Given** a committed snapshot, **when** inspecting it after playback changes, session replacement, restart, source outage, or source removal, **then** the same ordered selection is available with stable source labels and metadata fallbacks. Snapshot lists and entries use bounded pages; the UI does not accumulate the complete selection. Saved inspection remains reachable with zero configured servers.
8. **Given** an empty eligible selection or a persistence error, **when** saving finishes, **then** explain that nothing was saved or expose a recoverable error. Never expose a partial snapshot as complete, damage an earlier save, or claim failure merely because the success response was lost.
9. **Given** Windows, macOS, and Linux builds, **when** validating boundary races, repeats, skips/dislikes, Preview, mixed/unavailable sources, and interrupted persistence, **then** assert exact saved sequences and recovery. Saving and inspection issue no provider writes and no basket/device changes; playback ownership, controls, and restoration remain intact. Record unavailable platform evidence explicitly.

## Tasks / Subtasks

- [x] Implement the local snapshot contract and migration (AC: 1–8).
  - [x] Add `playback/export.rs` with typed save/read contracts, validation, inclusion policy, and persistence helpers; register it in `playback/mod.rs`. Keep remote export implementations out of this story.
  - [x] Extend `Database::init_playback_inner` in `playback/persistence.rs` from current playback schema v13 to v14, or the next unclaimed version if the baseline has moved. Add independent snapshot headers/entries and indexes in the existing migration transaction. Preserve v1–v13 upgrade/rollback behavior and unsupported-future-version rejection.
  - [x] Materialize the frozen sequence and source/display fallbacks atomically, with committed-operation uniqueness and bounded page reads. Save rows must survive deletion/replacement of live session rows and server records.
- [x] Integrate capture with the session owner and RPC boundary (AC: 1–6, 8–9).
  - [x] Add a serialized owner save command, optionally split into `playback/session/export.rs`. Resolve the canonical main session, validate the observed baseline, reject pending terminal-persistence/restoration errors, and retain the mutation guard through commit/rollback.
  - [x] Evaluate retained occurrences, main attempts, current/upcoming membership, and `playback_feedback_dispositions` within the same transaction. Implement the explicit ordering/rejection rules below; never construct saves from live UI pages or reporting rows.
  - [x] Add the four RPC methods below, preferably through `rpc/playback_export.rs`, with mutation classification only for save. Reuse authenticated local JSON-RPC and the existing Tauri proxy.
  - [x] Verify saved-operation recovery before live-session freshness checks, reject operation-ID reuse with changed intent, and keep later queue/disposition changes from affecting committed content.
- [x] Add local save and read-only inspection to Playback (AC: 4–8).
  - [x] Compose a focused `PlaybackSnapshots.ts` component into `PlaybackDestination.ts` using existing queue row/source badge styling. Provide naming, save state, saved-list access, bounded entry navigation, and a return to the live queue.
  - [x] Preserve one unresolved save request's operation ID and exact payload outside the disposable component, including navigation/reload/reopen, and reconcile it before another save. Bind saved-page requests to snapshot ID and component request generation, independently of live playback polling.
  - [x] Keep local snapshots reachable after the last source is removed, including after restart; narrowly adjust `main.ts` first-run routing while preserving onboarding for a genuinely empty installation.
  - [x] Add English, French, Spanish, and German labels, keyboard navigation, visible focus, polite result/error announcements, and explicit Preview/main-session and unavailable-source explanations.
- [x] Prove persistence, sequencing, UI, and resource behavior (AC: 1–9).
  - [x] Add meaningful Rust inclusion/transaction/owner/RPC tests, file-backed restart and failure-injection tests, and Node UI tests with exact sequence assertions from the matrix below.
  - [x] Measure capture duration, DB/owner lock time, queued control latency, and memory for short and long histories; record the result rather than claiming that pagination or `spawn_blocking` makes capture free.
  - [x] Run the focused regression/build checks and affected platform checks. Update API/data-model docs with policy, schema, paging, retention, errors, and measured limitations; correct adjacent stale Retry/attempt and queue-kind statements. Leave unavailable installed checks identified as gaps.

### Review Findings

- [x] [Review][Patch] Preserve the active Playback view when routing through an existing main layout (P2) [hifimule-ui/src/main.ts:453]
- [x] [Review][Patch] Keep saved inspection reachable when recovery storage is unavailable (P2) [hifimule-ui/src/localContentRoute.ts:8]
- [x] [Review][Patch] Restore paging cursor state after a saved-page request fails (P3) [hifimule-ui/src/components/PlaybackSnapshots.ts:163]

## Dev Notes

### Scope and architectural ownership

Story 16.9 produces a daemon-owned, durable, read-only local artifact. Story 16.10 will consume its stable snapshot ID for per-source server playlists; Story 16.11 will consume it for explicit physical basket Add/Replace. Do not implement either export now, reuse the basket's current-server playlist flow, automatically sync, start playback from a save, or create a live link to Radio. Rename/delete management and a general playlist editor are outside this story. Saving itself does not pause, seek, advance, report, alter feedback, change Radio exclusions, or increment the live queue revision.

Existing `SessionSnapshot` is a live transport DTO, not this durable artifact. Use distinct names such as `ListeningSnapshotSummary` and `ListeningSnapshotEntry`. Preserve the existing `TrackSource { server_id, track_id }` boundary; `server_id` is portable, not the machine-local server row/vault ID. Provider track IDs are opaque strings, including Audiobookshelf IDs. No provider call is necessary for save or inspection. Credentials, account secrets, and authenticated artwork/stream URLs must never enter saved rows, RPC results, or logs.

[Source: `epics.md` §§Stories 16.9–16.11; `architecture.md` §§Playback State and Ownership, Playback Implementation Contracts, Server Identity Model, Epic 17 amendment.]

### Closed implementation gate: inclusion and order

Use snapshot policy version **1**. These are local snapshot decisions, not changes to listening-report eligibility or Radio selection:

| Occurrence state at capture | Policy |
| --- | --- |
| Retained main occurrence with a closed listening attempt | Include in accepted history unless explicitly rejected. A partial listen, restart, Back departure, interruption, superseded attempt, or technical failure is not by itself rejection. |
| Current main occurrence, including paused/stopped/loading/unavailable or completed final current | Include once unless explicitly rejected; it need not have completed or produced a successful server report. |
| Retained upcoming occurrence | Include in accepted queue order unless explicitly rejected, even when never resolved or currently unavailable. |
| Any committed `explicitSkip` main attempt for that occurrence | Omit that occurrence, including if Back later revisits it. A deliberate new enqueue creates another occurrence and is evaluated independently. Like clears a dislike, not a committed skip. |
| Local feedback rejection for that exact occurrence | Omit while the rejection exists at capture. Explicit Like removes that rejection; neutral and feedback on another occurrence do not. Remote delivery state is irrelevant. |
| An earlier queue row jumped over without ever being entered/attempted | Do not invent a played listen from its ordinal. It is not accepted played history. |
| Removed/replaced live queue occurrence, even if its old attempt still exists | Omit from a new snapshot of the current retained selection. Already committed snapshots remain intact. |
| Temporary audition or audition outcome | Always omit. With no eligible main sequence, return the empty result. |

Define the sequence precisely:

1. Establish current/upcoming membership first. `H`: retained main occurrence rows before the main current ordinal having at least one closed main attempt, ordered by the first closed visit's `attempt_seq` (stable first-visit order, not the number of retries). Exclude the current and all upcoming rows from H even if previously heard: their accepted C/U positions take precedence after Back or reordering. For legacy occurrence terminal evidence, use the migrated legacy attempt; do not fabricate evidence for a row with neither an attempt nor an outcome.
2. `C`: the canonical main current occurrence, if any. During Preview this comes from the preserved main session, never `SessionSnapshot.current`, which points at the audition.
3. `U`: retained rows after the main current ordinal, in the accepted queue ordinal order. A valid session without a main current has an empty queue, so `U=[]` and saving returns `noMainSelection`. Nonempty retained rows without a main current are a state-integrity error, not an invented pending queue.
4. Concatenate `H`, `C`, `U`, omit the rejected occurrence IDs above, and retain only the **first appearance of each occurrence ID**. This boundary deduplication does not compare track IDs, recording keys, titles, or source copies. Assign fresh contiguous snapshot ordinals starting at zero.

Example: played `A1`, skipped `B1`, technically interrupted `T1`, current `C1`, upcoming `D1` plus a deliberate second occurrence `A2` of A's recording saves `[A1,T1,C1,D1,A2]`. If C1 already completed but remains final current, include C1 once. After Back revisits A1, previously heard B1/C1 belong to upcoming, not a second history copy. Without an edit the result is `[A1,B1,C1]`; reordering upcoming to C1/B1 must save `[A1,C1,B1]`. A disliked audition P does not reject a main occurrence with the same source track.

Do not select only `playback_occurrences.outcome='naturalCompletion'`: `outcome` preserves the **first** terminal result, while later attempts can explicitly skip the same occurrence. Do not use `playback_live_reports`, report success, or reporting `TerminalReason::Skip`; reporting maps some Back/restart departures to Skip although they are not explicit user rejection. Do not use source/recording-level Radio exclusion membership to reject a deliberate occurrence of the same track.

[Source: `playback/persistence.rs` §§persist_playback_terminal, start_playback_attempt, reset_playback_attempt, playback_attempts; `playback/session.rs` §§list_inner, canonical_main_current, live_terminal_reason; `playback/feedback.rs` §§record_feedback_disposition, rejected_playback_occurrences.]

### Closed implementation gate: atomicity and recovery

Capture is one serialized session-owner command followed by **one SQLite transaction** over the retained live rows, attempts, local rejection rows, and new snapshot rows. The actor ordering plus transaction is the disposition boundary. Queue revision alone is insufficient: accepted feedback does not increment either queue revision or state sequence, and a feedback journal insertion can fail after rejection was durably saved. A feedback sequence watermark therefore cannot certify acceptance.

For a new operation: validate schema/IDs/name and the observed instance, session, queue revision, and main current ID; reject an outdated baseline with a typed conflict so the UI refreshes before a new explicit save. Check `pending_terminal`, restoration errors, and shutdown admission. Do not persist an apparent pre-transition success while a terminal write is awaiting recovery. Read/copy content and labels in the transaction, insert header/entries, validate nonempty count, then commit. Only commit establishes success. Roll back header and entries together on any error. Do not increment the live queue revision or feed the resulting artifact into the live playback store.

Retain the current consistent baseline if a later owner command arrives while saving. An admitted later Dislike/Skip affects only future saves; one committed before capture affects this save. Playback advancement, source removal, queue edits, clear/replacement, and shutdown must serialize to before or after the capture, never split its rows. Acquire locks in the existing owner-to-DB order; do not call a helper that relocks `Database.conn` from inside the transaction. Reuse feedback's predicate through a transaction-aware helper or SQL join rather than opening a second read outside the boundary.

Use a UUID `operationId` supplied once by the caller and a separate daemon-generated UUID `snapshotId`. Store the operation's canonical request alongside the header or an equivalent exact intent representation. A matching committed operation is looked up **before** checking the current live instance/session/revision: retry after playback replacement or daemon restart returns the original saved result. A reused ID with a different canonical payload returns `SNAPSHOT_OPERATION_REUSED`, never a new save. Compare the requested optional name, not a newly generated timestamp-based default.

Successful operation deduplication lasts for the lifetime of the saved snapshot. This story performs no automatic snapshot expiration, pruning, rename, or deletion; explicit saves are durable user data. Thus success deduplication survives restart and generic playback command-cache eviction. Empty results and transactions proven rolled back do not retain a success identity; a subsequent explicit attempt may capture again after refreshing state. An uncertain transport response must first recover by `operationId` or repeat the exact request, not mint a fresh ID. Local database atomicity is not a promise about future remote exactly-once exports.

Keep at most one unresolved save request in a bounded local UI recovery record (for example, a versioned localStorage entry containing only the validated operation ID and canonical request), written before send. `main.ts` destroys PlaybackDestination when changing surfaces, so component memory is insufficient. Restore that record and reconcile on remount/reload/UI reopen, even when live playback is stale. Clear it only after a committed/empty result or a definite no-commit failure. A not-found lookup while the original save could still be running is not a terminal failure: retain/retry the same request. An actual new-daemon identity plus authoritative not-found can establish that the previous daemon left no committed save; explain the outcome before accepting a refreshed new operation. Never automatically resubmit a recovered request as a fresh operation ID.

### Closed implementation gate: storage and wire contracts

Keep snapshot storage independent from live-session cleanup and source configuration. Proposed tables:

- `playback_listening_snapshots`: monotonic local creation sequence; unique snapshot UUID; unique operation UUID; schema/policy version 1; canonical request; immutable name; UTC creation time; captured instance/session ID, optional Radio logical ID, queue revision, main current ID; entry count. Session/source provenance fields must not cascade-delete the save.
- `playback_listening_snapshot_entries`: snapshot UUID; zero-based ordinal; original occurrence UUID; portable server ID; provider track ID; inclusion origin (`history`, `current`, or `upcoming`); frozen source label/icon and optional local title/artist/album/duration metadata. Primary key `(snapshot_id, ordinal)`; unique `(snapshot_id, occurrence_id)`. No uniqueness on source-track or recording identity.

The exact SQL must include nonnegative ordinal/count checks, supported-version checks, a header-list index and indexed entry paging. Use `INSERT … SELECT`/window functions or equivalent bounded materialization; do not load the whole history into Rust or serialize a giant JSON array. Keep a saved row self-contained after its live occurrence/source disappears. Integrity tests must reject inconsistent counts, missing ordinals, and unsupported saved-schema versions instead of presenting corrupt content as complete.

Freeze source labels from local configured identity at capture, falling back to the portable ID if missing. Freeze already available safe metadata only; otherwise retain an explicit unknown-title/provider-track-ID fallback. Never fetch the entire library or wait for provider metadata to save. Optional current metadata must match the **main** occurrence; audition metadata cannot label the main track. Inspection reads the frozen display data. A separate current source-availability badge may change without changing saved content; it must not silently replace labels, order, or references.

Names: an optional trimmed name of 1–120 Unicode scalar values, with control characters rejected. The UI supplies a localized suggested “Listening snapshot” name plus date/time once, retaining that exact value with the save request. For direct callers omitting/leaving the name blank, the daemon persists the deterministic English fallback “Listening snapshot” plus its creation date/time once; the request does not carry the UI's localStorage locale. Duplicate display names are allowed and never overwrite another snapshot. The UUID/creation sequence distinguishes them.

All new RPC DTOs use schema version 1, camelCase, explicit nullability, bounded/strict parameter validation, and existing typed error conventions. Preserve the existing playback success `{data: ...}` envelope inside JSON-RPC `result`; the response column below describes `result.data`, which UI wrappers unwrap through `rpcCall`. Rust/SQL remain snake_case. IDs stay strings; SQLite 64-bit sequences, ordinals, counts, and revisions cross the wire as decimal strings. Creation time is an RFC 3339 UTC string; optional duration uses milliseconds with validated integer bounds.

| Method | Request | Response |
| --- | --- | --- |
| `playback.saveSnapshot` | `{schemaVersion:1, operationId, instanceId, sessionId, expectedQueueRevision, expectedMainOccurrenceId:string|null, name?:string}` | `{schemaVersion:1, status:'saved', snapshot:ListeningSnapshotSummary}` after commit, or `{schemaVersion:1, status:'empty', reason:'noMainSelection'|'noEligibleOccurrences'}` without a save |
| `playback.listSnapshots` | `{schemaVersion:1, cursor?:string, limit?:number}` | `{schemaVersion:1, snapshots:ListeningSnapshotSummary[], nextCursor:string|null}` newest first |
| `playback.getSnapshot` | `{schemaVersion:1, snapshotId?:string, operationId?:string}`; exactly one locator | Immutable summary, or a typed not-found result; works independently of live session identity |
| `playback.listSnapshotEntries` | `{schemaVersion:1, snapshotId, cursor?:string, limit?:number}` | `{schemaVersion:1, snapshotId, entries:ListeningSnapshotEntry[], nextCursor:string|null, totalCount:string}` in saved ordinal order |

`ListeningSnapshotSummary` exposes the immutable name, IDs, creation time, schema/policy version, source session/logical ID, queue revision, captured main ID and entry count; do not return a full entry array or the internal canonical-request record. Entry responses expose the frozen fields above. New reads require no provider, current queue revision, or current session match.

Paging: default 50, allowed 1–200. Use keyset cursors bound to schema/query kind and snapshot ID where applicable; reject malformed, overflowing, cross-snapshot or future-version cursors. New saves appearing above a list cursor must not duplicate/skip older results. Read `limit + 1` and discard the extra row when building the next cursor. The UI retains only its current page and a bounded navigation cursor stack; use existing bounded navigation patterns, not an ever-growing array of fetched rows.

Provide localized error codes distinguishing invalid request/name/cursor, stale instance/session/queue/main cursor, operation identity reuse, missing snapshot, storage/corruption/version failure, busy admission, pending-terminal recovery, and shutdown. Preserve existing playback error codes for equivalent conditions. Raw SQL errors and paths are daemon diagnostics, not user-facing messages.

### Resource and lifecycle guardrails

`Database` currently owns one `Arc<Mutex<rusqlite::Connection>>`; it is not a connection pool and does not establish WAL isolation for this feature. Playback schema migration lives in `playback/persistence.rs`, despite the architecture's broad assignment of migrations to `db.rs`. Follow the delivered implementation; do not introduce a second database or silently switch global journaling mode.

A SQL copy bounds application memory, but work still grows with history. Running that copy in `spawn_blocking` does not remove contention with the owner or shared connection. Start with the existing serialized transaction pattern and indexed SQL. Save/read RPC waiting belongs off Tokio executor threads, using existing blocking dispatch conventions; the audio callback must never perform persistence or acquire these locks. Admit at most one snapshot save in progress; reject excess saves as busy rather than queue unbounded work. Reads remain bounded and do not serialize the full artifact into the 500-ms session poll.

Measure short, 10,000-row, and 100,000-row captures, including many attempts per occurrence, unavailable sources, concurrent control commands, and existing sync DB activity. Record peak application memory, transaction/owner hold time, command latency, and any playback underrun against the same workload without saving. If the single-transaction implementation causes control timeouts or playback disruption, resolve persistence scheduling within this story before marking it complete; do not waive the issue or truncate accepted history. Any worker/chunking alternative must freeze a complete immutable capture boundary before allowing live rows to change. Repeated reads of changing tables with a high-water mark alone are insufficient.

The existing mutation/shutdown guard must cover durable save work even if the UI disconnects. Do not detach untracked blocking work or assume aborting its async waiter cancels a SQLite write. On process interruption, SQLite recovery leaves either the complete committed save or no save. Retry by operation identity resolves lost acknowledgements. Snapshot storage needs no automatic resume, source reconnection, or playback start during launch.

### Existing files: current state, changes, and preservation

Paths below are repository-relative. New modules are explicitly marked NEW; inspect actual code before changing an UPDATE file.

| File | Current state | Change / preserve |
| --- | --- | --- |
| `hifimule-daemon/src/playback/export.rs` (NEW) | No local saved-snapshot implementation exists. | Own contracts, bounded SQL persistence/reads and policy; leave server/basket export for later stories. |
| `hifimule-daemon/src/playback/mod.rs` (UPDATE) | Registers existing playback modules. | Register export without changing player ownership. |
| `hifimule-daemon/src/playback/persistence.rs` (UPDATE) | Schema v13, retained occurrences, immutable first outcomes, separate replay attempts, Radio, Preview, reporting and feedback tables. | Add snapshot schema/indexes in migration; preserve all existing rows, cleanup, failure recovery and paused restore. Never attach snapshot deletion to live cleanup triggers. |
| `hifimule-daemon/src/playback/session.rs` (UPDATE; optional NEW `session/export.rs`) | Serialized owner commands; main state survives Preview; `list_inner` partitions by current ordinal; Back creates attempts for existing occurrences. | Add capture admission/command integration; preserve terminal recovery, command fencing, generation IDs, queue revisions, current transport and Preview resume/Stop semantics. |
| `hifimule-daemon/src/playback/feedback.rs` and `session/feedback.rs` (REUSE; update only for a shared transaction-aware predicate) | Local rejection commits before remote journaling; Like removes only that occurrence; neutral is a no-op; logical-session cleanup is explicit. | Read the existing local rejection in capture. Do not change feedback/transport/report behavior or rely on a separate pre-transaction Vec of rejected IDs. |
| `hifimule-daemon/src/rpc.rs` (UPDATE; optional NEW `rpc/playback_export.rs`) | Authenticated dispatch, mutation classification and shutdown guards; blocking owner calls are forwarded off async work. | Add save and bounded reads; classify only save as mutation. Preserve all existing RPC envelopes and authentication. |
| `hifimule-daemon/src/playback/model.rs`, `db.rs` (REFERENCE; conditional UPDATE) | Existing occurrence/source identities and shared connection. | Reuse types/connection. Put new DTOs in export unless shared state truly needs an additive field; no unrelated schema or pool refactor. |
| `hifimule-ui/src/rpc.ts` (UPDATE) | Actual TypeScript playback DTOs and RPC wrappers; there is no separate bridge/types module. | Add typed snapshot contracts/wrappers, preserving caller operation ID, generic `rpcCall`, existing wire conventions and proxy behavior. |
| `hifimule-ui/src/components/PlaybackDestination.ts` (UPDATE), `PlaybackSnapshots.ts` (NEW) | Destination presents live paged history/upcoming, Radio/settings, keyed rows and request fences. | Compose save/list/read-only snapshot view. Preserve live queue mutations, focus, paging, disposal and no-device access. Do not add snapshot entries to live queue arrays. |
| `hifimule-ui/src/main.ts` (UPDATE) | Routes zero configured servers to first-run login; owns destination creation/destruction. | Distinguish existing saved local content from first-run emptiness. Preserve destination switching and physical basket flush behavior; save itself does not call basket mutation. |
| `hifimule-ui/src/state/playback.ts`, `hifimule-ui/src-tauri/src/lib.rs` (REFERENCE) | Shared live polling/order fencing and generic authenticated Tauri `rpc_proxy`. | Reuse without adding snapshot polling or a native command. Snapshot page identity is independent from live state sequence. |
| `hifimule-ui/src/styles.css`, `hifimule-i18n/catalog.json` (UPDATE) | Existing queue/source/focus/responsive styles; shared en/fr/es/de catalog. | Add minimal snapshot presentation and labels. Preserve parent scrolling, forced colors, hidden state and floating-bar clearance. |
| `scripts/tests/destination-ui.test.mjs` (UPDATE or adjacent NEW snapshot test) | `node:test`, VM-transpiled production TypeScript, lightweight DOM and fake timers. | Extend behavioral tests for snapshot pages, stale replies and first-run routing, not source-string assertions. Keep transport tests intact. |
| `docs/api-contracts-hifimule-daemon.md`, `docs/data-models-hifimule-daemon.md` (UPDATE) | Describe live playback and v13 feedback/reporting. | Document actual snapshot methods/tables, inclusion, no-expiry dedupe, offline display, pagination and error/recovery contract. |

`PlaybackWorkspace.ts`, `PlaybackQueue.ts`, and the architecture's planned `PlaybackBar.ts` are not the current integration points. Use `PlaybackDestination.ts` and the existing floating `PlaybackControls.ts`; the latter does not require modification merely to add snapshot access. Do not reuse `playbackDescribeOccurrences` as the saved-view data source: it depends on the active live session/revision and provider lookup.

The API document's older Retry paragraph still says replay resets the occurrence outcome; current code preserves it and creates another attempt. Its older queue-kind description also omits Radio. Correct those snapshot-adjacent descriptions when documenting this feature. Live occurrence paging currently defaults to 100/max 200; this story's new snapshot endpoints deliberately use 50/max 200 and do not change live paging defaults.

### UX requirements

Offer “Save listening snapshot” from Playback and a “Saved snapshots” entry that remains available for an empty live session. While Preview is active, say that the **main listening session** will be saved. Preview-only empty state explains why there is no main selection to save. Show the snapshot name, creation time, count and source badges in read-only inspection. Use normal row metadata fallbacks instead of dropping unknown tracks; mark missing sources separately.

With zero configured servers, query the local saved list before deciding whether the installation is genuinely empty. A failed existence/list read is a recoverable local-content error with retry, not evidence of emptiness and not a reason to force first-run login.

Save remains unavailable while its own request is in progress or the observed live state is stale, with an understandable status. Saved inspection remains usable independently of live session freshness. Distinguish saving, committed, empty, recoverable error, and result-unknown states. A result-unknown flow recovers the original operation before enabling a replacement save. Do not show server playlist success language or physical export controls.

Keep focused controls stable across transport updates, saved-page responses and source changes. Fence late page/save replies after selection change or component destruction. Do not let a save response for an earlier session overwrite a newer session; it may still announce and link to the correctly saved artifact. Use actual buttons, accessible names, polite status announcements, Escape/close behavior where applicable, and the existing responsive/focus tokens. No provider request is needed to render a saved page.

### Previous story and Git intelligence

Recent commits: `f01a9b5 Review 16.8`, `260303f Dev 16.8`, `aed8880 Story 16.8`, `ec9a562 Review 16.7`, `9a781c7 Dev 16.7`.

Story 16.8 delivered schema v13 and the local occurrence rejection consumed here. Its review corrected account-scoped feedback read watermarks, transient local settlement/startup-recovery retries, and ambiguity after unreadable post-send responses. Carry forward exact-identity fencing, bounded retries, and honest acknowledgement handling. Snapshot acceptance reads local disposition, not the remote feedback journal; that journal can fail while rejection still succeeds. Story 16.7's report rows and statuses have their own eligibility and must remain independent.

The prior story reports successful local macOS arm64 builds/automated tests and configured-server feedback effects, but no installed Story 16.8 UI interactions on any platform. Windows/Linux/macOS x64 runner gaps and Story 15.17's open packaging status are not passing evidence for this story. No provider-effect probe is required merely to test local snapshot storage; instead assert zero provider calls for save/read and preserve existing reporting/feedback behavior.

### Library requirements and current technical research

Retain the repository baseline: Rust edition 2024 / minimum 1.93.0; locked Tokio 1.49.0, rusqlite 0.38.0 with bundled SQLite (`libsqlite3-sys` 0.36.0), serde 1.0.228, serde_json 1.0.149, uuid 1.20.0; Tauri Rust 2.10.3 / UI API 2.10.1; TypeScript 5.6.3, Vite 6.4.3 and Shoelace 2.20.1. No library/framework upgrade or new UI framework is required.

Research checked 2026-09-29: current docs identify rusqlite 0.40.2 and Tokio 1.53.1; these newer versions are context, not upgrade instructions. SQLite still permits only one writer; separate statements outside a transaction cannot provide this capture guarantee. Transaction errors need explicit rollback handling rather than assuming every error rolled back prior statements. Use APIs supported by the locked crates. [Sources: [rusqlite transaction behavior](https://docs.rs/rusqlite/latest/rusqlite/enum.TransactionBehavior.html), [SQLite transactions](https://www.sqlite.org/lang_transaction.html), [SQLite isolation](https://www.sqlite.org/isolation.html).]

Tokio's blocking-task guidance confirms that started `spawn_blocking` tasks cannot be cancelled by aborting their async handle and can prolong shutdown. Bound admission and retain shutdown ownership of actual work. [Source: [Tokio spawn_blocking](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html).]

The current SQLite release notes list 3.53.4 and document a WAL-reset corruption fix in 3.53.0/3.51.3. This story does not introduce WAL or change SQLite. If persistence scheduling later proposes multiple WAL connections, first verify the actual bundled runtime and applicable fix; do not assume the system SQLite version is the linked one. [Source: [SQLite release history](https://www.sqlite.org/changes.html).]

### Required testing and completion evidence

| Area | Required assertions |
| --- | --- |
| Inclusion | Exact `[history,current,upcoming]` sequences; final completed current only once; incomplete/technical failures retained; skipped/disliked omitted; Like versus neutral; same-track distinct IDs; two sources sharing a track ID; recording copies retained. |
| Back and replacement | Multiple attempts yield one occurrence; Back followed by reordering previously heard upcoming rows preserves the new upcoming order; completion then replay-skip is omitted; skipped occurrence later revisited remains omitted; pure restart/Back is not skip; a jumped-over never-attempted past row is not history; old replaced/removed rows cannot reappear. |
| Preview | Main capture while audition plays, changes, finishes or stops; no audition metadata/identity leakage; disliked Preview does not reject matching main source; preview-only returns empty. |
| Capture races | Barriers before/after owner admission and commit versus natural advance, Next, remove/reorder, Dislike/Like, Radio refill/replacement, and source removal. Assert one whole before/after state, never a blend; a pending terminal-write error is recoverable, not a save of inconsistent state. |
| Persistence | Fresh and v13 upgrade; earlier migration fixtures; failed migration rollback; unsupported future version; trigger-injected header/entry failure; file-backed restart; saved rows survive clear/new session/source removal. Existing snapshots remain unchanged. |
| Idempotency | Concurrent duplicate operation; changed payload rejected; commit then dropped reply; retry after queue change, owner restart and command-cache expiry; no duplicate header/rows; default name stays fixed; explicit same-name new operation creates a separate artifact. |
| Paging | More than 200 and 10,000 entries; stable page boundaries and total; malformed/overflowing/cross-snapshot cursors; new save inserted during list pagination; only bounded rows/DOM nodes retained. |
| UI | Unknown/offline labels, zero-server reopen and lookup failure, genuine first-run onboarding, stale/out-of-order responses, navigation/destruction/reload during pending or lost response, not-found while capture is in flight, retry with original operation ID/payload, keyboard focus/status, small widths, all four locales; no live queue mutation controls in saved rows. |
| Side effects/resources | Spy providers and basket/device state show no effects from save/read; no playback revision/generation/current/output change; existing reporting/feedback remain independent; long-history measurements and concurrent controls/sync satisfy the lifecycle/resource guardrails above. |

Use existing deterministic owner/DB fixtures and co-located Rust tests. UI tests use Node's built-in runner, not Vitest/Jest. Suggested commands from the repository root (replace the test filter with the implemented module's actual path):

```sh
rtk npm run build:daemon -- test -p hifimule-daemon playback::export
rtk npm run build:daemon -- test -p hifimule-daemon playback::session
rtk npm run build:daemon -- test -p hifimule-daemon playback::persistence
rtk npm run build:daemon -- test -p hifimule-daemon feedback
rtk node --test scripts/tests/destination-ui.test.mjs scripts/tests/playback-ui.test.mjs
rtk cargo test -p hifimule-i18n
rtk npm run build --prefix hifimule-ui
rtk npm run build:daemon -- clippy -p hifimule-daemon --all-targets
rtk cargo fmt --all --check
rtk git diff --check
```

Include any new snapshot RPC/UI tests explicitly, and inspect test counts so a mismatched filter is not reported as coverage. The daemon wrapper establishes the controlled native FFmpeg environment. Run affected native/UI checks on Windows x64, Linux x64, macOS x64 and arm64; capture installed save/reopen/offline/Preview results where runners are available. Mark missing evidence as unverified, not passed, and retain it for the 16.14 expanded validation matrix. This story preparation has not run implementation tests.

### Project context and references

The persistent `project-context.md` still calls the project greenfield; current code, approved playback amendments, and recent completed stories are authoritative. `epic-16-context.md` also retains the historical “all stories backlog” statement; current sprint status supersedes it. Older no-device UI locks apply to physical basket/sync, not Playback or saved local inspection.

- `_bmad-output/planning-artifacts/epics.md` — Epic 16 overview; Story 16.9 at lines 4694–4751; Stories 16.10–16.11; P-NFR2/4–6, P-AR11 and P-UX-DR15.
- `_bmad-output/planning-artifacts/prd.md` — Desktop Playback scope; FR78–81 and playback NFRs.
- `_bmad-output/planning-artifacts/architecture.md` — Playback State and Ownership; Implementation Contracts; Project Structure; Validation Refinements; portable server identity.
- `_bmad-output/planning-artifacts/ux-design-specification.md` — design tokens, components and responsive/accessibility requirements, interpreted with the playback amendments.
- `_bmad-output/planning-artifacts/sprint-change-proposal-2026-09-19.md` — approved release split and old Story 15.23 → 16.9 mapping.
- `_bmad-output/planning-artifacts/project-context.md`; `_bmad-output/implementation-artifacts/epic-16-context.md` — persistent principles and cross-story boundaries.
- `_bmad-output/implementation-artifacts/16-8-save-supported-like-and-dislike-preferences-on-the-source-server.md` — local rejection contract, review fixes and evidence limits.
- `hifimule-daemon/src/playback/{session.rs,persistence.rs,feedback.rs,model.rs}`; `hifimule-daemon/src/playback/session/feedback.rs` — actual owner, attempts, schema v13, and disposition implementation.
- `hifimule-ui/src/{rpc.ts,main.ts,state/playback.ts,components/PlaybackDestination.ts}`; `scripts/tests/destination-ui.test.mjs` — actual UI integration and testing patterns.
- `Cargo.toml`, `Cargo.lock`, `hifimule-ui/package.json`, `hifimule-ui/package-lock.json` — checked dependency baseline.

## Dev Agent Record

### Agent Model Used

GPT-6 (Codex), story implementation.

### Debug Log References

- Final validation (2026-09-29, macOS arm64): controlled `build-daemon.mjs test --workspace` passed 1,329 daemon unit tests, 5 daemon integration tests, 7 i18n tests, 19 lifecycle tests and 10 native UI tests (1,370 total). Eight daemon tests are ignored by the normal suite: seven existing exclusions plus the resource probe, which was explicitly run for all six cases. Full Node suite passed 287 tests with one existing skip. TypeScript/Vite production build, `cargo fmt --all --check`, and `git diff --check` passed. Clippy completed with existing repository warnings and no warnings in the new snapshot modules.
- Final logs: `/private/tmp/hifimule-16-9-workspace.log`, `/private/tmp/hifimule-16-9-node.log`, `/private/tmp/hifimule-16-9-clippy.log`; resource logs `/private/tmp/hifimule-snapshot-bench-{10,10000,100000}-{0,1}.log`. Durable measurements and reproduction details are in `docs/api-contracts-hifimule-daemon.md`.
- Added deterministic owner tests for Preview-only empty capture, album/Radio provenance and replacement, feedback admission before/after capture, neutral/Like semantics, later queue reorder and Next, dropped replies and shutdown ownership. Added file-backed process interruption before commit, v13 migration rollback, and strict required-nullable wire validation. Save admission now releases its permit before acknowledgement, after commit/rollback, preventing a subsequent explicit save from racing the permit cleanup.
- Browser verification used the real component/CSS with local RPC fixtures: desktop English inspection; 360 px German, French and Spanish layouts without horizontal overflow; preserved repeated entries, unavailable-source/provider-ID fallbacks, heading focus and Escape return focus. Temporary browser fixture and server were removed/stopped. This is browser layout evidence, not an installed Tauri end-to-end claim.
- Resource probe: 10/10,000/100,000 retained rows, five attempts per occurrence, with/without capture, two unavailable sources, concurrent real sync-history DB writes and synthetic PCM consumption. At 100,000 rows capture held the DB/owner for about 1.032 seconds; queued Pause took 1.039 seconds and incremental peak RSS was about 1.1 MiB. All six cases met the explicit local 2-second/32-MiB probe limits with zero synthetic underruns. Installed hardware playback and Windows/Linux/macOS x64 evidence remain unverified for 16.14; Story 15.17 remains in-progress.

- UI implementation: focused save/list/inspection component, bounded 50-entry pages and 64-cursor navigation history, durable single-operation recovery coordinator with 15-second unknown-result deadlines, safe zero-server routing, four-locale labels and keyboard focus. Full Node suite: 286 pass, 1 existing skip. UI TypeScript/Vite build and 7 i18n tests pass.

- Owner/RPC integration: serialized capture with one active save, lifetime-bound mutation guard, recovery before freshness, Preview metadata isolation, bounded blocking read admission. RPC classification red test confirmed the missing mutation route; 1,320 daemon unit tests and 5 integration tests now pass (7 existing ignored).

- Storage implementation: independent v14 schema, SQL window ordering and transaction-bound rejection checks, durable canonical operation identity, bounded keyset reads. Initial schema test failed as expected; storage tests pass. Full daemon regression: 1,316 unit tests and 5 integration tests pass (7 existing ignored), using localhost permission for mock providers.

- Create-story preparation: 2026-09-29. Requirements, backend, and UI/history research performed against clean baseline `f01a9b5`.
- Validation: create-story checklist applied to requirements, inclusion, atomicity, operation recovery, migration, offline inspection, file locations, and verification scope. Implementation evidence belongs here after development.

### Completion Notes List

- Implemented policy-1 immutable local H+C+U capture in schema v14, preserving distinct occurrence/source references, local acceptance semantics and frozen display fallbacks. SQL materializes the complete artifact in one owner/SQLite boundary without a Rust history array.
- Added durable operation recovery before live freshness checks, bounded blocking admission and mutation ownership, four strict local RPCs, integrity validation and keyset paging. Saved artifacts survive live replacement, restart and source removal without automatic expiry.
- Added Playback naming/save/recovery/inspection UI, one persisted unresolved request, 50-row pages and bounded cursor history, safe zero-server routing, four-locale explanations and keyboard/status behavior. Saving does not change playback, provider preferences, server playlists, physical baskets or devices.
- Updated API and data-model documentation, including adjacent Retry/attempt and Radio queue-kind corrections, measured contention and explicit platform gaps. Full available regression and quality checks pass. Definition of Done: PASS for this story's implementation and available-runner evidence; status is review, with installed-platform gaps explicitly retained as required by the story.

### File List

- `docs/api-contracts-hifimule-daemon.md`
- `docs/data-models-hifimule-daemon.md`
- `hifimule-ui/src/rpc.ts`
- `hifimule-ui/src/components/PlaybackDestination.ts`
- `hifimule-ui/src/components/PlaybackSnapshots.ts`
- `hifimule-ui/src/state/snapshotSaves.ts`
- `hifimule-ui/src/localContentRoute.ts`
- `hifimule-ui/src/main.ts`
- `hifimule-ui/src/styles.css`
- `hifimule-i18n/catalog.json`
- `scripts/tests/snapshot-ui.test.mjs`
- `scripts/tests/destination-ui.test.mjs`
- `scripts/tests/playback-radio-ui.test.mjs`

- `hifimule-daemon/src/playback/session.rs`
- `hifimule-daemon/src/playback/session/export.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-daemon/src/rpc/playback_export.rs`

- `hifimule-daemon/src/playback/export.rs`
- `hifimule-daemon/src/playback/export_tests.rs`
- `hifimule-daemon/src/playback/mod.rs`
- `hifimule-daemon/src/playback/persistence.rs`

- `_bmad-output/implementation-artifacts/16-9-save-an-immutable-local-listening-snapshot.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`

## Change Log

- 2026-09-29: Code review fixed three UI edge cases: preserving the active Playback destination on existing-layout routing, permitting zero-server saved inspection despite recovery-storage failure, and clearing stale paging cursors after read errors. Full Node suite: 289 passed, 1 existing skip; UI build and diff check passed. Marked done; installed-platform evidence gaps remain tracked for 16.14.
- 2026-09-29: Implemented immutable local listening snapshots, v14 persistence and recovery, owner/RPC integration, paged offline Playback inspection, localization, regression/crash/race tests and resource measurements; marked review. Installed-platform gaps remain explicit; Story 15.17 is unchanged.
- 2026-09-29: Created Story 16.9 with immutable local capture, occurrence-level inclusion, durable operation identity, paged offline inspection, and implementation/test guardrails; marked ready-for-dev.
