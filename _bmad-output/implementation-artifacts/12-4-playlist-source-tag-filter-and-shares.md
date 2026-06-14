---
baseline_commit: 5538ecf88e16a34cb258ba7978b9a391abb0fd04
---

# Story 12.4: PlaylistSource, Tag/Genre Filter & Per-Source Shares

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a curator,
I want auto-fill to draw fills from specific playlists, pre-filter by genre, and blend multiple sources by share — by actually running each server's configured pipeline at sync time,
so that I can express "70% from 2 playlists, 30% library remainder, no Christmas music" and each server's slot honors its own configured pipeline instead of the one fixed favorites→playCount→dateCreated algorithm.

## Acceptance Criteria

1. **Configured pipelines run via the pure engine.** Given a device whose `manifest.auto_fill.pipeline_for(serverId)` returns a **non-default** `AutoFillPipeline` (any playlist/genre/favorites/history source beyond a single Library source, any non-empty `filter`, any per-source `share`, a non-`Track` `unit`, a non-legacy `ordering`, or a non-empty `fallback`), when that server's auto-fill slot expands at sync time, then the daemon **materializes the pipeline's source pools from the provider** (async fetch layer) and runs the pure `run_pipeline` engine (`crate::auto_fill::run_pipeline`) over them — replacing the `run_auto_fill_provider` default-algorithm call for that slot. [Source: 12-3 story Dev Notes "the seam Story 12.4 will replace"; architecture.md#Auto-Fill-Pipeline-Model lines 814-820; auto_fill/pipeline.rs:306 `run_pipeline`]

2. **`PlaylistSource` is first-class.** Given a `SourceEntry { kind: Playlist, ref: <playlistId> }` in a configured pipeline, when the slot expands, then the daemon fetches that playlist's tracks via `provider.get_playlist(ref)` and feeds them into the pipeline's `Playlist`/`ref` pool, so only that playlist's tracks (subject to filter/ordering/budget/dedup) are selected. A `Playlist` source whose `ref` is absent/blank is skipped with a log (no panic, no full-library leak). [Source: auto_fill/pipeline.rs:122-134 SourceKind→method mapping; providers/mod.rs:86 `get_playlist`; domain/models.rs:101-104 `PlaylistWithTracks`]

3. **Genre filter is materialized and applied (capability-gated).** Given a configured pipeline whose `filter.include_genres` and/or `filter.exclude_genres` are non-empty, when the slot expands **and the provider advertises `BrowseMode::Genres`**, then the daemon resolves genre membership via `provider.get_genre_tracks(genre, …)` for each referenced genre and attaches the matching genres to each `Candidate` so the engine's `filter_stage` includes/excludes correctly (include = keep only candidates in ≥1 included genre; exclude = drop candidates in any excluded genre). When the provider does **not** support genre enumeration (no `BrowseMode::Genres`, or `get_genre_tracks` returns `UnsupportedCapability`), the genre constraints are **dropped (treated as pass-through) with a log** — never silently emptying the fill. [Source: providers/mod.rs:111-131 `list_genres`/`get_genre_tracks`, :305-332 `BrowseMode`/`Capabilities`; auto_fill/pipeline.rs:377-398 `filter_stage`; auto_fill/pipeline.rs:224-241 `Candidate`]

4. **Tag filter is config-only in 12.4 (documented no-op).** Given a configured pipeline whose `filter.include_tags`/`filter.exclude_tags` are non-empty, when the slot expands, then because no provider currently enumerates per-track tags, the tag constraints are **dropped (pass-through) with a log** — the config shape is preserved and persisted, but tag filtering is not yet enforced (a real tag source/data is Epic 13). No candidate is dropped for a tag reason in 12.4. [Source: auto_fill/pipeline.rs:75-87 FilterStage; domain/models.rs:26-50 `Song` has no tag field; epics.md#Epic-13]

5. **Per-source share blending is honored end-to-end.** Given a configured pipeline with multiple `sources` carrying `share` weights, when the slot expands, then each source's pool is fetched and the engine's existing share allocation (`source_caps`) bounds each source's bytes — proving the "70%/30%" intent over real provider data (capability-unavailable sources contribute an empty pool but do not abort the slot). [Source: auto_fill/pipeline.rs:548-577 `source_caps`/`frac_bytes`, :323-327 primary-source fill]

6. **All three expansion sites honor the configured pipeline; oversubscription & dedup guarantees from 12.3 are preserved.** Given the daemon's three sync-time auto-fill expansion sites, when a configured pipeline applies, then:
   - `multi_provider_calculate_delta`'s per-slot loop reads `manifest.auto_fill.pipeline_for(af_server)` and routes the slot through the configurable expansion when the pipeline is non-default, else keeps `run_auto_fill_provider` (default path). [rpc.rs:3658-3733]
   - `provider_calculate_delta`'s single-server block reads the **selected** server's pipeline and does the same. [rpc.rs:2586-2671]
   - The Jellyfin-client fast path (`run_auto_fill`) is **only** taken for the pure-default case; when the selected (or any auto-fill) server has a configured **non-default** pipeline, routing diverts to the provider path so the configurable engine is used (the Jellyfin-direct `run_auto_fill` is never asked to run a configurable pipeline). [rpc.rs:4111-4138; sync_needs_provider_routing rpc.rs:3520-3560]
   - In every case the slot's effective byte budget remains `min(configured pipeline budget, descriptor/shared-remaining budget)`; manual items still win dedup; cross-slot dedup and the shared `remaining` decrement are unchanged; one failed/offline/capability-poor slot logs and continues (best-effort, per 12.3). [rpc.rs:3486-3518 `push_fill_items_dedup`, :3672-3732]

7. **History/memory stays inert in 12.4 (empty snapshot).** Given the `MemoryStage` (cooldown/played-exclusion) consumes a `HistorySnapshot`, when the slot expands, then 12.4 supplies an **empty** `HistorySnapshot` (default `now`, no entries) — the `autofill_history` DB table is **not** read or written here (that wiring is Epic 13). Cooldown/played-exclusion therefore have no effect yet even if configured; this is an explicit, documented scope boundary, not a bug. [Source: 12-2 AC5/AC7 (table is scaffolding, Epic 13 consumes); auto_fill/pipeline.rs:256-296 `HistorySnapshot`/`PipelineInput`; architecture.md#Enforcement line 922]

8. **Single-server default behavior is byte-for-byte unchanged (zero regression).** Given today's installs (legacy block migrated to a single `default_legacy` pipeline, or a freshly-toggled `{enabled,maxBytes}` slot), when a sync runs auto-fill, then because the resolved pipeline is **default-legacy-equivalent**, expansion still uses `run_auto_fill_provider` (provider path) / `run_auto_fill` (Jellyfin path) exactly as pre-12.4 — same tracks, ordering, budget, dedup, and emitted delta. The new configurable path is reached only by an explicitly non-default pipeline. [Source: rpc.rs:2641, 3713, 4138; auto_fill/pipeline.rs:197-217 `default_legacy`]

9. **Scope boundary — daemon expansion only.** Given epic sequencing, when implementation is complete, then this story does **NOT**: build or change any UI (pipeline builder, slot cards = Story 12.6); add `autoFill.setPipeline` / add `serverId` or inline-pipeline to `basket.autoFill` preview (Story 12.7 — `handle_basket_auto_fill` stays single-server, default-pipeline); add the headroom-reserve / duration-target / guaranteed-fallback-chain budget refinements (Story 12.5 — 12.4 only honors the existing `BudgetStage.max_bytes` capped by the slot budget); read/write `autofill_history` (Epic 13); implement Epic 13 ordering keys / smart sources (`Random` stays a no-op, no new `SourceKind`); change the manifest schema or accessors (12.2 done); or add a new crate dependency. [Source: epics.md#Epic-12 lines 3040-3057; sprint-change-proposal-2026-06-14-configurable-auto-fill.md:132-135]

10. **Build & tests green.** Given the workspace, when `rtk cargo test -p hifimule-daemon` runs, then all existing daemon tests pass (no regression) and new tests cover: the default-vs-configurable discriminator; playlist-source materialization (incl. missing `ref` skip); genre-filter attach + capability-unsupported drop-to-pass-through; tag-constraint drop; per-source-share fetch+blend; empty-history inertness; and single-server default-path parity. `rtk cargo clippy -p hifimule-daemon --all-targets` introduces no new warnings in touched modules.

## Tasks / Subtasks

- [ ] **Add the async materialization + run_pipeline layer** (`hifimule-daemon/src/auto_fill/` — new `fetch.rs` module, declared `pub mod fetch;` + re-export in `auto_fill/mod.rs`) (AC: 1, 2, 3, 4, 5, 7)
  - [ ] Add `pub async fn expand_with_pipeline(provider: Arc<dyn MediaProvider>, pipeline: &AutoFillPipeline, params: AutoFillParams) -> Result<Vec<AutoFillItem>>`.
  - [ ] **Materialize one pool per distinct `SourceKey`** referenced by `pipeline.sources` ∪ `pipeline.fallback` (dedupe identical `(kind, ref)` so a source used twice fetches once):
    - `Library` → paginate `list_all_songs_page(None, offset, PAGE_SIZE)`; reuse the bounds from `run_auto_fill_provider` (`PAGE_SIZE = 500`, `MAX_BULK_PAGES = 200`) — **do not** re-derive different constants. `UnsupportedCapability` → empty pool + log.
    - `Favorites` → `list_favorites(None, 0, MAX_PER_LIST)` (`MAX_PER_LIST = 2000`). `UnsupportedCapability` → empty pool + log.
    - `History` → `list_recently_played(None, 0, MAX_PER_LIST)`. `UnsupportedCapability` → empty pool + log.
    - `Playlist` → require a non-blank `ref`; `get_playlist(ref)` → `.tracks`. Missing/blank `ref` or `UnsupportedCapability`/`NotFound` → skip that source (empty pool) + log; never abort the slot.
  - [ ] **Genre membership for the filter (AC 3):** if `pipeline.filter.include_genres`/`exclude_genres` is non-empty **and** `provider.capabilities().browse.list_modes` contains `BrowseMode::Genres`: for each referenced genre, call `get_genre_tracks(genre, 0, limit)` (bounded like the priority lists) and build a `HashMap<track_id, Vec<genre>>`; then when wrapping each pool `Song` into a `Candidate`, set `candidate.genres` from that map. If `BrowseMode::Genres` is absent OR `get_genre_tracks` returns `UnsupportedCapability`, **clear the genre constraints** before running the engine (clone the pipeline, empty `filter.include_genres`/`exclude_genres`) and log once — pass-through, never empty the fill.
  - [ ] **Tag constraints (AC 4):** always clear `filter.include_tags`/`filter.exclude_tags` before running (no provider tag data in 12.4) and log once if they were set. Keep this in the same cloned-pipeline normalization as the genre fallback.
  - [ ] **Budget cap (AC 6):** clone the pipeline and set `budget.max_bytes = Some(min(pipeline.budget.max_bytes.unwrap_or(params.max_fill_bytes), params.max_fill_bytes))` so the slot never exceeds its shared-remaining/descriptor budget while still honoring a smaller user-configured budget. Carry `params.exclude_item_ids` into `PipelineInput.exclude_item_ids`. Do **not** add headroom/duration/fallback-chain refinements (Story 12.5).
  - [ ] **History (AC 7):** build `PipelineInput { pools, history: HistorySnapshot::default(), exclude_item_ids }` — empty history; no DB read. Document that cooldown/played-exclusion are inert until Epic 13.
  - [ ] Call `run_pipeline(&input, &normalized_pipeline)` and return the `Vec<AutoFillItem>`. The function is `async` only for the fetch; the selection core stays the pure `run_pipeline`.

- [ ] **Add the default-vs-configurable discriminator** (`auto_fill/pipeline.rs` or `fetch.rs`) (AC: 1, 8)
  - [ ] `pub fn needs_configurable_expansion(p: &AutoFillPipeline) -> bool`: returns `false` (use the fast default path) only when the pipeline is **default-legacy-equivalent** — `sources` is empty or exactly one `Library` (no `ref`, no `share`), `filter` is all-empty, `ordering` is empty or exactly `[Favorite, PlayCount, DateCreated]`, `unit == Track`, `memory == MemoryStage::default()`, and `fallback` is empty. Any deviation → `true`. Add a focused unit test pinning both branches (incl. `AutoFillPipeline::default_legacy(Some(n))` ⇒ `false`).

- [ ] **Introduce a single shared slot-expansion seam** used by all three sites (`hifimule-daemon/src/rpc.rs`) (AC: 6, 8)
  - [ ] Add `async fn expand_auto_fill_slot(provider: Arc<dyn MediaProvider>, pipeline: Option<&AutoFillPipeline>, params: AutoFillParams) -> Result<Vec<AutoFillItem>, ...>`: when `pipeline` is `Some(p)` and `needs_configurable_expansion(p)` → `crate::auto_fill::expand_with_pipeline(provider, p, params)`; else → `crate::auto_fill::run_auto_fill_provider(provider, params)`. Map errors consistently with the existing per-site handling.
  - [ ] Resolve the **portable** serverId for each slot and read `manifest.auto_fill.pipeline_for(serverId)` to obtain the `Option<&AutoFillPipeline>`. Remove the now-obsolete `#[allow(dead_code)]` on `pipeline_for` (`device/mod.rs:350`) once it is referenced.

- [ ] **Wire `multi_provider_calculate_delta` per-slot loop** (`rpc.rs:3658-3733`) (AC: 1, 6)
  - [ ] After resolving `af_server` and the slot `budget`/`provider`, read `manifest.auto_fill.pipeline_for(&af_server)` and call `expand_auto_fill_slot(provider, pipeline_opt, fill_params)` instead of the direct `run_auto_fill_provider`. Keep the best-effort log+`continue` on error and the `push_fill_items_dedup` merge + `remaining` decrement exactly as-is.

- [ ] **Wire `provider_calculate_delta` single-server block** (`rpc.rs:2586-2671`) (AC: 1, 6, 8)
  - [ ] Resolve the selected portable id (the same source `get_daemon_state`/12.2 uses: `state.db.get_server_config()?.server_id`, or the already-resolved `current_server_portable_id`), read `manifest.auto_fill.pipeline_for(selected_portable)`, and call `expand_auto_fill_slot(...)` instead of the direct `run_auto_fill_provider`. Preserve the existing budget derivation and dedup loop byte-for-byte for the default case.

- [ ] **Force provider routing when a configured pipeline applies (Jellyfin path)** (`rpc.rs:3520-3560` `sync_needs_provider_routing` + `handle_sync_calculate_delta`) (AC: 6, 8)
  - [ ] In `handle_sync_calculate_delta`, before choosing the Jellyfin-client fast path, check whether the selected server (or any resolved auto-fill descriptor's server) has a configured **non-default** pipeline (`pipeline_for(id).is_some_and(needs_configurable_expansion)`); if so, route through the provider path (`multi_provider_calculate_delta` / `provider_calculate_delta`) so the configurable engine runs. The Jellyfin-direct `run_auto_fill` block (`rpc.rs:4117-4138`) stays **unchanged** and is reached only for the pure-default case.
  - [ ] Do not break the existing routing for non-auto-fill or default-auto-fill baskets — extend the decision, don't replace it.

- [ ] **Tests** (`auto_fill` tests for `fetch`/discriminator; `rpc::` tests for routing) (AC: 10)
  - [ ] `needs_configurable_expansion`: `default()`, `default_legacy(Some(n))` ⇒ false; a pipeline with a `Playlist` source / non-empty `filter` / `share` / `Unit::Album` / `[Quality]` ordering / non-empty `fallback` ⇒ true.
  - [ ] `expand_with_pipeline` over a **mock `MediaProvider`** (build a minimal in-test impl, or reuse an existing test provider in `auto_fill`/`providers` tests if one exists — check before writing a new one): playlist source returns only that playlist's tracks; missing `ref` → that source skipped; genre filter attaches genres and includes/excludes correctly; provider without `BrowseMode::Genres` → genre constraints dropped (fill not emptied); tag constraints dropped; two shared sources blend by share; empty `HistorySnapshot` ⇒ cooldown/played config has no effect.
  - [ ] Routing: a basket whose selected server has a configured non-default pipeline forces the provider path (does not take the Jellyfin-client fast path); a default-pipeline single-server basket still takes the fast path (parity with 12.3 `test_sync_needs_provider_routing*`).
  - [ ] Run `rtk cargo test -p hifimule-daemon` (targeted `rtk cargo test -p hifimule-daemon auto_fill::` / `rpc::` if the full suite is sandbox-gated by mockito/networking — see Previous-story note), and `rtk cargo clippy -p hifimule-daemon --all-targets`.

## Dev Notes

### What this story is (and is not)

This is the story that **finally runs the pure `run_pipeline` engine against real provider data.** Stories 12.1 (engine), 12.2 (manifest schema + `pipeline_for`), and 12.3 (multi-slot sync-time expansion) all deliberately deferred the async fetch layer to here. 12.3 expanded N per-server slots but each slot still used the fixed default algorithm (`run_auto_fill_provider`). 12.4 builds the **fetch/materialization layer** that turns a configured `AutoFillPipeline` (playlist sources, genre filter, per-source shares) into materialized pools the pure engine consumes — and wires it at the three sync-time expansion sites, reading the per-server config from `manifest.auto_fill.pipeline_for(serverId)`. [Source: 12-3 story "The central design decision"; epics.md#Story-12.4]

It is **daemon-only**: no UI to *configure* a pipeline ships yet (Story 12.6), and no RPC to *set* one (`autoFill.setPipeline`, Story 12.7). So in practice a non-default pipeline only exists if hand-written into a manifest or set by a later story — but 12.4 makes the daemon **honor** it correctly the moment one exists, validated by tests. With today's UI (which still sends the legacy `{enabled,maxBytes}` slot), every install resolves to a default-legacy pipeline → the fast path → **zero observable change** (AC 8).

### The central design decision: keep the fast path for default, materialize only for configured

`run_auto_fill_provider` (`auto_fill/mod.rs:358`) is a **smart incremental fetch**: favorites → frequently-played → recently-played → bulk library pagination, byte-budgeted, stops the moment the budget is full. The pure `run_pipeline` instead needs **already-materialized pools** — for a `Library` source that means paginating the whole library into memory before selecting. For the overwhelmingly common default case that would be a pointless perf regression. So:

- **Default-legacy-equivalent pipeline** (`needs_configurable_expansion == false`) → keep `run_auto_fill_provider` / `run_auto_fill` exactly as today. This is the zero-regression guarantee (AC 8).
- **Any non-default pipeline** → `expand_with_pipeline`: materialize the pools the config references, then `run_pipeline`. The full-library materialization cost is bounded by the existing `MAX_BULK_PAGES`/`PAGE_SIZE` constants (reuse them — don't invent new ones). Optimizing configured-pipeline fetch (e.g. genre-scoped library fetch) is a later concern; correctness first.

`needs_configurable_expansion` is the linchpin. Make it conservative and well-tested: only the exact legacy shape takes the fast path; everything else materializes. Note a user who configures `ordering: [Quality]` over Library (Antoine) **must** materialize — the fixed `run_auto_fill_provider` cannot express quality ordering — so the discriminator correctly sends it to the engine.

### Genre filter — how to attach genres to candidates (AC 3)

`Song` carries **no genre field** (`domain/models.rs:26-50`), and the engine's `filter_stage` (`pipeline.rs:377-398`) operates on `Candidate.genres`. So the fetch layer must *attach* genres. The provider surface for this is `get_genre_tracks(genre, offset, limit)` (`providers/mod.rs:122`), which returns the songs in a genre. Strategy:

1. Collect the genres referenced by the filter (`include_genres ∪ exclude_genres`).
2. For each, fetch its track ids via `get_genre_tracks` → build `HashMap<track_id, Vec<genre_name>>`.
3. When wrapping each materialized pool `Song` into a `Candidate`, set `genres` from that map (default empty).
4. The engine's existing `filter_stage` then does include/exclude correctly.

**Capability gating (non-negotiable, AC 3):** if the provider does not advertise `BrowseMode::Genres` (`capabilities().browse.list_modes`) or `get_genre_tracks` returns `UnsupportedCapability`, you must **drop the genre constraints (pass-through)**, not run the engine with a genre filter no candidate can satisfy — otherwise the fill silently empties. Clear `filter.include_genres`/`exclude_genres` on a cloned pipeline and log once. Belt-and-suspenders: check the capability first, and also treat a runtime `UnsupportedCapability` from `get_genre_tracks` as the same graceful drop.

### Tag filter — config-only in 12.4 (AC 4)

`FilterStage` has `include_tags`/`exclude_tags`, but no provider enumerates per-track tags today. So 12.4 **persists and accepts** tag config (the shape is already in 12.1's model) but does **not** enforce it: clear the tag constraints before running and log. A real tag data source is Epic 13. Do not invent a tag source here (scope creep / review rejection).

### Three expansion sites — wire all, but route configured through the provider (AC 6)

All sync-time auto-fill expansion lives in `rpc.rs`:

| Site | Lines | 12.4 change |
|---|---|---|
| `multi_provider_calculate_delta` per-slot loop | 3658-3733 | Read `pipeline_for(af_server)`; call the shared `expand_auto_fill_slot` (configurable when non-default, else `run_auto_fill_provider`). Keep best-effort log+continue, dedup, shared `remaining`. |
| `provider_calculate_delta` single-server | 2586-2671 | Read the selected server's `pipeline_for(...)`; same shared seam. Preserve byte-for-byte for default. |
| Jellyfin-client fast path (`run_auto_fill`) | 4111-4138 | **Unchanged.** Reached only for the pure-default case. When the selected/any auto-fill server has a configured non-default pipeline, divert to the provider path (extend `sync_needs_provider_routing` / the routing decision in `handle_sync_calculate_delta`). |

Extract one shared `expand_auto_fill_slot(provider, pipeline_opt, params)` helper so the configurable-vs-default decision lives in exactly one place (mirrors how 12.3 extracted `push_fill_items_dedup`). The budget passed in (`params.max_fill_bytes`) is already the slot's `min(descriptor.maxBytes, shared remaining)` from 12.3 — `expand_with_pipeline` caps the configured pipeline budget by it, so the 12.3 oversubscription guarantee (AC 6) and dedup are untouched.

**Why route configured pipelines off the Jellyfin-client fast path:** `run_auto_fill` is a Jellyfin-direct optimization with no provider abstraction and no way to express a pipeline. Rather than teach it pipelines, force the provider path (Jellyfin servers have a `JellyfinProvider`) when a configured pipeline applies. This confines all new logic to the provider paths and leaves the Jellyfin-direct path a pure-default fallback.

### History/memory is inert here (AC 7) — the storage split

The `MemoryStage` (cooldown, played-exclusion) consumes a `HistorySnapshot` that, per the storage split, comes from the daemon DB `autofill_history` table. That table is **scaffolding only** (12.2 created it; **Epic 13** reads/writes it). So 12.4 supplies `HistorySnapshot::default()` (empty) — cooldown/played-exclusion config has no effect yet. This is correct and deliberate: keeping the DB read out of 12.4 holds the storage-split enforcement line and the engine stays pure. Do **not** read/write `autofill_history` here. [Source: architecture.md#Enforcement line 922; 12-2 AC5/AC7]

### Current code being changed (read before writing)

- **`auto_fill/pipeline.rs`** — the pure engine. `run_pipeline(&PipelineInput, &AutoFillPipeline)`, `PipelineInput { pools: HashMap<SourceKey, Vec<Candidate>>, history, exclude_item_ids }`, `Candidate { song, genres, tags }`, `SourceKey { kind, ref_id }`, `SourceKind { Library, Favorites, History, Playlist }`, `FilterStage`, `source_caps` (share blending), `default_legacy`. **Reuse all of it unchanged** — 12.4 is the fetch layer that *feeds* it, not a re-implementation. The module's `#![allow(dead_code)]` can stay (Epic 13 variants remain unused); referencing `run_pipeline`/`PipelineInput`/`Candidate` from `fetch.rs` makes them live. [pipeline.rs:33,52-296,306-343,548-577]
- **`auto_fill/mod.rs`** — `run_auto_fill_provider` (`:358`, the default path + the fetch pattern/constants to mirror: `MAX_PER_LIST=2000`, `PAGE_SIZE=500`, `MAX_BULK_PAGES=200`, `UnsupportedCapability` → skip), `run_auto_fill` (`:63`, Jellyfin-direct, unchanged), `AutoFillItem`, `AutoFillParams { exclude_item_ids, max_fill_bytes }`. Add `pub mod fetch; pub use fetch::*;`.
- **`MediaProvider`** (`providers/mod.rs:55-287`) — methods 12.4 calls: `list_favorites` (`:166`), `list_recently_played` (`:155`), `list_all_songs_page` (`:194`), `get_playlist` (`:86`), `get_genre_tracks` (`:122`), `capabilities()` (`:286`) → `Capabilities { browse: BrowseCapabilities { list_modes: Vec<BrowseMode> } }` (`:319-332`); `BrowseMode::{Genres, Playlists}` (`:305-317`). Default trait impls return `ProviderError::UnsupportedCapability` — handle it as a graceful skip everywhere.
- **`device/mod.rs`** — `AutoFillConfig::pipeline_for(server_id) -> Option<&AutoFillPipeline>` (`:351`, currently `#[allow(dead_code)]` — make it live). The manifest is already in scope at each expansion site as `manifest: &DeviceManifest` / `manifest.auto_fill`.
- **`rpc.rs`** — expansion sites and helpers: `provider_calculate_delta` (`:2533`), `multi_provider_calculate_delta` (`:3557`, slot loop `:3658-3733`), `parse_auto_fill_descriptors`/`AutoFillDescriptor` (`:3401-3477`), `push_fill_items_dedup` (`:3486`), `sync_needs_provider_routing` (`:3520`), `get_provider_by_server_id_for` (`:475-480`), `current_server_portable_id` (`:390`), the Jellyfin-client path (`:4117-4138`).

### Portable serverIds everywhere

The `pipelines` map key, the descriptor `serverId`, and `DesiredItem.server_id` are all the **portable** `server_id` (Story 2.13), not `local_id`. Read `pipeline_for(portable_id)`; route providers via `get_provider_by_server_id_for` (it does portable→local translation). Never key the manifest by `local_id`. [Source: architecture.md#Server-Identity-Model lines 836-841, 909; #Enforcement lines 920-922]

### Architecture compliance (non-negotiable)

- Run **per-slot** via `get_provider_by_server_id` (already done in 12.3) — never the active/global provider for a non-selected server. [architecture.md line 921]
- Config in manifest, history in DB, never mixed — 12.4 reads config (`pipeline_for`), supplies empty history, touches no DB. [architecture.md line 922]
- The pure engine stays pure: all I/O (provider fetches) lives in `fetch.rs`; `run_pipeline` is never made async and never handed a provider. [pipeline.rs:12-20]

### Previous story intelligence (12.1 / 12.2 / 12.3)

- **12.1** built and test-pinned the engine and its share/filter/budget/dedup semantics. Build on those tests — don't re-test engine internals; test the **fetch layer** (materialization, capability gating) and the **routing/seam**.
- **12.2** made `auto_fill` a `Map<serverId, AutoFillPipeline>` and reserved `pipeline_for` "specifically for this story." A single-server install holds exactly one pipeline (the migrated `default_legacy`), so `pipeline_for(selected)` returns a default-equivalent → fast path → no change.
- **12.3** lifted the single-slot limit and explicitly named `run_auto_fill_provider` "the seam Story 12.4 will replace," extracted `push_fill_items_dedup`, made slots best-effort (log+continue), and guaranteed shared-budget non-oversubscription. Preserve all of that; 12.4 only swaps *what each slot calls to expand*.
- **Sandbox caveat (all three prior stories):** full `rtk cargo test -p hifimule-daemon` may not finish where mockito/local networking is blocked (`Operation not permitted`) or macOS system-configuration returns null. Keep new engine/fetch tests provider-mock-based and pure where possible; run targeted `rtk cargo test -p hifimule-daemon auto_fill::` / `rpc::`.

### Git intelligence

Recent commits: `5538ecf Review 12.3`, `6c24ed5 Dev 12.3`, `2b3911b Story 12.3`, `176a0f6 Review 12.2`, `b2f1b0c Dev 12.2` — Epic 12 on its critical path; 12.3 just merged. No competing in-flight changes to `auto_fill/` or `rpc.rs` auto-fill paths. The multi-slot loop, `push_fill_items_dedup`, and per-server routing this story extends are all freshly landed in 12.3 — generalize them, don't rebuild.

### Latest technical context

- **No new crate dependency** (AC 9). `async-trait` (provider methods), `serde_json`, std collections cover everything. Rust edition is 2024-era (let-chains in use). Adding a dependency is scope creep / review rejection. [Source: Cargo.toml]
- For a test `MediaProvider`, check `hifimule-daemon/src/auto_fill/mod.rs` tests and `providers/` tests for an existing mock/fake before writing a new one (12.1's engine tests are pure and don't use a provider; `run_auto_fill_provider` tests may use the Jellyfin client harness — prefer a small hand-rolled `MediaProvider` impl returning canned pools/genres for `expand_with_pipeline`).

### Project Structure Notes

- New module: `hifimule-daemon/src/auto_fill/fetch.rs` (declared `pub mod fetch;` + `pub use fetch::*;` in `auto_fill/mod.rs`, beside `pub mod pipeline;`). The async materialization layer lives here; the pure engine stays in `pipeline.rs`. Do **not** put fetch I/O in `pipeline.rs` (keeps the pure core fixture-testable) and do **not** put config in `domain/models.rs`.
- Seam/routing edits in `rpc.rs`. Binary crate (no `lib.rs`); tests are `#[cfg(test)] mod tests`, run via `rtk cargo test -p hifimule-daemon`.
- No TS/UI changes (12.6 owns UI). No manifest/DB schema changes (12.2 done).

### Testing standards

- Engine internals are already covered by 12.1's pure tests — do not duplicate. New coverage targets: (1) `needs_configurable_expansion` both branches; (2) `expand_with_pipeline` against a hand-rolled `MediaProvider` mock — playlist materialization, missing-ref skip, genre attach + capability-drop, tag-drop, share fetch+blend, empty-history inertness; (3) the routing decision (configured pipeline forces provider path; default stays on the fast path). Mirror 12.3's pure-test style for routing and the provider-mock style for fetch. [Source: 12-1/12-3 testing standards]

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Epic-12 (lines 3040-3049 — Story 12.4; 3050-3057 — 12.5 budget boundary)]
- [Source: _bmad-output/planning-artifacts/architecture.md#Auto-Fill-Pipeline-Model (lines 788-826 — config, expansion loop, contract); #Server-Identity-Model (836-841); #Enforcement (920-922)]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-06-14-configurable-auto-fill.md (Sections 4.1 FR50, 4.2-4.3, 5 — sequencing)]
- [Source: _bmad-output/implementation-artifacts/12-1-autofill-pipeline-domain-model-and-engine.md (run_pipeline is pure; async fetch layer = 12.3/12.4; SourceKind→method mapping)]
- [Source: _bmad-output/implementation-artifacts/12-2-autofill-manifest-schema-and-db-history-scaffolding.md (pipeline_for reserved for this story; storage split; autofill_history is Epic 13)]
- [Source: _bmad-output/implementation-artifacts/12-3-multi-slot-sync-time-expansion.md ("the seam Story 12.4 will replace"; push_fill_items_dedup; best-effort slots; shared budget)]
- [Source: hifimule-daemon/src/auto_fill/pipeline.rs (run_pipeline:306, PipelineInput/Candidate/SourceKey:224-296, FilterStage:75-87, source_caps:548-577, default_legacy:197-217)]
- [Source: hifimule-daemon/src/auto_fill/mod.rs:358 (run_auto_fill_provider + fetch constants), :63 (run_auto_fill)]
- [Source: hifimule-daemon/src/providers/mod.rs:55-287 (MediaProvider: list_favorites:166, list_recently_played:155, list_all_songs_page:194, get_playlist:86, get_genre_tracks:122, capabilities:286), :305-332 (BrowseMode/Capabilities)]
- [Source: hifimule-daemon/src/device/mod.rs:351 (pipeline_for), :296-320 (enabled_for/max_bytes_for)]
- [Source: hifimule-daemon/src/rpc.rs:2533-2671 (provider_calculate_delta), :3401-3518 (descriptor/normalizer/push_fill_items_dedup), :3520-3560 (sync_needs_provider_routing), :3557-3748 (multi_provider_calculate_delta + slot loop), :4111-4138 (Jellyfin-client auto-fill path), :475-480 (get_provider_by_server_id_for), :390 (current_server_portable_id)]
- [Source: hifimule-daemon/src/domain/models.rs:16-22 (Genre), :26-50 (Song — no genre/tag field), :101-104 (PlaylistWithTracks)]

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List

## Change Log

- 2026-06-14 — Story 12.4 created via create-story workflow (ready-for-dev). Scope: build the async pool-materialization + `run_pipeline` fetch layer (`auto_fill/fetch.rs`), implement first-class `PlaylistSource` (`get_playlist`), genre filter (capability-gated via `get_genre_tracks`/`BrowseMode::Genres`, pass-through fallback), config-only tag filter, and per-source share blending over real provider data; wire `manifest.auto_fill.pipeline_for(serverId)` at the three sync-time expansion sites via a shared `expand_auto_fill_slot` seam (configurable when non-default, else default `run_auto_fill_provider`/`run_auto_fill`); force provider routing when a configured non-default pipeline applies. History/memory inert (empty snapshot; `autofill_history` is Epic 13); budget honored capped by the 12.3 shared-remaining slot budget (headroom/duration/fallback-chain = Story 12.5). Single-server default behavior byte-for-byte unchanged; no UI/RPC-contract/schema/new-deps.
