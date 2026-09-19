---
baseline_commit: bf8f16fb4b7ae05ce4d05b8bb962d26f881a3a86
---

# Story 15.12: Access Playback as an always-available destination

Status: review

## Story

As a HifiMule user,
I want Playback listed alongside my physical devices even when none are connected,
so that I can listen and curate my local listening queue without attaching a sync device.

**Requirements:** Destination and currently delivered configuration portions of FR55; device-navigation portion of FR59; amended FR33; P-NFR4–6; configuration portion of P-AR4; P-UX-DR1, P-UX-DR6, P-UX-DR10, P-UX-DR12–15.

**Dependencies:** Stories 15.1–15.11 are done in sprint tracking. Prepared 2026-09-19 against `bf8f16f` (Review 15.11). Reuse the existing daemon-owned main/preview session, output selection, versioned local output configuration, authoritative session snapshot and paged occurrence API. Story 15.11 leaves installed Windows/Linux/macOS physical-output evidence open; do not convert that inherited limitation into a passing claim here.

**Scope:** Typed destination navigation, deterministic physical-device arrival/removal behavior, a read-only Playback destination backed by the authoritative main queue, existing output/configuration status, unconfigured-device setup, actionable device-open failures, responsive accessibility and physical-action isolation. Story 15.13 owns queue mutation, 15.14 the floating bar, 15.15 Playback source/selection settings, and 15.16–20 Radio/Play something. No playback engine/provider/persistence redesign, fake mounted Playback device, manifest field, auto-fill copy, server playlist mutation, basket export, audio dependency upgrade or automatic listening report belongs here.

## Acceptance Criteria

1. **Playback is first and typed.** Given any number of physical devices, when destinations are displayed, Playback is always present and listed first. The state and wire/UI types distinguish `playback`, managed physical device and pending/unconfigured physical device; Playback is identified as a local listening context and is never represented by a magic path, mount, manifest or synchronizable `connectedDevices` entry.
2. **No-device fallback preserves listening.** Given no physical device is connected, when the UI opens or the selected physical destination disconnects, Playback is selected. Library browsing, Play, Preview and existing transport/output controls remain usable. Only physical basket-add, capacity, folder, manifest, repair, auto-fill and sync actions are unavailable; the legacy global no-device lock is narrowed to those actions.
3. **Read-only authoritative destination.** Given Playback is selected, when its view is displayed, it shows the daemon's current active listening state and a meaningful, source-labelled canonical main queue read-only, using `playback.getSession`, revision-bound `playback.listOccurrences` paging and the bounded occurrence-metadata contract below. Never render opaque provider IDs as track labels or issue unbounded/N+1 browser calls. During Preview, label the audition separately while keeping the preserved main queue canonical. With no main queue, show an actionable empty state directing the user to manual library Play/Preview. Do not show edit/reorder/remove, Radio, Play something, source-selection or other future controls.
4. **Arrival, setup and failure are deterministic.** Given a physical device arrives, when its discovery intent is applied, select that arriving physical destination and visibly identify it without moving keyboard focus. Discovery and explicit selections share one daemon-owned monotonic mutation order; an arrival first observed before a later explicit choice cannot steal selection when its slow probe finishes, while a genuinely later arrival can. Any number of blank/unconfigured arrivals remain represented as distinct pending destinations with target-safe accessible Setup actions. A sanitized, deduplicated open/read failure is shown as a non-selectable issue with recovery guidance and bounded retry instead of making the device silently disappear; a failed arrival does not change the current destination. Removing the selected device falls back to Playback, never to `HashMap` iteration order; removing another device does not change the destination or an unrelated active sync.
5. **Navigation cannot mutate listening identity.** Given main playback or an audition is active, when the user changes destination, browses another server, or a physical device arrives/disconnects, the daemon `instanceId`, `sessionId`, queue/generation identity, source server, current occurrence and main/preview mode remain unchanged unless an independent audio-output loss occurs. Destination events call no playback mutation and never reroute provider commands through the currently browsed server.
6. **Configuration stays local, minimal and recoverable.** Given Playback configuration is loaded or changed, when HifiMule stores values used by delivered behavior, reuse the strict local `playback.json` schema v1 (`output`, default `null`) and its validation, bounded read, serialized atomic write and invalid-file preservation/reset behavior. Do not add dormant source/Radio fields, copy device/server auto-fill settings, or modify portable manifests. `OUTPUT_CONFIG_INVALID` remains recoverable through existing output selection/reset; it never removes Playback or overwrites valid device configuration.
7. **Reconnect uses one authoritative presentation owner.** Given the UI reconnects, a page cursor becomes stale or configuration cannot load, when the destination renders, replace presentation from a fresh daemon session snapshot and restart paging on session/queue-revision conflict. PlaybackControls and the destination consume one shared UI playback store/poller; neither becomes a second player or competes with another high-frequency poll loop. Invalid configuration or transient server-label failure produces a localized recoverable explanation without discarding valid queue/device state.
8. **Accessible cross-platform evidence.** Given narrow (<600px), medium (600–1000px) and wide (>1000px) layouts on Windows, macOS and Linux, destination controls expose an accessible group/tab label, selected state, visible high-contrast focus and keyboard operation. Device-arrival selection is announced through a polite live region without focus theft or obscured actions. Automated/installed checks cover no-device launch, managed and blank-device arrival, overlapping arrivals, selected-device removal, open/read failure, server navigation, Preview/main state, invalid config and UI reconnection; they prove physical basket/sync restrictions remain intact and record unrun installed-platform rows honestly.

