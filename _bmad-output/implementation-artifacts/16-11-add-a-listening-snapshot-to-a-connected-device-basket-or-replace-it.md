---
baseline_commit: 0357befb460cbc4277f1a59e32fc3dc250cee816
---

# Story 16.11: Add a listening snapshot to a connected device basket or replace it

Status: review

## Story

As a HifiMule user,
I want to add my saved listening selection to a connected device basket or replace that basket,
so that I can take the music with me using HifiMule's existing sync workflow.

**Requirements:** FR80; applicable operation-integrity portion of FR81; P-NFR4-6; physical-target portion of P-AR11; P-UX-DR9 and P-UX-DR14-15.

**Dependencies:** Delivered Epic 15 playback foundation (Stories 15.1-15.17), completed Stories 16.1-16.10, and existing physical-device basket behavior. This story consumes the immutable local snapshot from Story 16.9. It does not recapture the live session, start sync, write or delete device media, or create a live link from Radio to the basket.

## Acceptance Criteria

1. **Eligible physical target and explicit actions.** Given a saved snapshot and an eligible connected, configured physical device, the saved-snapshot view offers separate **Add to basket** and **Replace basket** actions and clearly identifies the frozen target device by name and stable identity. Playback, a blank/pending device, or an unconfigured device is never offered as a physical target. If no eligible target exists, both actions are unavailable with an accessible explanation.
2. **Add semantics and fidelity.** Add retains the target's existing basket order and content, then incorporates compatible snapshot entries in snapshot order using the documented basket identity and duplicate policy. Deliberate repeats, identical raw track IDs on different servers, and other representation limits are disclosed before confirmation; the result is never called faithful when the basket cannot represent it faithfully. The immutable snapshot is unchanged.
3. **Atomic Replace semantics.** Replace swaps only the intended target's basket selection for the complete validated snapshot selection as one consistent manifest mutation. It cannot degrade into clear-plus-add, accidental append, partial replacement, another device's mutation, sync, or deletion of files already on the device. Other manifest fields, including auto-fill configuration, remain unchanged. The destructive confirmation names the target and scope.
4. **Preflight validation without silent narrowing.** Before confirmation, the daemon validates every snapshot entry against known portable server identities, basket/provider source locks, supported basket item types, identity collisions, duplicate/order fidelity, metadata/size availability, and downstream manual- and auto-sync resolvability. Mixed-source snapshots remain allowed only when every enabled sync path for that target routes each item by portable source identity. If auto-sync or another existing path cannot do so safely, this story must minimally fix that routing or block the affected export with a disclosed reason. Unsupported entries are returned with structured reasons; there is no silent source substitution, dropped entry, deduplication, or partial Replace presented as complete.
5. **Frozen target and commit-time revalidation.** Planning freezes `snapshotId`, `targetDeviceId`, target display identity, observed destination identity/revision, and observed basket revision/hash. Immediately before commit the daemon re-resolves that same configured physical device by durable device ID and revalidates its connection, identity and basket state. Selection of another device, disconnect, reconfiguration, mount-path reuse, or a newly selected target never redirects the operation; an invalid or changed target is rejected with a recoverable status.
6. **Concurrency policy.** Replace requires the exact planned basket revision/hash and returns a structured conflict rather than overwriting an unseen edit; it is never automatically replayed. Add executes under the target's per-device manifest commit lock and atomically merges against the latest basket: retain the latest existing vector unchanged, append only planned snapshot entries not already present under the frozen provider-identity policy, and preserve allowed snapshot-internal repeats in snapshot order. A same-identity entry with changed metadata retains the existing entry unless the frozen contract explicitly defines a safe refresh. UI local state and debounced legacy saves cannot race behind an accepted export.
7. **Durable idempotency and interruption recovery.** Each accepted mutation has a durable operation ID and canonical request. Reusing an operation ID with the same request returns its authoritative result; mismatched reuse is rejected. Recovery distinguishes intent recorded, manifest commit not started, commit confirmed, conflict, confirmed rollback/failure, and commit-uncertain/unresolved evidence. Persist canonical pre- and post-mutation basket hashes before device mutation. A retry never blindly duplicates Add or reapplies Replace; interruption cannot expose a partially serialized basket. The last valid basket is guaranteed only when rollback is confirmed; a rename/fsync or MTP/cache uncertainty is reconciled from device/cache evidence after restart/reconnect rather than mislabeled as failure.
8. **Truthful completion and isolation.** Success returns the authoritative target basket/revision or a refresh token, visibly announces the action and device, and rehydrates the UI from daemon state. Playback, queue, snapshot, reporting, feedback and server-playlist-export state remain unchanged. Sync starts only through the existing separate user action, and later playback/Radio changes never update the basket automatically.
9. **Verification and accessibility.** Deterministic tests cover Add, Replace, empty snapshot, no/blank/pending/Playback target, mixed sources, same raw IDs on different servers, deliberate repeats, unsupported content, target switch/disconnect/path reuse, concurrent edits, duplicate/mismatched operation IDs, interruption/restart, and persistence failure. They prove all other device manifests and device files are preserved and no sync starts. UI tests cover keyboard operation, visible focus, destructive confirmation, target/error announcements, stale responses and component disposal. Available Windows/macOS/Linux builds are exercised; installed-platform evidence not available here remains assigned to Story 16.14.

