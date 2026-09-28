---
baseline_commit: 5b93cbe5221293a662e7b550c8094f9b62d7b5dc
---

# Story 16.2: Replenish Radio within a bounded upcoming queue

Status: in-progress

## Story

As a HifiMule user,
I want my listening queue to replenish as I listen while respecting my edits and skips,
so that music can continue without assembling the whole library into a playlist or repeatedly suggesting tracks I rejected in this session.

## Acceptance Criteria

1. Starting from the explicit Playback selection settings action creates a logical Radio session after the first track is prepared. It retains that track's portable source identity and a conservative initial artist identity. Radio fills at most **five automatic upcoming occurrences** from eligible unheard tracks of that artist. If the artist identity is missing or ambiguous, it plays the prepared first track and reports an explained waiting state rather than guessing from a display name.
2. When automatic upcoming count drops below **two**, one bounded refill brings it toward five. It appends without moving any accepted occurrence. One accepted refill changes the queue revision once; duplicate, stale, cancelled and superseded results cannot append. Current/manual queue changes must be rechecked at owner admission, not just when fetch begins.
3. User append, reorder and remove actions retain their current revision and command-deduplication behavior. The automatic cap counts only automatic upcoming entries; it never truncates manual additions. Removing an automatic suggestion excludes its `(server_id, track_id)` from automatic selection for the logical session, even after restart. Removing a manual entry does not create an automatic exclusion.
4. Explicit Next/skip of a Radio occurrence excludes its source-qualified track for the logical session. Natural completion marks it heard; a replay or Back is still allowed as an explicit action. Fetch/decode/stream failure is a technical failure, never a skip or dislike. Radio may try another eligible suggestion, within the attempt budget below, while preserving a safe failure reason. Album's pause/retry behavior stays intact.
5. When the initial artist has no eligible unheard track, Radio enters `waiting` with an exhaustion reason and lets already queued tracks finish. It does not clear exclusions, silently repeat a cycle, drift by genre, or infer a related artist. No automatic work spins while waiting.
6. Pause preserves Radio and may finish bounded candidate lookup, but cannot make audio audible or admit a prepared successor contrary to paused intent. Stop cancels refill and leaves Radio stopped; preview preserves the main logical Radio session and its exclusions. Replacement/new Radio invalidates old work. Late results cannot restart stopped audio, mutate a replacement or alter preview return behavior.
7. Quit/restart restores the logical Radio ID, queue and automatic/manual provenance, initial artist, heard and excluded source-qualified tracks, and status **paused**. Resume continues the same exclusions; an explicit new Radio creates a new logical ID and empty sets. Corrupt or incompatible Radio state fails safely with a recoverable explanation rather than silently resetting exclusions.
8. History and exclusions remain durable without loading the full listening history into memory. Use indexed membership checks and paged history reads. Candidate lookup, one refill in flight, queue lookahead, compressed prefetch and PCM buffering have independent bounds. No refill loads a whole library or constructs a library-sized queue.
9. Windows, macOS and Linux verification covers threshold transitions, manual edits during refill, duplicate completion, skips versus technical failures, exhaustion, pause/Stop/preview/replacement races and restart. Results must not claim artist transitions, repeat cycles or cross-server recording deduplication.

## Tasks / Subtasks

- [x] Finalize the Radio contract before behavior changes (AC: 1–8).
  - [x] Define wire/storage schema and migration for logical ID, `Radio` queue kind, occurrence origin, center artist, state/reason and durable heard/skipped/removed identities. Preserve old Album/Manual rows and unknown-version handling.
  - [x] Define source-qualified artist identity from provider metadata. Use `(server_id, artist_id)` only when a stable ID is available; never join artists across servers or by name in this story. State whether no-ID and multiple-artist tracks wait.
  - [x] Define event-driven triggers and limits: target five auto upcoming, trigger below two, one refill in flight, maximum eight sources and 400 candidates/source (3,200/request), 15-second retrieval and 15-second preparation deadlines, and at most five candidate preparation failures in one refill. A failed pass waits for a new meaningful trigger or explicit retry; no timer spin. Retain existing 16.1 caps unless measurements justify a documented adjustment.
  - [x] Define heard at **natural completion** and exclusions at explicit skip/automatic removal; do not equate transport movement, preview, failed preparation or technical failure with heard. Define when a failed source may be retried after a new user action/source recovery without repeating a busy loop.