## Tasks / Subtasks

- [x] Establish the typed destination and device-event contract (AC: 1–2, 4–5, 8).
  - [x] Add a discriminated destination projection such as `Playback | Device { path } | PendingDevice { path }`; keep the daemon's physical sync target separate and reject magic/synthetic Playback paths.
  - [x] Add strict, additive destination selection/state fields to `get_daemon_state` (or a focused authenticated RPC), retaining legacy physical fields for existing consumers during migration.
  - [x] Route discovery intent, user selection and removal through one monotonic daemon mutation clock plus a single state lock; fence slow probes whose observation token predates a later choice. Define selected-removal→Playback fallback and announcement identity; replace nondeterministic remaining-device selection.
  - [x] Replace the singleton pending-device slot with a bounded-by-present-hardware collection ordered by observation token, while retaining same-device MTP-over-MSC deduplication/priority and independent Setup actions.
  - [x] Give every pending entry an opaque `pendingId`; require `device_initialize` to carry `pendingId` plus the observed destination revision and atomically validate/remove that exact entry. Never initialize whichever pending device happens to be globally current.
  - [x] Carry bounded sanitized device-open/read failures (`code`, safe display identity, retryable/recovery hint) from MSC/MTP discovery to UI state. Track failed-but-present devices, deduplicate announcements, retry with bounded backoff, and clear only the matching issue on success/removal. Never expose raw backend diagnostics, credentials or authenticated URLs.
  - [x] Pin every admitted sync/scrobble operation to its captured device path/ID/IO. Selecting another destination or removing an unrelated device cannot retarget/fail it; removing its actual target still fails/cancels it under existing integrity rules.
- [x] Build the always-present destination navigation and physical isolation (AC: 1–2, 4–5, 8).
  - [x] Extract a focused DestinationHub/state boundary rather than further coupling navigation to the large BasketSidebar. Render Playback first even with zero devices and pending/unconfigured devices explicitly.
  - [x] Selecting Playback shows the Playback destination; selecting a managed device flushes pending basket saves, calls the physical selection RPC and rehydrates that device exactly as today. Pending Setup remains accessible.
  - [x] Narrow `device-locked` and all enablement checks to physical basket/sync actions. Preserve browsing, Play, Preview and transport regardless of device presence.
  - [x] Announce programmatic arrival selection without calling `.focus()` or rebuilding the persistent main layout/transport controls.
