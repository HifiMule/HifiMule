---
baseline_commit: 8dc855a3b0eafe724db43ff506cb61aad7cd1bab
---

# Story 13.1: Memory & Rotation Strategies

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a HifiMule user who re-syncs the same device regularly,
I want the auto-fill pipeline to **remember what it has put on my device** — so it can skip tracks synced recently (cooldown), skip tracks I've already played, keep a stable core while rotating the rest fresh, dial how tolerant of repeats it is, and cycle through playlist-backed tiers across syncs,
so that each sync brings meaningful novelty instead of re-copying the same favorites, while still respecting my budget and per-server config.

This story **activates the Memory stage end-to-end**. Story 12.1 already built the pure-engine cooldown/played-exclusion logic (`memory_allows`), and Story 12.2 created the `autofill_history` DB table as empty scaffolding. Today both are **inert**: `fetch.rs` passes an empty `HistorySnapshot` and nothing ever writes a history row ([fetch.rs:182-187](../../hifimule-daemon/src/auto_fill/fetch.rs#L182-L187)). This story wires the DB (record on sync, read at fill), implements the three reserved Memory fields (`stable_core_pct`, `repeat_tolerance`, `tiers`), and surfaces the new controls in the configuration UI with i18n.

## Acceptance Criteria

### A — DB foundation (history read/write)

1. **autofill_history data-access layer.** Given the `autofill_history` table from Story 12.2 ([db.rs:220-231](../../hifimule-daemon/src/db.rs#L220-L231)), when this story is implemented, then `Database` gains methods (matching the existing `scrobble_history` style at [db.rs:719-755](../../hifimule-daemon/src/db.rs#L719-L755)): an **upsert** of `(device_id, server_id, track_id, last_synced_at, tier)` using `INSERT … ON CONFLICT(device_id, server_id, track_id) DO UPDATE`, a **bulk read** by `(device_id, server_id)` returning all rows, and a **prune** removing rows older than a retention cutoff. All time values are **Unix seconds (i64)**. No method reads the system clock — callers pass `now`. [Source: architecture.md#Auto-Fill-Pipeline-Model lines 809-812; db.rs:104-117 (`Arc<Mutex<Connection>>`), 719-795 (insert/query/upsert patterns)]

2. **Sync completion records synced tracks.** Given a sync run that successfully transfers tracks resolved from an auto-fill slot, when the sync completes, then for each track actually written to the device the daemon upserts an `autofill_history` row with `device_id` = the device's manifest id, `server_id` = the **portable** server id of the slot, `track_id` = the provider track id, and `last_synced_at` = `now` (Unix secs). Manual (non-auto-fill) selections are **not required** to be recorded for cooldown, but recording all synced tracks for the device+server is acceptable and simpler — see Dev Notes for the recommended hook point. Recording is **best-effort**: a DB write failure logs and never fails the sync. [Source: architecture.md line 922 (history in DB, config in manifest); enforcement: never write config to history]

3. **History snapshot built at fill time.** Given a configured pipeline whose Memory stage is non-default, when `expand_with_pipeline` runs ([fetch.rs:102-189](../../hifimule-daemon/src/auto_fill/fetch.rs#L102-L189)), then the empty `HistorySnapshot::default()` at [fetch.rs:185](../../hifimule-daemon/src/auto_fill/fetch.rs#L185) is replaced by a snapshot whose `now` is the caller-supplied current Unix seconds and whose `entries` are built from: (a) `last_synced_at` and `tier` read from `autofill_history` for this `(device_id, server_id)`, merged with (b) `last_played_at` derived from each candidate **provider Song** (see AC 5). `device_id`, `server_id`, and `now` are threaded into the fetch layer via new fields on `AutoFillParams` ([auto_fill/mod.rs:52-57](../../hifimule-daemon/src/auto_fill/mod.rs#L52-L57)). The DB read is best-effort: failure yields an empty snapshot (memory becomes inert) rather than aborting the slot.

### B — Memory strategies

4. **Sync cooldown (#4) works end-to-end.** Given a pipeline with `memory.cooldownWeeks = N` and a track whose most recent `autofill_history.last_synced_at` is within `N × 7 × 86400` seconds of `now`, when the slot expands, then that track is excluded from the result; a track synced longer ago, or never synced, is eligible. This is enforced by the **existing** `memory_allows` ([pipeline.rs:417-433](../../hifimule-daemon/src/auto_fill/pipeline.rs#L417-L433)) — do **not** reimplement it; only feed it real history. Repeat-tolerance (AC 7) modulates the effective window.

5. **Played-track exclusion (#5) works end-to-end.** Given a pipeline with `memory.playedExclusion = true`, when the slot expands, then any candidate the **media server reports as played** is excluded. "Played" is derived from the provider `Song` (the server is the source of truth for plays, not the device history): a track is played when `play_count.unwrap_or(0) > 0` **or** `last_played_at.is_some()` ([domain/models.rs Song fields `play_count`, `last_played_at`](../../hifimule-daemon/src/domain/models.rs)). For such candidates the snapshot entry's `last_played_at` is set to a non-`None` value (the parsed timestamp when available, else `now` as a present-but-unknown sentinel) so `memory_allows`' `played_exclusion && h.last_played_at.is_some()` branch fires. Played-exclusion does **not** require any `autofill_history` row.

6. **Stable-core + delta (#24).** Given a pipeline with `memory.stableCorePct = p` (0.0–1.0), when the slot expands against a non-empty budget ceiling `C`, then the fill is partitioned: up to `round(C × p)` bytes are filled **first** from candidates that are currently on the device (have a `last_synced_at` row for this device+server), exempt from cooldown — the *stable core*; the remaining budget is filled with the *delta* from the rest, honoring cooldown/played-exclusion as usual. The core is selected using the same Filter/Ordering/Unit/dedup rules as the delta. When no history exists (first sync) the core is empty and the entire budget behaves as a normal fill. `p = 0` (or unset) is a no-op (today's behavior). The combined result still never exceeds `C` and never emits a 0-byte track. [Engine change in `run_pipeline`/`Selector` — see Dev Notes.]

7. **Repeat-tolerance dial (#23).** Given a pipeline with `memory.repeatTolerance = t` (0.0–1.0), when cooldown is configured (`cooldownWeeks = Some(N)`), then the **effective** cooldown window is scaled to `N × 7 × 86400 × (1 − t)` seconds: `t = 0` (default) → full window (strict, current behavior); `t = 1` → zero window (recently-synced tracks fully allowed); intermediate `t` → proportionally shorter window. When `cooldownWeeks` is `None`, `repeatTolerance` has no effect (it only modulates cooldown — it is not an independent gate). This scaling is applied inside `memory_allows` (or a thin helper it calls) so the dial composes with the existing pure-function tests. Must remain deterministic — no RNG. [Source: brainstorm #23 via sprint-change-proposal-2026-06-14-configurable-auto-fill.md line 110]

8. **Rotation tiers (#25/#26), playlist-backed.** Given a pipeline with `memory.tiers` set to an ordered JSON array of tier definitions (each `{ "kind": "playlist", "ref": "<playlistId>" }` for #26, or `{ "kind": "library" }`), when the slot expands across **successive syncs**, then a per-`(device, server)` rotation cursor (stored machine-local in the daemon DB) advances by 1 on each completed sync, and the tier list is rotated by `cursor mod tiers.len()` so the "lead" tier — which receives the largest budget share — changes each sync, cycling the device through tiers over time. Each emitted track records its source tier index in `autofill_history.tier` (string). When `tiers` is unset/empty, no rotation occurs (today's behavior). Tier pools reuse the existing playlist/library materialization in `fetch.rs`. [Source: brainstorm #25/#26 via sprint-change-proposal line 110; architecture.md `tier` column line 811. **This is the least-specified sub-feature — read the Rotation Tiers design note in Dev Notes carefully and keep it conservative & deterministic.**]

### C — Routing, UI, i18n, scope

9. **Configurable-path routing recognizes the new Memory fields.** Given a pipeline whose only non-default aspect is a Memory strategy (cooldown, played-exclusion, stable-core, repeat-tolerance, or tiers), when `needs_configurable_expansion` ([fetch.rs:65-94](../../hifimule-daemon/src/auto_fill/fetch.rs#L65-L94)) is evaluated, then it returns `true` so the materialized engine path runs. This already holds via the `memory_default = p.memory == MemoryStage::default()` check (line 80) — **verify** it still triggers for `stableCorePct`/`repeatTolerance`/`tiers`-only pipelines and add a test; no logic change expected.

10. **Configuration UI exposes the new Memory controls.** Given the Auto-Fill pipeline-builder Memory section ([AutoFillPanel.ts:253-260](../../hifimule-ui/src/components/AutoFillPanel.ts#L253-L260)), when this story is implemented, then under the **Advanced** disclosure the Memory stage renders (in addition to the existing cooldown input + played-exclusion switch): a **stable-core %** control, a **repeat-tolerance** dial/slider (0–100%), and a **rotation tiers** editor (add/remove playlist-backed tiers, ordered). Each control reads/writes the matching `MemoryStage` field on the frontend pipeline type ([state/autoFill.ts:23-31](../../hifimule-ui/src/state/autoFill.ts#L23-L31)), is captured into the pipeline on save, and round-trips through `autoFill.setPipeline` / `get_daemon_state` like the existing controls. The simple (non-Advanced) default path is unchanged. The "ambition tier" cheap equivalents already exist (playlist sources) — surface them inline per UX. [Source: ux-design-specification.md §5.3 line 98-99]

11. **i18n parity across all locales.** Given the i18n catalog `[hifimule-i18n/catalog.json]` with 4 locales (`en`, `fr`, `es`, `de`), when new UI strings are added (stable-core %, repeat-tolerance, tiers labels + hints), then a key is added to **all 4 locales** (parity lock — the 12.7 pattern `57×4`), following the `basket.autofill.*` snake_case convention, and the i18n parity test stays green. [Source: 12.7 completion notes "i18n parity lock 57×4"; i18n.ts:11]

12. **Backward compatibility & scope.** Given existing devices/manifests, when this story ships, then: a pipeline with a default Memory stage behaves **exactly** as today (zero migration); the legacy fast path (`run_auto_fill_provider`) is untouched; config stays in the manifest and history/rotation state stays in the daemon DB (never mixed — [architecture.md line 922](../../_bmad-output/planning-artifacts/architecture.md)); and the story does **NOT** implement Epic 13 features owned by later stories — best-version/quality ordering (13.2), discovery sources (13.3), rarity draws & pity-timer (13.4 — note `tier` is *recorded* here but pity-timer *consumption* is 13.4), context/encoding-from-goals (13.5), advanced units & promotion (13.6).

13. **Build & tests green.** Given the workspace, when `rtk cargo test -p hifimule-daemon` runs, then all existing daemon tests pass (no regression) and new tests cover: DB upsert/read/prune; cooldown & played-exclusion with populated history (extend the persona suite — Claire already asserts cooldown at [pipeline.rs:788-853](../../hifimule-daemon/src/auto_fill/pipeline.rs#L788-L853)); stable-core partition; repeat-tolerance window scaling (boundaries t=0, t=1, mid); rotation cursor advance + lead-tier shift; `needs_configurable_expansion` for new fields. `rtk cargo clippy -p hifimule-daemon --all-targets` adds no new warnings in touched modules. Frontend `tsc` + build stay green.

## Tasks / Subtasks

- [x] **DB data-access layer for `autofill_history` + rotation cursor** (`hifimule-daemon/src/db.rs`) (AC: 1, 8)
  - [x] Add a `TrackHistoryRow` struct (or reuse a tuple) and `pub fn upsert_autofill_history(&self, device_id, server_id, track_id, last_synced_at: Option<i64>, tier: Option<&str>) -> Result<()>` using `INSERT … ON CONFLICT(device_id, server_id, track_id) DO UPDATE SET last_synced_at=excluded.last_synced_at, tier=excluded.tier`. Mirror `record_scrobble` ([db.rs:719-735](../../hifimule-daemon/src/db.rs#L719-L735)).
  - [x] `pub fn get_autofill_history(&self, device_id, server_id) -> Result<Vec<(String /*track_id*/, Option<i64>, Option<String>)>>` (bulk read for snapshot build).
  - [x] `pub fn prune_autofill_history(&self, device_id, server_id, older_than_unix: i64) -> Result<usize>` (retention; call after recording — see Dev Notes for cutoff).
  - [x] Rotation cursor: add a tiny `autofill_rotation(device_id TEXT, server_id TEXT, cursor INTEGER, PRIMARY KEY(device_id, server_id))` table in `Database::init()` after the `autofill_history` block ([db.rs:230](../../hifimule-daemon/src/db.rs#L230)), `CREATE TABLE IF NOT EXISTS` style. Add `get_rotation_cursor(device_id, server_id) -> Result<i64>` (default 0) and `advance_rotation_cursor(device_id, server_id) -> Result<i64>` (upsert cursor+1, return new value).
  - [x] Tests: in-memory DB upsert→read round-trip; conflict update overwrites `last_synced_at`; prune deletes old rows only; cursor defaults to 0 and advances.

- [x] **Thread `device_id`/`server_id`/`now` through the fetch seam** (`hifimule-daemon/src/auto_fill/mod.rs`, `fetch.rs`) (AC: 3)
  - [x] Extend `AutoFillParams` ([auto_fill/mod.rs:52-57](../../hifimule-daemon/src/auto_fill/mod.rs#L52-L57)) with `pub device_id: String`, `pub server_id: String`, `pub now_unix: i64`. Update every `AutoFillParams { … }` construction site (grep — primarily `rpc.rs` `handle_basket_auto_fill` ~[rpc.rs:5871-5990](../../hifimule-daemon/src/rpc.rs#L5871-L5990) and the sync-expand path ~[rpc.rs:2636-2680](../../hifimule-daemon/src/rpc.rs#L2636-L2680)). The fast legacy path (`run_auto_fill`) ignores the new fields.
  - [x] In `expand_with_pipeline` ([fetch.rs:182-187](../../hifimule-daemon/src/auto_fill/fetch.rs#L182-L187)) accept a `&Database` (or pass a pre-built `HistorySnapshot` from the RPC caller — **preferred**, keeps `fetch.rs` provider-only and pure-adjacent; decide and document). Build `HistorySnapshot { now: params.now_unix, entries }` where entries merge DB `last_synced_at`/`tier` with per-candidate `last_played_at` (AC 5). Best-effort on DB error → empty snapshot.

- [x] **Activate cooldown (#4) + played-exclusion (#5)** (`hifimule-daemon/src/auto_fill/fetch.rs` or RPC) (AC: 4, 5)
  - [x] Cooldown: rely on existing `memory_allows`; just supply real `last_synced_at`. Add an integration-style test that a recently-synced track is dropped and an old one survives.
  - [x] Played-exclusion: when building the snapshot, set `entry.last_played_at = Some(ts)` for candidates with `play_count > 0` or a parseable `last_played_at`. Parse provider ISO timestamps to Unix secs; on parse failure but a play signal exists, use `now` as a present sentinel.

- [x] **Repeat-tolerance dial (#23)** (`hifimule-daemon/src/auto_fill/pipeline.rs`) (AC: 7)
  - [x] In `memory_allows` ([pipeline.rs:417-433](../../hifimule-daemon/src/auto_fill/pipeline.rs#L417-L433)), compute `effective_secs = (weeks × 7 × 86400) as f32 × (1.0 − repeat_tolerance.clamp(0,1))` and compare against it. Guard: tolerance only applies when `cooldown_weeks` is `Some`. Keep saturating arithmetic.
  - [x] Unit tests: t=0 == current; t=1 → no cooldown exclusion; t=0.5 → half window.

- [x] **Stable-core + delta (#24)** (`hifimule-daemon/src/auto_fill/pipeline.rs`) (AC: 6)
  - [x] In `run_pipeline` ([pipeline.rs:306-343](../../hifimule-daemon/src/auto_fill/pipeline.rs#L306-L343)) / `Selector::fill`, when `memory.stable_core_pct` is `Some(p > 0)`: compute `core_cap = round(ceiling × p)`. **Pass 1** fills `core_cap` from candidates with a history entry having `last_synced_at.is_some()` (the device's current set), bypassing the cooldown check (these are kept on purpose) but honoring filter/ordering/unit/dedup and played-exclusion. **Pass 2** fills the remaining ceiling from all candidates honoring full memory rules; dedup against Pass 1.
  - [x] Ensure the total respects `ceiling` and the existing budget guarantees (no 0-byte fillers). Reuse `estimated_size`/`Selector` accounting; do not double-count.
  - [x] Tests: with history present, ~p fraction of bytes comes from previously-synced tracks; p=0 unchanged; first-sync (empty history) → core empty, normal fill.

- [x] **Rotation tiers (#25/#26)** (`hifimule-daemon/src/auto_fill/{pipeline.rs,fetch.rs}`, `rpc.rs`) (AC: 8)
  - [x] Define a typed `TierDef { kind: TierKind (Playlist{ref}|Library), }` parsed from `memory.tiers` (`serde_json::Value` → typed via `serde_json::from_value`); tolerate malformed → treat as no tiers (log).
  - [x] Caller reads `cursor = db.get_rotation_cursor(device, server)`; rotate `tiers` by `cursor % len`; assign budget shares so the lead tier dominates (e.g. lead gets 50%, rest split remainder, or a documented schedule). Materialize each tier's pool via existing `materialize_pool`/`fetch_playlist` ([fetch.rs:318-379](../../hifimule-daemon/src/auto_fill/fetch.rs#L318-L379)).
  - [x] After a **successful sync** that used tiers, call `db.advance_rotation_cursor(device, server)`. Record each emitted track's tier index into `autofill_history.tier` via the upsert (AC 2).
  - [x] Tests: cursor advance shifts the lead tier; emitted tracks carry tier strings; empty/malformed `tiers` → no rotation.

- [x] **Sync-completion recording hook** (`hifimule-daemon/src/sync.rs` and/or `rpc.rs` sync path) (AC: 2, 8)
  - [x] Identify the once-per-run post-transfer completion point (sync.rs ~2944+ per exploration; confirm). For each successfully transferred track that belongs to an auto-fill slot's server, upsert `autofill_history` with `last_synced_at = now`, `tier = <assigned index or NULL>`. Best-effort. Prune stale rows (cutoff e.g. `now − max(cooldownWeeks across pipelines, default 26) weeks`, or a fixed generous retention — document).
  - [x] Advance the rotation cursor here if the run used tiers.

- [x] **Routing verification** (`hifimule-daemon/src/auto_fill/fetch.rs`) (AC: 9)
  - [x] Add tests proving `needs_configurable_expansion` returns `true` for pipelines whose only deviation is `stableCorePct`/`repeatTolerance`/`tiers`. The `memory_default` check at [fetch.rs:80](../../hifimule-daemon/src/auto_fill/fetch.rs#L80) should already cover this since `MemoryStage` derives `PartialEq` and these fields are non-default — confirm, no logic change expected.

- [x] **Frontend Memory controls** (`hifimule-ui/src/components/AutoFillPanel.ts`, `state/autoFill.ts`) (AC: 10)
  - [x] In `renderStage('memory', …)` ([AutoFillPanel.ts:253-260](../../hifimule-ui/src/components/AutoFillPanel.ts#L253-L260)) add: a stable-core % input/slider, a repeat-tolerance slider (0–100% → 0.0–1.0), and a tiers editor (list of playlist pickers, add/remove, ordered). Bind them in the input-capture handlers (mirror `#af-cooldown` at ~line 346 and `#af-played-exclusion` at ~line 361).
  - [x] Ensure `serializePipeline` ([state/autoFill.ts:107-113](../../hifimule-ui/src/state/autoFill.ts#L107-L113)) emits `stableCorePct`/`repeatTolerance`/`tiers` from real controls (they're already preserved verbatim — now they have UI sources). Keep them inside the Advanced disclosure.
  - [x] Wire the live preview (`previewAutoFill` / `basket.autoFill`) so the new controls re-trigger the debounced preview ([AutoFillPanel.ts:567-618](../../hifimule-ui/src/components/AutoFillPanel.ts#L567-L618)).

- [x] **i18n keys ×4 locales** (`hifimule-i18n/catalog.json`) (AC: 11)
  - [x] Add `basket.autofill.stable_core_pct` (+ `_hint`), `basket.autofill.repeat_tolerance` (+ `_hint`), `basket.autofill.tiers` (+ `_hint`, + add/remove labels) to `en`, `fr`, `es`, `de`. Keep parity test green.

- [x] **Full verification** (AC: 13)
  - [x] `rtk cargo test -p hifimule-daemon` (or targeted `auto_fill::`, `db::`, `device::` if sandbox blocks mockito — see Dev Notes), `rtk cargo clippy -p hifimule-daemon --all-targets`, frontend `rtk tsc` + build.

## Dev Notes

### What this story is (and is not)

Stories 12.1 and 12.2 left a **fully-wired-but-inert Memory stage**. The engine already excludes cooled-down and played tracks ([`memory_allows`, pipeline.rs:417-433](../../hifimule-daemon/src/auto_fill/pipeline.rs#L417-L433)); the DB table exists ([db.rs:220-231](../../hifimule-daemon/src/db.rs#L220-L231)); the frontend type already has the fields ([state/autoFill.ts:23-31](../../hifimule-ui/src/state/autoFill.ts#L23-L31)). The reason nothing happens is two gaps: (1) `fetch.rs` passes `HistorySnapshot::default()` ([fetch.rs:185](../../hifimule-daemon/src/auto_fill/fetch.rs#L185)) so `entries` is always empty, and (2) nothing ever writes a history row. **This story closes those gaps and implements the three reserved fields.** Do **not** rebuild what 12.1/12.2 already shipped — extend it.

### The pure-function discipline (non-negotiable)

The engine ([auto_fill/pipeline.rs](../../hifimule-daemon/src/auto_fill/pipeline.rs)) is a **pure, synchronous, deterministic** function. It reads `now` and history **only** from the caller-supplied `HistorySnapshot` ([pipeline.rs:259-271](../../hifimule-daemon/src/auto_fill/pipeline.rs#L259-L271)) — never `SystemTime::now()`, never RNG. `OrderingKey::Random` is deliberately a no-op ([pipeline.rs:511](../../hifimule-daemon/src/auto_fill/pipeline.rs#L511)). All five Memory strategies in this story are **deterministic**: cooldown and played-exclusion are time/flag comparisons; stable-core is a budget partition; repeat-tolerance scales the window; rotation tiers advance a *stored* cursor (the entropy is the cursor, supplied by the caller, not generated in the engine). Keep it that way — the four-persona suite ([pipeline.rs:787-976](../../hifimule-daemon/src/auto_fill/pipeline.rs#L787-L976)) depends on determinism.

### The async/sync split (where each piece goes)

- **Async, provider/DB-bound** → `fetch.rs` (`expand_with_pipeline`) and `rpc.rs`: materialize pools, **read** `autofill_history`, build the `HistorySnapshot`, read the rotation cursor.
- **Pure, sync** → `pipeline.rs` (`run_pipeline`, `memory_allows`, `Selector`): consume the snapshot; apply cooldown/played/stable-core/repeat-tolerance; emit tier indices on items.
- **Recommended seam:** build the full `HistorySnapshot` in the RPC layer (which holds `state.db`) and pass it into `expand_with_pipeline`, rather than handing `fetch.rs` a `&Database`. This keeps `fetch.rs` free of DB coupling and matches how it's already provider-only. Document whichever you choose.

### `AutoFillItem` tier reporting

`AutoFillItem` ([auto_fill/mod.rs:34-49](../../hifimule-daemon/src/auto_fill/mod.rs)) has `priority_reason`. For tiers you need the **tier index** to make it back to the sync-completion recorder. Either (a) extend `AutoFillItem` with `tier: Option<String>`, or (b) encode it in `priority_reason` (e.g. `"tier:0"`) and parse it at the recording hook. Option (a) is cleaner — prefer it, and update the basket/preview serialization accordingly (check `handle_basket_auto_fill` JSON shape so the UI preview doesn't break).

### Played-exclusion source of truth (read AC 5 carefully)

Played-exclusion is about *what the user has listened to*, which the **media server** knows — not the device sync history. So derive it from the candidate `Song`'s `play_count`/`last_played_at` (already fetched into pools), **not** from `autofill_history`. `autofill_history.last_synced_at` answers a different question ("did we copy this to the device recently?") and powers cooldown + stable-core. Do not conflate them. (The DB table has no `last_played_at` column — `TrackHistory.last_played_at` is populated from provider data, not the DB. Confirmed: db.rs has only `last_synced_at`, `tier`.)

### Rotation Tiers design note (highest-risk sub-feature — keep it conservative)

The source brainstorm catalog (idea #25/#26) is not in this checkout; the only spec is "rotation tiers / playlist-backed tiers" + the `tier` column. Implement the **minimal deterministic** version: ordered playlist-backed tiers + a stored rotation cursor that shifts the lead tier each sync. Avoid inventing play-count-band auto-tiering (#25's "tiers from library bands") unless trivial — playlist-backed (#26) is the concrete, low-risk path and the "cheap equivalent" the ambition-tier model favors. Budget-share schedule for the lead tier: pick a simple documented rule (e.g. lead = 50% of ceiling, remaining tiers split the other 50% equally) and unit-test it. If during implementation tiers prove larger than the rest of the story combined, **flag it** — see the scope note. Record tier index on each emitted track for observability and future pity-timer (13.4).

### Current code being changed (read before writing)

- **Engine:** [pipeline.rs:54-73](../../hifimule-daemon/src/auto_fill/pipeline.rs#L54-L73) (`AutoFillPipeline`), `:168-180` (`MemoryStage` — `cooldown_weeks`, `played_exclusion` consumed; `stable_core_pct`, `repeat_tolerance`, `tiers` reserved), `:226-282` (`Candidate`/`TrackHistory`/`HistorySnapshot`/`PipelineInput`), `:306-343` (`run_pipeline`), `:417-433` (`memory_allows`), `:520-559` (`compare_by_ordering`, `estimated_size`, `budget_ceiling`, `source_caps`), `:787-976` (4-persona tests + fixtures `song_sized`/`song_bitrate`/`cand`/`cand_meta`/`ids` at `:728-775`).
- **Fetch:** [fetch.rs:65-94](../../hifimule-daemon/src/auto_fill/fetch.rs#L65-L94) (`needs_configurable_expansion`), `:102-189` (`expand_with_pipeline` — **history seam at 182-187**), `:318-379` (`materialize_pool`/`fetch_playlist`/`fetch_library`), MockProvider + tests `:382-1325`.
- **DB:** [db.rs:104-117](../../hifimule-daemon/src/db.rs#L104-L117) (`Database = Arc<Mutex<Connection>>`), `:135-231` (`init()` migrations — add `autofill_rotation` after `:230`), `:719-795` (scrobble insert/query + upsert patterns to copy), `:630-643` (`get_server_config` → portable `server_id`).
- **Params/RPC:** [auto_fill/mod.rs:52-57](../../hifimule-daemon/src/auto_fill/mod.rs#L52-L57) (`AutoFillParams`), [rpc.rs:2636-2680](../../hifimule-daemon/src/rpc.rs#L2636-L2680) (sync-expand, reads pipeline from manifest by portable serverId, calls `expand_auto_fill_slot` at `:3535-3546`), `:5871-5990` (`handle_basket_auto_fill` preview).
- **Sync:** `hifimule-daemon/src/sync.rs` post-transfer completion (~2944+ — confirm the exact once-per-run hook; that's where recording + cursor advance go).
- **Frontend:** [AutoFillPanel.ts:253-260](../../hifimule-ui/src/components/AutoFillPanel.ts#L253-L260) (Memory render), `:143-156`/`:351-355` (Advanced disclosure), `:567-618` (debounced preview); [state/autoFill.ts:23-31](../../hifimule-ui/src/state/autoFill.ts#L23-L31) (`MemoryStage` TS type), `:107-113` (serialize); [BasketSidebar.ts:347-366,417-427](../../hifimule-ui/src/components/BasketSidebar.ts#L417-L427) (`autoFill.setPipeline` persist + hydrate from `get_daemon_state.autoFill.pipelines`); [rpc.ts:387-400](../../hifimule-ui/src/rpc.ts#L387-L400) (`previewAutoFill`).
- **i18n:** `hifimule-i18n/catalog.json` (4 locales: en/fr/es/de), keys under `basket.autofill.*`; loader [i18n.ts:11](../../hifimule-ui/src/i18n.ts#L11). Existing memory keys: `basket.autofill.memory`, `.cooldown_weeks`, `.played_exclusion`.

### Architecture compliance (non-negotiable)

- **Storage split:** pipeline **config** → manifest (portable `server_id`-keyed `Map<serverId, AutoFillPipeline>`); cooldown/stable-core/rotation **runtime state** → daemon DB keyed by `(device_id, server_id)`. **Never mix.** [architecture.md line 922]
- **Portable id everywhere:** `autofill_history.server_id` and `autofill_rotation.server_id` use the **portable** `server_id` (the same id on `SyncedItem.server_id` and the manifest `pipelines` key), resolved via `db.get_server_config()?.server_id`. Never write a machine-local `local_id`. [architecture.md lines 840-841, 909-911]
- **Route per server:** every expansion goes through `get_provider_by_server_id(slot.serverId)` — never the active provider. [architecture.md lines 889-893, 921]
- Reuse Story 12.1's types — do **not** redefine any pipeline/stage struct. `MemoryStage` fields already exist; just consume them.

### Previous story intelligence

- **12.2 review deferred two items to Epic 13 that this story now owns:** "`autofill_history` timestamp unit & NULL semantics undefined" → define here: **Unix seconds (i64), NULL `last_synced_at` = never synced, NULL `tier` = untiered**. And the multi-server accessor seam went live in 12.3 — history/rotation must likewise be keyed per server. [Source: 12-2 story Review Findings, deferred items]
- **12.1 left the engine under a module-level `#![allow(dead_code)]`** with `stable_core_pct`/`repeat_tolerance`/`tiers` reserved and `OrderingKey::Random` a no-op. Consuming these fields is expected; don't strip the blanket allow (other internals stay unused until later 13.x stories).
- **Sandbox caveat (recurring across Epic 12):** full `rtk cargo test -p hifimule-daemon` may not finish where mockito/local networking is blocked. New tests here are pure engine (`auto_fill::`), in-memory SQLite (`db::`), and serde — run targeted: `rtk cargo test -p hifimule-daemon auto_fill::`, `… db::`. [Source: 12.2/12.6 dev notes]
- **Persona suite is the acceptance bar for engine behavior** — Claire (commuter, hates repeats) already asserts cooldown ([pipeline.rs:788-853](../../hifimule-daemon/src/auto_fill/pipeline.rs#L788-L853)) but with hand-built history. Strengthen her test with the new repeat-tolerance dial and stable-core, and avoid `if persona == …` branches — every behavior must emerge from config composition ([pipeline.rs:780-785](../../hifimule-daemon/src/auto_fill/pipeline.rs#L780-L785)).

### Git intelligence

Recent commits (`8dc855a Fix issue`, `f1790db Review 12.7`, `db30397 Dev 12.7`, `b65854a Story 12.7`, `db9f8ea Review 12.6`) confirm Epic 12 is fully closed and this is the first Epic 13 story. No competing in-flight changes to `auto_fill/`, `db.rs`, or `AutoFillPanel.ts`. The frozen contract: legacy fast path + default-Memory pipelines behave identically — that invariant must survive this story.

### Latest technical context

- **No new crate dependency.** `rusqlite ~0.38` (bundled), `serde`/`serde_json ~1.0` cover everything. `tiers` parses via `serde_json::from_value` into a typed `Vec<TierDef>`. Rust edition 2024 (let-chains in use — see [12.2 migration snippet, device/mod.rs](../../hifimule-daemon/src/device/mod.rs)).
- **Time:** use `std::time::SystemTime::now().duration_since(UNIX_EPOCH)` **only** at the RPC/sync boundary to produce `now_unix`; pass it inward. The engine never reads the clock.

### Project Structure Notes

- Daemon (Rust): engine logic in `hifimule-daemon/src/auto_fill/{pipeline.rs,fetch.rs,mod.rs}`; DB in `db.rs`; recording hook in `sync.rs`; params plumbing in `rpc.rs`. Binary crate (no `lib.rs`); tests are `#[cfg(test)] mod tests`.
- Frontend (TS): `hifimule-ui/src/components/AutoFillPanel.ts`, `state/autoFill.ts`, `rpc.ts`; i18n catalog in `hifimule-i18n/catalog.json`. No UI test framework currently configured — match the existing no-unit-test pattern unless adding one is trivial; rely on `tsc` + manual preview.
- Do **not** put selection/runtime types in `domain/models.rs` (provider-neutral entities only). History/rotation access lives in `db.rs`; tier types live in `auto_fill/`.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Epic-13 (lines 3079-3086, Story 13.1)]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-06-14-configurable-auto-fill.md (Section 4.2, FR53; Epic 13 table line 110; storage split line 63; success criterion "no repeats within 3 weeks" line 142)]
- [Source: _bmad-output/planning-artifacts/architecture.md#Auto-Fill-Pipeline-Model (lines 788-826); #Enforcement (lines 920-922); portable id (lines 840-841, 889-911)]
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md §5.3 (lines 98-99 — Memory controls: cooldown weeks, repeat-tolerance dial, stable-core %; Advanced disclosure)]
- [Source: _bmad-output/implementation-artifacts/12-1-autofill-pipeline-domain-model-and-engine.md (engine, MemoryStage reserved fields, persona suite, determinism)]
- [Source: _bmad-output/implementation-artifacts/12-2-autofill-manifest-schema-and-db-history-scaffolding.md (autofill_history schema, deferred timestamp-semantics item, storage split)]
- [Source: hifimule-daemon/src/auto_fill/pipeline.rs:54-73,168-180,226-282,306-343,417-433,520-559,728-976]
- [Source: hifimule-daemon/src/auto_fill/fetch.rs:65-94,102-189,318-379]
- [Source: hifimule-daemon/src/auto_fill/mod.rs:52-57]
- [Source: hifimule-daemon/src/db.rs:104-117,135-231,630-643,719-795]
- [Source: hifimule-daemon/src/rpc.rs:2636-2680,3535-3546,5871-5990]
- [Source: hifimule-ui/src/components/AutoFillPanel.ts:143-156,253-260,346-361,567-618; state/autoFill.ts:23-31,107-113; components/BasketSidebar.ts:347-366,417-427; rpc.ts:387-400; i18n.ts:11; hifimule-i18n/catalog.json]

## Dev Agent Record

### Agent Model Used

claude-opus-4-8 (Opus 4.8, 1M context) — bmad-dev-story workflow.

### Debug Log References

- `rtk cargo test -p hifimule-daemon` → **534 passed** (full suite; includes the new `auto_fill::`, `db::` tests).
- `rtk cargo test -p hifimule-daemon auto_fill::` → 57 passed; `… db::` → 29 passed; `… sync::` → 66 passed.
- `rtk cargo clippy -p hifimule-daemon --all-targets` → 0 errors; **no new warnings in touched modules** (pre-existing warnings only).
- `rtk cargo test -p hifimule-i18n` → 6 passed (catalog embeds + parses).
- Frontend `rtk npx tsc --noEmit` → No errors found; `rtk npm run build` → built green.

### Completion Notes List

**Engine (pure, deterministic — pipeline.rs):**
- **Repeat-tolerance dial (#23, AC 7):** `memory_allows` now scales the cooldown window by `(1 − repeat_tolerance)`. Only modulates cooldown (inert when `cooldown_weeks` is `None`). Boundary-tested (t=0 strict, t=1 off, t=0.5 half-width).
- **Stable-core + delta (#24, AC 6):** `run_pipeline` runs a `FillMode::Core` pass first (up to `round(ceiling × p)` bytes, on-device candidates only, cooldown bypassed, played-exclusion still applied) then the normal delta pass on the same `Selector` (dedup automatic). No-op when `p = 0`, unbounded ceiling, or no history. `FillMode` enum (Core/Primary/Fallback) replaced the old `is_fallback` bool to keep `fill` within the arg-count lint.
- Determinism preserved: no clock/RNG in the engine; the four-persona suite still green.

**Async/DB seam (fetch.rs, db.rs, rpc.rs):**
- **DB layer (AC 1):** `Database` gained `upsert_autofill_history` (`INSERT … ON CONFLICT … DO UPDATE`), `get_autofill_history`, `prune_autofill_history`, and a new `autofill_rotation(device_id, server_id, cursor)` table with `get_rotation_cursor`/`advance_rotation_cursor`. All Unix-seconds; callers pass `now`. NULL `last_synced_at` = never synced, NULL `tier` = untiered (resolves 12.2's deferred timestamp-semantics item).
- **History snapshot (AC 3/4/5):** the RPC layer builds the DB-sourced snapshot (`build_autofill_history`) and threads it via new `AutoFillParams` fields (`device_id`, `server_id`, `now_unix`, `history`, `rotation_cursor`) — chosen over handing `fetch.rs` a `&Database`, keeping the fetch layer DB-free. `expand_with_pipeline` overrides `now` and merges per-candidate `last_played_at` from provider `Song` play signals (AC 5). Played-exclusion uses `now` as a present-sentinel (the value is never compared — only `.is_some()`), so no ISO-date parser/dep was added.
- **Cooldown/played activation (AC 4/5):** purely a matter of feeding real history — `memory_allows` was not reimplemented.

**Rotation tiers (#25/#26, AC 8) — kept conservative per Dev Notes:**
- `TierDef` (`{kind:playlist,ref}` | `{kind:library}`) parsed from `memory.tiers`; malformed → no rotation (logged, never aborts). Tiers define the run's sources; the cursor rotates the list (`cursor mod len`) so the lead tier shifts each sync; lead gets 50% of the ceiling, the rest split 50% equally (documented schedule). Each emitted track is tagged with its **original** tier index. `AutoFillItem` gained `tier: Option<String>`.
- **Recording (AC 2/8):** `tier` flows AutoFillItem → `delta.adds` (via `patch_delta_tiers`, set on `SyncAddItem` which survives the delta JSON round-trip to sync-execute) — DesiredItem was deliberately **not** widened. At sync completion `record_autofill_history_after_sync` upserts `(device, server, track, now, tier)` for each add carrying a portable `server_id` (best-effort; never fails the sync), prunes rows older than 52 weeks, and advances the rotation cursor once per completed sync for servers whose pipeline uses tiers. Wired into both provider sync-completion paths (multi-server grouped + single non-Jellyfin); the legacy Jellyfin path has no portable id so is naturally skipped.

**Routing (AC 9):** verified `needs_configurable_expansion` already returns `true` for `stableCorePct`/`repeatTolerance`/`tiers`-only pipelines via the existing `memory == default` check; locked in with a test (no logic change).

**Frontend (AC 10) + i18n (AC 11):** `AutoFillPanel` Memory section (under Advanced) now renders a stable-core % range, a repeat-tolerance range, and an add/remove ordered rotation-tiers editor (playlist/library), all reading/writing the `MemoryStage` fields and round-tripping through `serializePipeline`; edits invalidate the live preview. `MemoryStage.tiers` is now typed (`TierDef[]`). 7 new keys (`stable_core_pct`(+`_hint`), `repeat_tolerance`(+`_hint`), `tiers`(+`_hint`), `tiers_add`) added to all 4 locales — **65×4 parity** (was 58×4).

**Scope/backward-compat (AC 12):** default-Memory pipelines and the legacy fast path are byte-for-byte unchanged; config stays in the manifest, runtime history/rotation in the daemon DB. No later-Epic-13 features implemented (tier is *recorded* here; pity-timer *consumption* is 13.4).

### File List

- `hifimule-daemon/src/db.rs` — `autofill_rotation` table; `upsert_autofill_history`/`get_autofill_history`/`prune_autofill_history`/`get_rotation_cursor`/`advance_rotation_cursor` + tests.
- `hifimule-daemon/src/auto_fill/pipeline.rs` — repeat-tolerance scaling + `is_on_device` in `memory_allows`; stable-core pass + `FillMode` enum in `run_pipeline`/`Selector::fill`; `tier: None` in `make_item`; repeat-tolerance/stable-core tests.
- `hifimule-daemon/src/auto_fill/fetch.rs` — `TierDef`/`parse_tiers`/`derive_last_played`; history-snapshot merge + tier rotation/tagging in `expand_with_pipeline`; cooldown/played/tier + routing tests.
- `hifimule-daemon/src/auto_fill/mod.rs` — `AutoFillItem.tier`; `AutoFillParams` new fields (`device_id`/`server_id`/`now_unix`/`history`/`rotation_cursor`); `tier: None` in legacy item builders.
- `hifimule-daemon/src/rpc.rs` — `now_unix_secs`/`build_autofill_history`/`patch_delta_tiers`/`record_autofill_history_after_sync`; `AutoFillParams` construction updated at all sites; tier patch on both provider delta paths; recording wired into both provider sync-completion paths.
- `hifimule-daemon/src/sync.rs` — `SyncAddItem.tier` field (serde-default) + construction sites.
- `hifimule-daemon/src/main.rs` — `AutoFillParams` construction updated at the two daemon-auto-sync legacy sites.
- `hifimule-ui/src/state/autoFill.ts` — `TierDef` type; typed `MemoryStage` fields; `serializePipeline` emits the new Memory fields only when meaningful.
- `hifimule-ui/src/components/AutoFillPanel.ts` — stable-core %, repeat-tolerance, rotation-tiers editor + handlers.
- `hifimule-i18n/catalog.json` — 7 new `basket.autofill.*` keys × 4 locales (en/fr/es/de).

## Change Log

- 2026-06-14 — Dev complete (Status → review). Activated the Memory stage end-to-end: DB read/write for `autofill_history` + new `autofill_rotation` cursor table; repeat-tolerance (#23), stable-core (#24), and playlist-backed rotation tiers (#25/#26) implemented; cooldown (#4) and played-exclusion (#5) wired via a DB+provider history snapshot threaded through `AutoFillParams`; sync-completion recording hook (best-effort) + cursor advance + pruning; UI controls under Advanced + 7 i18n keys ×4 locales (65×4 parity). Verification: 534 daemon tests pass, clippy clean (no new warnings in touched modules), frontend tsc + build green. Backward compatible (default Memory == today; legacy fast path untouched; storage split enforced).
- 2026-06-14 — Story 13.1 created via create-story workflow (ready-for-dev). Scope: activate the Memory stage end-to-end (DB read/write wiring for `autofill_history` + a new `autofill_rotation` cursor table), implement the three reserved `MemoryStage` fields (`stable_core_pct` #24, `repeat_tolerance` #23, `tiers` #25/#26), activate cooldown (#4) and played-exclusion (#5) by populating the `HistorySnapshot` from DB + provider play data, surface the new controls in the pipeline-builder UI under Advanced, and add i18n keys across 4 locales. Backward compatible (default Memory == today's behavior); storage split enforced (config in manifest, runtime state in DB); legacy fast path untouched.