- [x] Extend the owner-owned session and persistence model (AC: 1–7).
  - [x] Add Radio mode and automatic/manual occurrence provenance without changing existing Album and Manual behavior. Persist logical state and exclusions transactionally with accepted queue mutations/outcomes; add indexed source-qualified membership and paged reads.
  - [x] Admit refill through the session owner with session ID, generation, expected queue revision, refill ID and current Radio state checks. Atomically append a batch and increment revision once. On conflict, recompute remaining capacity/exclusions from current state; do not overwrite an edit or replay a stale batch.
  - [x] Preserve Radio kind on manual queue edits. Only removed automatic entries add exclusions. Preserve manual order, accepted queue revisions, command ID dedup and occurrence identity.
  - [x] Connect completion, explicit Next, Stop, preview, replacement, shutdown and restoration to Radio state without changing album retry, Back/replay, output-loss inhibition or preview return semantics.
- [x] Reuse the shared selector for incremental artist-scoped refill (AC: 1–2, 4–5, 8).
  - [x] Extend Playback selection to filter candidates by the stable center artist and durable source-qualified eligibility **before** the shared pure ordering. Keep sync inputs and ordering unchanged. Use bounded provider retrieval and a bounded request cache; do not reuse a stale pool indefinitely or reload the entire library on every consumed track.
  - [x] Fetch/prepare outside the owner, with a generation fence at each asynchronous boundary and final owner admission. Skip bounded technical failures while retaining their reason and source identity. Ensure an exhausted source or all-failed pass reaches waiting/backoff.
- [x] Wire explicit settings-view start and truthful status (AC: 1, 5–6, 9).
  - [x] Build on `playback.startSelection`'s validated settings, source routing, preflight and admission fence. Saving settings alone must not start/replace playback. Starting a new session must preserve the existing main session on pre-admission failure.
  - [x] Surface filling, waiting/exhausted and temporary source failure in authoritative snapshots and localized UI status. Keep controls keyboard accessible, with names, focus and live feedback. Do not expose final bar/menu **Play something** yet.
- [ ] Verify with deterministic owner, persistence, provider and UI tests (AC: 1–9).
  - [x] Include same local track IDs on different servers, manual edits racing refill, duplicate delivery, explicit replay, all candidates failed, ambiguous artist, corrupt state and long-history indexed/paged behavior.
  - [ ] Run relevant Rust, TypeScript, UI and cross-platform build/test gates; report which platforms and installed scenarios were actually exercised.

## Dev Notes

### Contract and implementation guardrails

