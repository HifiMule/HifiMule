---
baseline_commit: 9fb42946ac506f1f8b60c3649c8f99ec569e4c96
---

# Story 15.3: Preserve and restore a paused listening session

Status: ready-for-dev

## Story

As a HifiMule user,
I want HifiMule to retain my listening queue and position between launches,
so that I can resume deliberately without reconstructing my session or being surprised by automatic playback.

**Requirements:** Session-state foundation for FR56, FR58 and FR60; P-NFR2, P-NFR4–5; P-AR3–4 and P-AR10.

**Dependencies:** Stories 15.1–15.2 are done. Preparation baseline: `396c5e6` (Linux startup fix), following `d6ccddf` (15.2 review) and `aefe9f2` (15.2 implementation). This story establishes production session ownership, persistence and the authenticated daemon command contract. Command-driven fixtures prove restoration without a decoder. Audible Resume belongs to 15.4; Radio exclusions, preview preservation and exports belong to their later stories.

## Acceptance Criteria

1. **Paused round trip.** Given a queue with source-server track references and deliberate repeats, when checkpointed and restarted, queue order, occurrence IDs, current occurrence and last checkpointed position are restored. A nonempty valid session restores paused regardless of its previous transport state. Credentials and authenticated stream URLs are absent from persisted session data and responses.
2. **Idle round trip.** Given no queue or an explicitly cleared session, restart restores idle and never revives the preceding queue.
3. **Offline restoration.** Given a locally valid session with an unavailable source, restore its queue and position without network access. Expose source availability separately; do not delete entries, substitute recordings or use the currently browsed server.
4. **Recoverable invalid state.** Unsupported schema versions, malformed records and invalid current references produce a recoverable restoration error. Preserve stored evidence and prevent automatic empty checkpoints from replacing it. Supported migration is transactional; failed migration retains the previous state.
5. **Serialized commands.** One owner processes commands with explicit command identity and queue revision. Reject stale queue edits with the authoritative revision. Progress alone does not advance queue revision. Duplicate command behavior is bounded and specified below.
6. **Authoritative reconnect.** Reconnecting clients obtain a versioned snapshot with explicit identity, revisions and position units. They fetch missing pages rather than replay commands or create another owner.
7. **Periodic and final checkpoint.** Position persistence happens outside any future audio callback. Orderly Quit attempts a final checkpoint, surfaces failure through the shutdown contract and does not claim successful preservation. Abrupt termination restores at most the last committed checkpoint.
8. **Bounded and atomic state.** Long queue/history reads are paged with the limits below. An interrupted transaction leaves the last committed queue/current/position internally consistent. Checkpoint cadence and work limits are explicit; no active-audio memory guarantee is implied.
9. **Platform evidence.** Windows, macOS and Linux tests cover round trip, idle, repeats, stale commands, reconnect, migration failure and interrupted checkpoint with the same paused/idle contract. Record OS, architecture, binary revision and actual outcomes; an ARM64 run does not certify x64.

## Tasks / Subtasks

- [ ] Define typed contract and one session owner (AC: 1–6, 8)
  - [ ] Add `playback/{mod,model,session,persistence}.rs`; encode the contract below and contract serialization fixtures.
  - [ ] Implement bounded command admission, instance/session fencing, revision conflicts, command deduplication and an internal generation-fenced progress boundary.
  - [ ] Implement bounded snapshot/occurrence queries without cloning all queue/history entries.
- [ ] Add transactional session persistence (AC: 1–4, 7–8)
  - [ ] Add isolated playback schema migration through `db.rs` and database transaction methods; preserve existing server/device/scrobble/auto-fill tables and migrations.
  - [ ] Restore offline before publishing playback readiness; validate all invariants with bounded scans.
  - [ ] Persist structural transitions atomically; coalesce position checkpoints and preserve dirty state after failures.
  - [ ] Block mutation of invalid/unsupported stored state; expose safe retry without destructive automatic reset.
- [ ] Expose authenticated daemon commands (AC: 3, 5–6)
  - [ ] Add the methods below to `rpc.rs`, forwarding to the shared session handle; register mutation classification and update all AppState fixtures.
  - [ ] Add exact camelCase JSON and error-shape tests, bounded pages and no-provider-call offline tests.
  - [ ] Document reconnect and cursor invalidation; do not add a second event transport or frontend player store.
