---
baseline_commit: 0100d3dd61eefb7c2666058947287b88b04ca425
---

# Story 12.6: Auto-Fill Configuration UI & Coexisting Multi-Server Slot Cards

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a user,
I want a pipeline-builder configuration surface and a separate auto-fill slot card per server,
so that I can configure and see each server's fill independently in one basket.

## Scope decision (read first — this is a vertical slice)

The epic split UI (12.6) from the daemon RPC/state contract (12.7). The contract pieces the UI needs to **persist and reload** per-server pipeline config do not exist yet (`autoFill.setPipeline`, per-server `get_daemon_state.autoFill`, `basket.autoFill`+serverId). Per the explicit product decision for this story, **12.6 is a vertical slice**: it pulls the minimum daemon contract in so the feature works end-to-end and is independently shippable/testable. Story 12.7 then shrinks to contract polish + full i18n completeness + any remaining state wiring.

In scope here (both ends of the wire):
- **Daemon:** `autoFill.setPipeline { serverId, pipeline }` (write), expand `get_daemon_state.autoFill` to expose the per-server pipeline map (read/hydrate), and add optional `serverId` (+ optional inline `pipeline`) routing to `basket.autoFill` (per-server preview). New `AutoFillConfig::set_pipeline`.
- **Frontend:** pipeline-builder configuration panel (stage sections + Advanced disclosure + Default simple state) replacing the single toggle+slider; one auto-fill slot card **per configured server**; non-selected-server slots render read-locked; multi-slot sync payload (array of per-server descriptors — already accepted by the shipped 12.3 path).
- **i18n:** new keys in all four catalog languages.

**Out of scope (deferred to 12.7 or Epic 13):** the advanced *strategy* surfaces that need DB-history (memory beyond cooldown/played-exclusion: stable-core %, repeat-tolerance, rotation tiers, pity timer — these are Epic 13 and their config fields are *reserved/persisted-verbatim* only); any provider trait additions; the `autofill_history` DB consumption.

## Context — what is already built (read before writing code)

This story sits on top of a fully-built, persona-validated daemon pipeline. **You are building the UI for an engine that already works**, plus the thin RPC seam to drive it. Do not reinvent selection logic, budget math, or the manifest schema — they exist and are tested (510 daemon tests as of Story 12.5).

**Daemon — pipeline domain model (Story 12.1, do not modify the engine):**
- `AutoFillPipeline { enabled: bool, filter: FilterStage, sources: Vec<SourceEntry>, unit: Unit, ordering: Vec<OrderingKey>, memory: MemoryStage, budget: BudgetStage, fallback: Vec<SourceEntry> }` — [hifimule-daemon/src/auto_fill/pipeline.rs:52-73]. All fields `#[serde(rename_all = "camelCase", default)]`. **This struct IS the JSON contract the UI produces.** The exact wire shape:
  - `filter`: `{ includeTags: [], excludeTags: [], includeGenres: [], excludeGenres: [] }` — [pipeline.rs:80-87]
  - `sources` / `fallback`: array of `{ kind: "library"|"favorites"|"history"|"playlist", ref?: <playlistId>, share?: <0.0..1.0> }`. `kind` is camelCase (`SourceKind`, [pipeline.rs:127-134]); the id field serializes as `"ref"` (not `refId`) — [pipeline.rs:96]; `share` omitted when unset — [pipeline.rs:99].
  - `unit`: `"track" | "album" | "artist"` (default `"track"`) — [pipeline.rs:137-144]
  - `ordering`: array of `"favorite" | "playCount" | "dateCreated" | "random" | "quality"` — [pipeline.rs:149-162]. `random` is reserved (deterministic no-op until Epic 13); `quality` orders by bitrate.
  - `memory`: `{ cooldownWeeks?: u32, playedExclusion?: bool, stableCorePct?: f32, repeatTolerance?: f32, tiers?: any }` — [pipeline.rs:166-180]. Only `cooldownWeeks` + `playedExclusion` are consumed today; the rest are **reserved (Epic 13)** — persist verbatim, do not surface as functional controls.
  - `budget`: `{ maxBytes?: u64, targetDurationSecs?: u64, headroomBytes?: u64 }` — [pipeline.rs:183-189]. All three are live (Story 12.5).
- Legacy mapping: `AutoFillPipeline::default_legacy(maxBytes)` = `{ sources:[library], ordering:[favorite,playCount,dateCreated], budget:{maxBytes} }` — [pipeline.rs:197-216]. This is exactly what the "Default" UI state must produce/round-trip.