- **Scope:** This is the bounded same-artist Radio core behind the settings-view action. Story 16.3 adds relationship transitions, fresh centers and later listening cycles; 16.4 adds confident recording-level deduplication; 16.6 adds final bar/menu entry points. Do not claim those behaviors here. The story's numerical limits above are the initial implementation contract; measure and document if they must change before acceptance. [Source: `_bmad-output/planning-artifacts/epics.md` §Story 16.2; `_bmad-output/planning-artifacts/architecture.md` §Playback Radio Selection]
- **Selection:** `playback/selection.rs` already validates a separate `playback-selection.json`, limits to eight configured sources and 400 candidates per source, calls `auto_fill::pipeline::run_pipeline`, and maps internal IDs back to `TrackSource`. Its default `max_tracks` can be one, so refill must request a bounded batch through a deliberate Playback-only interface instead of assuming current `select()` yields five. Preserve pure selector equivalence and sync's existing ordering. Playlist retrieval was recently changed to bounded provider APIs; preserve that fix. [Source: `hifimule-daemon/src/playback/selection.rs`; `_bmad-output/implementation-artifacts/16-1-configure-playback-selection-and-start-its-first-selected-track.md` §Review Findings]
- **Artist evidence:** `Song` has `artist_id` and `artist_name`, but current `PlaybackTrackMetadata` exposes only the name and `selection::select()` returns only `TrackSource`. Retain stable source-qualified artist evidence from the candidate used for the first track and each refill candidate. Do not derive Radio center from the UI's currently browsed artist or a title/name match. [Source: `hifimule-daemon/src/domain/models.rs`; `hifimule-daemon/src/playback/{model,selection}.rs`]
- **Start:** `rpc/playback_selection.rs::start_with_config` currently loads saved settings, fetches source pools, selects, preflights a real audio response and admits one `PlayTrack` under `START_GATE`/`START_EPOCH`. Extend the explicit start to establish Radio atomically with the first prepared result; keep cancel/newer-request fencing and the single-response handoff. The current session must survive all pre-admission failures. `playback.cancelSelectionStart` cancels pending start, not an already accepted session. [Source: `hifimule-daemon/src/rpc/playback_selection.rs`]
- **Session owner:** `PlaybackSession` in `playback/session.rs` serializes mutations and snapshots. Current `SessionOperation::AppendQueue`, `RemoveUpcoming` and `MoveUpcoming` convert queue kind to `Manual`; reusing them unchanged would erase Radio mode. `QueueKind` is currently only `Album | Manual`, and `Occurrence` has no origin flag. Add an owner-only refill command and preserve Radio on user edits. Use `queue_revision` as a decimal string on the JSON wire and checked `u64` internally; progress/state sequence is not a queue edit. Refill IDs and command IDs must be bounded in retention and safe on retry. [Source: `hifimule-daemon/src/playback/model.rs`; `hifimule-daemon/src/playback/session.rs` §apply_inner; `_bmad-output/planning-artifacts/architecture.md` §Playback Implementation Contracts]
- **Outcomes and storage:** `playback/persistence.rs` currently persists `playback_sessions`, `playback_occurrences` and `playback_attempts`; it validates only Album/Manual queue kinds, and attempt outcomes distinguish `naturalCompletion`, `explicitSkip` and `technicalFailure`. Extend those paths and the migration, rather than adding an unrelated event store or an ever-growing in-memory history. Keep transactional checkpoints and existing paged occurrence reads. A track may have multiple occurrence/attempt IDs; exclusion membership is by source-qualified track, while replay and queue edits still use occurrence IDs. [Source: `hifimule-daemon/src/playback/persistence.rs`; `hifimule-daemon/src/playback/model.rs`]
- **Audio and continuity:** Natural completion, Next and technical failure already enter separate session pathways; connect Radio refill to those owner transitions. Preserve album successor preparation/continuity, preview's saved main session, Back semantics, paused intent, output-loss inhibition and stale generation checks. Network fetch/decode/database writes stay out of the audio callback. Manual additions can exceed the auto lookahead but remain subject to the existing manual queue safety cap (`MAX_MANUAL_ACTIVE_OCCURRENCES = 10_000`). [Source: `hifimule-daemon/src/playback/{session,audio,continuity,model}.rs`; `_bmad-output/planning-artifacts/architecture.md` §Playback Implementation Contracts]
- **Independent audio budgets:** Existing streaming bounds compressed data at 8 MiB including the upstream HTTP chunk and uses a one-slot channel; each PCM queue targets 500 ms and is capped at 1 MiB. Radio's five metadata occurrences are not five prepared audio streams. Retain these playback limits unless a measured change is explicitly justified and tested. [Source: `hifimule-daemon/src/playback/streaming.rs` §§bounded reader/constants; `hifimule-daemon/src/playback/audio.rs` §PCM queue allocation]
- **Security/privacy:** Resolve credentials only through existing provider adapters and portable server IDs. Do not put authenticated stream URLs in snapshots, UI or logs. No durable cross-session taste profile; Radio membership tables belong to a logical session and are deleted/replaced according to the normal session lifecycle. [Source: `_bmad-output/planning-artifacts/prd.md` P-NFR5, FR69; `hifimule-daemon/src/providers/`]

### Files and current behavior to preserve

| File/boundary | Current state | Story change and preservation requirement |
| --- | --- | --- |
| `hifimule-daemon/src/playback/model.rs` | Typed `TrackSource`, occurrence/session snapshots, `QueueKind`, revisioned operations | Add Radio state/origin with compatible wire schema; retain Album/Manual and existing `schemaVersion: 1` behavior or make a versioned migration explicit. |
| `hifimule-daemon/src/playback/session.rs` | Single owner handles queue edits, control, events, preview and restore | Admit Radio start/refill/outcomes under owner; preserve edit conflicts, manual order, preview/Back/album/Stop/output semantics. |
| `hifimule-daemon/src/playback/persistence.rs` | SQLite session, paged occurrences, attempts/outcomes and migrations | Persist logical Radio identity/center/origin and indexed heard/exclusion membership with atomic queue checkpoints; old rows restore unchanged. |
| `hifimule-daemon/src/playback/selection.rs` and `auto_fill/pipeline.rs` | Bounded Playback candidate pools feed shared selector | Add artist-scoped eligibility/batch interface; preserve sync order, seed semantics, source collisions and 16.1 retrieval fixes. |
| `hifimule-daemon/src/rpc/playback_selection.rs`, `rpc.rs`, `playback/commands.rs` | Explicit settings start/cancel and session command dispatch | Make start establish Radio with preflight and fences; route owner refill without exposing a client-forgeable automatic append. |
| `hifimule-daemon/src/playback/audio.rs`, `continuity.rs` | Prepared audio and successor handoff | Change only if Radio needs a hook; retain bounded compressed/PCM queues and existing album failure/continuity behavior. |
| `hifimule-ui/src/components/PlaybackSelectionSettings.ts`, `PlaybackDestination.ts`, `state/playback.ts`, `rpc.ts`, `hifimule-i18n/catalog.json` | Settings start/cancel, authoritative playback snapshot and localized status | Show honest Radio/waiting/error state without dead final entry points; preserve responsive keyboard and reconnect behavior. |