- [ ] Integrate lifecycle checkpoint and diagnostics (AC: 4, 7, 9)
  - [ ] Wire session ownership into `start_daemon_core` and `run_server`; keep one owner across their separate runtimes.
  - [ ] Add final-checkpoint participation, dedicated error/blocker state and narrowly authorized retry to committed shutdown.
  - [ ] Preserve sync cancellation even if checkpoint blocks/fails; join persistence before core-runtime teardown and ownership release.
  - [ ] Extend tray and existing shutdown UI with localized checkpoint failure/retry wording, owner-bound observation and accessible status.
- [ ] Verify and record evidence (AC: 1–9)
  - [ ] Run deterministic model, authenticated router, real-file SQLite and process-restart/fault tests from the matrix below.
  - [ ] Run relevant regression suites and three-OS command-driven restoration checks; record limitations without marking unrun checks passed.

## Dev Notes

### Selected contract — implementation gate resolved

The decisions in this section are the implementation contract selected during story preparation, not a claim that these APIs exist today. Extend this contract in later stories rather than prebuilding their entities.

#### Identity, units and states

- `schemaVersion: 1` identifies the playback wire contract; persistence has its own version `1`. Do not change the lifecycle protocol version for these additive playback APIs.
- `instanceId` is the existing daemon owner identity. `sessionId` and `occurrenceId` are daemon-generated UUID strings. Every insertion generates a distinct occurrence, even for identical tracks. Never deduplicate occurrences by track ID.
- `source: { serverId, trackId }` uses the existing **portable** server ID plus opaque nonempty provider track ID. It does not use the machine-local server UUID. Recording identity is deliberately absent until recording deduplication is implemented in 15.18; source identity must not masquerade as recording identity.
- `queueRevision`, `stateSequence` and persisted checkpoint sequence use canonical nonnegative decimal strings on the JSON wire (SQLite signed 64-bit nonnegative integers internally). Reject overflow rather than wrap. `positionMs` is a nonnegative integer JSON number, bounded by JavaScript's safe integer range and by known duration when available. Reject fractional/negative/nonfinite/out-of-range values. Unknown duration remains unknown; do not invent one or mix seconds/ticks/samples.
- A singleton durable session row exists even when cleared. It retains `sessionId` and monotonic `queueRevision` across clear/restart, preventing old commands from targeting a recreated zero-revision session. Explicit future new-listening-session behavior can rotate identity in its own story.
- Normal externally usable states here are `idle` (empty queue, null current, position zero) and `paused` (nonempty queue, valid current). Model the architecture's `buffering`, `playing` and `stopping` variants for future integration and restoration fixtures, but do not expose a fake Play/Resume command. Loading persisted active states always produces paused. Restoration failure is a separate `restoration.status: "error"` with structured reason, not healthy idle.
- `generationId` is an opaque runtime UUID changed on current-occurrence selection, clear/replacement and startup. It is independent of queue revision and the lifecycle launch generation. Progress updates carry generation, occurrence and increasing per-generation sample sequence; reject late, duplicate or out-of-order samples. Accepted progress may move backward only through an explicit future seek transition, which must rotate generation.

#### Minimum SQLite schema and ownership

Use the existing daemon database at its existing OS-local path. Add playback-specific version metadata and these logical tables; SQL names remain snake_case:

| Table | Required data and invariants |
| --- | --- |
| `playback_schema` | Singleton version; initialized/migrated only inside a transaction. Version 0 means no playback tables, not an inferred arbitrary legacy JSON format. |
| `playback_sessions` | Singleton ID, session UUID, queue revision, checkpoint sequence, transport state, nullable current occurrence, position milliseconds. A clear commits empty occurrences plus null current/zero position in the same transaction. |
| `playback_occurrences` | Session ID, occurrence UUID, ordered integer ordinal, portable server ID and track ID. Unique occurrence ID and unique ordinal per session; repeated source tuples are allowed. Keep earlier/current/upcoming occurrences in this ordered table so past entries can be paged without a separate reporting/history system. |