- [x] Present the daemon-owned queue and delivered Playback settings read-only (AC: 3, 5–7).
  - [x] Add a single shared `state/playback.ts` snapshot/poll store with instance/session/state-sequence ordering; adapt PlaybackControls to subscribe while preserving stable control nodes, focus, interpolation, output and Preview semantics.
  - [x] Type `occurrences`, `totalOccurrenceCount` and `nextCursor` in `rpc.ts`; add the existing `playback.listOccurrences` wrapper with decimal-string revisions and bounded pages (default 100, maximum 200).
  - [x] Add the bounded occurrence-display contract below, resolving each page by portable source, preserving occurrence order/duplicates and returning partial per-row availability rather than opaque IDs or browser-side provider calls.
  - [x] Add a focused PlaybackDestination/PlaybackQueue component that replaces one bounded page at a time, labels active Preview separately, discards stale pages and restarts from a fresh snapshot on session/revision/cursor conflict. Long-history virtualization/editing remains Story 15.13.
  - [x] Show existing output configuration/status only. Render a manual-Play/Preview empty state and localized recoverable errors; omit all future queue, Radio and source-selection controls.
- [x] Apply current responsive design and localization (AC: 1–4, 7–8).
  - [x] Reuse current `DESIGN.md` and implemented CSS tokens: Inter, Void/Panel/Surface, Signal Cyan only for active/actionable state, Amber only for actionable warnings, no decorative glass/card shadow. The current design system supersedes the older purple/Outfit/glass examples in the 2026-01 UX artifact.
  - [x] Use native buttons or a correct WAI-ARIA tabs/composite pattern with selected state and arrow/Enter/Space behavior; retain outline-based `:focus-visible` for Windows high contrast.
  - [x] Add EN/FR/ES/DE catalog parity for destination, local-context, read-only queue, empty/config/failure/recovery and arrival-announcement text.
- [x] Verify behavior and document the contract (AC: 1–8).
  - [x] Add Rust device/RPC tests for ordering, fallback, pending Setup, structured failures and physical-only selection; prove session identity is unchanged across destination events.
  - [x] Add behavior-first DOM tests using production TypeScript components for selection, focus, announcements, physical lock boundaries, Preview separation, paging/conflict recovery and zero playback mutation calls.
  - [x] Run controlled daemon/UI suites and cross-platform installed checks where available; update API/evidence docs and leave unavailable hardware/OS rows open.

## Dev Notes

### Closed implementation gate

These decisions close Story 15.12's planning gate and are requirements, not suggestions:

1. **One navigation selection; physical operations require a device destination.** Destination navigation is a discriminated union. Playback never enters `connected_devices`, `device.list`, device manifests, storage queries, auto-fill or sync enumeration. When Playback or a pending destination is active, the compatibility `selectedDevicePath` projection is `null` and every new/legacy physical-action enablement path must additionally require `selectedDestination.kind === 'device'` plus a currently validated target. Failed discoveries are issues, not selected destinations. Before leaving a managed device, flush its pending basket save; switching back explicitly selects its path and rehydrates from its manifest. This does not delete or rewrite the previous basket.
2. **One total mutation order.** A shared daemon-owned atomic token is assigned when a physical arrival is first observed and when an authenticated user selection/removal intent is accepted. Managed/pending/failure probe results carry their original token. Under the destination-state write lock, apply selection only for a newer successful managed/pending result, then bump the wire revision. A failure updates its keyed issue but never changes selection. Thus a slow older probe cannot overwrite a later click. With no physical destination select Playback; a genuinely later managed/pending arrival selects itself. Removing the selected destination selects Playback even when other devices remain; removing another preserves selection. Never use map iteration as UX policy.
3. **Plural pending and failure semantics.** Store pending and failed-but-present devices by stable discovery identity/path with observation token; do not replace a different blank device merely because only one slot existed before. Deduplicate MSC/MTP representations of the same hardware while preserving MTP priority. A backend open or non-missing manifest-read failure publishes a bounded sanitized issue, never a usable device. MSC must not become permanently non-retryable merely because its mount was inserted into `known_mounts`; MTP disappearance must clear failures even when it never entered `known_ids`. Use deduplicated bounded backoff (2, 4, 8, 16, then at most every 30 seconds while present), reset on success/removal, and announce only issue revision changes.
4. **One UI playback presentation authority.** Extract the existing `PlaybackControls` polling logic into a shared store. The store accepts a snapshot only on new daemon instance/session or a strictly newer decimal-string `stateSequence`, replaces state after reconnect and exposes subscriptions. The queue view pages against the captured `sessionId` + `queueRevision`; any conflict discards accumulated pages and refreshes instead of replaying or guessing.
5. **Canonical queue versus active transport.** `occurrences`/`listOccurrences` describe the preserved main queue. During Preview, `current`, `mode`, `preview` and `playback` describe the audition. Show both honestly; never insert the audition into the queue or derive active transport from main occurrence state. This preserves the central 15.11 review correction.
6. **Minimal configuration.** The existing schema-v1 `PlaybackConfig { schemaVersion, output }` already satisfies the currently delivered setting. Missing file means the validated default `{schemaVersion:1, output:null}` without creating a file. Invalid/future/oversized configuration stays preserved until explicit reset; no implicit reset occurs during destination rendering. Do not bump the schema for placeholders.
7. **Operation target safety.** Destination selection is presentation/current-target state, not authority to retarget admitted work. Sync and scrobble admission capture path, device ID and IO together and retain that identity through completion. Arrival/selection/removal of unrelated devices has no effect. Removal of the captured target invokes the existing incomplete-write failure/cancellation path only for operations bound to that target; fix the current global “any removal fails every sync” behavior.

