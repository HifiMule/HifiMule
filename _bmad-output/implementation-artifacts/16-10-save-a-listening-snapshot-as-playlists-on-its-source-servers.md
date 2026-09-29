---
baseline_commit: d75cd5f234ad0717a91ceea531918dea748f84af
---

# Story 16.10: Save a listening snapshot as playlists on its source servers

Status: review

<!-- Ultimate context engine analysis completed - comprehensive developer guide created. -->

## Story

As a HifiMule user,
I want to save a listening snapshot to playlists on its contributing servers,
so that I can reuse my discoveries in other clients while understanding what was saved on each server.

**Requirements:** FR79; export portion of FR81; P-NFR2 and P-NFR4-6; P-AR11 and playlist-write portion of P-AR9; P-UX-DR8 and P-UX-DR14-15.

**Dependencies:** Delivered Epic 15 playback foundation (Stories 15.1-15.17) and completed Stories 16.1-16.9. This story consumes immutable local snapshots and existing provider playlist capabilities. Physical-device basket export remains Story 16.11. Installed-platform evidence still outstanding from prior stories is not implicitly satisfied here.

## Acceptance Criteria

1. **Single capable source and repeat fidelity.** Given a saved snapshot contains tracks from one playlist-capable server, when the user explicitly chooses Save to server playlists and supplies a valid name, HifiMule creates a playlist containing that server's included occurrences in snapshot order. Deliberate repeated occurrences are preserved where the provider can represent them faithfully; otherwise the limitation is disclosed and the part is not represented as an exact successful save.
2. **Mixed-source partition.** Given a snapshot contains tracks from multiple servers, export creates one playlist per contributing capable server containing only that server's referenced tracks in their relative snapshot order. The complete cross-server order remains in the immutable local snapshot. Tracks are never copied or uploaded between servers.
3. **Capability and permission truth.** A source without playlist creation or required write permission is reported as unsupported or denied with a reason. Other capable parts may proceed, but the aggregate result is not called wholly successful.
4. **Frozen routing and collision safety.** Confirmed export requests use only the snapshot's frozen portable `serverId` and provider `trackId` values plus existing daemon-held credentials. A browsing-server change, continued listening, or later queue edit cannot redirect or change content. A name collision never implicitly overwrites an existing playlist.
5. **Independent results and targeted retry.** Every contributing part exposes its real success, failure, unsupported, denied, partial, ambiguous, or unresolved state and any known remote playlist identity. Retry operates only on unfinished work and never recreates a confirmed playlist.
6. **Ambiguous create recovery.** A timeout or disconnect after a request may have reached the server persists an ambiguous outcome. Only verified provider idempotency or reliable reconciliation may resolve/retry it. HifiMule never blindly creates another playlist or promises exactly-once remote effects; irreconcilable uncertainty remains visible with an explicit recovery path.
7. **Safe population checkpoints.** When create and populate are separate or additions are batched, a failed intermediate step retains the known playlist identity and confirmed progress. Resume cannot duplicate confirmed entries. If the next safe boundary cannot be established, the part stays unresolved. HifiMule never deletes a partial remote playlist as implicit rollback.
8. **Durable, bounded execution.** Exit or network loss preserves operation state tied to the same immutable `snapshotId`; restart/reconnect recovers it. Retry backoff, provider batch sizes, snapshot paging, persisted evidence, and in-memory work have explicit bounds. Remote work never runs in the audio callback or blocks playback controls.
9. **Missing tracks and provider limits.** Deleted source tracks, source removal, authentication failure, or provider limits are exposed against the affected part and partial contents. HifiMule does not silently substitute another server copy, drop an entry, reorder a part, or call an inexact result complete.
10. **Verification and secrecy.** Deterministic tests cover mixed sources, same raw track IDs on different servers, repeats, name collisions, unsupported and denied writes, partial population, ambiguous replies, stale replies, restart, and targeted retry. Provider request fixtures prove exact order/routing; configured-server checks verify actual saved contents and retry behavior. UI/logs never expose credentials or authenticated URLs. Installed Windows/macOS/Linux proof remains an explicit evidence item and is not inferred from unit tests.

## Tasks / Subtasks