## Tasks / Subtasks

- [x] Freeze the basket-export contract and representation policy before coding (AC: 1-9)
  - [x] Define exact plan/commit/status/list/recover RPC names, DTOs, stable error codes, operation states, transition table and `schemaVersion: 1`; use strict `deny_unknown_fields` params and camelCase wire names.
  - [x] Freeze repeat/collision policy: either extend the model with a separate basket occurrence/key while retaining the real provider `trackId`, or reject/disclose repeats and same-raw-ID collisions that downstream UI/sync cannot preserve. Never put synthetic occurrence IDs where providers expect track IDs.
  - [x] Encode Add's latest-state atomic merge and Replace's exact-revision semantics. Document identity comparison, existing-entry preservation, allowed snapshot-internal repeats, metadata conflicts, faithful-result criteria and blocking limitations.
  - [x] Define stable target identity and revision/hash inputs without treating a mount path or current UI selection as identity.
- [x] Add a target-bound atomic basket mutation primitive (AC: 2-7)
  - [x] Build on `DeviceManager::update_manifest_for_device*`, `get_manifest_for_device` and existing per-device `manifest_commit_locks`; resolve by durable `device_id`, not current selection.
  - [x] Add a result-bearing checked-update primitive that acquires the per-device commit lock, re-resolves identity, computes the canonical basket hash, conditionally mutates/persists, and returns the authoritative manifest/hash. Do not check before calling the existing non-fallible closure API and create a TOCTOU window.
  - [x] Define the canonical hash over the ordered full basket representation plus basket schema/policy version, with stable serialization and no unrelated manifest fields.
  - [x] Add preserves current content/order according to the frozen policy; Replace assigns the entire validated vector once. Mutate only `basket_items`; preserve auto-fill, profiles, synced items, playlists, dirty state and every other device manifest.
  - [x] Preserve existing verified/atomic manifest persistence and rollback behavior for MSC/MTP/cache paths. Model `ManifestCacheCommitUncertain` and equivalent post-rename/fsync uncertainty explicitly; reconcile pre/post hashes and device/cache copies before claiming either old or new state.
- [x] Implement bounded snapshot planning and conversion (AC: 1-5)
  - [x] Page Story 16.9 entries by ascending global ordinal from daemon persistence; preserve its frozen `serverId`, `trackId`, occurrence identity and order. Never build identity/order from live playback, a rendered UI page, the selected server, or Story 16.10 remote export state.
  - [x] Resolve each frozen `(serverId, trackId)` read-only through its provider to obtain the `BasketItem` type, name, duration, child count and authoritative size needed by basket capacity/sync. Define supported track-type mapping and metadata bounds; provider unavailable, not found, type changed, or incomplete/overflow metadata is a structured limitation, not fabricated `Audio`/zero-size data or source substitution.
  - [x] Validate configured portable sources, target/source locks, basket item representation, identity collisions, repeats, and both manual- and auto-sync routing before permitting Replace or claiming faithful Add. Where auto-sync currently uses one selected provider for a mixed-source basket, fix routing minimally or block export for that target with an explanation.
  - [x] Return target display name/icon/ID, observed target/basket revision, entry counts, ordered limitations and exact projected action. Bound snapshot paging and planning memory without truncating entries.