### Destination and queue wire shape

Exact type names may follow local conventions, but the semantics must remain equivalent:

```ts
type Destination =
  | { kind: 'playback'; id: 'playback'; selected: boolean }
  | { kind: 'device'; path: string; deviceId: string; name: string; icon?: string | null; selected: boolean }
  | { kind: 'pendingDevice'; pendingId: string; name: string; selected: boolean };

type DeviceDiscoveryIssue = {
  discoveryId: string;
  code: 'DEVICE_OPEN_FAILED' | 'DEVICE_READ_FAILED';
  displayName?: string | null;
  retryable: boolean;
  revision: string;
};
```

- `destination.select` (if introduced) is authenticated, strict-schema and accepts only `{kind:'playback'}` or a currently known physical/pending identity. Do not send a Playback sentinel through `device.select`.
- `device_initialize` becomes a strict target-safe request carrying opaque `pendingId` and `observedDestinationRevision` with the existing profile/folder/name fields. Under the same state lock, reject stale/missing/replaced IDs, initialize only the captured pending IO, and remove only that entry after successful commit. Two open Setup dialogs, a later arrival and removal during submit cannot redirect the operation.
- `device.select` stays physical-only. Direct RPC callers cannot use destination selection to bypass manifest/storage/sync validation.
- `playback.getSession` remains the authoritative reconnect entry. `playback.listOccurrences` already validates schema/session/revision/cursor and enforces page bounds; add only the missing TypeScript wrapper/types.
- Queue progress does not change `queueRevision`; mutations from later stories do. A state update may refresh status without invalidating a page, while revision/session change invalidates every outstanding cursor/result.

`Occurrence` currently contains identity only, so a readable queue needs one explicit additive metadata boundary. Add a read-only RPC such as `playback.describeOccurrences` (name may follow local convention) with strict `{schemaVersion, sessionId, expectedQueueRevision, occurrenceIds}` input capped to one occurrence page. The daemon validates that every occurrence ID belongs to that session/revision, deduplicates identical `(serverId, trackId)` lookups per request, groups work by portable server, resolves through `MediaProvider::get_song` with a named/tested global concurrency bound (maximum 8) and reuses existing provider/cache instances. Return results in requested occurrence order without collapsing deliberate duplicate occurrences:

```ts
type OccurrenceDisplay = {
  occurrenceId: string;
  source: { serverId: string; trackId: string };
  title?: string | null;
  artist?: string | null;
  album?: string | null;
  durationMs?: number | null;
  status: 'available' | 'sourceUnavailable' | 'trackUnavailable';
};
```

Partial/offline/unconfigured sources return localized fallback rows and source badges; they do not fail the whole page, leak provider errors, reorder entries or mutate availability/session state. Cache only bounded non-secret display data keyed by portable source and invalidate on server removal/identity change. The browser makes one metadata request per visible page, never one provider/RPC call per row. If implementation instead persists immutable admission-time display metadata, it must add an explicit migration and size bound and still support legacy rows; do not improvise that larger persistence change silently.