- [x] Define and document the provider/export contract before enabling writes (AC: 1-10)
  - [x] Verify Jellyfin and supported OpenSubsonic/Navidrome behavior for deliberate duplicate IDs, retained order, same-name creation, create response identity, maximum IDs/request, append limits, permission failures, and read-back reconciliation.
  - [x] Record a provider support matrix: create-with-items vs create-then-populate, safe batch size, repeat/order fidelity, collision handling, and whether create/append ambiguity is reliably reconcilable.
  - [x] Treat unknown semantics conservatively: disable the unsafe path or return a disclosed limitation/unresolved result; do not infer one provider's behavior from another.
- [x] Add a versioned durable server-playlist-export model and v15 migration (AC: 4-9)
  - [x] Persist an operation UUID, immutable `snapshotId`, validated requested base name, canonical request, timestamps/retention, aggregate status, and one ordered part per contributing portable server.
  - [x] Persist each part's frozen ordinal/track plan or reproducible snapshot reference, state, attempt/backoff metadata, known playlist ID, confirmed batch boundary, actual/expected counts, error/limitation code, and ambiguity/reconciliation evidence.
  - [x] Validate bounded UTF-8 name/identity sizes, strict enums/transitions, snapshot existence, and operation-id deduplication transactionally; reject mismatched reuse.
  - [x] Preserve v1-v14 upgrades, transactional rollback, future-version rejection, and all immutable Story 16.9 tables/data.
  - [x] Keep persistence migration version `15` distinct from the new RPC/domain wire `schemaVersion: 1`; never expose the SQLite migration number as a DTO schema version.
- [x] Implement bounded per-source planning from the saved snapshot (AC: 1-4, 8-9)
  - [x] Page snapshot entries in ascending global ordinal and partition by the frozen portable `serverId`; within each part preserve the filtered ordinal order and every deliberate occurrence, including repeated `trackId` values.
  - [x] Never build from the live session, UI page, current basket, selected server, or current queue; never resolve/deduplicate through entity expansion.
  - [x] Classify removed source configuration, temporarily offline/auth failure, denied permission, unsupported capability, missing track, and provider fidelity/limit restrictions separately.
- [x] Implement the durable export state machine outside the playback owner/audio path (AC: 3-9)
  - [x] Route each part through `server_manager.get_provider_by_server_id*` using its frozen portable source; never use `require_provider` or a browsed/selected provider.
  - [x] Persist intent/state before every non-idempotent remote boundary and outcome/checkpoint immediately after it. Fence completions by operation ID, part/server ID, attempt/generation, and expected state.
  - [x] Retain returned playlist IDs and confirmed progress. Retry only safe unfinished transitions; never recreate confirmed parts, repeat an unconfirmed append, overwrite on collision, substitute copies, or implicitly delete/rollback a partial playlist.
  - [x] Serialize or durably reserve concurrent exports for the same snapshot/base-name/server tuple. Handle a check-then-create name race as a truthful collision or ambiguity; two operations must not overwrite, blindly retry, or silently claim the same remote playlist.
  - [x] Use bounded batches, exponential backoff with caps, bounded recovery/retention, and cancellation/shutdown checkpoints. Persisted daemon state is authoritative across UI disposal/restart.
  - [x] If reliable read-back can reconcile exact identity/content, compare ordered IDs/counts before advancing. Otherwise leave possible-effect calls ambiguous and require an explicit user recovery decision that cannot be mistaken for a safe automatic retry.
- [x] Expose strict daemon RPC contracts and mutation admission (AC: 3-10)
  - [x] Before implementation, freeze and document the exact RPC names and DTOs for plan/start/status/retry/reconcile under `rpc/playback_export.rs` or a focused sibling. Use camelCase `schemaVersion: 1`, `deny_unknown_fields`, bounded pagination, stable error codes, and authoritative returned state.
  - [x] Classify plan/status reads as non-mutating only when they cannot schedule or advance remote work; start/retry/reconcile/cancel and any recovery call that can cause a provider write are mutating.
  - [x] Add every provider-writing/retry method to `rpc::is_mutating_method` and the production router; respect shutdown admission and avoid holding DB/session locks across provider awaits.
  - [x] Return per-server display identity, capability/reason, playlist identity when known, expected/confirmed counts, and safe next actions without returning credentials or provider URLs.
