---
baseline_commit: 176a0f6165614dbc4b5404d894b13d6cd4155eb8
---

# Story 12.3: Multi-Slot Sync-Time Expansion & Lift Single-Slot Limit

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a multi-server user,
I want each configured server's auto-fill slot to expand against its own server at sync time,
so that I can define auto-fill for several servers at once and one server's fill never overwrites or oversubscribes another's.

## Acceptance Criteria

1. **`autoFill` sync param accepts an array of per-server descriptors (legacy object still accepted).** Given a `sync_calculate_delta` request, when its `autoFill` param is supplied, then the daemon accepts **either** the legacy single object `{ enabled, maxBytes?, serverId?, excludeItemIds? }` **or** a new array `[ { serverId, maxBytes?, enabled?, excludeItemIds? }, … ]`, normalizing both to an internal `Vec<AutoFillDescriptor>`. A legacy object with `enabled == true` becomes a single-element vec; `enabled == false` / absent / empty array yields no descriptors (no auto-fill). A descriptor with no `serverId` falls back to the selected server's portable id (the legacy single-server behavior). [Source: architecture.md#Auto-Fill-Pipeline-Model lines 822-824; rpc.rs:3506-3563, 3628-3641]

2. **Each slot expands against its own server, routed by portable serverId.** Given one or more enabled descriptors, when the delta is calculated, then for each descriptor the daemon resolves its provider via `get_provider_by_server_id_for(state, serverId)` and runs the default-pipeline expansion (`run_auto_fill_provider`) against **that** provider only — never the global/active provider. Resulting `DesiredItem`s are tagged with the descriptor's portable `server_id`. [Source: architecture.md#Auto-Fill-Pipeline-Model lines 814-824; architecture.md#Enforcement line 921; rpc.rs:3478-3504, 3520-3558]

3. **Multiple slots (one per server) coexist; manual items win dedup; slots dedup in order.** Given a basket with manual items across servers **and** ≥2 enabled auto-fill descriptors, when the delta is calculated, then: manual items are resolved first and are never displaced; each subsequent slot's expansion excludes every already-selected id (manual items + earlier slots' fills) via the existing `seen_ids` set; and no id is emitted twice. Enabling auto-fill for one server never removes or shrinks another server's fill. [Source: epics.md#Story-12.3 lines 3035-3038; architecture.md line 820 "manual items still win dedup"; rpc.rs:3474-3504, 3543-3560]

4. **Combined fill never oversubscribes the device.** Given multiple slots filling the **same physical device**, when expansion runs, then slots expand **sequentially against a shared remaining-capacity budget**: the running budget starts at the device's available capacity minus already-selected (manual + prior-slot) bytes, and each slot's effective budget is `min(descriptor.maxBytes (if given), remaining)`; after each slot the remaining budget is decremented by the bytes that slot actually added. The total of all auto-filled + manual bytes never exceeds device capacity. For a single slot this is byte-for-byte identical to today (`min(maxBytes, free − manual) == maxBytes` since the UI already computed `maxBytes = free − manual`). [Source: rpc.rs:2588-2625 (current budget derivation); sprint-change-proposal-2026-06-14-configurable-auto-fill.md Success-criterion-1]

5. **Routing decision accounts for all auto-fill servers.** Given the `autoFill` descriptors, when `sync_needs_provider_routing` decides whether to use the per-provider path, then **every** descriptor's resolved serverId is considered (not just one): a basket whose manual items are all on the selected server but which carries auto-fill descriptors for ≥2 servers (or for a single non-selected server) routes through `multi_provider_calculate_delta`. The pure single-server fast paths (`provider_calculate_delta`, the Jellyfin-client path) are taken only when all items **and** all auto-fill descriptors resolve to the single selected server. [Source: rpc.rs:3397-3425, 3642-3652]

6. **Single-server behavior is byte-for-byte unchanged (zero regression).** Given today's UI (which still sends one `autoFill` object for the selected server — the multi-slot UI is Story 12.6), when a single-server sync runs auto-fill, then the selected tracks, ordering, byte budget, dedup, and emitted delta are identical to pre-12.3. The Jellyfin-client fast path (`run_auto_fill`) and the single-provider path (`run_auto_fill_provider`) continue to serve the 1-descriptor selected-server case. [Source: rpc.rs:2540-2671, 3950-4010; sprint-change-proposal-2026-06-14-configurable-auto-fill.md Success-criterion-1]