### Current files: behavior, change and preservation

| File | Current behavior | Story change / preserve |
|---|---|---|
| `hifimule-daemon/src/device/mod.rs` | Physical `HashMap` plus nullable selected path; new managed device only selects when none; selected removal chooses an arbitrary remaining key; one pending slot; MSC failures stop retrying while mounted and MTP failures are log-only. | Add the shared mutation clock, plural pending/failure tracking, deterministic fallback/retry and ordered projection. Preserve IO ownership, same-device MTP-over-MSC priority and real-device-only APIs. |
| `hifimule-daemon/src/main.rs` | Serialized device-event loop drives detection, auto-sync/scrobble and removal; scrobble currently re-reads globally selected IO and any removal fails all running syncs. | Carry observation tokens/failures; capture arriving IO for scrobble and match removals to operation targets. Keep real-device-only side effects and never issue playback commands. |
| `hifimule-daemon/src/rpc.rs` | `get_daemon_state` emits unordered physical devices/selected path; authenticated playback snapshot/paging exist but queue rows lack display metadata. | Add typed/ordered destination, strict selection, sanitized issues and bounded occurrence display RPC. Preserve auth, mutation guards and errors. |
| `hifimule-daemon/src/playback/config.rs` | Strict local schema-v1 output preference, 64 KiB bound, validated backend/identity keys, serialized atomic persistence and invalid archive/reset. | Expected no production change beyond tests/docs. Reuse as minimal Playback settings and do not add dormant fields. |
| `hifimule-ui/src/components/BasketSidebar.ts` | Owns physical hub/polling, flush+select+rehydrate, device-only settings, sync states and a null-device locked placeholder. | Split/consume typed destination state; keep physical basket logic intact. Do not render Playback content through manifest/folder/capacity code. |
| `hifimule-ui/src/main.ts` | Mounts the stable main layout, ServerHub, PlaybackControls and BasketSidebar once; zero configured servers still routes to login. | Mount destination navigation/view without rebuilding persistent controls. Server login gating may remain: this story removes the physical-device requirement, not the configured-server requirement. |
| `hifimule-ui/src/rpc.ts` | Playback DTO omits queue page fields and has no `listOccurrences` wrapper. | Add precise occurrence/page and destination/failure types/wrappers; preserve string revisions, portable source IDs and error sanitization. |
| `hifimule-ui/src/components/PlaybackControls.ts` | Polls the daemon, owns stable focusable controls, locally interpolates position, handles Preview/Return and existing output config recovery. | Consume shared snapshot store. Preserve all transport/seek/output behavior and do not move it into destination navigation. |
| `hifimule-ui/src/library.ts` and browse components | Play/Preview use portable source identity; basket actions are visually locked without a physical target. | Ensure only basket actions remain locked; preserve virtualization, multi-selection, playlist actions and source routing. |
| `hifimule-ui/src/styles.css` | Current Record Crate tokens, physical card active/focus styles and `.device-locked .basket-toggle-btn` physical lock. | Generalize destination styles/responsive presentation and live status without obsolete palette or focus overrides. |
| `hifimule-i18n/catalog.json` | Central four-locale catalog. | Add complete key parity; no component-local literal strings. |
| `scripts/tests/playback-ui.test.mjs` | Production-component behavioral harness covers stable focus, Preview and playback controls. | Add destination/store/queue behavior tests; source-regex assertions alone are insufficient. |
| `docs/api-contracts-hifimule-daemon.md`, `docs/playback-installed-test-checklist.md` | Playback queue/reconnect and installed evidence contracts. | Document typed destination/additive state, arrival/failure semantics and actual evidence. |

Recommended new focused files: `hifimule-ui/src/state/playback.ts`, `hifimule-ui/src/state/destination.ts`, `hifimule-ui/src/components/DestinationHub.ts`, and `hifimule-ui/src/components/PlaybackDestination.ts` or the architecture-reserved `PlaybackQueue.ts`/`PlaybackSettings.ts`. Names may vary, but do not create a second daemon/player state owner or bury every concern in the already-large BasketSidebar.