**Daemon — manifest persistence (Story 12.2):**
- `manifest.auto_fill: AutoFillConfig` — [hifimule-daemon/src/device/mod.rs:101-105]. `AutoFillConfig { pipelines: HashMap<String, AutoFillPipeline>, legacy: Option<AutoFillPrefs> }` — [device/mod.rs:190-198]. Keys are the **portable `server_id`** (Story 2.13), shared with `BasketItem.server_id`.
- Custom serde reads either the per-server map OR the legacy `{ enabled, maxBytes }` block — [device/mod.rs:208-264]. Serialization emits the map when non-empty, else the legacy block, else the empty default (byte-for-byte unchanged for legacy devices).
- Existing accessors: `enabled_for(Option<&str>)`, `max_bytes_for(Option<&str>)`, `pipeline_for(&str) -> Option<&AutoFillPipeline>`, `set_for(server_id, enabled, max_bytes)` (legacy-shape upsert), `set_legacy(...)`, `resolve_pipeline` (keyed → single-entry fallback) — [device/mod.rs:266-354]. **There is NO method to persist a full pipeline yet — you add `set_pipeline`.**

**Daemon — RPC surface today:**
- `get_daemon_state` emits `autoFill: { enabled, maxBytes }` for the **selected** server only — [rpc.rs:1855-1869, 1960]. **You extend this to also expose the per-server map.**
- `basket.autoFill` takes `{ maxBytes?, excludeItemIds? }` and runs the **Jellyfin-only** `run_auto_fill` directly against `state.jellyfin_client` — [rpc.rs:5856-5914]. No `serverId`, no provider routing. **You add optional `serverId` + inline `pipeline` routing.**
- `sync.setAutoFill { autoFillEnabled, maxFillBytes?, autoSyncOnConnect }` persists via `set_for`/`set_legacy` and also writes `auto_sync_on_connect` — [rpc.rs:5993-6064]. **Keep this working (legacy); `autoFill.setPipeline` supersedes it for config but does NOT touch `auto_sync_on_connect`.**
- `sync_calculate_delta` **already accepts an array of per-server auto-fill descriptors** `{ serverId, maxBytes?, enabled?, excludeItemIds? }` via `parse_auto_fill_descriptors` (Story 12.3) — [rpc.rs:3407-3482]. Multi-provider routing through `get_provider_by_server_id` and manual-wins dedup are implemented. **The frontend multi-slot sync payload targets this existing contract — no new sync-time daemon work.**
- RPC dispatch table is a string match in `handle_rpc` — [rpc.rs:198-241]. Add `"autoFill.setPipeline"` there.
- `device_set_auto_sync_on_connect` already exists as a standalone RPC — [rpc.rs:230-232]. Auto-sync-on-connect stays server-independent and is configured through this path, **decoupled from auto-fill** (architecture enforcement).

**Daemon — capability gating:**
- `Capabilities { open_subsonic, supports_changes_since, supports_server_transcoding, supports_playlist_write, browse: BrowseCapabilities }` — [hifimule-daemon/src/providers/mod.rs:325-332]. `BrowseMode::Genres` / `Playlists` in `browse.list_modes` gate the Filter (genre) and Playlist-Source UI — [providers/mod.rs:305-323]. `browse.listModes` RPC already exposes the selected provider's modes to the frontend.

**Frontend — what you replace/extend (all in `hifimule-ui/`):**
- `BasketSidebar.ts` (~92 KB, single component) holds today's single auto-fill model: state `autoFillEnabled` / `autoFillMaxBytes` / `autoSyncOnConnect` [BasketSidebar.ts:181-183], `renderAutoFillControls()` (toggle + `<sl-range>` slider + auto-sync switch) [~612-643], `bindAutoFillEvents()` [~380-416], `insertAutoFillSlot()` [~348-364], `renderAutoFillSlotCard()` [~1634-1649], persist via `sync.setAutoFill` [~366-378], sync payload build in `handleStartSync()` [~1163-1177].
- `state/basket.ts`: `AUTO_FILL_SLOT_ID = '__auto_fill_slot__'` (singleton) [basket.ts:6], `BasketItem { id, serverId?, sizeBytes, autoFilled?, priorityReason?, ... }` [basket.ts:8-19], `hydrateFromDaemon` strips the slot by exact id `item.id !== AUTO_FILL_SLOT_ID` [basket.ts:160-172], `getManualItemIds()` / `getManualSizeBytes()` exclude the slot [basket.ts:309-322], `isItemLocked(item)` = item.serverId ≠ active [basket.ts:64-70], `hasMultipleServers()` / `serverIdsInBasket()` [basket.ts:73-90], `setActiveServerId` [basket.ts:50-56].
- Multi-server basket rendering already exists: server-grouped list with per-group icon+name divider, locked-item rendering (lock icon, no remove button, `basket-item--locked`) — `renderItemsList()` [~1689-1720], locked logic [~1624-1632, 1763-1768]. **Reuse this pattern for read-locked slot cards.**
- Server identity: `serverIdentity.ts::formatServerIdentity()` → `{ label, icon, tooltip }`; `serversById: Map<portableServerId, ServerSummary>` populated from `get_daemon_state` [BasketSidebar.ts:178, ~266]. `currentServerId` = selected portable id [BasketSidebar.ts:176].
- RPC client: `rpc.ts::rpcCall(method, params)` over Tauri `invoke('rpc_proxy', …)` [rpc.ts:93-110].
- i18n: `i18n.ts::t(key, replacements)` with `{placeholder}` substitution; catalog at `hifimule-i18n/catalog.json` with languages **en, fr, es, de**. Existing auto-fill keys live under `basket.autofill.*` (e.g. `basket.autofill.slot`, `.slot_meta`, `.max_fill_size`, `.hint`, `.full`). `t()` falls back to the raw key if missing.
- Reusable UI patterns already in the codebase: collapsible disclosure with `aria-expanded` + rotating chevron (`device-folders-header` [~785-791]), `<sl-switch size="small">`, `<sl-range>`, `<sl-select>`, `<sl-input clearable>`, icon-tile picker grid (`device-settings-icon-picker` [~490-497]), `<sl-dialog>` with footer buttons. Styling tokens in `styles.css` (`.auto-fill-controls` [~1170-1196], `.basket-item-auto-fill-slot` dashed border, `.basket-item--locked`).