7. **Scope boundary — daemon sync-time expansion only.** Given the epic sequencing, when implementation is complete, then this story does **NOT**: build or change any UI (per-server slot cards, pipeline-builder, read-locked rendering, and the basket sending the descriptor array are all **Story 12.6**); add `autoFill.setPipeline` / add `serverId` to the `basket.autoFill` preview RPC (**Story 12.7** — `handle_basket_auto_fill` stays single-server); materialize provider pools to drive the pure `run_pipeline` engine or read `manifest.auto_fill.pipeline_for(serverId)` to run a *configurable* pipeline with playlist/tag sources & filters (**Story 12.4**); read/write the `autofill_history` DB table (**Epic 13**); change `autoSyncOnConnect` / make `main.rs` auto-sync multi-server (one server is connected at a time — it remains single-server via the 12.2 `legacy_*` accessors); or add a new crate dependency. The per-slot expansion uses today's **default-pipeline** path (`run_auto_fill_provider`); swapping it for the materialized `run_pipeline` engine is Story 12.4. [Source: epics.md#Epic-12 lines 3040-3049; sprint-change-proposal-2026-06-14-configurable-auto-fill.md:132-135; ux-design-specification.md §5.3]

8. **Build & tests green.** Given the workspace, when `rtk cargo test -p hifimule-daemon` runs, then all existing daemon tests pass (no regression), and new unit tests cover: descriptor normalization (legacy object → 1 elem; disabled → 0; array → N; missing serverId → selected fallback); `sync_needs_provider_routing` returning true for a multi-server auto-fill set with single-server manual items; the multi-slot expansion loop (manual-wins dedup, cross-slot dedup, shared-budget non-oversubscription); and single-descriptor parity with the pre-12.3 path. `rtk cargo clippy -p hifimule-daemon --all-targets` introduces no new warnings in touched modules.

## Tasks / Subtasks