No expected edits: playback audio engine, provider adapters/trait, physical manifest schema, auto-fill engine/config, server playlist adapters or native-media-control implementation. A small playback read-model DTO/handler and sync-operation target/removal fix are expected for the metadata and device-safety contracts; avoid changing playback mutation/persistence unless the chosen metadata design explicitly requires the documented migration alternative.

### Regression and safety guardrails

- Selecting Playback must not clear or rewrite any device manifest/basket. It may hide/disable the physical view; switching back rehydrates through the existing safe flush/select flow.
- Arrival/removal changes navigation only. It must not call `playback.applySession`, `playback.control`, `playback.previewTrack`, output selection or provider resolution.
- Browse-server selection remains independent of occurrence `source.serverId`; all active playback, later reporting and feedback keep portable source identity.
- A pending/open-failed device never gains storage, folder, manifest, auto-fill, basket or sync controls. Setup/recovery is the only physical action until valid.
- Keep long queues bounded and paged. Story 15.12 replaces one page at a time (maximum 200 mounted rows, with cursor navigation); it does not implement Story 15.13's editable/virtualized long-history surface. Never reconstruct a queue from the first page, retain an ever-growing DOM, or poll solely to animate progress.
- Existing sync-progress/cancel/error screens, dirty-manifest repair, per-server locked basket items, auto-fill slots and device settings retain their behavior when a managed device destination is selected.
- Zero configured servers may retain the first-run server login: Story 15.12 promises no *physical device* requirement, not offline browsing without a media source.

### Testing requirements

1. **Destination state and races:** zero devices→Playback selected; Playback always index 0; managed A then B→B by observation order; use barriers to prove a slow A probe cannot override a later explicit Playback/B choice, while later C can; selected B removal→Playback; unselected A removal leaves selection unchanged; no unordered fallback.
2. **Pending/failure lifecycle:** two simultaneous blank devices retain two independently operable Setup cards; two open dialogs each submit an opaque `pendingId` + observed revision and cannot initialize the other device after a later arrival/removal. Managed↔pending and pending↔pending overlaps follow tokens. MTP/MSC failures produce keyed non-selectable issues, never change selection, expose stable codes/safe identity only, deduplicate announcements, follow bounded retry, and clear only the matching issue on success/disappearance. No physical actions become enabled.
3. **Listening causality:** with transport frozen/mocked, assert destination/server/arrival/removal transitions issue zero playback mutation/provider-routing calls and leave instance/session/queue/generation/current source/mode/preview unchanged. In installed asynchronous tests assert no state change is attributable to navigation rather than brittle literal equality across a natural track boundary. Device loss is not audio-output loss.
4. **Queue metadata/reconnect:** empty, one, preview-over-main, deliberate duplicate sources/occurrences, multi-server/offline/unconfigured rows and >200 entries. Assert one bounded metadata request per page, unique-source deduplication, concurrency ≤8, stable order/duplicates, localized partial fallback, page replacement, 100/200 bounds and refresh on stale cursor/revision/session/instance. Active Preview remains separate.
5. **Configuration/privacy:** missing/valid/invalid/future/oversized `playback.json`; no implicit file creation/reset; existing explicit archive-and-replace path; navigation always survives. Assert no credentials, authenticated URLs, raw backend errors, device auto-fill values or manifest data enter Playback config.
6. **UI/accessibility:** keyboard selection using the chosen semantic pattern, correct selected state/label, visible focus, stable focus across programmatic arrival, polite announcement, screen-reader names for Playback/Setup/recovery, and responsive non-overlap at <600, 600–1000 and >1000 px. Preserve grid/list multi-selection and final-row reachability.
7. **Physical restrictions:** with Playback/no device selected, Play/Preview/browse/output work; basket add, storage/folders/manifest/repair/auto-fill/sync remain unavailable. With a managed device selected, the complete current basket/sync workflow still works.
8. **Operation pinning:** start sync/scrobble on A, then select/arrive/remove B and prove A remains the target; remove A and prove only A-bound work fails/cancels without success promotion. Start sync on B, remove unrelated A and prove B continues. Cover selection changes during active sync without cross-device manifest/IO writes.
9. **Platform evidence:** run Windows/macOS/Linux no-device launch, managed/blank arrival, removal, open failure, server navigation, reconnect and active main/Preview cases. Record OS/architecture/commit and distinguish automated mocks from actual installed/device evidence.