- [x] Add durable local mutation operation state and recovery (AC: 5-8)
  - [x] Prefer a focused `playback/basket_export.rs` journal/state machine. If stored in playback SQLite, add schema v16 with migration, rollback and future-version tests while preserving v1-v15 data.
  - [x] Persist operation UUID, canonical request, `snapshotId`, action, target identity, observed basket revision/hash, canonical pre/post basket hashes, planned identity/order policy, timestamps/retention, outcome and evidence.
  - [x] Persist intent before manifest mutation and confirmed authoritative basket revision/result immediately after it. On restart, reconcile against operation evidence and the exact target manifest; never infer success merely from current UI state.
  - [x] Bound retention/recovery attempts and clean retained operation rows transactionally. Keep device/database locks scoped; do not hold playback/session or database mutexes across unrelated awaits.
- [x] Expose strict daemon RPC and admission boundaries (AC: 1, 4-8)
  - [x] Add focused basket-export RPC handlers or extend the playback export namespace without conflating local basket mutation with remote server-playlist export.
  - [x] Classify commit/recover/retry calls that can mutate a basket as mutating in `rpc::is_mutating_method`; respect shutdown admission. Plan/status calls stay read-only only when they cannot advance work.
  - [x] Serialize legacy `manifest_save_basket` through the same per-device commit lock/revision epoch so another window, old client or ordinary action cannot overwrite an accepted export. Preserve backward compatibility; current clients send target/revision identity when available and stale writes return a conflict.
  - [x] Return structured limitation/conflict/interruption states and authoritative target/basket data. Do not return credentials, authenticated URLs or raw mount paths unnecessarily.
  - [x] Keep legacy `manifest_save_basket` compatible for ordinary curation, but do not route snapshot Add/Replace through its current-device whole-vector save contract.
- [x] Add the accessible saved-snapshot device export experience (AC: 1-9)
  - [x] In `PlaybackSnapshots`, place physical-device Add/Replace separately from Save snapshot and Save to server playlists. Show the selected eligible target's configured name/icon and explain disabled states.
  - [x] Show preflight counts/limitations before confirmation. Require an explicit target-named destructive confirmation for Replace; do not use UI-side `clear()` followed by `add()` loops or optimistic localStorage as operation truth.
  - [x] Fence every async result by snapshot ID, operation ID and target device ID. A snapshot/device switch, equal-layout rerender, late reply or component disposal cannot update the wrong view or target.
  - [x] Before plan/commit, await or drain both pending and already-dispatched legacy basket saves behind a generation+target fence. After success/conflict/recovery, rehydrate `basketStore` from the authoritative daemon result; a stale legacy completion/localStorage hydration cannot overwrite it.
  - [x] Add English, French, Spanish and German strings, live status announcements, visible focus and responsive styling without obscuring saved entries or existing export controls.
- [x] Add backend, UI, integration and documentation evidence (AC: 1-9)
  - [x] Backend/device tests cover exact Add/Replace order, target-bound updates after UI selection changes, per-device lock races, revision conflicts, provider unavailable/not-found/type-changed/metadata-overflow conversion, collision/repeat policy, operation replay/reuse mismatch and persistence rollback.
  - [x] Failure-injection tests cover exit/failure before intent, after intent/before manifest commit, after commit/before acknowledgement, MSC/MTP rename/fsync commit uncertainty, and restart/reconnect reconciliation. Assert no duplicate Add, destructive replay, partial serialization, file deletion or sync call.
  - [x] UI production-component tests cover eligibility, target naming, confirmation, conflicts, daemon-authoritative recovery, localStorage failure, pending legacy save coordination, stale/late replies, keyboard/focus and locale parity.
  - [x] Concurrency tests admit a legacy basket-save RPC after export planning and complete it after export commit, including a second-client simulation; assert revision conflict or safe serialization, never a lost export update.
  - [x] End-to-end regression tests export then run both manual sync and auto-sync for supported single- and mixed-source cases, proving exact source routing and no raw-ID deduplication loss. Also prove export itself causes no playback/session/snapshot/provider-playlist mutation, provider write, sync start, other-device mutation or non-basket manifest change.
  - [x] Update daemon API/data-model and device/basket architecture docs with schemas, identity/revision policy, operation transitions, limits, fidelity rules and evidence gaps.

## Dev Notes

### Non-negotiable implementation contract

Story 16.9's daemon-owned immutable snapshot is the only input. It contains ordered occurrences with frozen `TrackSource { serverId, trackId }`; do not reinterpret the live queue or currently browsed server. The complete cross-server order remains in that snapshot even if today's basket cannot reproduce all occurrence semantics.