Completed-listen eligibility, Radio history/exclusions, preview slots, output preferences, saved snapshots, exports and Playback settings are **not** introduced here. Occurrences preceding current are navigation history, not proof of a completed listen. Structural queue operations keep previous occurrences unless explicitly replacing/clearing the queue. Later stories extend semantics with migrations.

Use explicit transactions for queue rows + current pointer + position + revision. Validate cross-row invariants transactionally even if a cyclic current-row foreign key is awkward. Do not add a foreign key to `server_config` with delete cascade: removing a server must retain the session's source references. Playback schema/version checks must not mutate unknown-version playback data. Migration v0→v1 is the only production migration initially; test injected failure between DDL and version commit. Add future versions only with actual formats and fixtures.

`Database` currently wraps a synchronous rusqlite connection in `Arc<Mutex<Connection>>`; keep it encapsulated. Put SQL/migration access behind its methods and call these from one serialized playback worker outside async executor/native-loop threads. A bounded worker mailbox plus request/reply handle may implement the session owner. Do not hold sync admission, provider or device locks while waiting for SQLite. Keep health/status in a small independent cached snapshot so DB stalls cannot block it. Do not spawn an unbounded thread/task per progress sample or create another daemon/database.

Structural command success means the transaction committed; publish the new authoritative revision only afterward. Failure leaves the previous structural state authoritative. Position can advance in memory before its next checkpoint; use dirty/checkpoint sequences so completion of an older checkpoint cannot mark newer progress clean. Serialize persistence writes so an old periodic checkpoint cannot overwrite a newer clear/current selection. No session blob containing the whole queue is permitted.

#### RPC methods, revisions and retry behavior

Use the existing JSON-RPC envelope and `result.data` conventions. `params.schemaVersion` is required. Methods below are new scoped APIs, not aliases for existing browser/basket methods:

| Method | Request and result |
| --- | --- |
| `playback.getSession` | Read-only; returns snapshot metadata and at most the first queue page. Snapshot contains schemaVersion, instanceId, sessionId, queueRevision, stateSequence, generationId, state, current occurrence/source or null, positionMs, checkpointedPositionMs, persistence/restoration status, total occurrence count and next cursor. |
| `playback.listOccurrences` | `{ sessionId, expectedQueueRevision, cursor?, limit? }`; returns bounded ordered occurrences and next cursor. Cursor binds session, revision and last ordinal. Current pointer and source availability are separate metadata, never inferred from page presence. |
| `playback.applySession` | `{ instanceId, sessionId, commandId, expectedQueueRevision, operation }`; operation is `replaceQueue { sources[] }`, `appendQueue { sources[] }`, `selectCurrent { occurrenceId }`, or `clear`. These prepare paused session state, never audio. Return authoritative metadata/revision and assigned occurrences for the bounded inserted batch. |
| `playback.retryRestore` | Explicit authenticated retry after failed initial restoration, only when no valid live session has been accepted; reload/revalidate preserved data without deleting it. Reject when a valid/dirty session exists, shutdown is committed or another restore/write is unresolved. |

`replaceQueue` selects the first inserted occurrence at position zero, or idle for empty input. `appendQueue` preserves current/position, selecting the first entry only when previously empty. `selectCurrent` keeps order/identities and sets the chosen occurrence paused at zero; it advances stateSequence and generation, not queueRevision. `clear` is durable and invalidates generation. Replace/append/clear each advance queueRevision once when accepted, including an explicit empty clear; selecting an unknown occurrence fails without changes. All mutations validate expected revision and identity at execution, not merely before enqueue.

Every accepted state change advances runtime `stateSequence`; progress and current-position changes do not change `queueRevision`. A new daemon instance resets runtime sequence and generation. Reconnect first validates instance identity, then replaces client presentation from `getSession`; cursor revision conflicts require refreshing. No events are needed in 15.3: the existing transport is request/response. Future event delivery must carry the same instance and state ordering. Do not silently claim a working WebSocket/broadcast subscription.