- [x] **Define `AutoFillDescriptor` + a normalizer** (`hifimule-daemon/src/rpc.rs`) (AC: 1)
  - [x] Add a small internal struct near `parse_item_specs` (rpc.rs:3376):
    ```rust
    struct AutoFillDescriptor {
        server_id: Option<String>,   // resolved to selected portable id when None
        max_bytes: Option<u64>,
        exclude_item_ids: Vec<String>,
    }
    ```
  - [x] Add `fn parse_auto_fill_descriptors(params: &Value) -> Vec<AutoFillDescriptor>`:
    - If `params["autoFill"]` is an **object**: read it as today (`enabled`, `maxBytes`, `serverId`, `excludeItemIds`). Return `vec![desc]` only when `enabled == true`; else `vec![]`.
    - If `params["autoFill"]` is an **array**: map each element to a descriptor, keeping only those with `enabled != false` (treat missing `enabled` as enabled in array form — a descriptor's presence means "this server has a slot").
    - Else (`null`/absent): `vec![]`.
    - Do **not** resolve the selected-server fallback here — keep `server_id: None` and resolve at the call site where `selected_id` is available (mirrors the existing `or_else(|| selected_id.clone())` pattern at rpc.rs:3519, 3455).

- [x] **Generalize `sync_needs_provider_routing` to all auto-fill servers** (`hifimule-daemon/src/rpc.rs:3405-3425`) (AC: 5)
  - [x] Change the signature from `auto_fill_server: Option<&str>` to `auto_fill_servers: &[String]` (resolved serverIds, selected-fallback already applied at the call site). Insert each into the `servers` set before the `match selected_id` decision. Keep the existing item-span logic intact.
  - [x] Update the caller in `handle_sync_calculate_delta` (rpc.rs:3628-3646): replace the single `auto_fill_server` derivation with `parse_auto_fill_descriptors(&params)`, resolve each descriptor's serverId against `selected_id`, collect the resolved ids, and pass that slice. Route to `multi_provider_calculate_delta` when it returns true.

- [x] **Implement the multi-slot expansion loop** (`hifimule-daemon/src/rpc.rs` — `multi_provider_calculate_delta`, :3506-3563) (AC: 2, 3, 4)
  - [x] Replace the single-descriptor `if params.get("autoFill")…enabled` block with a loop over `parse_auto_fill_descriptors(&params)`.
  - [x] Compute the **shared remaining budget** once before the loop: start from the device's available capacity. Reuse the existing derivation: if a descriptor supplied `maxBytes` use it as that slot's ceiling; the shared cap is the device free space plus existing synced minus already-selected bytes (mirror rpc.rs:2600-2616 / 2588-2625). Track `remaining: u64`, initialized to `device_free (+ synced) − sum(desired_items.size_bytes so far)`; if device storage is unavailable and no `maxBytes` is given, surface the same `ERR_CONNECTION_FAILED` as today.
  - [x] For each descriptor (in order received):
    - Resolve `af_server = descriptor.server_id.or(selected_id)`; skip with a clear log if neither resolves.
    - `let provider = get_provider_by_server_id_for(state, &af_server).await?;`
    - `let budget = descriptor.max_bytes.map_or(remaining, |mb| mb.min(remaining));` — skip the slot if `budget == 0`.
    - `exclude_item_ids` = current `desired_items` ids (manual + all earlier slots) — i.e. snapshot `seen_ids` / map over `desired_items`, exactly as the current code builds `exclude_ids` at :3528-3531.
    - Run `run_auto_fill_provider(provider, AutoFillParams { exclude_item_ids, max_fill_bytes: budget })`; map errors to `ERR_CONNECTION_FAILED` as today.
    - Push each returned item as a `DesiredItem { server_id: Some(af_server.clone()), … }` **only if `seen_ids.insert(item.id)` is new** (manual-wins + cross-slot dedup). Decrement `remaining` by each newly added item's `size_bytes` (saturating).
  - [x] Keep the post-loop `calculate_delta` + `augment_delta_with_existence_check` + `delta_value_with_cleanup_metadata` exactly as-is (:3565-3576).

- [x] **Keep the single-server fast paths correct for the 1-descriptor case** (AC: 6)
  - [x] `provider_calculate_delta` (rpc.rs:2540-2671): no functional change required — it already handles the single selected-server descriptor. Optionally refactor its auto-fill block to call a shared helper if you extract one, but **preserve byte-for-byte behavior**. The descriptor's serverId, when present and equal to the selected server, must still take this fast path (verified by AC5 routing).
  - [x] Jellyfin-client path in `handle_sync_calculate_delta` (rpc.rs:3950-4010): unchanged — serves the single selected-server Jellyfin case. (When ≥2 servers or a non-selected single auto-fill server is involved, routing has already diverted to `multi_provider_calculate_delta`.)
  - [x] Consider extracting a small helper `fn push_fill_items_dedup(items, desired_items, seen_ids, server_id, remaining)` to share the "convert `AutoFillItem` → `DesiredItem`, dedup, decrement budget" logic across the three sites — only if it does not change behavior. Otherwise leave the three sites inline.

- [x] **Tests** (`hifimule-daemon/src/rpc.rs` `#[cfg(test)] mod tests`, or a focused module) (AC: 8)
  - [x] `parse_auto_fill_descriptors`: legacy object enabled → 1; legacy disabled/absent → 0; array of 2 → 2; array element `enabled:false` filtered; missing `serverId` left `None` (selected-fallback applied at call site).
  - [x] `sync_needs_provider_routing`: single-server manual items + 2 auto-fill servers → `true`; all-selected (1 descriptor on selected server) → `false`; single non-selected auto-fill server → `true`.
  - [x] Multi-slot dedup + shared budget: with hand-built/faked desired items, prove a track present in manual items is not re-added by a slot; a track added by slot 1 is excluded from slot 2; and `remaining` decrements so a tiny shared budget truncates the second slot. (Use the existing test patterns in `rpc.rs` tests; mock providers only if the existing harness already supports it — otherwise unit-test the pure normalization/routing/dedup-budget helpers and keep provider expansion covered by `auto_fill` tests.)
  - [x] Run `rtk cargo test -p hifimule-daemon` (targeted `rtk cargo test -p hifimule-daemon rpc::` if the full suite is sandbox-gated by mockito/networking — see Previous-story note) and `rtk cargo clippy -p hifimule-daemon --all-targets`.

## Dev Notes

### What this story is (and is not)

This is the **sync-time expansion** story that lifts the "one auto-fill slot total" limit to "one slot per server," delivering the multi-server auto-fill ask at the daemon layer. It is the third step on the critical path (12.1 engine → 12.2 schema/migration → **12.3 multi-slot expansion**). [Source: sprint-change-proposal-2026-06-14-configurable-auto-fill.md:132-135]

It changes **only the daemon's delta-calculation expansion**. It does **not** touch the UI, the RPC config contract, the configurable pipeline engine wiring, or the history DB — those are explicitly later stories (12.4/12.6/12.7/Epic 13, see AC7). After this story, the daemon can expand N per-server slots in one sync; the UI to *drive* multiple slots arrives in 12.6, so with today's UI the observable behavior is unchanged (one selected-server slot) — the new capability is exercised by tests and ready for 12.6.

### The central design decision (read this first): default pipeline, not `run_pipeline` yet

The architecture's expansion pseudocode shows `tracks = run_pipeline(provider, pipeline, db_history, manual_exclude_ids)` per slot (architecture.md:814-820). But `run_pipeline` (Story 12.1) is a **pure, synchronous** function over **already-materialized song pools** — it does **not** fetch from a provider. The async layer that materializes pools from a `MediaProvider` (and the capability-gated playlist/genre sources + filters that make a *configurable* pipeline meaningful) is **Story 12.4** ([12-1 story AC2 + Dev Notes](12-1-autofill-pipeline-domain-model-and-engine.md); epics.md#Story-12.4). Today the only pipeline shape that exists in practice is the legacy default (favorites → play count → date), and the existing `run_auto_fill_provider` already implements its **smart incremental fetch+select** (favorites → frequently-played → recently-played → bulk-library pagination, byte-budgeted, stops when full). Reproducing that via `run_pipeline` would require fetching the whole library to build a pool — a perf and behavior regression.

**Decision for 12.3:** per-slot expansion delegates to the existing `run_auto_fill_provider` (the default pipeline's faithful implementation), looped per server. This guarantees Success-criterion-1 (zero behavior change for existing single-server devices) and keeps 12.3 a tight, low-risk routing change. **Story 12.4 owns** swapping in pool-materialization + the pure `run_pipeline` for configurable sources/filters, and reading `manifest.auto_fill.pipeline_for(serverId)` (the accessor 12.2 reserved for this). Treat `run_auto_fill_provider` as the seam 12.4 will replace. *(This is a deliberate, documented deviation from the architecture's illustrative pseudocode — flagged in the open question below.)*

### Current code being changed (read before writing)

All three sync-time auto-fill expansion sites live in `hifimule-daemon/src/rpc.rs`:

| Site | Lines | Role | 12.3 change |
|---|---|---|---|
| `multi_provider_calculate_delta` | 3431-3577 | Routes mixed-server baskets per provider; auto-fill block at **3506-3563** | **Primary:** replace single-descriptor block with the multi-slot loop |
| `handle_sync_calculate_delta` | 3579-4065 | Entry point; derives `auto_fill_server` (3628-3641), decides routing (3642-3648), then fast paths | Replace single `auto_fill_server` with `parse_auto_fill_descriptors`; pass all resolved ids to routing |
| `provider_calculate_delta` | 2533-2728 | Single-server provider fast path; auto-fill at **2540-2671** | Preserve (serves 1-descriptor selected-server case) |
| Jellyfin path in `handle_sync_calculate_delta` | 3950-4010 | Single-server Jellyfin-client fast path (`run_auto_fill`) | Preserve (1-descriptor selected-server Jellyfin) |

Key helpers (all in rpc.rs):
- `parse_item_specs` (3376-3394) — the model to mirror for `parse_auto_fill_descriptors`: accepts string or `{id, serverId}`.
- `sync_needs_provider_routing` (3405-3425) — **change its `auto_fill_server: Option<&str>` param to `&[String]`.** Its logic: route when any resolved server ≠ selected, or (nothing selected) any concrete server exists.
- `get_provider_by_server_id_for(state, server_id)` (475-480) → wraps `server_manager::get_provider_by_server_id` (server_manager.rs:203); maps **portable** id → local → cached provider. **This is the per-slot routing primitive.**
- `current_server_portable_id(state)` (390) — selected server's portable id (the None-serverId fallback target).
- `tag_untagged_with_selected_portable` (399) — used by the single-server path to tag items; the multi path tags explicitly with `server_id: Some(af_server)`.
- Budget derivation to mirror: `provider_calculate_delta` at 2588-2625 (UI `maxBytes` preferred; else `free + synced − basket`).

`DesiredItem` shape (set `server_id: Some(portable)` per slot): see the literal at rpc.rs:3545-3558.

### Why portable serverIds everywhere

Descriptor `serverId`, item `serverId`, and the `pipelines` map key are all the **portable** `server_id` (Story 2.13), not the machine-local `local_id`. Route via `get_provider_by_server_id_for` (it does portable→local translation); never key providers/vault by the portable id directly. Never write a `local_id` into a `DesiredItem.server_id`. [Source: architecture.md#Server-Identity-Model lines 836-841, 909; #Enforcement lines 909, 921]

### Budget / oversubscription (AC4) — the multi-slot correctness trap

Today one slot fills `free − manual` on one server → one device, no overlap possible. With N slots filling the **same physical device**, naively running each at its own `maxBytes` would oversubscribe capacity (e.g. two 100 GB slots on a 128 GB device). The UI (12.6) will apportion per-server budgets, but the daemon must **guarantee** no oversubscription regardless: expand slots sequentially against one shared `remaining` budget, capping each at `min(descriptor.maxBytes, remaining)` and decrementing `remaining` by what each slot actually adds. For one slot this is identical to today. Fair apportioning/headroom-reserve/duration targets are **not** this story (headroom + duration = Story 12.5); 12.3 only guarantees the hard ceiling. [Source: epics.md#Story-12.5 lines 3050-3057]

### Dedup ordering (AC3)

Manual items are pushed into `desired_items` first (the per-server groups loop, 3478-3504), so they own their ids in `seen_ids` and **win** over any auto-fill pick of the same id. Slots then run in descriptor order, each excluding all already-seen ids (manual + earlier slots). The existing `seen_ids: HashSet` + `seen_ids.insert(...)` guard (3486, 3498, 3544) is exactly the mechanism — extend it across the loop, don't reset it per slot. [Source: architecture.md line 820; rpc.rs:3474-3560]

### Out of scope — do not touch

- **UI:** `BasketSidebar.ts` still builds a single `autoFill` object (BasketSidebar.ts:1163-1176) and `basket.ts` still holds one `__auto_fill_slot__`. Multi-slot cards, the pipeline-builder panel, read-locked per-server slot rendering, and sending the descriptor array are **Story 12.6** (ux-design-specification.md §5.3). The daemon must stay backward-compatible with the current single-object payload (AC1/AC6).
- **`handle_basket_auto_fill` preview** (rpc.rs:5653-5709): leave single-server; adding `serverId` to it is Story 12.7.
- **`main.rs` auto-sync-on-connect** (601 Jellyfin, 1166 provider): one server is connected at a time, so it stays single-server via the 12.2 `legacy_enabled()`/`legacy_max_bytes()` accessors. `autoSyncOnConnect` stays server-independent. [Source: sprint-change-proposal-2026-06-14-configurable-auto-fill.md:124-125 (FR51 note); architecture.md line 825]
- **`autofill_history` DB table** (12.2 scaffolding): Epic 13 only. No reads/writes here.

### Previous story intelligence (12.1 / 12.2)

- 12.2 added `AutoFillConfig` to the manifest with accessors `enabled_for(Some(id))` / `max_bytes_for(Some(id))` and reserved `pipeline_for(serverId) -> Option<&AutoFillPipeline>` **specifically for this story** (`device/mod.rs`). 12.3 does **not** yet need to read manifest pipeline config — the sync descriptors come from `params` (matching today's flow and the architecture contract "`sync.start` carries an array of per-server auto-fill descriptors"). Reading `pipeline_for` to run a configurable pipeline is 12.4. [Source: 12-2 story Dev Agent Record; device/mod.rs]
- 12.2 froze single-server behavior and emits `get_daemon_state autoFill: { enabled, maxBytes }` unchanged — do not alter that JSON.
- **Sandbox caveat (from both prior stories):** full `rtk cargo test -p hifimule-daemon` may not finish where mockito/local networking is blocked (`Operation not permitted`) and macOS system-configuration returns null. Keep new tests **pure** (normalization, routing decision, dedup/budget arithmetic) so they run via targeted `rtk cargo test -p hifimule-daemon rpc::`. Provider-expansion correctness is already covered by `auto_fill` tests — don't re-test it.

### Git intelligence

Recent commits: `176a0f6 Review 12.2`, `b2f1b0c Dev 12.2`, `31282fa Story 12.2`, `8b4ff37 Review 12.1`, `3af9768 Dev 12.1` — Epic 12 on its critical path; 12.2 just merged. No competing in-flight changes to `rpc.rs` auto-fill paths. The multi-server item-routing scaffolding (`multi_provider_calculate_delta`, `sync_needs_provider_routing`, `get_provider_by_server_id_for`) already exists from Epic 2 (Stories 2.11/2.13) and is the foundation this story extends — generalize it, don't rebuild it.

### Latest technical context

- **No new crate dependency** (AC7). `serde_json` (`~1.0`) value-peeking covers the dual object/array param shape — same technique 12.2 used for its dual-shape manifest serde. Rust edition is 2024-era (let-chains in use, e.g. rpc.rs:3952-3954).
- `run_auto_fill_provider` (`auto_fill/mod.rs:358`) and `run_auto_fill` (`:63`) are unchanged by this story; you only call them per slot.

### Project Structure Notes

- All changes in `hifimule-daemon/src/rpc.rs` (delta paths + helpers + tests). No new modules. Binary crate; tests are `#[cfg(test)] mod tests`, run via `rtk cargo test -p hifimule-daemon`.
- No TS/UI changes (12.6 owns UI). No manifest/DB schema changes (12.2 done).

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Epic-12 (lines 3029-3038 — Story 12.3; 3040-3057 — 12.4/12.5 boundary)]
- [Source: _bmad-output/planning-artifacts/architecture.md#Auto-Fill-Pipeline-Model (lines 788-826 — config, expansion loop, contract amendments); #Server-Identity-Model (836-841); #Enforcement (lines 909, 920-922)]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-06-14-configurable-auto-fill.md (Sections 2, 4.2, 5 — sequencing & success criteria)]
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md §5.3 (auto-fill UI = Story 12.6)]
- [Source: _bmad-output/implementation-artifacts/12-1-autofill-pipeline-domain-model-and-engine.md (run_pipeline is pure; async fetch layer = 12.3/12.4)]
- [Source: _bmad-output/implementation-artifacts/12-2-autofill-manifest-schema-and-db-history-scaffolding.md (AutoFillConfig accessors; pipeline_for reserved for 12.3; behavior frozen)]
- [Source: hifimule-daemon/src/rpc.rs:3376-3425 (parse_item_specs, sync_needs_provider_routing), :3431-3577 (multi_provider_calculate_delta), :3579-3652 + 3950-4010 (handle_sync_calculate_delta), :2533-2671 (provider_calculate_delta), :475-480 (get_provider_by_server_id_for), :390-399 (current_server_portable_id, tag_untagged_with_selected_portable)]
- [Source: hifimule-daemon/src/auto_fill/mod.rs:63 (run_auto_fill), :358 (run_auto_fill_provider)]
- [Source: hifimule-ui/src/components/BasketSidebar.ts:1163-1176 (current single autoFill payload — 12.6 will send the array); hifimule-ui/src/state/basket.ts:6 (single AUTO_FILL_SLOT_ID — 12.6 lifts this)]

## Dev Agent Record

### Agent Model Used

claude-opus-4-8 (dev-story workflow)

### Debug Log References

- `rtk cargo build -p hifimule-daemon` → 0 errors (only the pre-existing `api.rs::rename_item` dead-code warning, untouched).
- `rtk cargo test -p hifimule-daemon --bin hifimule-daemon rpc::` → 87 passed.
- `rtk cargo test -p hifimule-daemon --bin hifimule-daemon` (full suite) → 488 passed, 0 failures (no regression; networking did not gate the run this time).
- `rtk cargo clippy -p hifimule-daemon --all-targets` → no new warnings in `rpc.rs`. One redundant-closure introduced by the array `.map(|el| read_descriptor(el))` was fixed to `.map(read_descriptor)`. Remaining `rpc.rs` clippy hits (MutexGuard-held-across-await at 6452/6586) are pre-existing test code outside this story's diff.

### Completion Notes List

- **AC1 — dual-shape `autoFill` param.** Added `struct AutoFillDescriptor` + `parse_auto_fill_descriptors(&Value) -> Vec<AutoFillDescriptor>` next to `parse_item_specs`. Legacy object → 1 elem only when `enabled == true`; array → one descriptor per object element with `enabled != false` (missing `enabled` = enabled, "presence means a slot"); `null`/absent/scalar/empty-array → `vec![]`. Selected-server fallback for `server_id: None` is left to the call sites (mirrors the existing `or_else(|| selected_id…)` pattern).
- **AC2/AC3/AC4 — multi-slot expansion loop.** `multi_provider_calculate_delta`'s single-descriptor block is replaced by a loop over the descriptors. Each slot resolves its provider via `get_provider_by_server_id_for(state, &af_server)`, runs the default-pipeline `run_auto_fill_provider`, and tags items with `server_id: Some(af_server)`. Manual items (resolved first) own their ids in `seen_ids` and win dedup; each slot excludes all already-selected ids (manual + earlier slots, plus the descriptor's own `excludeItemIds`). A single shared `remaining: Option<u64>` budget (device free + synced − already-selected) is capped per slot with `min(maxBytes, remaining)` and decremented by each newly added item — guaranteeing the device is never oversubscribed.
- **AC5 — routing accounts for all auto-fill servers.** `sync_needs_provider_routing`'s `auto_fill_server: Option<&str>` param became `auto_fill_servers: &[String]`; the caller resolves every enabled descriptor's serverId (selected fallback applied) and passes the slice. A single-server basket carrying an auto-fill slot for any non-selected server now routes through the per-provider path.
- **AC6 — single-server zero regression.** `provider_calculate_delta` and the Jellyfin-client fast path are untouched and continue to serve the 1-descriptor selected-server case. For a single slot, `min(maxBytes, remaining) == maxBytes` (the UI already computed `maxBytes = free − manual`), so the multi path is byte-for-byte identical when reached. All 488 daemon tests pass.
- **AC7 — scope held.** No UI, no `autoFill.setPipeline`, no `basket.autoFill` serverId, no `run_pipeline`/pool materialization, no `autofill_history` reads/writes, no `main.rs` auto-sync change, no new crate dependency. Per-slot expansion delegates to today's default-pipeline `run_auto_fill_provider` (the seam Story 12.4 will replace).
- **AC8 — tests.** Extracted a pure `push_fill_items_dedup(...)` helper (used by the loop and unit-testable without a provider). New tests: `test_parse_auto_fill_descriptors_shapes`, `test_sync_needs_provider_routing_multi_auto_fill`, `test_multi_slot_dedup_and_shared_budget`; updated `test_sync_needs_provider_routing` to the new slice signature.
- **Design note (from Dev Notes):** per-slot expansion uses the existing default-pipeline `run_auto_fill_provider`, NOT the pure `run_pipeline` engine — a deliberate, documented deviation from the architecture's illustrative pseudocode. Swapping in pool-materialization + `run_pipeline` + `manifest.auto_fill.pipeline_for(serverId)` is Story 12.4.

### File List

- `hifimule-daemon/src/rpc.rs` (modified) — `AutoFillDescriptor` + `parse_auto_fill_descriptors`, `push_fill_items_dedup` helper, `sync_needs_provider_routing` signature (`&[String]`), multi-slot expansion loop in `multi_provider_calculate_delta`, updated caller in `handle_sync_calculate_delta`, and new/updated tests.

### Review Findings

_Code review 2026-06-14 (Blind Hunter + Edge Case Hunter + Acceptance Auditor). Acceptance Auditor: all 8 ACs implemented as specified. Edge Case Hunter surfaced one AC1 contract gap the auditor missed._

- [x] [Review][Patch] (fixed) (resolved Decision 1 → patch in 12.3) Array-form `autoFill` is silently dropped on the single-server fast paths — `parse_auto_fill_descriptors` (the AC1 normalizer) is only used by `multi_provider_calculate_delta` and the routing decision. The single-server paths `provider_calculate_delta` ([rpc.rs:2542](hifimule-daemon/src/rpc.rs#L2542)) and the inline Jellyfin path ([rpc.rs:4076](hifimule-daemon/src/rpc.rs#L4076)) still read `params["autoFill"]["enabled"]` directly as a JSON object. When every descriptor resolves to the selected server, `sync_needs_provider_routing` returns `false` → these paths run, and `.get("enabled")` on a JSON **array** is `None` → auto-fill is silently skipped (no error). With today's object-form UI this is inert, but AC1 mandates the daemon "accepts either … normalizing both" **now**, and when the array UI ships (12.6) single-server auto-fill — the dominant case — silently dies. Decision: patch in 12.3 (normalize the array on the single-server paths too) vs. defer to 12.6 as an explicit UI-story prerequisite.
- [x] [Review][Patch] (fixed) (resolved Decision 2 → best-effort log+continue) One unresolvable/offline auto-fill slot aborts the entire multi-server delta — in the loop, provider resolution `get_provider_by_server_id_for(...)?` ([rpc.rs:3670](hifimule-daemon/src/rpc.rs#L3670)) and `run_auto_fill_provider(...)?` ([rpc.rs:3682](hifimule-daemon/src/rpc.rs#L3682)) propagate via `?`, aborting the whole `multi_provider_calculate_delta` — including all manual items and every healthy sibling slot. The spec Task says "map errors to `ERR_CONNECTION_FAILED` as today," so fail-fast is arguably sanctioned, but with N slots one deleted/offline/unauthorized server now poisons the entire multi-server sync. Decision: keep fail-fast (spec-literal) vs. make auto-fill slots best-effort (log + `continue` on a bad slot). _(Also covers the `(None,None)` budget early-return at [rpc.rs:3658](hifimule-daemon/src/rpc.rs#L3658), which is spec-sanctioned in isolation but shares the "aborts siblings" concern.)_
- [x] [Review][Patch] (fixed) Empty-string `serverId` (`""`) is not treated as missing — `read_descriptor` keeps `serverId: ""` as `Some("")` ([rpc.rs:~3428](hifimule-daemon/src/rpc.rs#L3428)), so the selected-server fallback (`None` → selected) is skipped; `get_provider_by_server_id_for(state, "")` then fails and (per the abort above) kills the delta. Fix: treat blank/whitespace-only serverId as `None`.
- [x] [Review][Patch] (fixed) Invalid `maxBytes` (negative / float / `> u64::MAX`) silently becomes "no cap" — `read_descriptor`'s `as_u64()` ([rpc.rs:~3432](hifimule-daemon/src/rpc.rs#L3432)) returns `None` for those JSON values, so the slot fills up to the full shared `remaining` instead of the intended (malformed) cap. The shared device budget still prevents oversubscription, so impact is "fills more than the bad cap, never past device capacity." Fix: validate/reject or clamp a non-`u64` `maxBytes` rather than silently dropping the ceiling.
- [x] [Review][Defer] Duplicate `serverId` across two enabled descriptors → redundant full provider pagination ([rpc.rs:~3640](hifimule-daemon/src/rpc.rs#L3640)) — deferred, low priority. `seen_ids` keeps the result correct, but the second same-server slot re-paginates the entire library only to exclude everything slot 1 took (doubled network/server cost). The 12.6 UI generates descriptors and will not emit duplicates; no current caller triggers it.

## Change Log

- 2026-06-14 — Review 12.3: code review (Blind Hunter + Edge Case Hunter + Acceptance Auditor). Acceptance Auditor confirmed all 8 ACs; Edge Case Hunter surfaced an AC1 gap the auditor missed. 4 patches applied to `rpc.rs`: (1) the single-server fast paths (`provider_calculate_delta`, Jellyfin) now normalize the array-form `autoFill` via `parse_auto_fill_descriptors` instead of reading it as an object — closes the AC1 contract gap that would have silently disabled single-server auto-fill once the 12.6 array UI ships; (2) multi-slot loop is now best-effort — a single unresolvable/offline auto-fill slot logs and `continue`s instead of aborting the whole multi-server delta; (3) blank/whitespace `serverId` treated as missing so the selected-server fallback applies; (4) malformed `maxBytes` (negative/float/non-numeric) is floored/clamped instead of silently becoming "no cap". 1 finding deferred (duplicate-serverId redundant pagination — perf, low priority, 12.6 owns descriptor generation). 3 dismissed (2 spec-sanctioned, 1 false positive). Build clean, 488/488 daemon tests pass, no new `rpc.rs` clippy warnings. Status → done.
- 2026-06-14 — Dev 12.3: lifted the single-auto-fill-slot limit at the daemon sync-time expansion. `sync_calculate_delta` `autoFill` now normalizes to `Vec<AutoFillDescriptor>` (legacy object still accepted); `multi_provider_calculate_delta` expands one slot per server (each routed via `get_provider_by_server_id_for` + default-pipeline `run_auto_fill_provider`), with manual-wins dedup, in-order cross-slot dedup, and a shared remaining-capacity budget that prevents device oversubscription. `sync_needs_provider_routing` generalized to consider every auto-fill server. Single-server behavior unchanged (488/488 daemon tests pass). Status → review.
- 2026-06-14 — Story 12.3 created via create-story workflow (ready-for-dev). Scope: lift the single-auto-fill-slot limit at the daemon sync-time expansion. `sync_calculate_delta` `autoFill` param accepts an array of per-server descriptors (legacy single object still accepted); `multi_provider_calculate_delta` expands one slot per server, each routed via `get_provider_by_server_id_for` and expanded with the default-pipeline `run_auto_fill_provider`; manual items win dedup, slots dedup in order, and a shared remaining-capacity budget prevents device oversubscription. Single-server behavior frozen byte-for-byte; UI multi-slot cards (12.6), configurable `run_pipeline` wiring (12.4), `basket.autoFill`+serverId (12.7), and `autofill_history` (Epic 13) are out of scope.