This is a local durable mutation, not a remote provider export. Unlike Story 16.10, HifiMule controls the target manifest and can require an atomic compare-and-mutate boundary. Use that stronger guarantee: validate all entries before Replace, serialize by stable device identity, mutate the complete vector once, and distinguish confirmed rollback from commit-uncertain persistence. An operation journal provides idempotent acknowledgement/recovery; it must not turn a recoverable local mutation into a blind retry workflow.

The current basket path is unsafe for this feature as-is:

- `manifest_save_basket` accepts a whole vector and writes whichever device is selected at handling time. It has no target ID, expected revision or operation ID.
- `basketStore` is a `Map<string, BasketItem>` keyed only by raw item ID. Same raw IDs across servers and deliberate repeated occurrences collide; `add()` also overwrites source identity with the active browsed server. If the model is extended, its UI occurrence key must remain separate from the provider track ID used by sync/provider calls.
- `basketStore.clear()` plus multiple `add()` calls is neither atomic nor target-stable, and its one-second debounced save can overwrite newer daemon state.
- `reconcile_basket_server_ids` silently drops unknown/removed sources. Snapshot export must instead expose unsupported entries and refuse an inexact Replace.

### Current files that must be understood before editing

- `hifimule-daemon/src/device/mod.rs`: `BasketItem`, `DeviceManifest`, destination identity/revision, per-device manifest commit locks, stable-device helpers and atomic/verified persistence. Current `save_basket` uses the selected device; add a stable-target seam rather than depending on it.
- `hifimule-daemon/src/rpc.rs`: current basket handlers, router and mutating-method admission. Preserve ordinary basket callers while adding strict target-bound commands.
- `hifimule-daemon/src/main.rs` and the manual delta/sync resolver: downstream basket expansion and auto-sync routing. Current auto-sync can pass the whole basket through one selected provider, and raw-ID deduplication can lose cross-source or repeated entries; affected exports remain blocked until these paths are safe.
- `hifimule-daemon/src/playback/export.rs`: authoritative immutable snapshot header/entries and bounded reads. Snapshot semantics must not change.
- `hifimule-daemon/src/playback/server_export.rs`: reusable Story 16.10 operation identity, canonical-request, retention and recovery patterns. Do not copy its provider-specific ambiguity model where local atomicity can give a stronger result.
- `hifimule-daemon/src/playback/persistence.rs`: current schema v15 and migration/future-version behavior; a durable basket-export journal implies v16.
- `hifimule-daemon/src/rpc/playback_export.rs`: current snapshot/server-export namespace and strict DTO conventions. Use a focused sibling if cohesion would suffer.
- `hifimule-ui/src/state/basket.ts`: ordinary curation/localStorage/debounced-save behavior. Add only the coordination/authoritative hydration seam needed after daemon commit; do not make it the export state machine.
- `hifimule-ui/src/components/PlaybackSnapshots.ts`: correct saved-snapshot action surface, existing async fences, playlist-export status and paging lifecycle.
- `hifimule-ui/src/rpc.ts`, `hifimule-i18n/catalog.json`, `hifimule-ui/src/styles.css`, `scripts/tests/snapshot-ui.test.mjs`: DTOs, locale parity, presentation and production-component tests.
- `docs/api-contracts-hifimule-daemon.md`, `docs/data-models-hifimule-daemon.md`, `docs/architecture-hifimule-ui.md`: authoritative contracts to update.

### Existing behavior to preserve

- Device manifests are portable and basket/source routing uses deterministic portable server IDs; machine-local server UUIDs and selected/browsed server state are not export identities.
- A basket may contain mixed-server items; non-selected-server entries can render read-only. Story 16.11 must respect that model rather than force all entries to the browsed server.
- Device auto-fill settings live separately in the manifest and must survive Replace unchanged. Existing sync, initialization, repair and ordinary basket curation continue to work.
- Physical target admission is separate from Playback. No device only disables physical basket/sync actions; saved snapshots and playback remain usable.
- Snapshot entries survive server removal for inspection. That does not make an unavailable source basket-compatible; surface the limitation before mutation.
- Saving a basket never starts sync. This story also performs no provider write, playback transport command or device media-file deletion.

### Architecture compliance and bounds