### Previous story and Git intelligence

- Story 16.1 is `done` at baseline `a33e9b9` and established versioned Playback-only settings, source-qualified selection, bounded retrieval, pre-admission audio preparation and owner-commit fencing. Review repaired six defects: source option paging/exact validation, bounded playlist retrieval, final audio-install fence, advanced-setting preservation, missing size/bitrate eligibility, and partial artist retrieval. Treat each as a regression guard. [Source: `_bmad-output/implementation-artifacts/16-1-configure-playback-selection-and-start-its-first-selected-track.md` §Dev Agent Record; `rtk git log -5`]
- Story 15.17 remains `in-progress` in sprint tracking. Epic 16 work cannot turn its installed manual-playback verification into a pass by assumption; record separate evidence for the Radio changes. [Source: `_bmad-output/implementation-artifacts/sprint-status.yaml` §Epic 15]

### Testing and current library guidance

- Focused Rust tests belong beside the changed `playback` modules and RPC. Extend `scripts/tests/playback-selection-ui.test.mjs` for settings/status and existing playback queue UI tests for authoritative revision refresh. Use deterministic fake providers and paused Tokio time/controlled completions to prove ordering and stale-work rejection without audio hardware. Verify SQLite migration and a long logical session through indexed membership and paged reads, including restart. Run the repository's controlled native FFmpeg test wrapper where direct `cargo test` lacks the runtime; then TypeScript/Vite, localization parity and four-platform CI. [Source: `_bmad-output/implementation-artifacts/16-1-configure-playback-selection-and-start-its-first-selected-track.md` §Debug Log References; `_bmad-output/planning-artifacts/architecture.md` §Playback Project Structure]
- The repo already uses Tokio and rusqlite. Tokio's current docs say dropping a `JoinHandle` detaches work and `abort()` is cooperative, so an abort alone cannot be the correctness fence; retain owner admission checks and generation/epoch tokens. Bounded Tokio channels provide backpressure if a producer channel is introduced, but the existing owner command path is preferable where sufficient. Do not upgrade Tokio/rand just for Radio. [Tokio task cancellation](https://docs.rs/tokio/latest/tokio/task/), [bounded mpsc](https://docs.rs/tokio/latest/tokio/sync/mpsc/fn.channel.html). The repository's lockfile and tests, rather than current online crate releases, define compatible versions.

### Project Structure Notes

Architecture places Radio in `hifimule-daemon/src/playback/radio.rs` and persistence in the existing playback/SQLite boundary. `radio.rs` does not yet exist: add it for policy, triggers and bounded candidate work; keep mutation authority in `session.rs`, provider APIs in `providers/`, shared ordering in `auto_fill/`, and UI integration in the existing Playback components. Rust/storage use snake_case and JSON uses camelCase. [Source: `_bmad-output/planning-artifacts/architecture.md` §Playback Project Structure and §Playback Implementation Contracts]

### References

- `_bmad-output/planning-artifacts/epics.md` — Epic 16, Story 16.2 and adjacent 16.1/16.3 scope.
- `_bmad-output/planning-artifacts/prd.md` — FR60, FR65, FR67–69, FR73 and P-NFR2/4/5.
- `_bmad-output/planning-artifacts/architecture.md` — Playback Radio Selection, Session Control, Implementation Contracts and Project Structure.
- `_bmad-output/planning-artifacts/ux-design-specification.md` — Desktop Playback refinement; use established responsive/focus/status patterns.
- `_bmad-output/implementation-artifacts/epic-16-context.md` — sequencing, source identity and release gate.
- `_bmad-output/implementation-artifacts/16-1-configure-playback-selection-and-start-its-first-selected-track.md` — implementation and review learnings.