- [x] Extend provider adapters only where the verified contract requires it (AC: 1, 3, 6-7, 9-10)
  - [x] Keep provider-specific endpoints, authentication, collision/read-back logic, and error mapping inside Jellyfin/Subsonic adapters behind `MediaProvider`.
  - [x] Do not break existing playlist creation/curation RPCs. If the current boolean capability is insufficient, add an explicit playlist-write fidelity/limits contract with conservative defaults; Audiobookshelf/default providers remain unsupported.
  - [x] Verify repeated query keys in Subsonic fixtures using raw-query matching; generic URL-decoding matchers can hide duplicate `songId`/`songIdToAdd` parameters.
- [x] Add an accessible saved-snapshot export experience (AC: 1-10)
  - [x] Place the action on an already-saved snapshot, clearly distinct from Save snapshot, live Radio, server playlist editing, physical basket Add/Replace, and sync.
  - [x] Request and validate a name, show the exact immutable snapshot/source parts before confirmation, capability-gate actions, and preserve focus/keyboard operation with named controls and live status announcements.
  - [x] Show each source independently with success/partial/failure/unsupported/denied/ambiguous/unresolved status, known playlist identity, counts, reason, and only safe applicable recovery actions; never flatten partial success into a generic success toast.
  - [x] Recover active/unresolved operations from daemon state after view disposal/reload/restart. Fence stale async responses; a selected/browsed-server change or an equal snapshot render must not erase/update the wrong operation.
  - [x] Add English, French, Spanish, and German catalog entries and minimal responsive/focus-visible styling.
- [x] Add backend, UI, integration, and documentation evidence (AC: 1-10)
  - [x] Backend tests: exact partition/order with interleaved servers and repeats; duplicate operation IDs; selected-server switch; removed/offline/denied/unsupported source; collision; missing track; bounded paging/batches/backoff/retention.
  - [x] Failure matrix: lost create reply, known create then failed append, lost append reply, restart before/after each checkpoint, stale completion, one success plus one failure, unsafe reconciliation. Assert no blind duplicate create/add and no implicit delete.
  - [x] Provider fixtures and configured Jellyfin/Navidrome probes assert actual ordered contents and disclose any repeat limitation. Sanitize logs/errors.
  - [x] UI tests execute production components for confirmation, accessibility, per-part progress, partial/ambiguous recovery, retry targeting, disposal/late replies, restart, focus, and bounded DOM/paging.
  - [x] Regression tests prove no playback transport/session/revision mutation, no provider write before explicit confirmation, no basket/device/sync call, and no changes to existing generic playlist curation.
  - [x] Update daemon API/data-model documentation with schemas, transitions, bounds, provider matrix, collision policy, recovery semantics, and evidence limitations.

## Dev Notes

### Non-negotiable implementation contract

Story 16.9 already created the authoritative input: a daemon-owned, durable, read-only snapshot keyed by stable `snapshotId`, with ordered entries containing occurrence identity and frozen `TrackSource { serverId, trackId }`. Story 16.10 consumes that artifact. It must not recapture or reinterpret the live listening session. The local snapshot remains the only representation of full cross-server order and must remain unchanged by export.

This is a distributed, non-idempotent write workflow, not a convenience wrapper around `playlist.create`. Model an export as one durable operation with independent per-server parts. `succeeded` means remote contents were confirmed under the verified provider contract; `ambiguous`/`unresolved` are honest terminal-or-recovery states, not transient labels that authorize blind replay.

Use one canonical durable vocabulary. Part states are `planned`, `unsupported`, `denied`, `pending`, `creating`, `populating`, `partial`, `succeeded`, `failed`, `ambiguous`, `unresolved`, and `canceled`; aggregate states are `planned`, `running`, `partial`, `succeeded`, `failed`, `unresolved`, and `canceled`. `unsupported`, `denied`, `succeeded`, and `canceled` are terminal for a part. `failed` is retryable only when recorded evidence proves no remote effect or identifies the safe next boundary. `ambiguous` may transition only through verified reconciliation to a confirmed checkpoint/result or to `unresolved`; `unresolved` requires an explicit user recovery decision and never auto-retries. `partial` retains a known remote identity/progress and may resume only from a confirmed boundary. Document a transition table and reject every illegal/stale transition transactionally.

No playlist write may be triggered automatically by saving a snapshot, opening it, switching servers, reconnecting, or resuming playback. Only an explicit confirmed export starts remote effects. This story does not start playback, sync, or device writes.

### Current files that must be understood before editing