Deduplicate `applySession` by `(instanceId, commandId)` for **10 minutes**, retaining **at most 1,024** results in FIFO order. Require UUID command IDs. Compare canonical typed payloads, including session/expected revision; reuse with different payload returns `COMMAND_ID_REUSED`. An identical retained command returns its original result without reapplying, even after the queue revision changed; include current metadata separately so the caller can refresh. Check dedup before stale-revision rejection. Failed validation/conflict results may be retained under the same bounded policy. Expiry/capacity eviction permits re-evaluation; no indefinite exactly-once guarantee. The cache is not persisted. On restart, reject the old instance ID (`INSTANCE_MISMATCH`) and require snapshot refresh, never automatic mutation replay.

Use existing numeric JSON-RPC codes: invalid params `-32602`, conflict `409`, internal/storage `-32603`; include stable playback `error.data.code` plus applicable authoritative instance/session/revision. Define codes `UNSUPPORTED_PLAYBACK_VERSION`, `INVALID_SESSION`, `RESTORE_FAILED`, `PERSISTENCE_FAILED`, `QUEUE_REVISION_CONFLICT`, `INSTANCE_MISMATCH`, `SESSION_MISMATCH`, `COMMAND_ID_REUSED`, `INVALID_CURSOR`, `PLAYBACK_BUSY`, `STALE_PROGRESS`. Reuse established lifecycle stopping rejection. Errors carry sanitized diagnostics, never raw SQL/session contents or credential-bearing URLs. A timed-out caller must query state or retry the same identity; a timeout is not proof the command failed.

`applySession` and `retryRestore` must enter `is_mutating_method`; queue admission and execution must both honor shutdown. Retain an admitted command's mutation guard until the owner completes/rejects it, even if the requesting RPC is dropped. At committed shutdown, reject queued unstarted mutations, finish/join any already executing transaction, then freeze the final session snapshot; no earlier admitted command may commit after that snapshot. Use the existing authenticated native proxy and middleware. Do not expose the internal progress fixture boundary as an unrestricted production RPC that can claim real playback.

#### Bounds and checkpoint cadence

| Resource | Selected limit / behavior |
| --- | --- |
| Read page | Default 100, maximum 200 occurrences; reject zero/out-of-range limits. Keyset query with revision-bound cursor; no whole-history materialization. |
| Insert batch | Maximum 200 sources per command. Build long queues by append batches; do not impose a misleading 200-entry total-session limit. |
| Identity fields | Server/track strings nonempty, maximum 1,024 UTF-8 bytes each; reject unknown payload fields, URLs-as-extra-fields and oversized input. Do not require a server network connection for admission. |
| Command mailbox | 64 pending requests; reject overflow with retryable PLAYBACK_BUSY. Reserve/independently signal shutdown so a saturated normal mailbox cannot prevent fencing. |
| Position ingress | One coalesced latest sample, sampled by the owner at most once per 250 ms; producer never blocks on queue or database. Final checkpoint uses the latest accepted sample after fencing progress. |
| Periodic position checkpoint | Every 5,000 ms while dirty; no writes for unchanged/idle state. One write in flight; missed ticks coalesce, never accumulate. |
| Transition checkpoint | Queue changes, clear and current selection commit before success. Final Quit checkpoint explicitly commits latest accepted position. |
| Storage contention | Set/verify a 1,000 ms SQLite busy timeout for the playback operation under the connection serializer and restore the prior policy for other consumers if changed. This is a lock-wait bound, not a filesystem completion deadline. |
| Cached status | Metadata + current occurrence + at most one 200-entry page. Read requests and restoration validation scan batches; never keep an ever-growing history vector. |

Do not invent automatic history deletion or a total memory target. This story bounds working sets and payloads; later active-resource acceptance measures audio/candidate/history budgets. Test a 10,000-occurrence queue across pages including deliberate repeats and invalid late rows. Startup validation can scan incrementally, but must not publish a valid session until the complete stored session passes validation.

#### Restoration, privacy and recovery

Restore session data independently of provider availability or selected UI server. Resolve local configured-server presence from portable IDs; availability is `unknown` (configured but not contacted) or `notConfigured`, never falsely `online`. A previously reported temporary unavailable condition must not erase references. Provider probing and stream resolution remain later work, through `MediaProvider`/portable-to-local routing.