## Dev Agent Record

### Implementation Plan

- Radio is a logical session identified by the existing UUID `session_id`; `queueKind: radio` is the wire value. Version 8 SQLite migration retains old `playback_sessions` and `playback_occurrences` rows and adds Radio metadata, indexed automatic occurrence origin, and indexed source-qualified membership without loading membership into memory. An unknown future playback schema remains a restore error. Existing snapshot `schemaVersion: 1` remains compatible through additive optional fields. The old SQLite queue-kind constraint stores Radio as Manual in its legacy column; the companion Radio row determines the restored kind.
- `Song.artist_id` is accepted only when it is a nonempty single stable ID; its identity is `(server_id, artist_id)`. Missing or ambiguous metadata waits with an explanation. Jellyfin and Subsonic multi-artist metadata is marked ambiguous even when a primary ID exists. Artist names never identify a center. Radio 16.2 stays within that artist and server.
- A refill is triggered by an owner event when indexed automatic upcoming count falls below two; it targets five, with one request in flight. Retrieval takes at most 15 seconds over at most eight sources and 400 candidates per source. Preparation has a separate 15-second deadline and at most five failures. Each asynchronous boundary and owner admission checks session, generation, revision and refill identity. A failed pass waits for a meaningful new trigger or explicit retry, with no timer spin. Existing compressed and PCM limits are unchanged.
- Natural completion records heard membership. Explicit Next and removal of an automatic upcoming occurrence record exclusion membership. Technical failure, failed preparation, preview, and transport movement do not. A failed source can be retried after an explicit user action or source recovery. Back and replay remain explicit even for heard or excluded sources. Existing streaming and PCM caps remain in force.

### Agent Model Used

GPT-6 Codex

### Debug Log References

- Red/green checks exercised Radio owner policy, persistence, artist identity, refill admission and UI status before the implementation passed. The controlled native audio wrapper was required for daemon tests.
- The default sandbox denies loopback listeners used by existing daemon tests. The full daemon suite passed with the permitted loopback-capable test command.
- An unchanged audio test contained one Clippy `unused_io_amount` error. Its read now explicitly discards the returned byte count; the four affected audio tests pass and all-target Clippy completes.
- Local verification host: macOS arm64. The four-runner CI matrix was updated, but Windows, Linux and macOS x64 jobs have not executed on this change. No installed-package Radio result is claimed.

### Completion Notes List

- Implemented bounded same-artist Radio behind the explicit settings action, with durable source-qualified exclusions, indexed automatic provenance and owner-only revisioned admission.
- Added conservative provider artist evidence, authoritative localized Radio status with accessible Retry, and deterministic owner, persistence, provider and UI coverage.
- Local gates passed: 1,209 daemon unit tests and five provider contract tests, 257 Node tests (256 pass, one skip), two Python playback-evidence unit tests, seven localization tests, UI production build, all-target daemon Clippy, Rust format, and diff whitespace check.
- Cross-platform CI is pending. The story remains in progress until Windows, Linux and macOS x64 verification runs.

### File List

- `_bmad-output/implementation-artifacts/16-2-replenish-radio-within-a-bounded-upcoming-queue.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `.github/workflows/build.yml`
- `hifimule-daemon/src/domain/models.rs`
- `hifimule-daemon/src/playback/audio/queue_edit_tests.rs`
- `hifimule-daemon/src/playback/mod.rs`
- `hifimule-daemon/src/playback/model.rs`
- `hifimule-daemon/src/playback/native.rs`
- `hifimule-daemon/src/playback/persistence.rs`
- `hifimule-daemon/src/playback/radio.rs`
- `hifimule-daemon/src/playback/selection.rs`
- `hifimule-daemon/src/playback/session.rs`
- `hifimule-daemon/src/playback/session/album_admission.rs`
- `hifimule-daemon/src/providers/audiobookshelf.rs`
- `hifimule-daemon/src/providers/jellyfin.rs`
- `hifimule-daemon/src/providers/subsonic.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-daemon/src/rpc/playback_selection.rs`
- `hifimule-i18n/catalog.json`
- `hifimule-ui/src/components/PlaybackDestination.ts`
- `hifimule-ui/src/rpc.ts`
- `scripts/tests/playback-radio-ui.test.mjs`

### Change Log

- 2026-09-28: Added bounded same-artist Radio session, transactional persistence, owner refill admission, provider ambiguity handling, localized UI status, regression tests, and four-platform CI coverage configuration.