- Rust daemon owns target validation, conversion, durable operation state and manifest commit; TypeScript UI presents plans and sends commands.
- Rust/SQL names are `snake_case`; serialized JSON/TypeScript names are `camelCase`. Use string wire values for revisions/counters that can exceed JavaScript's safe integer range.
- Use stable `device_id` plus current connected/configured evidence. Mount paths may be implementation evidence but never the sole identity. Do not redirect to the currently selected target.
- Do not hold database, playback/session or device-manager state locks across file/device I/O. Revalidate and commit inside the existing per-device serialization boundary.
- Set and test explicit limits for operation retention, snapshot page size, maximum bounded validation batch, limitation/status page size and operation-ID retention. Limits protect resources; they never silently truncate an export.
- Bound provider-resolution fan-out, per-request and overall planning deadlines, and cancellation. Aggregate entry limitations deterministically in snapshot order; timeout/cancellation performs no commit and never leaves a partial plan confirmable.
- No new dependency is required. Retain the locked baseline: Rust edition 2024/MSRV 1.93.0; Tokio 1.49.0; rusqlite 0.38.0; serde 1.0.228; serde_json 1.0.149; uuid 1.20.0; Tauri 2.10.x/API 2.10.x; TypeScript 5.6.3; Vite 6.4.3; Shoelace 2.20.1.

### RPC contract to freeze before implementation

| Method | Mutating | Required identity | Result / idempotency |
|---|---:|---|---|
| `playback.planSnapshotBasketExport` | No | `snapshotId`, `targetDeviceId`, action | Frozen target/basket hash, projected counts, limitations and canonical plan; performs no mutation. |
| `playback.startSnapshotBasketExport` | Yes | canonical plan/request + operation ID | Atomically commits or returns conflict/limitation/commit-uncertain state; same operation+request returns the authoritative result, mismatched reuse fails. |
| `playback.getSnapshotBasketExport` | No | operation ID | Durable current state, target identity, pre/post hashes and authoritative basket revision when known. |
| `playback.listSnapshotBasketExports` | No | snapshot ID with bounded page | Recoverable operations only; never schedules work. |
| `playback.reconcileSnapshotBasketExport` | Yes | operation ID | Re-reads exact target/cache evidence for commit-uncertain state; never reapplies Add/Replace. |

If implementation uses different names, update this table and all router/admission/UI/docs/tests together before coding proceeds. Recovery that can advance or mutate durable state remains behind mutation admission.

### Previous story and Git intelligence

Story 16.10 was introduced by `d75cd5f`, implemented by `6232f74` and reviewed by `0357bef`. Reuse its operation UUID/canonical-request validation, strict typed RPCs, bounded paging, durable recovery, status truthfulness, stale-response fencing and restart tests.

Carry forward its review lessons: never retain a database mutex while serving an idempotent repeat; recover pending journal state after restart; capability/compatibility-gate every part before confirmation; derive retry safety from persisted evidence; clean retained rows transactionally; do not strand durable work on client cancellation; and surface recovery-load failures.

Carry forward the Story 16.9/16.10 UI regression guards: equal-layout renders must not destroy the active component, browser/localStorage failure must not hide daemon-owned state, and failed pages/stale async responses must not retain misleading cursors or statuses. Configured-provider and full installed-platform evidence was unavailable for 16.10; do not manufacture it here. Story 16.14 owns the expanded installed matrix.

### Latest technical information

No external provider endpoint or new library is needed. Keep the repository's locked stack and internal JSON-RPC/Tauri proxy. Current Tauri 2 guidance supports typed async commands/invocation, but the existing release boundary remains `rpc_proxy`; do not introduce a parallel frontend authority. SQLite transactions provide atomic database journal updates, while device-manifest atomicity must continue to use the repository's verified per-device persistence/rollback path; a SQLite commit alone cannot prove a removable-device manifest write.

### Project Structure Notes