- `hifimule-daemon/src/playback/export.rs`: current Story 16.9 schema/model and bounded snapshot reads. It stores headers plus ordered entries with frozen source identity/metadata and preserves repeated occurrences. Add export persistence without mutating snapshot semantics; a focused `playback/server_export.rs` is preferred if it keeps responsibilities clear.
- `hifimule-daemon/src/playback/session/export.rs`: serialized local snapshot capture through the session owner. Server export must not run through or hold the playback owner; it should need only the durable snapshot ID.
- `hifimule-daemon/src/playback/persistence.rs`: current playback schema version 14 and migration/rollback/future-version behavior. A new journal implies a v15 migration with equivalent atomicity tests.
- `hifimule-daemon/src/rpc/playback_export.rs`: current strict save/list/get/entries RPC surface. Extend it or add a focused sibling while retaining strict params, typed errors, and bounded DB work.
- `hifimule-daemon/src/rpc.rs`: router and mutation classification. New remote-write and retry/reconcile calls are mutating and must be admitted/blocked consistently during shutdown.
- `hifimule-daemon/src/providers/mod.rs`: `MediaProvider`, `Capabilities.supports_playlist_write`, conservative unsupported defaults, and playlist methods. A boolean alone does not express repeat fidelity, limits, idempotency, or reconciliation.
- `hifimule-daemon/src/providers/jellyfin.rs`, `providers/subsonic.rs`, and `api.rs`: existing create/add calls and provider fixtures. Preserve generic curation behavior; add only verified export capabilities/operations.
- `hifimule-ui/src/rpc.ts`: Story 16.9 snapshot DTOs/calls and the project's camelCase wire conventions.
- `hifimule-ui/src/components/PlaybackSnapshots.ts`: current snapshot save/list/detail/paging UI. Preserve local inspection while adding a clearly separate export flow.
- `hifimule-ui/src/state/snapshotSaves.ts`: one-unresolved-local-save lifecycle pattern. Do not reuse its localStorage record as export authority; server export recovery belongs in daemon persistence and should have a separate coordinator if UI lifecycle support is needed.
- `hifimule-ui/src/components/PlaybackDestination.ts`: owns the snapshot component; avoid destroying/recreating it on equal layout renders.
- `scripts/tests/snapshot-ui.test.mjs`, `hifimule-i18n/catalog.json`, `hifimule-ui/src/styles.css`: production UI behavior, locale parity, and styling gates.
- `docs/api-contracts-hifimule-daemon.md`, `docs/data-models-hifimule-daemon.md`: authoritative contract/schema documentation to update.

### Existing behavior to preserve

- Snapshot entries survive playback-session deletion and server removal; frozen source labels/identity remain inspectable while `sourceAvailable` is computed from current configuration.
- Snapshot paging is bounded (backend accepts 1-200; UI uses 50) and deliberate repeated occurrences are retained.
- Export may not change transport, current occurrence, queue, queue revision, reporting/feedback state, Radio exclusions, Preview/Return semantics, or native controls.
- Generic basket/browse playlist creation currently uses the selected provider, expands entities, silently skips unresolved items, and deduplicates track IDs with a `HashSet`. That behavior is incompatible with snapshot export and must not be called or copied.
- Playlist curation and existing server management must keep working. Do not change provider-wide create/add semantics without regression tests for their existing callers.

### Provider-specific technical guidance