Run focused tests first, then shared regressions:

```sh
rtk npm run build:daemon -- test -p hifimule-daemon device::
rtk npm run build:daemon -- test -p hifimule-daemon rpc::
rtk npm run build:daemon -- test -p hifimule-daemon playback::
rtk npm run build:daemon -- test -p hifimule-daemon
rtk proxy node --test scripts/tests/playback-ui.test.mjs
rtk npm --prefix hifimule-ui run build
rtk proxy python3 -m unittest discover -s scripts/tests -p 'test_playback_installed_evidence.py'
rtk git diff --check
```

Use the controlled daemon build wrapper. A story-preparation document does not prove runtime or installed-platform acceptance; keep unrun rows unchecked.

### Previous-story and git intelligence

Recent commits: `bf8f16f Review 15.11`, `ab3a1ce fix(playback): start preview audio after output opens`, `b7cb2c7 Story 15.11`, `ea1f796 Review 15.10 fix`, `bd215ee Review 15.10`.

Story 15.11 established the distinction this UI must preserve: the active Preview projection (`mode/current/playback`) differs from the canonical main queue (`occurrences`). Its review fixed active-transport identity, persistence failure handling, output-loss inhibition, receipt expiry, cursor validation and focus-preserving behavioral coverage. A post-review defect occurred when output authorization read suspended main `Paused/Idle` rather than active Preview state; never derive destination status from the main queue.

Reuse the review's behavior-first TypeScript test pattern and extracted components. Preserve stable DOM/focus, immutable portable source capture, one daemon owner, explicit instance/session/generation fencing, and honest physical-platform limitations. Destination work must not recompute album representation/gain or change main/preview state.

### Architecture and current technical references