- Preferred new backend modules: `hifimule-daemon/src/playback/basket_export.rs` and, if needed, `hifimule-daemon/src/rpc/playback_basket_export.rs`.
- Prefer a focused module over mixing physical-manifest mutation into provider-specific `server_export.rs`.
- Keep `playback/export.rs` focused on immutable local snapshots; add only bounded internal read helpers if necessary.
- Do not redesign the general basket, cross-server sync resolver, device discovery or sync engine unless the frozen representation contract proves a minimal prerequisite is necessary. If faithful approved mixed-source content cannot be represented without a broader redesign, report the blocker and split the prerequisite rather than silently narrowing FR80.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 16 and Story 16.11, lines 4819-4877]
- [Source: `_bmad-output/planning-artifacts/prd.md` — FR78-81, P-NFR4-6, UJ-P3, physical-target implementation decision]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Playback Provider Integration; Playback UI and Session Control; Playback Implementation Contracts; Project Structure]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md` — Sync Basket, Device Hub, responsive accessibility]
- [Source: `_bmad-output/planning-artifacts/playback-epic-validation.md` — Story 16.11 mixed-source/basket constraint gate]
- [Source: `_bmad-output/implementation-artifacts/epic-16-context.md` — sequence, identity, recovery and scope]
- [Source: `_bmad-output/implementation-artifacts/16-9-save-an-immutable-local-listening-snapshot.md` — implemented immutable snapshot contract]
- [Source: `_bmad-output/implementation-artifacts/16-10-save-a-listening-snapshot-as-playlists-on-its-source-servers.md` — durable operation and review learnings]
- [Source: `hifimule-daemon/src/device/mod.rs`; `hifimule-daemon/src/rpc.rs`; `hifimule-daemon/src/playback/export.rs`; `hifimule-daemon/src/playback/server_export.rs`; `hifimule-daemon/src/playback/persistence.rs`]
- [Source: `hifimule-ui/src/state/basket.ts`; `hifimule-ui/src/components/PlaybackSnapshots.ts`; `hifimule-ui/src/rpc.ts`]
- [External: Tauri 2 Calling Rust from the Frontend](https://v2.tauri.app/develop/calling-rust/)
- [External: SQLite Atomic Commit](https://www.sqlite.org/atomiccommit.html)

## Dev Agent Record

### Agent Model Used

GPT-5 Codex

### Debug Log References

- 2026-09-29: `rtk npm run build --prefix hifimule-ui` passed (TypeScript + Vite production build).
- 2026-09-29: `rtk test node --test scripts/tests/snapshot-ui.test.mjs` passed.
- 2026-09-29: targeted daemon basket-export tests passed (3/3), including durable operation reuse and restart uncertainty.
- 2026-09-29: full daemon regression attempted through the controlled FFmpeg wrapper: 1,091 passed, 245 existing network/provider tests failed under the restricted environment; completion gate remains blocked.
- 2026-09-29: Alexis reran the full test suite in the normal project environment and confirmed all tests pass, clearing the regression gate.

### Completion Notes List

- Current basket whole-vector/current-target/debounced UI path is explicitly barred from snapshot Add/Replace until stable target, revision, representation and idempotency contracts are added.
- No new dependency or provider API is required; external research confirmed the existing Tauri command and SQLite atomic-journal patterns, while removable-device manifest integrity remains repository-specific.
- Implemented the durable target-bound contract, atomic Add/Replace primitive, bounded provider preflight, journal/recovery hashes, strict RPC admission, legacy-save revision fencing, accessible four-locale UI, and architecture/API/data-model documentation.
- User confirmed the full regression suite passes in their environment; all acceptance and definition-of-done gates are satisfied.

### File List

- `_bmad-output/implementation-artifacts/16-11-add-a-listening-snapshot-to-a-connected-device-basket-or-replace-it.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `docs/api-contracts-hifimule-daemon.md`
- `docs/architecture-hifimule-ui.md`
- `docs/data-models-hifimule-daemon.md`
- `hifimule-daemon/src/device/mod.rs`
- `hifimule-daemon/src/playback/basket_export.rs`
- `hifimule-daemon/src/playback/mod.rs`
- `hifimule-daemon/src/playback/persistence.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-daemon/src/rpc/playback_export.rs`
- `hifimule-i18n/catalog.json`
- `hifimule-ui/src/components/BasketSidebar.ts`
- `hifimule-ui/src/components/DestinationHub.ts`
- `hifimule-ui/src/components/PlaybackSnapshots.ts`
- `hifimule-ui/src/rpc.ts`
- `hifimule-ui/src/state/basket.ts`
- `hifimule-ui/src/styles.css`

### Change Log

- 2026-09-29: Added target-bound listening-snapshot basket export foundation; validation remains in progress because the full regression gate is not green.
- 2026-09-29: User-confirmed full regression pass; completed schema v16/list/recover contract and moved story to review.