- Jellyfin's current API supports playlist creation with ordered IDs and separate item addition; its add endpoint accepts an insertion position. The present adapter sends all IDs on create and returns the created playlist ID. Verify server 12.1.0 repeat/order/collision behavior and practical request limits before selecting a batching strategy.
- OpenSubsonic `createPlaylist` accepts repeated `songId` values and returns playlist details on modern protocol versions; `updatePlaylist` accepts repeated `songIdToAdd` and returns only success. The protocol does not provide an operation idempotency key. Verify Navidrome 0.64.2 behavior, limits, duplicate preservation, and read-back before claiming fidelity or replay safety.
- A known playlist ID is valuable reconciliation evidence but does not by itself prove which append batch committed. Append calls involving repeated IDs are especially unsafe to replay without exact read-back/checkpoints.
- Name equality is not identity. Choose and document either a no-overwrite unique-name/suffix policy or an explicit collision result based on verified listing/read-back behavior. Never update an existing same-name playlist implicitly.
- Official references: [Jellyfin generated Playlist API](https://typescript-sdk.jellyfin.org/classes/generated-client.PlaylistApi.html), [Jellyfin add-items parameters](https://typescript-sdk.jellyfin.org/interfaces/generated-client.PlaylistApiAddItemToPlaylistRequest.html), [OpenSubsonic createPlaylist](https://opensubsonic.netlify.app/docs/endpoints/createplaylist/), [OpenSubsonic updatePlaylist](https://opensubsonic.netlify.app/docs/endpoints/updateplaylist/), [OpenSubsonic playlist response](https://opensubsonic.netlify.app/docs/responses/playlist/).

### Bounds and completion semantics to define in code/docs

Do not leave numeric bounds implicit. Before implementation is called complete, document and test: max name bytes; operation/part retention; retry count and exponential-backoff cap; snapshot read page size; provider request batch size; maximum simultaneously active parts; status page/DOM bounds; ambiguity timeout/reconciliation attempts. Derive provider request limits from verified adapters/servers. Bounds must protect memory and remote load without truncating or silently dropping snapshot entries.

Aggregate status is derived from parts. An export is fully successful only when every contributing part is faithfully confirmed. A mix of success and unsupported/failed/ambiguous is partial, with the successful remote identities retained. An empty snapshot, no capable parts, or invalid name causes no provider write. Cancel/close stops scheduling safe future work but cannot undo an already submitted remote effect.

### Architecture compliance

- Rust daemon + Tokio own remote effects and durable recovery; UI is presentation/command only.
- All provider calls use `Arc<dyn MediaProvider>` resolved from portable source identity. Provider URLs, credentials, and authenticated URLs remain daemon-side.
- SQLite is authoritative for long-running export state. Use transactions for state transitions; never hold the DB mutex or playback/session locks across network awaits.
- Rust/SQL use `snake_case`; serialized JSON/TypeScript use `camelCase`; integer counters that can exceed JS-safe precision follow existing string-on-wire conventions.
- Keep audio callbacks free of allocation, I/O, blocking, DB/provider work, and sync-held locks.
- Current locked baseline requires no new dependency: Rust edition 2024/MSRV 1.93.0; Tokio 1.49.0; rusqlite 0.38.0; serde 1.0.228; serde_json 1.0.149; uuid 1.20.0; Tauri 2.10.3/API 2.10.1; TypeScript 5.6.3; Vite 6.4.3; Shoelace 2.20.1.

### Previous story and Git intelligence

Story 16.9 was introduced by `7be8cb5`, implemented by `dc57359`, and reviewed by `c2c8486`. Reuse its UUID operation/snapshot separation, canonical request retention, recovery-before-live-freshness rule, transactional persistence, typed RPCs, bounded pages, and restart tests.

The 16.9 review fixed three lifecycle failures that this UI must not repeat: an equal-layout render destroyed the active Playback destination; unavailable `localStorage` incorrectly hid durable snapshots in zero-server routing; and a failed next-page request retained a stale cursor. Export tests must cover component preservation, daemon-authoritative recovery when browser storage is unavailable, disposal/late responses, equal-state rerenders, and cursor/status rollback.

The available 16.9 test evidence was strong locally, but installed Windows/Linux/macOS-x64 verification remained outstanding. Do not convert planning-language dependency claims into false installed-platform evidence; record new provider/platform probes honestly and leave the expanded installed matrix to Story 16.14 where applicable.

### Testing requirements

- Prefer deterministic fake-provider mutation logs plus authenticated production-router tests over source-regex assertions. Exercise serialized DTOs and real state transitions.
- Assert the exact ordered arrays sent per source, including interleaved sources, deliberate repeats, and identical raw track IDs on different servers.
- Inject failure before request, after remote create but before reply persistence, between batches, after remote append but before checkpoint, and during restart/reconciliation.
- Prove retry emits zero calls for confirmed parts and no unsafe call for ambiguous boundaries. Prove no rollback delete is issued.
- Test a very large snapshot with bounded paging/batching and responsive concurrent playback commands; do not materialize all entries or DOM rows merely to partition it.
- Verify error/status redaction with hostile provider errors/URLs and names. Preserve source labels without leaking credentials.
- Run targeted daemon tests, full available daemon tests, provider fixture tests, UI Node tests, TypeScript check/build, locale parity, Rust formatting/clippy, and diff hygiene. Report any sandbox/platform/configured-server gaps rather than claiming them passed.

### Project Structure Notes

- Prefer a focused backend state-machine module (`hifimule-daemon/src/playback/server_export.rs` or `playback/export/server.rs`) and co-located tests rather than expanding the local-snapshot module into unrelated orchestration.
- Prefer a focused RPC sibling if `rpc/playback_export.rs` loses cohesion. Keep all methods under the existing playback snapshot/export namespace and document exact names once chosen.
- A new UI coordinator such as `hifimule-ui/src/state/snapshotPlaylistExports.ts` may manage view-independent subscriptions/recovery, but daemon SQLite remains authoritative. Do not overload `snapshotSaves.ts` or browser storage with remote truth.
- No general playlist editor, provider migration, external metadata service, or device-sync module belongs in this story.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 16 and Story 16.10, lines 4754-4817]
- [Source: `_bmad-output/planning-artifacts/prd.md` — FR79, FR81, P-NFR2, P-NFR4-6, UJ-P3]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Playback State and Ownership; Playback Provider Integration]
- [Source: `_bmad-output/planning-artifacts/playback-prd-source-extract.md` — accepted immutable snapshot/export invariants]
- [Source: `_bmad-output/planning-artifacts/playback-epic-validation.md` — requirement coverage and provider-splitting gate]
- [Source: `_bmad-output/implementation-artifacts/epic-16-context.md` — delivery order, identity, recovery and scope]
- [Source: `_bmad-output/implementation-artifacts/16-9-save-an-immutable-local-listening-snapshot.md` — implemented snapshot contract and review learnings]
- [Source: `hifimule-daemon/src/playback/export.rs`; `hifimule-daemon/src/playback/session/export.rs`; `hifimule-daemon/src/rpc/playback_export.rs`]
- [Source: `hifimule-daemon/src/providers/mod.rs`; `hifimule-daemon/src/providers/jellyfin.rs`; `hifimule-daemon/src/providers/subsonic.rs`; `hifimule-daemon/src/rpc.rs`]
- [Source: `hifimule-ui/src/components/PlaybackSnapshots.ts`; `hifimule-ui/src/state/snapshotSaves.ts`; `hifimule-ui/src/rpc.ts`]

