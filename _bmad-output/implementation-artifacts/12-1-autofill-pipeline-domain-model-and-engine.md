---
baseline_commit: aefee3f7e7625c72130938a5225fb98cba3d3357
---

# Story 12.1: Auto-Fill Pipeline Domain Model & Pure-Function Engine

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer,
I want the auto-fill pipeline expressed as composable pure functions over a provider's library (filter → source → unit → order → dedupe-vs-memory → budget),
so that selection logic is testable without UI or network and is validated against real user needs before any UI, persistence, or sync wiring is built.

## Acceptance Criteria

1. **Pipeline config model.** Given the daemon defines an auto-fill pipeline, when a pipeline is constructed, then an `AutoFillPipeline` type and its stage sub-types exist exactly per the architecture data model (`enabled`, `filter`, `sources`, `unit`, `ordering`, `memory`, `budget`, `fallback`), each stage is independently optional, and a pipeline with as little as one stage configured is valid. [Source: architecture.md#Auto-Fill-Pipeline-Model]
2. **Pure-function engine.** Given a set of already-materialized candidate inputs (one or more named song pools + a memory/history snapshot + manual-exclude ids), when `run_pipeline(input, pipeline)` is invoked, then it returns a deterministic, ordered `Vec<AutoFillItem>` produced entirely by synchronous pure functions — **no network, no `async`, no `MediaProvider` calls, no clock/RNG reads** inside the pure core. Each stage is its own pure function composed in fixed order: filter → source-blend(by share) → unit → ordering → dedupe-vs-memory → budget(+fallback).
3. **Legacy = default pipeline.** Given no pipeline is configured (or a legacy `{ enabled, maxBytes }` block is supplied), when the engine runs, then it behaves identically to today's algorithm — a single ordering stage `["favorite", "playCount", "dateCreated"]` over the library source, byte-budgeted to `maxBytes` — so existing devices select the same tracks with zero behavior change. [Source: architecture.md#Auto-Fill-Pipeline-Model; hifimule-daemon/src/auto_fill.rs]
4. **Four personas, one model.** Given the four brainstorm personas (Claire/Antoine/Léo/Nadia), when each persona's intent is expressed as an `AutoFillPipeline`, then every persona is representable with **no special-case code paths in the engine** — only by composing the stage algebra — and a unit test per persona proves the resulting selection matches the intent. [Source: brainstorming-session-2026-06-12-1.md#Action-Planning]
5. **Budget & determinism guarantees.** Given a byte budget, when the engine selects, then the cumulative estimated size never exceeds the budget; tracks of unknown/zero/negative size are skipped (never counted as 0-byte fillers); manual-exclude ids and within-run duplicates are never emitted twice; and the terminal fallback chain is applied in order to reach the budget target when earlier sources are exhausted. [Source: hifimule-daemon/src/auto_fill.rs:212-336]
6. **No wiring, no I/O, no new deps.** Given this is Story 12.1, when implementation is complete, then it adds **only** the model + engine + tests: it does NOT touch the manifest schema/serialization (Story 12.2), the DB `autofill_history` table (Story 12.2), `sync.start`/sync-time expansion (Story 12.3), async provider fetching or capability gating (Story 12.4), RPC contracts/`basket.autoFill`/`autoFill.setPipeline` (12.7), or any UI (12.6); and it introduces no new crate dependency.
7. **Build & tests green.** Given the workspace builds, when `rtk cargo test -p hifimule-daemon` runs, then all new modules compile and all new unit tests (including the four persona tests and the legacy-equivalence test) pass with no regression to the existing `auto_fill` tests.

## Tasks / Subtasks

- [x] Define the pipeline config domain model (AC: 1, 3)
  - [x] **Convert `hifimule-daemon/src/auto_fill.rs` into a directory module:**
    - [x] Move the existing file contents to `hifimule-daemon/src/auto_fill/mod.rs` (keeps `run_auto_fill`, `run_auto_fill_provider`, `rank_and_truncate`, `AutoFillItem`, `AutoFillParams`, `ProviderFillState`, and the existing `#[cfg(test)] mod tests` intact with their `super::*` imports). Add `pub mod pipeline;` and `pub use pipeline::*;` so the new types are reachable as `crate::auto_fill::*`.
    - [x] Create `hifimule-daemon/src/auto_fill/pipeline.rs` for the new model + engine + their tests.
    - [x] Leave `mod auto_fill;` at `hifimule-daemon/src/main.rs:59` unchanged — the directory module resolves through it automatically. Verify no other module path (`crate::auto_fill::*`) breaks after the move.
    - [x] Do NOT scatter the selection model into `domain/models.rs` — that module is reserved for provider-neutral entities, not feature config.
  - [x] Define `AutoFillPipeline { enabled: bool, filter: FilterStage, sources: Vec<SourceEntry>, unit: Unit, ordering: Vec<OrderingKey>, memory: MemoryStage, budget: BudgetStage, fallback: Vec<SourceEntry> }`.
  - [x] Define stage sub-types matching architecture.md#Auto-Fill-Pipeline-Model:
    - `FilterStage { include_tags, exclude_tags, include_genres, exclude_genres }` (all `Vec<String>`, default empty = pass-through).
    - `SourceEntry { kind: SourceKind, ref_id: Option<String>, share: Option<f32> }` where `SourceKind` enumerates at least `Library`, `Favorites`, `History`, `Playlist` (extensible for Epic 13). `share` is a 0.0–1.0 blend weight; `None`/empty = equal/remainder.
    - `Unit { Track | Album | Artist }` (default `Track`).
    - `OrderingKey` enum (`Favorite`, `PlayCount`, `DateCreated`, `Random`, `Quality`, …) — an **ordered** list applied as a stable multi-key sort. Implement the keys whose data is on `Song` today (`Favorite`, `PlayCount`, `DateCreated`, `Quality` via bitrate); reserve the rest as variants for Epic 13.
    - `MemoryStage { cooldown_weeks: Option<u32>, played_exclusion: bool, stable_core_pct: Option<f32>, repeat_tolerance: Option<f32>, tiers: Option<...> }` — in 12.1 the engine only *consumes* a supplied history snapshot to exclude/cool-down; it does not read or write any DB.
    - `BudgetStage { max_bytes: Option<u64>, target_duration_secs: Option<u64>, headroom_bytes: Option<u64> }`.
  - [x] Derive `Debug, Clone, PartialEq` and `Serialize/Deserialize` with `#[serde(rename_all = "camelCase")]` and `#[serde(default)]` on every optional field, matching the JSON shape in architecture.md (config is later persisted verbatim in the manifest by Story 12.2 — get the field names right now). Use `#[serde(default)]` so an empty/partial pipeline deserializes cleanly.
  - [x] Provide `AutoFillPipeline::default_legacy(max_bytes: Option<u64>) -> Self` returning `{ ordering: [Favorite, PlayCount, DateCreated], budget: { max_bytes }, .. }` and document it as the backward-compatibility mapping for legacy `{ enabled, maxBytes }`.

- [x] Define the pure engine input and implement `run_pipeline` (AC: 2, 3, 5)
  - [x] Define `PipelineInput` carrying everything the pure core needs without touching a provider: named candidate pools (e.g. `pools: HashMap<SourceKey, Vec<Song>>` or `Vec<(SourceEntry, Vec<Song>)>`), a `HistorySnapshot` (track ids → last-synced/played info supplied by the caller, **not** read from DB here), and `exclude_item_ids: Vec<String>`. The async layer that materializes pools from a `MediaProvider` is Story 12.3/12.4 — do not build it here.
  - [x] Implement each stage as a standalone pure `fn` over `Vec<Song>` (or grouped units), composed in fixed order. Keep each function small and individually unit-testable.
  - [x] Implement source-share blending: when multiple `SourceEntry` have shares, interleave/allocate the budget across sources proportionally; remainder/unshared sources fill what's left.
  - [x] Reuse the existing size logic: prefer `Song.size_bytes` when present, else estimate `(bitrate_kbps * 1_000 / 8) * duration_seconds`; **skip** tracks whose size is unknown/0 (mirror `ProviderFillState::try_add` and `rank_and_truncate`, do not duplicate-emit). [Source: hifimule-daemon/src/auto_fill.rs:304-331]
  - [x] Budget stage: accumulate by estimated size; stop at `min(max_bytes, capacity − headroom_bytes)`; if `target_duration_secs` is set, derive a byte ceiling from it (duration→bytes via the same bitrate estimate). Apply the `fallback` source list in order once primary sources can't reach target. Never exceed the ceiling.
  - [x] Produce ordered `Vec<AutoFillItem>` (reuse the existing struct) with a `priority_reason` string describing the winning stage/source (e.g. `"favorite"`, `"playCount:N"`, `"playlist:<id>"`, `"fallback:library"`) so downstream/preview UX keeps working.

- [x] Validate the model against the four personas (AC: 4)
  - [x] Write one unit test per persona that constructs an `AutoFillPipeline` and asserts the selection over a hand-built fixture library — proving the algebra expresses each with no engine special cases:
    - **Claire** — commuter, ~8 GB, hates repeats: small `budget.max_bytes`, `memory.cooldown_weeks`/`repeat_tolerance` low, sources favorites + library. Assert recently-synced (cooled-down) tracks are excluded and the set fits the small budget.
    - **Antoine** — audiophile, 512 GB DAP, quality-first: large budget, `ordering: [Quality, …]`. Assert higher-bitrate tracks rank first and the large budget is filled.
    - **Léo** — gym-goer, tiny device, energy-driven: tiny budget, a `Playlist` source ("energy"), tight budget truncation. Assert only the playlist pool's tracks are picked, truncated to the tiny budget.
    - **Nadia** — parent filling a kid's player: `filter.include_genres`/`exclude_tags` (kids music / no explicit), plus a source. Assert filtered-out tracks never appear.
  - [x] Add a comment block in the test module restating "four personas, one model" — these tests are the Story-12.1 success gate (over-abstraction risk mitigation). [Source: sprint-change-proposal-2026-06-14-configurable-auto-fill.md#Section-5]

- [x] Backward-compatibility & guarantee tests (AC: 3, 5, 7)
  - [x] Test that `default_legacy(Some(maxBytes))` over a fixture library yields the same favorites→playCount→dateCreated ordering and byte-truncation as today's `rank_and_truncate`/`run_auto_fill_provider` priority order.
  - [x] Test budget never exceeded; unknown/0/negative-size skipped; manual-exclude and within-run dedup honored; empty library → empty result; zero budget → empty result; fallback chain reached only after primary exhaustion.
  - [x] Run `rtk cargo test -p hifimule-daemon`; confirm existing `auto_fill::tests` still pass unchanged.

## Dev Notes

### What this story is (and is not)

This is a **foundation** story: a pure, in-memory selection algebra plus the data model that later stories persist, wire, and surface. The unifying design insight from the change proposal: *an auto-fill definition is one pipeline config per `(device, portable serverId)` pair; today's hardcoded algorithm becomes the default single-Ordering-stage pipeline.* Story 12.1 builds the engine that makes that true, validated by the persona test **before** any UI exists. [Source: epics.md#Epic-12; sprint-change-proposal-2026-06-14-configurable-auto-fill.md#Section-3]

The single most important architectural decision: **separate fetching (async, impure, provider-bound) from selection (sync, pure, fixture-testable).** The pure engine receives already-materialized song pools + a history snapshot and returns the ordered selection. This is exactly what makes "testable without UI or network" achievable and is the brainstorm's explicit Priority-1 action ("Spike the pipeline as pure functions over the library … testable without UI"). Do not make `run_pipeline` `async`; do not give it a `MediaProvider`. [Source: brainstorming-session-2026-06-12-1.md#Action-Planning]

### Current code being extended (read before writing)

- **`hifimule-daemon/src/auto_fill.rs`** is the foundation to generalize — keep it working. Two existing paths:
  - `run_auto_fill(client, params)` — legacy Jellyfin-direct path (server-sorted by `IsFavoriteOrLiked,PlayCount,DateCreated`, paginate-until-budget). [auto_fill.rs:54-206]
  - `run_auto_fill_provider(provider, params)` — provider-neutral path: favorites → frequently-played → recently-played → bulk library pagination, byte-budgeted via `ProviderFillState`. **This priority order is exactly the default pipeline you must reproduce.** [auto_fill.rs:349-477]
  - `rank_and_truncate(tracks, max_fill_bytes) -> (Vec<AutoFillItem>, bool)` and `ProviderFillState::try_add` — reuse their size-estimation and skip-on-unknown-size semantics verbatim; do not re-derive them differently. [auto_fill.rs:212-336]
  - `AutoFillItem` — the output struct (`id, name, album, artist, provider_album_id, provider_content_type, provider_suffix, size_bytes, priority_reason`). **Reuse it as the engine's output** so sync expansion (Story 12.3) and preview stay compatible. [auto_fill.rs:17-34]
  - `AutoFillParams { exclude_item_ids, max_fill_bytes }` — the engine input should carry the same exclude/budget concepts. [auto_fill.rs:36-42]
  - Existing tests in `auto_fill::tests` must keep passing; if you convert the file to a directory, move them with their `super::*` imports intact.

- **`Song`** (`hifimule-daemon/src/domain/models.rs:24-50`) is the candidate type. Fields the engine relies on: `id`, `duration_seconds: u32`, `bitrate_kbps: Option<u32>`, `play_count: Option<u32>`, `is_favorite: Option<bool>`, `date_added: Option<String>`, `last_played_at: Option<String>`, `size_bytes: Option<u64>`, `content_type`, `suffix`, `album_id`, `album_title`, `artist_id`, `artist_name`. Note `Song` carries **no genre/tag field** today — so `FilterStage` semantics in 12.1 operate on pools the caller pre-filters or on data the caller supplies; full genre/tag enumeration via the provider is Story 12.4. Model the filter fields now (config shape must be right for 12.2), and implement the include/exclude logic against whatever genre/tag the test fixtures attach (consider a thin `Candidate` wrapper carrying optional `genres`/`tags` alongside the `Song` if needed — keep it internal to the engine, not in `domain/models.rs`).
- **`MediaProvider`** (`hifimule-daemon/src/providers/mod.rs:55-287`) — the data source the async layer (12.3/12.4) will call: `list_favorites`, `list_frequently_played`, `list_recently_played`, `list_all_songs_page`, `list_genres`/`get_genre_tracks`, `list_playlists`/`get_playlist`. Capability-gated methods return `ProviderError::UnsupportedCapability`. **12.1 does not call any of these** — but design `SourceKind` so each maps cleanly to one of these methods later. [Source: providers/mod.rs]

### Architecture compliance (non-negotiable)

- The pipeline is `Filter → Sources → Unit → Ordering → Memory → Budget` with a terminal fallback chain; a config is "an ordered list of `(Source, Picker, share)` entries + global modifiers + budget"; the legacy favorites→playCount→dateCreated behaviour is the **default single-Ordering-stage pipeline**. Match the JSON field names in the model block exactly. [Source: architecture.md#Auto-Fill-Pipeline-Model (lines 788-826); architecture.md:77]
- **Storage split — enforced even though 12.1 persists nothing:** pipeline **config** is portable manifest data (Story 12.2); runtime **history** (cooldown windows, stable-core, pity-timer) is daemon-DB, machine-local (Story 12.2 scaffolds it, Epic 13 consumes it). In 12.1 the `MemoryStage` engine logic must take history as an **input parameter** (the `HistorySnapshot`), never read a DB or a clock. Keeping config and history strictly separate is an explicit all-agents enforcement rule. [Source: architecture.md#Enforcement (lines 920-922)]
- The engine is provider-agnostic; per-server routing via `get_provider_by_server_id` happens at the *fetching* layer (Story 12.3), not in the pure core. Don't bake any server identity into `run_pipeline`. [Source: architecture.md (lines 814-824)]

### The four personas (the success gate)

From the brainstorm (`brainstorming-session-2026-06-12-1.md:46-48`): **Claire** (commuter, 8 GB, hates repeats), **Antoine** (audiophile, 512 GB DAP, quality-first), **Léo** (gym-goer, tiny device, energy-driven), **Nadia** (parent filling a kid's player). The breakthrough that justifies this whole epic is *Source × Strategy separation*: "drawing from what?" (Source) × "picking how?" (Picker/ordering) as independent axes, blended by share, bounded by budget. The risk is over-abstraction; the mitigation is the persona test — **four personas, one model, no special cases.** If a persona forces an `if persona == …` branch in the engine, the algebra is wrong, not the persona. [Source: brainstorming-session-2026-06-12-1.md:113,151-167; sprint-change-proposal-2026-06-14-configurable-auto-fill.md:137]

### Determinism

The pure core must be deterministic for the same input. The `Random` ordering key is a model variant for later epics — in 12.1 do not pull entropy inside the engine. If you implement `Random` at all, take a caller-supplied seed in `PipelineInput` so tests stay deterministic (or defer `Random` to Epic 13 and leave it a no-op/unsupported variant). Same rule for any time-based memory math: derive "now" from a value passed in `HistorySnapshot`, never from the system clock. (AC: 2)

### Latest technical context

- No new crate is needed or permitted for this story. `serde`/`serde_json` (workspace `~1.0`), `async-trait` (already present), and std collections cover everything. Rust edition is 2024-era (let-chains are in use, e.g. `auto_fill.rs:147-149`); target the existing workspace toolchain. Adding a dependency here would be scope creep and a review rejection. [Source: Cargo.toml; AC #6]

### Project Structure Notes

- Module: the existing top-level `auto_fill` module (declared `mod auto_fill;` at `hifimule-daemon/src/main.rs:59`) becomes a **directory module**. Required layout: `auto_fill/mod.rs` (the moved existing fns + `pub mod pipeline; pub use pipeline::*;`) and `auto_fill/pipeline.rs` (new model + engine). The `mod auto_fill;` declaration in `main.rs` is unchanged. Do **not** put selection config in `domain/models.rs` — that module is reserved for provider-neutral entities, not feature config.
- This is a binary crate (`main.rs`, no `lib.rs`); tests are `#[cfg(test)] mod tests` inside the module and run via `rtk cargo test -p hifimule-daemon`.

### Testing standards

- Pure functions + hand-built `Song` fixtures; no mockito, no async, no provider mocks. Highest-value tests: the four persona tests, the legacy-equivalence test, and the budget/dedup/skip guarantee tests. Mirror the fixture style of `auto_fill::tests::make_track` but build `domain::models::Song` values. Keep each stage function independently testable. [Source: hifimule-daemon/src/auto_fill.rs:479-626]

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Epic-12 (lines 3002-3018)]
- [Source: _bmad-output/planning-artifacts/architecture.md#Auto-Fill-Pipeline-Model (lines 788-826)]
- [Source: _bmad-output/planning-artifacts/architecture.md (line 77 — Auto-Fill Pipeline component); (lines 913-923 — Enforcement)]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-06-14-configurable-auto-fill.md (Sections 3, 4.2-4.3, 5)]
- [Source: _bmad-output/brainstorming/brainstorming-session-2026-06-12-1.md (personas line 46-48; pipeline model line 138; action plan 151-167)]
- [Source: hifimule-daemon/src/auto_fill.rs (existing algorithm, AutoFillItem, size logic)]
- [Source: hifimule-daemon/src/domain/models.rs (Song)]
- [Source: hifimule-daemon/src/providers/mod.rs (MediaProvider surface — for SourceKind→method mapping only)]

## Dev Agent Record

### Agent Model Used

claude-opus-4-8 (Claude Code dev-story workflow)

### Debug Log References

- `rtk cargo test -p hifimule-daemon` → 469 passed (17 new `auto_fill::pipeline::tests`, no regressions to the 6 existing `auto_fill::tests`).
- `rtk cargo clippy -p hifimule-daemon --all-targets` → no warnings for `auto_fill` (pre-existing warnings in unrelated modules untouched).

### Implementation Plan

The selection algebra is implemented as composable pure functions in `auto_fill/pipeline.rs`, in the fixed order `filter → source-blend(by share) → unit → ordering → dedupe-vs-memory → budget(+fallback)`:

- **Fetching/selection split (the core decision):** `run_pipeline(&PipelineInput, &AutoFillPipeline) -> Vec<AutoFillItem>` is fully synchronous and pure — no `async`, no `MediaProvider`, no clock, no RNG. The caller materializes `PipelineInput.pools` (keyed by `SourceKey { kind, ref_id }`) and supplies a `HistorySnapshot { now, entries }`; "now" is a value on the snapshot, never the system clock. The async fetch layer is Story 12.3/12.4.
- **Stages as standalone fns:** `filter_stage`, `memory_stage`, `unit_stage`, `compare_by_ordering`, and the `Selector` (budget + dedupe + fallback) are each small and independently unit-testable.
- **Ordering** is a stable multi-key sort; keys present on `Song` are implemented (`Favorite`, `PlayCount`, `DateCreated`, `Quality`/bitrate). `Random` is a deterministic no-op in 12.1 (entropy deferred to Epic 13 via a caller-supplied seed) so the core stays deterministic.
- **Size & budget** reuse the legacy semantics: prefer `Song.size_bytes`, else `(bitrate_kbps*1000/8)*duration`; unknown/zero-size tracks are skipped (never 0-byte fillers). The `Selector` enforces the global ceiling (`max_bytes − headroom_bytes`), per-source share caps, the optional duration target, manual-exclude ids, and within-run dedup, with the same stop-on-first-oversized behavior as `rank_and_truncate`/`ProviderFillState::try_add`.
- **Unit grouping** treats `Track` as one-unit-per-song (identical to today's track-level behavior); `Album`/`Artist` group by id and add whole units atomically.
- **`default_legacy(max_bytes)`** reproduces today's `[Favorite, PlayCount, DateCreated]` ordering over the `Library` source, byte-budgeted — the legacy-equivalence test pins this.

### Completion Notes List

- **Module conversion:** `auto_fill.rs` → `auto_fill/mod.rs` (via `git mv`, history preserved) + new `auto_fill/pipeline.rs`. All existing `run_auto_fill*`, `rank_and_truncate`, `AutoFillItem`, `AutoFillParams`, `ProviderFillState`, and the 6 existing tests are unchanged. `mod auto_fill;` in `main.rs:59` is untouched; all 12 `crate::auto_fill::*` call-sites in `main.rs`/`rpc.rs` continue to resolve.
- **Four personas, one model:** Claire/Antoine/Léo/Nadia are each expressed purely by composing the stage algebra — there are **no `if persona == …` branches** in the engine. A comment block in the test module restates this success-gate.
- **No wiring / no new deps (AC #6):** only the model + engine + tests were added; manifest schema, DB, `sync.start`, provider fetching, RPC, and UI are all untouched. No new crate dependency (only `serde`/`serde_json`/std). The engine is intentionally unreferenced by the binary until later stories, so a documented module-level `#![allow(dead_code)]` (and one `#[allow(unused_imports)]` on the `pub use`) keeps the build clean — consistent with the codebase's "reserved for future use" convention.
- **Guarantees proven by test:** budget never exceeded; headroom subtracted; unknown/zero-size skipped; manual-exclude + within-run dedup honored; empty library/zero budget → empty; fallback reached only after primary exhaustion; share allocation across sources; camelCase serde round-trip + empty-object default.

### File List

- `hifimule-daemon/src/auto_fill.rs` → `hifimule-daemon/src/auto_fill/mod.rs` (renamed via `git mv`; added `pub mod pipeline;` + re-export)
- `hifimule-daemon/src/auto_fill/pipeline.rs` (new — pipeline config model, pure-function engine, and 17 unit tests)
- `_bmad-output/implementation-artifacts/12-1-autofill-pipeline-domain-model-and-engine.md` (story tracking: frontmatter `baseline_commit`, tasks, Dev Agent Record, Status)
- `_bmad-output/implementation-artifacts/sprint-status.yaml` (status: ready-for-dev → in-progress → review)

## Change Log

- 2026-06-14 — Story 12.1 implemented: auto-fill pipeline domain model + pure-function engine. Converted `auto_fill.rs` into a directory module and added `auto_fill/pipeline.rs` (model + `run_pipeline` engine + 17 unit tests, incl. the four persona tests and legacy-equivalence). No wiring/I/O/new deps. `rtk cargo test -p hifimule-daemon` → 469 passed; clippy clean for `auto_fill`. Status → review.