Validate version, UUIDs, unique occurrence identities/order, integer bounds, legal state, current membership and empty-state invariants. Do not silently repair an invalid pointer by selecting the first track. A fresh database can create healthy idle; a corrupt/unsupported existing session cannot. Preserve errors across periodic ticks and Quit without writing a fallback empty row. Permit normal sync/application startup if only playback restoration fails; expose playback error through its snapshot and an additive bounded health status, while leaving invalid rows unchanged.

Restore retry re-reads evidence after an external repair or transient initial-load problem; it does not reset a valid session. A periodic checkpoint failure leaves accepted live progress dirty and retries saving that state on the next five-second tick; it must never trigger a DB reload that rolls position back. A restore error with no accepted in-memory mutations requires no final overwrite and may allow clean daemon exit while accurately reporting that restoration was unavailable. This differs from a final checkpoint failure after accepted position updates, which blocks preservation completion. Do not add destructive reset/export-recovery actions or copy the entire credential-containing database into a diagnostic location.

Persist only typed source identities and necessary session fields. Never serialize provider objects, credentials, stream URLs, tokens or raw provider error strings. Existing credential vault and provider cache remain keyed by machine-local IDs; translate only at future source-routing boundaries. Use source IDs in diagnostics without dumping entire rows.

#### Orderly Quit integration

Retain 15.2's durable launch fence, one shutdown ID and 5,000 ms warning deadline. The committed path fences session mutations/progress, captures a final position and starts its checkpoint before signalling sync cancellation. **Do not wait for a blocked database write before requesting sync cancellation:** initiate checkpoint first, then continue independent cancellation/drain while joining persistence. In 15.4 an audio-stop acknowledgement precedes this capture. No fake audio subsystem is needed now.

Add an additive checkpoint substate to `ShutdownSnapshot` (`notRequired`, `pending`, `failed`, `succeeded`), a `sessionCheckpoint` blocker and `PLAYBACK_CHECKPOINT_FAILED` diagnostic. Keep the existing phase/deadline fields compatible. A pending/stalled checkpoint retains ownership and keeps health/tray/UI responsive. Completed failure remains observable and blocks final teardown until explicit successful retry; it never calls `fail_shutdown_fence`, reopens admission or claims preserved playback. Timeout remains a warning; dropping/aborting a blocking writer does not establish completion.

Expose a narrow authenticated `playback.retryCheckpoint` usable only for the same committed shutdown ID and owner after the previous write has completed with failure. Concurrent retries join one attempt; never start a second write while a previous one may commit. It retries the frozen final snapshot, retains the shutdown ID/deadline and cannot resume work. Add it explicitly to the stopping exception and native proxy only as needed, retaining owner/epoch checks. Repeated Quit continues to observe the current attempt.

Tray and the existing shutdown screen distinguish device-drain waiting from session-save failure and offer **Retry saving session** only when eligible. They must not offer “Continue using HifiMule” after committed shutdown. Preserve precommit fence-failure Retry Quit/Continue behavior. Extend all four catalog locales, polite live-region status, keyboard focus, one polling schedule, two-second native health timeout and no-election refresh. Successful retry still waits for sync workers/mutations/RPC teardown before owner release.

### Source tree change and preservation map