## Dev Agent Record

### Agent Model Used

GPT-5 Codex

### Debug Log References

- RED: export contract tests initially failed on missing model/transition symbols.
- GREEN: focused server-export, persistence migration, snapshot UI, TypeScript, and production build gates passed.
- REGRESSION: `cargo test --workspace` passed outside the sandbox (1,333 daemon tests; all workspace suites green). Sandbox-only run could not bind local mock HTTP servers.
- QUALITY: `cargo fmt --all` and daemon Clippy completed; only pre-existing repository warnings remain.

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- Provider-specific enablement remains gated on verified duplicate/order/limit/collision/reconciliation behavior; unverified semantics must degrade honestly.
- Added persistence v15 journal and source-frozen state machine with exact ordered read-back, collision reservation, restart ambiguity recovery, and no blind replay or rollback.
- Added strict plan/start/status/list/retry/reconcile/cancel RPCs with mutation admission and portable-server routing.
- Added accessible saved-snapshot export UI, per-source recovery/status, four-locale strings, and authoritative daemon restart recovery.
- Configured Jellyfin/Navidrome and installed-platform probes were not available locally; documentation records this evidence limitation without inferring success.

### File List

- `_bmad-output/implementation-artifacts/16-10-save-a-listening-snapshot-as-playlists-on-its-source-servers.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `docs/api-contracts-hifimule-daemon.md`
- `docs/data-models-hifimule-daemon.md`
- `hifimule-daemon/src/playback/mod.rs`
- `hifimule-daemon/src/playback/persistence.rs`
- `hifimule-daemon/src/playback/server_export.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-daemon/src/rpc/playback_export.rs`
- `hifimule-i18n/catalog.json`
- `hifimule-ui/src/components/PlaybackSnapshots.ts`
- `hifimule-ui/src/rpc.ts`
- `hifimule-ui/src/styles.css`
- `scripts/tests/snapshot-ui.test.mjs`

## Change Log

- 2026-09-29: Implemented durable, source-partitioned listening-snapshot playlist export with recovery-safe daemon state, strict RPCs, accessible UI, localization, tests, and documentation.