## Acceptance Criteria

### Daemon contract (vertical-slice seam)

1. **`autoFill.setPipeline` persists a full per-server pipeline.** Given a connected device and params `{ serverId: <non-empty portable id>, pipeline: <AutoFillPipeline JSON> }`, when `autoFill.setPipeline` is called, then the pipeline is deserialized into `AutoFillPipeline`, written into `manifest.auto_fill.pipelines[serverId]` via a new `AutoFillConfig::set_pipeline(server_id, pipeline)` (which inserts/replaces that server's entry and clears any parked `legacy` block), and the manifest is persisted atomically (single `update_manifest` write). The handler returns `{ status: "success", serverId }`. A blank/whitespace `serverId` returns `ERR_INVALID_PARAMS`; a malformed `pipeline` returns `ERR_INVALID_PARAMS`. **`auto_sync_on_connect` is NOT read or written by this RPC.** [Source: architecture.md#Auto-Fill-Pipeline-Model:825, #Enforcement:920-922]

2. **`autoFill.setPipeline` never disturbs other servers' slots.** Given a manifest already holding pipelines for servers A and B, when `setPipeline` writes server A, then server B's pipeline in `manifest.auto_fill.pipelines` is byte-for-byte unchanged. [Source: architecture.md#Enforcement:920]

3. **`get_daemon_state` exposes the per-server pipeline map.** Given a device with one or more configured pipelines, when `get_daemon_state` is called, then the `autoFill` object additionally carries `pipelines: { "<serverId>": <AutoFillPipeline JSON>, … }` covering every configured server. The existing `autoFill.enabled` and `autoFill.maxBytes` fields (selected-server resolution) are **retained unchanged** for backward compatibility with the current frontend read path. For a legacy device with no per-server map, `pipelines` is an empty object `{}` and `enabled`/`maxBytes` reflect the legacy block exactly as today. [Source: architecture.md#Auto-Fill-Pipeline-Model:794; rpc.rs:1864-1869]

4. **`basket.autoFill` routes per server when `serverId` is supplied.** Given params `{ serverId?: <portable id>, maxBytes?, excludeItemIds?, pipeline?: <inline AutoFillPipeline> }`: when `serverId` is present, the preview is computed for that server's provider via `get_provider_by_server_id(serverId)` (using the supplied inline `pipeline` if given, else the persisted `pipelines[serverId]`, else the default-legacy pipeline) and returns ranked items for that server; when `serverId` is absent, behavior is unchanged (existing Jellyfin `run_auto_fill` path). An unknown/unroutable `serverId` returns a connection error, not a panic. Reuse the existing sync-time expansion seam (`expand_auto_fill_slot` / `run_auto_fill_provider`) rather than duplicating selection logic. [Source: architecture.md#Auto-Fill-Pipeline-Model:826; rpc.rs:5856-5914, 3531-3542]

5. **Legacy paths preserved.** Given the changes above, when `sync.setAutoFill` (legacy) is called, then it continues to persist `{ enabled, maxBytes }` (mapped to the default pipeline via `set_for`) and `auto_sync_on_connect` exactly as before; and a legacy single-server device with no per-server map continues to round-trip with zero migration. No existing RPC signature is removed. [Source: architecture.md#Auto-Fill-Pipeline-Model:825; epics.md#Story-12.6]

### Frontend — configuration panel

6. **Pipeline-builder panel replaces the toggle+slider.** The auto-fill configuration surface opened from the Basket header is a pipeline builder with collapsible stage sections in fixed order **Filter → Sources → Unit → Ordering → Memory → Budget**, scoped to the **currently selected server**. It edits an in-memory `AutoFillPipeline` and on confirm calls `autoFill.setPipeline { serverId: <selectedServerPortableId>, pipeline }`. The legacy single `<sl-switch>` + `<sl-range>` controls are superseded by this panel (the simple Default state, AC8, preserves the one-click feel). The configuration affordance is disabled when no server is selected (`selectedServerPortableId == null`). [Source: ux-design-specification.md#5.3:99; epics.md#Story-12.6]

7. **Advanced disclosure keeps the simple path one-click.** Advanced controls — Memory beyond cooldown/played-exclusion (the reserved stable-core/repeat-tolerance/tiers fields are NOT surfaced as functional controls), multi-source share blending, ordering-key reordering, fallback chain, duration-target & headroom budget — appear progressively behind an "Advanced" disclosure. The default (collapsed) view shows only: an enable toggle, a single size budget, and (when capability-supported) a genre exclude — nothing more. [Source: ux-design-specification.md#5.3:99]

8. **"Default" state behaves like the legacy fill.** Given a server with no configured pipeline (empty/Default state), when the user enables auto-fill without touching Advanced, then the produced pipeline is the default-legacy equivalent (`sources:[library]`, `ordering:[favorite,playCount,dateCreated]`, optional `budget.maxBytes`) and the resulting fill is identical to today's favorites → play count → creation-date behavior. Round-tripping a legacy device's config through the panel and saving must not change observable fill behavior. [Source: ux-design-specification.md#5.3:99; pipeline.rs:197-216]

9. **Capability gating on stages.** The Filter→genre controls and the Playlist source option are shown only when the selected server's provider advertises the matching browse mode (`Genres` / `Playlists` via `browse.listModes`); when unsupported they are hidden (not shown-then-erroring). Library / Favorites / History sources are always available. [Source: ux-design-specification.md#5.3:99; providers/mod.rs:305-332; auto_fill/fetch.rs:195-279]

10. **Per-source share blending.** When multiple sources are added, each exposes a share slider; shares are written into each `SourceEntry.share` (`0.0..=1.0`). A single source needs no share (engine splits remainder equally when unset). [Source: ux-design-specification.md#5.3:99; pipeline.rs:90-101]

### Frontend — per-server slot cards

11. **One slot card per server, coexisting.** Each configured server whose pipeline is enabled produces its **own** auto-fill slot card in the basket; multiple coexist in one basket simultaneously. Enabling/reconfiguring auto-fill for the selected server inserts/updates only that server's slot card and never removes another server's slot. The singleton `AUTO_FILL_SLOT_ID` model is generalized to a per-server slot id (e.g. a `__auto_fill_slot__:<serverId>` prefix scheme) and all slot-detection sites (`hydrateFromDaemon` strip, `getManualItemIds`/`getManualSizeBytes` exclusion) are updated to match the prefix, not the exact legacy id. [Source: ux-design-specification.md#5.3:100-101; architecture.md#Enforcement:920; basket.ts:6,160-172,309-322]

12. **Slot card content & non-selected read-lock.** Each slot card is a dashed-border card carrying the server's icon + name badge and a readout "Auto-Fill · {server} · will fill ~X GB / ~Y h at sync time" (duration shown only when a duration target is set; derived bytes shown when the target maps to bytes; a subtle "fallback fills the rest" hint when sources can't reach the target). Slot cards for **non-selected** servers render **read-locked** (no toggle/edit/remove affordance), reusing the existing locked-basket-item pattern (`isItemLocked` / `basket-item--locked` / lock icon). The slot card readout is derived from the pipeline budget + device capacity locally; **no RPC call is made when auto-fill is toggled or reconfigured** — slots are local UI markers and the pipeline persists via `autoFill.setPipeline`. (The `basket.autoFill`+serverId preview from AC4 may back an explicit, debounced preview affordance but must not fire on every render/toggle.) [Source: ux-design-specification.md#5.3:101-103]

13. **Removed: per-track auto badges.** Individual auto-filled tracks are not displayed in the basket prior to sync; no "Auto" badge / priority-reason tags are rendered. (If any such rendering remains from 3.6/3.8, remove it.) [Source: ux-design-specification.md#5.3:103-104]

### Frontend — sync wiring & i18n

14. **Multi-slot sync payload.** When sync starts with one or more enabled auto-fill slots, the frontend sends `autoFill` to `sync_calculate_delta` as an **array** of per-server descriptors `[{ serverId, maxBytes?, enabled: true, excludeItemIds }, …]` (one entry per enabled slot), targeting the already-shipped Story 12.3 `parse_auto_fill_descriptors` contract. Each descriptor's `excludeItemIds` carries that server's manual item ids (daemon manual-wins dedup is the safety net). A single enabled slot may still use the legacy single-object form, but the array form must be correct for ≥2 servers. [Source: architecture.md#Auto-Fill-Pipeline-Model:824; rpc.rs:3407-3482]

15. **Config hydration on load and server switch.** On `get_daemon_state` load and whenever the selected server changes, the panel reflects that server's persisted pipeline (read from `autoFill.pipelines[serverId]`), and slot cards are (re)derived for every server with an enabled pipeline. Switching the selected server shows that server's config in the panel; non-selected servers keep their read-locked cards. [Source: ux-design-specification.md#5.3:100; AC3]

16. **i18n complete, no raw keys.** All new UI strings use `t()` keys added to **all four** catalog languages (en, fr, es, de) under the `basket.autofill.*` namespace (extend, don't rename existing keys). No new UI string is hard-coded or left as a raw fallback key. [Source: i18n.ts; catalog.json; epics.md#Story-12.7 (i18n)]

### Quality gates

17. **Daemon build & tests green.** `rtk cargo test -p hifimule-daemon` passes with no regressions (baseline 510 tests from Story 12.5) plus new tests for: `set_pipeline` (upsert + clears legacy + leaves other servers untouched), `autoFill.setPipeline` round-trip (persist → `get_daemon_state.pipelines` reflects it), legacy device → empty `pipelines` map, and `basket.autoFill`+serverId routing/validation. `rtk cargo clippy -p hifimule-daemon --all-targets` adds no new warnings.

18. **Frontend builds & typechecks.** `cd hifimule-ui && npx tsc --noEmit` (the project's typecheck; there is no vitest harness) passes with no errors. The full `pnpm build` / `tsc && vite build` succeeds. New camelCase JSON produced by the panel matches the daemon `AutoFillPipeline` serde shape exactly (notably `ref` not `refId`, `kind` lowercase camelCase, `playCount`/`dateCreated` ordering keys).

19. **Zero-regression for legacy single-server users.** A single-server device that never opens the new panel continues to: load (empty `pipelines`, legacy `enabled`/`maxBytes` honored), show a single working slot card, sync with the legacy fill, and persist via `sync.setAutoFill` if the simple toggle path is used. No migration prompt, no behavior change. [Source: epics.md#Story-12.6; architecture.md backward-compat]

## Tasks / Subtasks

- [ ] **Daemon: `AutoFillConfig::set_pipeline` + unit tests** (AC: 1, 2)
  - [ ] Add `pub fn set_pipeline(&mut self, server_id: &str, pipeline: AutoFillPipeline)` to `AutoFillConfig` [device/mod.rs:266-354]: `self.pipelines.insert(server_id.to_string(), pipeline); self.legacy = None;`. Mirror the doc-comment style of `set_for`.
  - [ ] Unit tests: upsert replaces the server's entry; clears parked legacy; a second server's entry is untouched; serialize→deserialize round-trips the full pipeline (filter/sources/unit/ordering/memory/budget/fallback) with camelCase + `ref`.

- [ ] **Daemon: `autoFill.setPipeline` RPC** (AC: 1, 2, 5, 17)
  - [ ] Add `"autoFill.setPipeline" => handle_auto_fill_set_pipeline(&state, payload.params).await` to the dispatch table [rpc.rs:198-241].
  - [ ] Implement `handle_auto_fill_set_pipeline`: parse `serverId` (trim; `ERR_INVALID_PARAMS` if blank) and `pipeline` (`serde_json::from_value::<AutoFillPipeline>`; `ERR_INVALID_PARAMS` on failure). Persist via `state.device_manager.update_manifest(|m| m.auto_fill.set_pipeline(&server_id, pipeline.clone()))`. Do **not** touch `auto_sync_on_connect`. Return `{ status: "success", serverId }`. Model error handling on `handle_sync_set_auto_fill` [rpc.rs:5995-6064].
  - [ ] RPC test: call setPipeline, then `get_daemon_state`, assert `autoFill.pipelines[serverId]` equals what was sent.

- [ ] **Daemon: expand `get_daemon_state.autoFill` with the per-server map** (AC: 3, 17)
  - [ ] In `handle_get_daemon_state` [rpc.rs:1864-1869], add `"pipelines": d.auto_fill.pipelines` (serialize the map) to the `auto_fill` JSON object; keep `enabled`/`maxBytes` exactly as-is. For `None` device, keep `auto_fill = None` as today.
  - [ ] Test: legacy device → `pipelines == {}`; device with two configured servers → both appear.

- [ ] **Daemon: `basket.autoFill` per-server routing** (AC: 4, 17)
  - [ ] Extend `handle_basket_auto_fill` [rpc.rs:5856-5914]: read optional `serverId` and optional inline `pipeline`. When `serverId` present, resolve the provider via `get_provider_by_server_id`, pick pipeline = inline ?? `manifest.auto_fill.pipeline_for(serverId)` ?? `AutoFillPipeline::default_legacy(maxBytes)`, and expand through the existing seam `expand_auto_fill_slot` / `run_auto_fill_provider` ([rpc.rs:3531-3542]) — do not duplicate ranking. When `serverId` absent, keep the current Jellyfin path verbatim. Map unknown serverId / provider failure to `ERR_CONNECTION_FAILED`.
  - [ ] Test: serverId routes to the correct provider; absent serverId preserves legacy behavior; bad serverId errors cleanly.

- [ ] **Frontend: generalize the slot model to per-server** (AC: 11, 19)
  - [ ] In `state/basket.ts`: introduce `AUTO_FILL_SLOT_PREFIX = '__auto_fill_slot__'` and a helper `isAutoFillSlotId(id)` / `autoFillSlotId(serverId)`. Update `hydrateFromDaemon` strip [basket.ts:160-172], `getManualItemIds`/`getManualSizeBytes` [basket.ts:309-322], `hasMultipleServers`/`serverIdsInBasket` [basket.ts:73-90] to use the prefix predicate, not the exact legacy id. Keep the old exact id parsing as a recognized prefix so any persisted state strips cleanly.
  - [ ] Slot card basket items carry `serverId` so they group/lock with the existing multi-server rendering.

- [ ] **Frontend: pipeline-builder configuration panel** (AC: 6, 7, 8, 9, 10, 15, 16)
  - [ ] Build the panel scoped to `selectedServerPortableId`. Stage sections in fixed order (Filter → Sources → Unit → Ordering → Memory → Budget) using existing disclosure + Shoelace controls (reuse `device-folders-header` collapse pattern, `<sl-switch>`, `<sl-range>`, `<sl-select>`, chip/`<sl-input>` for genre/tag, icon-tile or segmented control for Unit).
  - [ ] Default (collapsed) view = enable toggle + one size budget + (capability-gated) genre-exclude. "Advanced" disclosure reveals sources/shares, ordering reorder, fallback, duration target + headroom, cooldown/played-exclusion. Do NOT surface reserved Epic 13 fields (`stableCorePct`, `repeatTolerance`, `tiers`) as functional controls — preserve them verbatim if present in a loaded pipeline.
  - [ ] Produce JSON matching the daemon serde shape exactly (`kind` camelCase, `ref` for playlist id, `share` 0..1, `playCount`/`dateCreated`/`targetDurationSecs`/`headroomBytes`/`maxBytes` camelCase). Default state emits the default-legacy equivalent.
  - [ ] Gate Filter-genre + Playlist-source via `browse.listModes` for the selected server; always allow Library/Favorites/History.
  - [ ] On confirm → `rpcCall('autoFill.setPipeline', { serverId, pipeline })`; on load / server-switch, hydrate from `get_daemon_state.autoFill.pipelines[serverId]`.

- [ ] **Frontend: per-server slot cards + read-lock** (AC: 11, 12, 13, 15)
  - [ ] Render one dashed-border slot card per server with an enabled pipeline (server icon+name badge + "~X GB / ~Y h" readout derived locally from budget + capacity). Non-selected servers' cards render read-locked via the existing locked pattern. No RPC on toggle/reconfigure.
  - [ ] Remove any remaining per-track Auto badge / priority-reason rendering in the basket.
  - [ ] Optional explicit preview affordance backed by `basket.autoFill`+serverId — debounced, never on every render.

- [ ] **Frontend: multi-slot sync payload** (AC: 14)
  - [ ] In `handleStartSync()` [BasketSidebar.ts:1163-1177], replace the single `autoFill` object with an array of per-server descriptors (one per enabled slot): `{ serverId, maxBytes?, enabled: true, excludeItemIds: <that server's manual ids> }`. Keep the single-object form valid for the 1-slot case; verify the array form for ≥2 servers.

- [ ] **i18n** (AC: 16)
  - [ ] Add all new `basket.autofill.*` keys (stage labels, advanced labels, source kinds, ordering keys, budget labels, fallback hint, per-server slot readout) to en/fr/es/de in `hifimule-i18n/catalog.json`. Extend, don't rename existing keys.

- [ ] **Quality gates** (AC: 17, 18, 19)
  - [ ] `rtk cargo test -p hifimule-daemon` (510 baseline + new) and `rtk cargo clippy -p hifimule-daemon --all-targets` clean.
  - [ ] `cd hifimule-ui && npx tsc --noEmit` and `pnpm build` succeed.
  - [ ] Manual: single-server legacy device unchanged; two servers each get an independent enabled slot; switching servers swaps the panel config; non-selected slot read-locked.

## Dev Notes

### Architecture compliance (non-negotiable)

- **One slot per server, never a global singleton.** Toggling/reconfiguring auto-fill for the selected server must never remove or mutate another server's slot or pipeline. This applies at both the daemon (`set_pipeline` only touches its key) and the UI (per-server slot ids). [Source: architecture.md#Enforcement:920]
- **Config in manifest, history in DB — never mixed.** This story writes only pipeline *config* (manifest, portable `server_id`-keyed). It must not write the `autofill_history` DB table (Epic 13) and must not read runtime history. The reserved Memory fields (`stableCorePct`, `repeatTolerance`, `tiers`) are config placeholders only — persist verbatim, do not wire behavior. [Source: architecture.md#Enforcement:922]
- **Route every expansion via `get_provider_by_server_id(serverId)`.** The new `basket.autoFill`+serverId path must resolve the provider this way; never assume the active provider. [Source: architecture.md#Enforcement:921]
- **Portable `server_id` everywhere on the wire.** Slot ids, descriptor `serverId`, `pipelines` map keys, and `setPipeline.serverId` all use the portable `server_id` (= `selectedServerPortableId` / `BasketItem.serverId`), never the machine-local `id`. [Source: architecture.md#Server-Identity:909, #Enforcement:917]
- **`autoSyncOnConnect` is server-independent.** It lives on the device and is configured via `device_set_auto_sync_on_connect` (or the legacy `sync.setAutoFill`), decoupled from per-server pipeline config. `autoFill.setPipeline` must not read or write it. [Source: architecture.md#Auto-Fill-Pipeline-Model:825]
- **Do not modify the pure engine.** `pipeline.rs` (selection, budget, fallback math) and `fetch.rs` (materialization, capability gating) are correct and tested. The new `basket.autoFill` path reuses the existing seam; it does not add a parallel ranking implementation. [Source: 12-5 story Dev Notes; auto_fill/pipeline.rs, fetch.rs]

### The exact JSON contract the UI must produce (get this byte-exact)

The single most common failure mode here is a UI/daemon serde mismatch. The daemon `AutoFillPipeline` uses `#[serde(rename_all = "camelCase")]` with two non-obvious renames:
- `SourceEntry.ref_id` serializes as **`ref`** (not `refId`) and is omitted when `None` — [pipeline.rs:96].
- `SourceEntry.share` is omitted when `None` — [pipeline.rs:99].
- Enum variants are camelCase: `kind` ∈ `library|favorites|history|playlist`; `unit` ∈ `track|album|artist`; `ordering` keys ∈ `favorite|playCount|dateCreated|random|quality`.
- Budget keys: `maxBytes`, `targetDurationSecs`, `headroomBytes` (all optional u64).
A worked Default-state example (must round-trip unchanged):
```json
{ "enabled": true,
  "filter": { "includeTags": [], "excludeTags": [], "includeGenres": [], "excludeGenres": [] },
  "sources": [ { "kind": "library" } ],
  "unit": "track",
  "ordering": ["favorite", "playCount", "dateCreated"],
  "memory": { "playedExclusion": false },
  "budget": { "maxBytes": 8000000000 },
  "fallback": [] }
```
Because every field has `#[serde(default)]`, the UI may omit unset stages; but to be safe and match `default_legacy`, emit `sources`/`ordering` explicitly for the Default state.

### Why the sync path needs no new daemon work

Story 12.3 already shipped `parse_auto_fill_descriptors` accepting BOTH a legacy single object and the new array `[{ serverId, maxBytes?, enabled?, excludeItemIds? }]`, with multi-provider routing via `get_provider_by_server_id` and manual-wins dedup [rpc.rs:3407-3482]. The only frontend change for multi-slot sync is to emit the array instead of the single object in `handleStartSync()`. Do not re-implement sync-time expansion.

### Read-locked slot cards — reuse, don't rebuild

The basket already renders non-selected-server *items* read-locked (lock icon, no remove button, `basket-item--locked`, server-grouped with an icon+name divider) — `renderItemsList()` / locked logic [BasketSidebar.ts:1624-1632, 1689-1720, 1763-1768]. Make slot cards ordinary basket items (with `serverId` + the slot-prefix id) so they flow through this same grouping/locking machinery; only the card body (dashed border + budget readout) differs from a media card.

### Capability gating source of truth

`browse.listModes` returns the selected provider's `BrowseMode` list; gate the genre Filter UI on `Genres` and the Playlist source on `Playlists`. The daemon already drops genre constraints for providers lacking `Genres` ([fetch.rs:195-279]) and tag filters are always dropped in 12.x (no provider tag data) — so **do not surface free-text tag include/exclude as a functional control** beyond what the schema persists; genres are the live filter dimension. `supports_playlist_write` (from `get_daemon_state`) governs playlist *write*, not playlist *enumeration* — use `browse.listModes` `Playlists` for the source picker.

### Previous story intelligence (12.1 → 12.5)

- Each Epic-12 story kept the pure engine pure and confined I/O to `fetch.rs`/`rpc.rs`; reviews reward minimal, well-tested diffs and reject scope creep (e.g. new crates). Expect a code-review pass — write defensive tests, especially for serde round-trip and the "other server untouched" invariant.
- 12.5 review caught a `Some(0)` duration edge case (silently empty fill). When the UI lets a user clear a duration/headroom field, emit `null`/omit rather than `0` to avoid the inert-vs-empty trap; the daemon normalizes `Some(0)` → `None` for duration in `expand_with_pipeline`, but don't rely on it for headroom. [Source: 12-5 Review Findings]
- 12.2 made `get_daemon_state.autoFill` deliberately shape-stable (`{enabled,maxBytes}`) and noted "UI is Story 12.6" — this story is the planned consumer; adding `pipelines` is additive and safe. [Source: rpc.rs:1855-1857]
- Manifest serialization stays byte-for-byte identical for legacy devices (empty `pipelines` → emits legacy block) — do not break this; a legacy device that only uses the simple toggle should still serialize the legacy shape. [Source: device/mod.rs:208-225]

### Latest technical context

- No new crate is needed or permitted (daemon side): `serde`/`serde_json`, `anyhow`, existing provider/manifest infra cover everything. Frontend uses Shoelace `^2.19.1` + Lit-style component already in `BasketSidebar.ts`; reuse existing components, no new dependency.
- Drag-to-reorder for ordering keys: prefer up/down icon-buttons (the pattern already used for playlist track reorder in `PlaylistCurationView.ts`) over introducing a drag library — it's lower risk and matches the codebase. The UX spec says "drag-to-reorder" but up/down controls satisfy the intent without a new dep; note this as a deliberate simplification.

### Project Structure Notes

- Daemon edits: `hifimule-daemon/src/device/mod.rs` (`set_pipeline`), `hifimule-daemon/src/rpc.rs` (dispatch + `handle_auto_fill_set_pipeline` + `get_daemon_state` autoFill block + `handle_basket_auto_fill` routing). Binary crate; tests are `#[cfg(test)] mod tests` in each module + `rpc.rs` integration-style tests.
- Frontend edits: `hifimule-ui/src/state/basket.ts` (slot prefix model), `hifimule-ui/src/components/BasketSidebar.ts` (panel + slot cards + sync payload). New panel may be a sub-component or a section method within `BasketSidebar.ts` to match the existing single-file pattern (the file is already large — a separate `AutoFillPanel.ts` component is acceptable and cleaner if it imports the shared `rpc`/`i18n`/`basket` modules).
- i18n: `hifimule-i18n/catalog.json` (en/fr/es/de).

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Epic-12 (Story 12.6, lines 3059-3067; Story 12.7 line 3069-3077 for the deferred contract/i18n boundary)]
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#5.3 (lines 98-104) — pipeline builder, per-server slots, read-lock, no per-track badge]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-06-14-configurable-auto-fill.md (§4.2 architecture, §4.4 UX, §5 success criteria)]
- [Source: _bmad-output/planning-artifacts/architecture.md#Auto-Fill-Pipeline-Model (lines 788-826); #Enforcement (lines 913-923)]
- [Source: hifimule-daemon/src/auto_fill/pipeline.rs:52-217 (AutoFillPipeline + stages + default_legacy)]
- [Source: hifimule-daemon/src/device/mod.rs:101-105, 190-354 (AutoFillConfig + accessors; add set_pipeline)]
- [Source: hifimule-daemon/src/rpc.rs:198-241 (dispatch), 1825-1966 (get_daemon_state), 3407-3482 (parse_auto_fill_descriptors), 3531-3542 (expand_auto_fill_slot seam), 5856-6064 (basket.autoFill + sync.setAutoFill)]
- [Source: hifimule-daemon/src/providers/mod.rs:305-332 (Capabilities / BrowseMode)]
- [Source: hifimule-ui/src/state/basket.ts:6,8-19,50-90,160-172,309-322; hifimule-ui/src/components/BasketSidebar.ts:176-183,348-416,612-643,1163-1177,1624-1720; hifimule-ui/src/rpc.ts:93-110; hifimule-ui/src/i18n.ts; hifimule-i18n/catalog.json]
- [Source: _bmad-output/implementation-artifacts/12-5-budget-headroom-duration-fallback-chain.md (Some(0) edge, engine-untouched discipline); 12-2 / 12-3 stories (manifest map, descriptor array)]

## Open Questions / Clarifications

1. **Vertical-slice boundary with 12.7 (resolved by product decision).** This story pulls `autoFill.setPipeline`, the `get_daemon_state.autoFill.pipelines` map, and `basket.autoFill`+serverId into 12.6 so the feature is end-to-end. Story 12.7 therefore shrinks to: any remaining state-contract polish, full i18n completeness/audit, and the explicit decoupling of `autoSyncOnConnect` if not already clean. **Confirm 12.7's scope is updated accordingly** (recommend annotating sprint-status when 12.6 lands).
2. **i18n language set.** The epic specifies "en/fr/es" for 12.7, but the live catalog also contains a complete `de`. This story adds keys to all four to avoid leaving `de` partial. Confirm `de` parity is desired (recommended).
3. **Drag-to-reorder vs up/down controls.** The UX spec says "drag-to-reorder ranking keys"; this story recommends up/down icon-buttons (existing codebase pattern, no new dependency). Confirm the simplification is acceptable, or request a drag implementation.
4. **`unit: album|artist` exposure.** The Unit stage (track/album/artist) is in the schema and engine. Confirm it should be surfaced as a user control in this UI pass, or kept Advanced/hidden (recommend: Advanced segmented control, default track).

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List

## Change Log

| Date       | Change                                                                 |
|------------|------------------------------------------------------------------------|
| 2026-06-14 | Story drafted (ready-for-dev). Scoped as a vertical slice per product decision: daemon contract (`autoFill.setPipeline`, per-server `get_daemon_state`, `basket.autoFill`+serverId) + pipeline-builder UI + per-server slot cards + multi-slot sync payload + i18n. |