| File | Current state → change; behavior to preserve |
| --- | --- |
| `hifimule-daemon/src/playback/{mod,model,session,persistence}.rs` (NEW) | Typed command handle, serialized owner, snapshot/progress boundary and persistence orchestration. Create only these needed modules, no decoder/Radio/preview skeletons. |
| `hifimule-daemon/src/db.rs` (UPDATE) | Synchronous encapsulated SQLite connection with incremental existing migrations. Add transaction-backed playback schema/data access and bounded queries. Preserve existing tables, portable server identity, credential metadata and history APIs; do not convert every old migration or silently ignore playback migration errors. |
| `hifimule-daemon/src/main.rs` (UPDATE) | Owns Tao tray, core commands, DB startup and separate core/RPC runtimes; CommitShutdown currently drains sync and ends core. Initialize/share session handle, initiate/join final checkpoint and render status/retry. Preserve detached UI lifetime, launch-generation fence, native event loop and ownership-release order. |
| `hifimule-daemon/src/rpc.rs` (UPDATE) | AppState/server initialization, authenticated router, explicit mutation classification and stopping health exception. Add session forwarding/status/retry and update all construction/test sites. Preserve numeric reauthentication behavior, secret boundary, admission/mutation guards and responsive health on the independent runtime. |
| `hifimule-daemon/src/sync.rs` (UPDATE, coordinator sections only) | Owns ShutdownSnapshot, admission, committed cancellation and worker drain. Add checkpoint participation to status/final completion without waiting under global locks. Preserve finalization gate, failure precedence, current-file safe boundary and cancellation of all registered work. Do not fix unrelated manual-cancel behavior here. |
| `hifimule-ui/src-tauri/src/lib.rs` (UPDATE) | Authenticated proxy plus owner/epoch-bound shutdown observation. Allow only the scoped retry path; preserve no launch/election during refresh and existing platform startup behavior. |
| `hifimule-ui/src/main.ts`, `src/shutdownStatus.ts` (UPDATE) | Existing shutdown renderer, retry/continue fence recovery and single rate-limited poller. Add checkpoint-specific presentation/action without building playback controls or changing normal browser/basket selection. |
| `hifimule-i18n/catalog.json` (UPDATE) | Shared four-locale catalog. Add localized playback persistence/retry diagnostics with matching keys/placeholders. |
| `hifimule-ui/tests/shutdownStatus.test.mjs`, relevant daemon tests and `scripts/smoke-tests/` (UPDATE/NEW as needed) | Extend rendered-action, owner-bound retry and real-file/process evidence. Preserve existing lifecycle, sync-integrity and smoke checks. |

`hifimule-lifecycle/src/lib.rs` currently owns secure discovery/health identity, not playback state. Avoid changing ownership protocol; modify it only if its typed health decoding requires additive fields. Native media registration, source streaming, FFmpeg/CPAL dependencies, full Playback UI, configuration and shared selection extraction remain out of scope.

### Previous story intelligence and Git context

- `396c5e6` fixes Linux/WebKit startup by binding default shutdown polling timers through `globalThis`; it adds a test requiring the Window receiver. Retain this binding when editing the poller.
- `d6ccddf` (Review 15.2) hardened captured device identity, atomic terminal outcome/clean-commit decisions, MTP cache rollback/durability, precommit fence recovery, single UI polling, fencing timeout visibility and owner-bound native health. Do not regress these while introducing the second kind of persistence failure.
- `aefe9f2` introduced active-Quit cancellation/drain and status. `a7e887e` created its contract; `e863368` reviewed 15.1 ownership/lifecycle. Production implementations supersede experimental playback supervisor/probe behavior.
- Story 15.2 post-review evidence records daemon 657, lifecycle 13, native UI 6, frontend 8 and smoke-evidence 3 passing tests on macOS arm64. These are historical counts, not acceptance evidence for this story. Physical-device smoke preceded review patches; retain that limitation.
- Deferred 15.2 F9 is a pre-existing manual cancel versus clean-manifest race. Keep it visible without expanding 15.3 into another sync rewrite.
- The project-context file's “Greenfield / Analysis” status is stale. Current production code and approved playback amendments take precedence; managed-device ownership and provider abstraction principles remain applicable.

### Libraries and current technical research

Keep the workspace's Rust 2024/MSRV 1.93.0 and locked dependencies: rusqlite 0.38.0 with bundled SQLite (`libsqlite3-sys` 0.36.0), Tokio 1.49.0, serde 1.0.228 and uuid 1.20.0. No audio or database framework upgrade is required. Generate types if an existing generator is available; otherwise enforce equivalent exact serialization tests rather than adding a new toolchain solely for this story.

Official documentation checked during preparation on 2026-09-12:

- [SQLite atomic commit](https://www.sqlite.org/atomiccommit.html) and [transactions](https://www.sqlite.org/lang_transaction.html): commit/error handling must preserve the previous transaction state; test real-file recovery and do not disable journaling/durability for speed. Session checkpoint means committing session data, not merely requesting a WAL checkpoint.
- [Tokio blocking tasks](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html): started blocking work cannot be stopped by aborting its handle. Keep completion ownership until persistence returns. The current documentation surfaced Tokio 1.53.1; this story stays on the locked 1.49.0 line.
- [rusqlite documentation](https://docs.rs/rusqlite/latest/rusqlite/) currently reports 0.40.2, newer than the project. Use APIs available in locked 0.38.0; verify against local crate sources when version-specific documentation cannot be fetched. A latest-version result is not authorization to upgrade or proof of security validation.

### Testing requirements

Use deterministic barriers and injectable clock/storage boundaries, plus real SQLite files. In-memory tests alone do not prove restart durability. Required matrix:

| Area | Required checks |
| --- | --- |
| Round trip | Fresh idle, explicit clear after nonempty, duplicate sources with distinct occurrences, current not first, nonzero position, previously playing restored paused; exact IDs/order across process restart. |
| Validation/recovery | Unknown version, malformed field, negative/overflow values, duplicate ordinals/IDs, invalid late-page row/current pointer, migration rollback, unavailable/read-only DB. Evidence remains unchanged after timer, reconnect and Quit; successful retry recovers. |
| Concurrency | Two edits with same revision: one wins; identical command retry applies once; reused ID/different payload rejected; TTL and 1,024-cap eviction; old-instance retry rejected after restart; progress cannot invalidate a queue edit or update a replaced current. |
| Checkpoint | 5-second dirty cadence with injected time, no idle writes, latest coalesced sample, older completion cannot clear newer dirty state, queue mutation/clear racing periodic write, failed commit preserves old rows; retryRestore after a periodic failure cannot roll back live progress. Kill a subprocess mid-transaction and verify the previous coherent commit on reopening. |
| Paging | 10,000 occurrences with repeats, 100/200-page bounds, invalid/foreign/stale cursor, no missing/duplicated page entries at fixed revision, bounded retained memory independent of total history. |
| Offline/privacy | No provider/network calls during restore, same raw track ID on two portable servers stays distinct, missing/removed server retains references, no credential/URL fields in serialized DB/wire data or diagnostics. |
| Shutdown | Pending/failing final write still cancels sync; no owner release before writer/drain completion; retry after completed failure succeeds once; no parallel retry during unknown write outcome; same shutdown ID/deadline; responsive authenticated health; no automatic empty overwrite after restore failure. |
| UI/native | Actual rendered checkpoint failure and retry action, no Continue after committed Quit, original precommit recovery still works, one timer after repeated refresh, stale owner response/action rejected, catalog parity and keyboard/live-region behavior. |

Add authenticated router tests, not just direct handler calls: verify mutation classification, stopping exceptions and wrong-token/owner rejection. Serialize tests touching process-global lifecycle state or refactor fixtures to avoid poisoning unrelated tests. Update every AppState constructor; `make_test_state` does not cover all existing inline fixtures.

Run `rtk cargo test -p hifimule-daemon`, `rtk cargo test -p hifimule-lifecycle`, `rtk cargo test -p hifimule-ui --lib`, relevant frontend Node behavior tests, `rtk cargo fmt --check`, appropriate clippy and `rtk npm run build` in `hifimule-ui`. Distinguish sandbox restrictions on mock HTTP/macOS APIs from product failures. No runtime tests are claimed by story preparation.

Record command-driven daemon restart evidence on Windows, macOS and Linux against production paths and isolated test data. Include OS/architecture, revision, fixture, crash point, persisted before/after state and outcome. Never modify the user's live session database for failure injection. Full audible, media-key, decoder-packaging and physical-output acceptance belongs to later stories; the state contract still requires actual three-OS execution here.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Story 15.3; Epic 15 requirements, dependencies and implementation gates; Stories 15.4/15.11/15.13/15.16/15.18/15.23]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Playback State and Ownership; Implementation Contracts; Project Structure; Validation Refinements; portable server identities]
- [Source: `_bmad-output/planning-artifacts/prd.md` — FR56/58/60, state integrity and source privacy]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md` — accessibility, headless feedback and existing visual conventions]
- [Source: `_bmad-output/planning-artifacts/playback-epic-validation.md`, `playback-story-review.md` — staged readiness and contract gates]
- [Source: `_bmad-output/planning-artifacts/project-context.md` — retained managed-sync and provider abstraction principles]
- [Source: `_bmad-output/implementation-artifacts/15-2-quit-safely-while-a-device-sync-is-running.md` — shutdown participation contract, review fixes and evidence limits]
- [Source: production files in the source tree map; workspace `Cargo.toml` and `Cargo.lock`; commits listed above]

## Dev Agent Record

### Agent Model Used

GPT-6 (Codex)

### Debug Log References

- 2026-09-12: Story preparation reviewed planning artifacts, prior story/review, production persistence/RPC/lifecycle paths and official SQLite/Tokio/rusqlite documentation. No production implementation or runtime testing performed.
- 2026-09-12: Captured baseline `9fb42946ac506f1f8b60c3649c8f99ec569e4c96`; marked sprint story in progress.
- 2026-09-12: Implemented the initial daemon playback contract, SQLite persistence, restoration, paging, command fencing/deduplication, progress checkpoints, authenticated RPC methods, quit-fence checkpoint participation and localized retry wording.
- 2026-09-12: Validation: playback tests 8/8; full daemon suite 663/663; lifecycle 13/13; Rust UI 6/6; Node behavior 9/9; `cargo fmt --check` passed; daemon clippy passed with 13 pre-existing warnings; UI production build passed with existing Vite chunk warnings.
- 2026-09-12: macOS ARM64 environment recorded (`Darwin 25.6.0`, `aarch64-apple-darwin`, baseline revision above). Windows, Linux and macOS x64 command-driven production-path runs were not available in this workspace and are not claimed.
- 2026-09-12: Added a non-release GitHub Actions `Build` matrix for Windows x64, Linux x64, macOS x64 and macOS ARM64. It runs the isolated playback restoration evidence command on pull requests and pushes to `main`, then uploads a sanitized per-platform JSON artifact even on failure. Workflow YAML and the evidence command were validated locally; remote runner results remain unclaimed until the workflow executes.

### Implementation Plan

- Establish a daemon-owned typed playback session and transactional playback-only schema without coupling restoration to providers.
- Publish authenticated, revision-bound request/response RPCs and bounded page contracts.
- Reuse the existing shutdown fence and UI retry path for final checkpoint failure rather than introducing another lifecycle protocol.
- Prove deterministic model behavior, real-file restart/paging, router authentication and regression compatibility; leave unavailable platform execution explicitly unverified.

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- Scoped contract gates resolved: identities, versions, position units, serialized commands, revisions/deduplication, transactional schema, recovery, page/checkpoint limits and shutdown failure/retry.
- Status ready-for-dev; implementation tasks and platform verification remain unchecked.
- Initial implementation slice is working and regression-green: paused/idle persistence, repeated occurrence identity, offline restoration metadata, unsupported-version evidence preservation, stale revisions, command reuse, position checkpointing, 10,000-entry paging and authenticated snapshot routing are covered.
- Completion gate remains open: the selected story contract still requires a bounded 64-request serialized worker (the current owner is mutex-serialized), fully bounded structural/current lookups, complete injected migration/interrupted-transaction coverage, and actual Windows/macOS/Linux command-driven evidence. Per the story’s evidence rule, these are not marked complete or promoted to review.
- Cross-platform evidence is now wired into the normal build rather than the release workflow. Each artifact records OS release, architecture, source/binary revision, isolated database scope, executed fixtures, exit code and actual outcome.

### File List

- `_bmad-output/implementation-artifacts/15-3-preserve-and-restore-a-paused-listening-session.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `.github/workflows/build.yml`
- `docs/api-contracts-hifimule-daemon.md`
- `hifimule-daemon/src/db.rs`
- `hifimule-daemon/src/main.rs`
- `hifimule-daemon/src/playback/mod.rs`
- `hifimule-daemon/src/playback/model.rs`
- `hifimule-daemon/src/playback/persistence.rs`
- `hifimule-daemon/src/playback/session.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-i18n/catalog.json`
- `scripts/playback-session-evidence.py`

## Change Log

- 2026-09-12: Created story 15.3 with production session persistence contract, shutdown integration and acceptance evidence requirements.
- 2026-09-12: Added initial production playback-session contract, transactional persistence/restoration, authenticated RPC surface, checkpoint-aware shutdown integration, localized diagnostics and deterministic test coverage; story remains in progress pending remaining contract and platform gates.
- 2026-09-12: Added the normal-build GitHub Actions playback evidence matrix and sanitized evidence runner for Windows, Linux and both macOS architectures; no release trigger added.