- Keep repository pins: Rust edition 2024/MSRV 1.93.0; Tokio ~1.49; Tauri 2.10 line; Shoelace package declaration ^2.19.1 (lock currently resolves 2.20.1); TypeScript 5.6.3; Vite 6.4.1; existing playback CPAL 0.18.2, ffmpeg-next/sys 9.0.0, controlled FFmpeg 9.0.1, crossbeam-queue 0.3.12, libpulse-binding 2.30.1 and Souvlaki 0.8.3. No dependency upgrade is required.
- Shoelace 2.20.1 is now the final/latest archived release and the project points to Web Awesome for future development. Migration is explicitly out of scope; reuse the installed components and test actual behavior.
- The [WAI-ARIA Tabs Pattern](https://www.w3.org/WAI/ARIA/apg/patterns/tabs/) defines selected state, associated panels and arrow/Enter/Space behavior. Use it only if destinations are implemented as tabs; otherwise prefer native buttons with a correctly labelled group. Preserve focus when programmatic selection changes.
- [Shoelace releases](https://github.com/shoelace-style/shoelace/releases) document 2.20.1 accessibility/focus fixes. Repository locks and current behavioral tests remain authoritative for this story.
- [Tauri 2 documentation](https://v2.tauri.app/) confirms the existing Rust/web frontend boundary. Do not introduce a second browser audio/session owner or migrate to the Tauri 3 alpha line.

These web checks are current as of 2026-09-19 and provide API/accessibility context, not permission to upgrade dependencies or claim platform certification.

### Project Structure Notes

Remain inside the existing Rust daemon, authenticated JSON-RPC/Tauri bridge and vanilla TypeScript/Shoelace UI. Use Rust/SQL snake_case and JSON/TypeScript camelCase. `DESIGN.md` and implemented tokens override the older visual examples in `ux-design-specification.md`; retain the UX artifact's responsive breakpoints, device-hub behavior and WCAG intent. The project-context file's greenfield label is stale; this is a brownfield extension with completed sync and playback foundations.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Playback Extension inventory; Epic 15; Story 15.12; Stories 15.13–15.15 boundaries]
- [Source: `_bmad-output/planning-artifacts/prd.md` — amended FR33; Playback Extension FR55/FR59; P-NFR4–6]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Playback State and Ownership; UI and Session Control; Implementation Contracts; deployment sequence/project structure]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md` — Device Hub; responsive breakpoints; accessibility; superseded visual tokens noted above]
- [Source: `_bmad-output/planning-artifacts/playback-prd-source-extract.md`; `playback-epic-validation.md`; `reconcile-playback-architecture.md`]
- [Source: `_bmad-output/implementation-artifacts/epic-15-context.md`; `15-11-preview-a-full-track-without-losing-the-main-listening-session.md`]
- [Source: `DESIGN.md`; `Cargo.toml`; `hifimule-ui/package.json`; `hifimule-ui/package-lock.json`]
- [Source: `hifimule-daemon/src/device/mod.rs`; `hifimule-daemon/src/rpc.rs`; `hifimule-daemon/src/playback/model.rs`; `hifimule-daemon/src/playback/config.rs`]
- [Source: `hifimule-ui/src/main.ts`; `hifimule-ui/src/rpc.ts`; `hifimule-ui/src/components/BasketSidebar.ts`; `hifimule-ui/src/components/PlaybackControls.ts`; `hifimule-ui/src/styles.css`; `scripts/tests/playback-ui.test.mjs`]

## Dev Agent Record

### Agent Model Used

GPT-5

### Debug Log References

- Implemented typed destination state and RPC contracts, mutation-clock race fencing, plural pending setup, discovery retry/issues and operation target pinning.
- Added shared UI playback state, persistent destination navigation and bounded read-only queue metadata/paging.
- Verification: daemon `991 passed, 6 ignored`; UI behavior `33 passed`; installed-evidence validator `33 passed`; production UI build and `git diff --check` passed.
- Full daemon tests require local mock socket access; the restricted first run failed at socket creation, and the controlled rerun passed without code changes.

### Completion Notes List

- Playback is always projected first and selected as the deterministic no-device/removal fallback without entering physical-device APIs.
- Device discovery now uses observation-time ordering, exact pending identities/revisions, sanitized retryable issues and device-bound sync/scrobble targets.
- PlaybackControls and PlaybackDestination share one ordered poll store; the destination renders one canonical queue page with deduplicated bounded metadata lookup and separate Preview status.
- Added responsive, localized, keyboard/focus-safe destination UI and preserved physical-only basket/sync restrictions.
- Updated daemon API and installed-test evidence documentation; unavailable Windows/Linux/macOS installed-device rows remain explicitly unchecked.

### File List

- `_bmad-output/implementation-artifacts/15-12-access-playback-as-an-always-available-destination.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `docs/api-contracts-hifimule-daemon.md`
- `docs/playback-installed-test-checklist.md`
- `hifimule-daemon/src/api.rs`
- `hifimule-daemon/src/device/mod.rs`
- `hifimule-daemon/src/device/tests.rs`
- `hifimule-daemon/src/main.rs`
- `hifimule-daemon/src/playback/model.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-daemon/src/sync.rs`
- `hifimule-i18n/catalog.json`
- `hifimule-ui/src/components/BasketSidebar.ts`
- `hifimule-ui/src/components/DestinationHub.ts`
- `hifimule-ui/src/components/InitDeviceModal.ts`
- `hifimule-ui/src/components/PlaybackControls.ts`
- `hifimule-ui/src/components/PlaybackDestination.ts`
- `hifimule-ui/src/main.ts`
- `hifimule-ui/src/rpc.ts`
- `hifimule-ui/src/state/playback.ts`
- `hifimule-ui/src/styles.css`
- `scripts/tests/destination-ui.test.mjs`
- `scripts/tests/playback-ui.test.mjs`

## Change Log

- 2026-09-19: Implemented Story 15.12 typed destinations, deterministic device lifecycle, always-available Playback queue UI, shared playback presentation state, accessibility/localization, tests and contract documentation.
