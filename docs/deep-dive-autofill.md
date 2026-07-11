# Auto-Fill — Deep Dive Documentation

**Generated:** 2026-06-15
**Scope:** `hifimule-daemon/src/auto_fill/` + `hifimule-ui/src/{state/autoFill.ts, components/AutoFillPanel.ts}` (+ manifest / RPC / DB wiring)
**Workflow Mode:** Exhaustive Deep-Dive (configuration model, engine, discovery stages, quality/promotion modifiers, and builder UI)

---

## Overview

Auto-fill automatically fills the free space on a portable device with tracks pulled from the
media server, so a sync tops the device up without manual track-picking. The configuration is a
**per-`(device, serverId)` pipeline** describing *which* tracks to draw and *how* to rank, group,
and bound them.

The system has a strict architectural split that everything else hangs off of:

- **`pipeline.rs` is PURE** — no async, no network, no clock, no RNG. All non-determinism (current
  time, RNG seed, sync history, rotation cursor, pity streak, local civil time) enters as *values*
  via `PipelineInput` / `AutoFillParams` / `HistorySnapshot`. This is what lets the entire
  selection algebra be unit-tested without UI or network — the bulk of `pipeline.rs`'s ~5,160 lines
  are those tests.
- **`fetch.rs` is the IMPURE half** — it does all `MediaProvider` I/O, materializes source pools,
  and decides routing.
- **Config is portable manifest data; the discovery counters (rotation cursor, pity streak, sync
  history) are machine-local daemon-DB state**, never stored in the manifest.

---

## File Inventory (core scope)

| File | LOC | Role |
|---|---|---|
| [auto_fill/pipeline.rs](../hifimule-daemon/src/auto_fill/pipeline.rs) | ~5,160 | **Pure** config domain model + `run_pipeline` engine (most LOC are unit tests) |
| [auto_fill/fetch.rs](../hifimule-daemon/src/auto_fill/fetch.rs) | ~1,880 | **Impure** pool materialization; `needs_configurable_expansion` routing; tier parsing/rotation |
| [auto_fill/mod.rs](../hifimule-daemon/src/auto_fill/mod.rs) | ~673 | Legacy fast path (`run_auto_fill`, `run_auto_fill_provider`), `AutoFillParams`, `AutoFillItem` |
| [device/mod.rs](../hifimule-daemon/src/device/mod.rs) | — | Manifest persistence: `AutoFillConfig`, `AutoFillPrefs`, resolver helpers |
| [rpc.rs](../hifimule-daemon/src/rpc.rs) | — | RPC methods, sync-time budget resolution, slot expansion, clock/DB-counter threading |
| [db.rs](../hifimule-daemon/src/db.rs) | — | `autofill_history`, `autofill_rotation`, `autofill_pity` tables + accessors |
| [hifimule-ui/src/state/autoFill.ts](../hifimule-ui/src/state/autoFill.ts) | ~352 | TypeScript mirror of the serde contract; normalize/serialize |
| [hifimule-ui/src/components/AutoFillPanel.ts](../hifimule-ui/src/components/AutoFillPanel.ts) | ~1,267 | Pipeline-builder modal UI |

---

## 1. Where config lives & how it's keyed

Config is stored in the device manifest (`.hifimule.json`, on the device), in
[device/mod.rs:191](../hifimule-daemon/src/device/mod.rs#L191):

- `AutoFillConfig.pipelines: HashMap<String, AutoFillPipeline>` — **one pipeline per portable
  server id**. A device synced against multiple servers keeps an independent config for each.
- A legacy `AutoFillPrefs { enabled, max_bytes }` block
  ([device/mod.rs:174](../hifimule-daemon/src/device/mod.rs#L174)) is still read for backward
  compatibility and mapped up into a full pipeline via `pipeline_from_legacy` / `default_legacy`.
- A fresh/default `{ enabled: false, maxBytes: null }` is treated as "no config", never a legacy block.

Resolution helpers pick the right pipeline for the selected server:
`pipeline_for(server_id)`, `enabled_for`, `max_bytes_for`
([device/mod.rs:285-307](../hifimule-daemon/src/device/mod.rs#L285-L307)), with a single-entry
fallback when only one server is configured.

**RPC surface** ([rpc.rs:234](../hifimule-daemon/src/rpc.rs#L234)):
- `autoFill.setPipeline` — persists a full pipeline for a server.
- `sync.setAutoFill` — toggles the legacy enabled / maxBytes pair.
- `basket.autoFill` — runs a **preview** (does not persist).
- Device-state queries return the entire `pipelines` map so the UI builder can hydrate every server.

---

## 2. The pipeline model (`AutoFillPipeline`)

Defined canonically in [pipeline.rs:54](../hifimule-daemon/src/auto_fill/pipeline.rs#L54) and
mirrored **byte-for-byte** in TypeScript at
[autoFill.ts:136](../hifimule-ui/src/state/autoFill.ts#L136). Every field carries
`#[serde(default)]`, so a partial/empty pipeline deserializes cleanly.

| Stage | Field | Controls |
|---|---|---|
| **Enabled** | `enabled` | Whether auto-fill runs (a *caller* concern — `run_pipeline` itself runs regardless) |
| **Filter** | `filter` | Per-candidate include/exclude by tags & genres; empty = pass-through |
| **Sources** | `sources` | Ordered `library` / `favorites` / `history` / `playlist` draws, optionally blended by `share` (0.0–1.0) |
| **Unit** | `unit` | Selection granularity: `track` / `album` / `artist` |
| **Ordering** | `ordering` | Stable multi-key sort: `favorite`, `playCount`, `dateCreated`, `random`, `quality`, `excavation`, `rediscovery`, `rarity` |
| **Memory** | `memory` | Cooldown weeks, played-exclusion, stable-core %, repeat tolerance, rotation tiers (Story 13.1) |
| **Budget** | `budget` | `maxBytes`, `targetDurationSecs`, `headroomBytes`, `encodingFromGoals` |
| **Fallback** | `fallback` | Terminal sources applied to reach the budget once primaries are exhausted |
| **Quality** | `quality` | Best-version collapse + ordered version-trait preference (Story 13.2) |
| **Rarity** | `rarity` | Weighted loot-table draw (legendary/rare/common) (Story 13.4) |
| **Pity** | `pity` | Deterministic discovery guarantee after a dry streak (Story 13.4) |
| **Context** | `context` | Clock-driven rules: time-of-day / months / date-range windows that activate sources & filters (Story 13.5) |
| **Promotion** | `promotion` | Artist spotlight, album/track ratio, affinity album promotion, coherence reorder (Story 13.6) |

**Source kinds → provider methods** ([pipeline.rs:143-150](../hifimule-daemon/src/auto_fill/pipeline.rs#L143)):
`Library`→`list_all_songs_page`, `Favorites`→`list_favorites`, `History`→`list_recently_played`,
`Playlist`→`get_playlist`.

**The default-legacy pipeline** (`default_legacy`,
[pipeline.rs:622](../hifimule-daemon/src/auto_fill/pipeline.rs#L622); mirror at
[autoFill.ts:170](../hifimule-ui/src/state/autoFill.ts#L170)): one bare `library` source,
`unit: track`, ordering `[favorite, playCount, dateCreated]`, a bare `maxBytes` budget, everything
else off. This reproduces the original pre-Epic-12 behavior exactly (favorites → most-played → newest).

---

## 3. Key design decision — fast path vs. configurable engine

`needs_configurable_expansion` ([fetch.rs:146](../hifimule-daemon/src/auto_fill/fetch.rs#L146))
decides per pipeline:

- **Default-legacy-equivalent** → **fast path**: `run_auto_fill_provider` / `run_auto_fill`
  ([mod.rs:395](../hifimule-daemon/src/auto_fill/mod.rs#L395)). It streams pre-sorted tracks
  (favorites → frequently played → recently played → bulk library pagination) and truncates to
  budget — no pool materialization. Equivalent means: sources empty or one bare `Library`; empty
  filter; ordering empty or exactly `[Favorite, PlayCount, DateCreated]`; `unit == Track`; default
  memory/quality/rarity/pity/context/promotion; no fallback; budget is a bare `maxBytes` (no
  headroom reserve or duration target).
- **Any deviation** → **configurable path**: `expand_with_pipeline`
  ([fetch.rs:211](../hifimule-daemon/src/auto_fill/fetch.rs#L211)) materializes source pools from
  the provider, then runs the pure `run_pipeline` engine.

**Omit-when-default serialization** — `serializePipeline`
([autoFill.ts:224](../hifimule-ui/src/state/autoFill.ts#L224)) strips every default/empty optional
(`ref`/`share`, zero budgets, disabled stages emit `{}`). A "Default" config therefore round-trips
**byte-identically** and stays on the fast path; a stale inert field would needlessly force the
slower engine path. (E.g. `spotlightShare` is emitted only when `spotlight` is on, because the
daemon gates the reserve on `spotlight`.)

---

## 4. The pure engine — `run_pipeline`

`run_pipeline(input, pipeline) -> Vec<AutoFillItem>`
([pipeline.rs:785](../hifimule-daemon/src/auto_fill/pipeline.rs#L785)). Synchronous, pure.

### 4.1 Inputs (`PipelineInput`, [pipeline.rs:744-761](../hifimule-daemon/src/auto_fill/pipeline.rs#L744))
- `pools: HashMap<SourceKey, Vec<Candidate>>` — materialized candidate pools (filled by the fetch layer; `with_pool` adds one).
- `history: HistorySnapshot` — carries `now: i64` (Unix seconds), `entries: HashMap<id, TrackHistory>`, and `local: CivilTime`.
- `exclude_item_ids` — manually-selected ids auto-fill must never re-emit.
- `seed: u64` — caller entropy; every `Random`/`Rarity` decision derives from it. Same `(input, seed, pipeline)` ⇒ byte-identical output.
- `pity_streak: i64` — dry-streak counter (machine-local DB state).

> Note: `now`/`local` live on `HistorySnapshot`, not directly on `PipelineInput`; `rotation_cursor`
> never reaches the pure engine (it is resolved into source shares by the fetch layer).

### 4.2 Stage execution order ([pipeline.rs:786-1034](../hifimule-daemon/src/auto_fill/pipeline.rs#L786))
1. Compute the **ceiling** via `budget_ceiling` = `max_bytes − headroom_bytes` (saturating), or `u64::MAX` if unbounded ([pipeline.rs:1789](../hifimule-daemon/src/auto_fill/pipeline.rs#L1789)).
2. **Best-version collapse** pre-pass (only when `quality.best_version`) — see §6.
3. Resolve sources (empty ⇒ single `Library`).
4. **Context stage** (only when `context.enabled && history.local.is_set()`) — derives an effective filter + effective source/fallback sets — see §5.4.
5. Build one shared `Selector` (ceiling, duration target, derived `target_kbps`, exclude set, coherence flag) threaded through every pass so dedup is automatic.
6. **Reserve pre-passes**, in order: **stable-core** (§5.1) → **artist-spotlight** (§6.2a) → **album/track-ratio** (§6.2b) → **pity discovery** (§5.3). Each idiom: temporarily lower `selector.ceiling = cum_bytes + reserve (min ceiling)`, split via `source_caps`, fill, restore. All no-ops on an unbounded ceiling.
7. **Primary sources** (delta against budget *remaining* after reserves), then **fallback chain** (only if primary can't fill, reasons prefixed `fallback:`).
8. `selector.into_items()` — emits, applying the coherence reorder if enabled (§6.2d).

### 4.3 Pool lookup & `share` blending
Pools are looked up by `SourceKey = (kind, ref_id)` ([pipeline.rs:675](../hifimule-daemon/src/auto_fill/pipeline.rs#L675)); a missing pool yields an empty vec. `source_caps`
([pipeline.rs:1825](../hifimule-daemon/src/auto_fill/pipeline.rs#L1825)): with no shares, the
ceiling is split equally; with shares present, each shared source gets `frac_bytes(share, ceiling)`
and unshared sources split the leftover `(1 − Σshare)` equally.

### 4.4 Multi-key ordering (stable sort)
`build_source_units` sorts within each unit, then sorts units by their best track — both via the
**stable** `Vec::sort_by` ([pipeline.rs:1135-1145](../hifimule-daemon/src/auto_fill/pipeline.rs#L1135)),
so equal-ranked candidates keep pool order. The comparator `compare_by_ordering`
([pipeline.rs:1315](../hifimule-daemon/src/auto_fill/pipeline.rs#L1315)) walks `ordering` and returns
on the first non-`Equal` arm; **version-preference is a final tiebreak** after all keys. Per key:

| Key | Behavior |
|---|---|
| `Favorite` | favorites first |
| `PlayCount` | higher play count first |
| `DateCreated` | newer first (lexicographic ISO-8601) |
| `Quality` | `(format_quality_rank, bitrate)` desc — lossless(2) > lossy(1) > unknown(0), then bitrate — see §6.1c |
| `Excavation` | **fewer plays first** (inverse PlayCount — deep cuts) |
| `Rediscovery` | oldest `date_added` first, with unknown-date sorted **last** (not masquerading as oldest) |
| `Rarity` | seeded weighted draw (Efraimidis–Spirakis key `u^(1/w)`); weight from rarity class — see §5.2 |
| `Random` | the `w=1.0` case of the same draw — a deterministic seeded permutation |

The seeded RNG core is `draw_unit01` ([pipeline.rs:1407](../hifimule-daemon/src/auto_fill/pipeline.rs#L1407)):
a stable FNV-1a hash of the track id XOR-folded with `seed` through a splitmix64 finalizer →
uniform `[0,1)`. **No global entropy** — all randomness is seed-derived.

### 4.5 Unit grouping & atomic admission
`unit_stage` ([pipeline.rs:1229](../hifimule-daemon/src/auto_fill/pipeline.rs#L1229)): `Track` ⇒
singletons; `Album`/`Artist` ⇒ `group_by(album_id|artist_id)` preserving first-seen order. In
`Selector::fill` ([pipeline.rs:1975](../hifimule-daemon/src/auto_fill/pipeline.rs#L1975)) each unit
is staged whole and admitted **all-or-nothing** — if it would exceed the ceiling, source cap, or
duration target, the source **stops** (no back-filling smaller later units). So album/artist units
sync whole-or-not; a `Track` is a one-element atomic unit.

### 4.6 Byte-size estimation
`estimated_size` ([pipeline.rs:1761](../hifimule-daemon/src/auto_fill/pipeline.rs#L1761)): prefers
`size_bytes` (>0), else `bitrate_kbps*1000/8 * duration_seconds`; `None` for unknown/zero so the
selector skips it (never a 0-byte filler). With `target_kbps = Some(b>0)` it is bitrate-aware:
`min(source_estimate, b*1000/8*duration)` — transcoding only shrinks. `target_bitrate_kbps`
([pipeline.rs:1803](../hifimule-daemon/src/auto_fill/pipeline.rs#L1803)) derives the transcode
bitrate backwards from goals only when `encoding_from_goals` AND both `max_bytes` and positive
`target_duration_secs` are set, clamped to `[32, 320]` kbps.

---

## 5. Discovery & memory stages (Story 13.1 / 13.4 / 13.5)

### 5.1 Memory (`MemoryStage`, [pipeline.rs:202](../hifimule-daemon/src/auto_fill/pipeline.rs#L202))
- **playedExclusion / cooldownWeeks** — `memory_allows` ([pipeline.rs:1197](../hifimule-daemon/src/auto_fill/pipeline.rs#L1197)). No history row ⇒ always allowed. `played_exclusion` drops any track with a recorded `last_played_at`. Cooldown window = `cooldown_weeks × 7 × 86400`, **scaled by repeatTolerance**: `effective = base × (1 − repeat_tolerance.clamp(0,1))`; a track synced more recently than `now − window` is rejected. "now" comes from the snapshot, never a clock.
- **repeatTolerance** — only modulates the cooldown window (no effect when `cooldown_weeks` is unset).
- **stableCorePct** — a first pass ([pipeline.rs:856-882](../hifimule-daemon/src/auto_fill/pipeline.rs#L856)) reserves `round(ceiling × pct)` from **on-device** candidates only (`FillMode::Core`), cooldown-exempt but still played-excluded — the part of the selection that stays stable across syncs.
- **rotation tiers (`TierDef`)** — `MemoryStage.tiers` is opaque manifest JSON parsed in `fetch.rs`. `parse_tiers` ([fetch.rs:83](../hifimule-daemon/src/auto_fill/fetch.rs#L83)): `{kind:'playlist', ref}` or `{kind:'library'}`; malformed ⇒ empty (rotation disabled, never aborts), de-duped by `SourceKey`. `expand_with_pipeline` ([fetch.rs:271-299](../hifimule-daemon/src/auto_fill/fetch.rs#L271)): `lead = rotation_cursor.rem_euclid(n)`; the lead tier gets `share = 0.5`, the other `n−1` split `0.5` equally; tiers are mirrored uncapped into `fallback` for spillover; each emitted track is tagged with its **original** tier index so the recorded tier is stable regardless of lead.

### 5.2 Rarity (`RarityStage`, [pipeline.rs:291](../hifimule-daemon/src/auto_fill/pipeline.rs#L291))
Loot-table classes derived purely from `play_count` by `rarity_class_weight`
([pipeline.rs:1437](../hifimule-daemon/src/auto_fill/pipeline.rs#L1437)): `0` → legendary,
`1..=rare_max_plays` → rare, `> rare_max_plays` → common. The `Rarity` ordering key draws via the
Efraimidis–Spirakis weighted key, tie-broken on `song.id` for canonical output. When
`rarity.enabled == false`, weights collapse to `1.0` ⇒ uniform shuffle identical to `Random`. The
seed is minted in `rpc.rs` as `now as u64` — varies per run, deterministic within it.

### 5.3 Pity (`PityStage`, [pipeline.rs:311](../hifimule-daemon/src/auto_fill/pipeline.rs#L311))
`pity_reserve_bytes` ([pipeline.rs:608](../hifimule-daemon/src/auto_fill/pipeline.rs#L608)) is the
**single source of truth** for the fire condition: returns `0` unless `enabled && pity_streak >=
threshold_syncs && ceiling != u64::MAX`; otherwise `round(ceiling × guaranteed_ratio)`. The reserve
pass ([pipeline.rs:963-995](../hifimule-daemon/src/auto_fill/pipeline.rs#L963)) fills it with
`FillMode::Discovery { max_plays }` — candidates **not** on device AND `play_count <=
discovery_max_plays` (genuinely new gems). The streak is reset to 0 only when the reserve genuinely
fired (`delta.pity_fired_servers`, computed with the *same* gate), else incremented — so a streak
that crosses the threshold while the budget is unbounded stays armed instead of being silently consumed.

### 5.4 Context (`ContextStage`, [pipeline.rs:368](../hifimule-daemon/src/auto_fill/pipeline.rs#L368))
`ContextRule` = window + `source_refs` + optional `weight` + scheduled include/exclude tags & genres.
`ContextWindow` = `TimeOfDay{start,end}` (with midnight wrap when `start>end`), `Months`, or
`DateRange` (`(month,day)` encoded `m*100+d`, with year-end wrap). `context_rule_active`
([pipeline.rs:450](../hifimule-daemon/src/auto_fill/pipeline.rs#L450)) is pure against the supplied
`CivilTime`. Only consulted when `context.enabled && history.local.is_set()` (`is_set` = `month != 0`,
so the all-zero default stays inert). Active rules: (a) activate/boost sources via `effective_sources`
([pipeline.rs:515](../hifimule-daemon/src/auto_fill/pipeline.rs#L515)) — multiple active rules compose
by **max** weight, retained sources' shares are recomputed `base × weight` and normalized; (b) union
their tag/genre filters into the effective filter.

### 5.5 The machine-local DB counters ([db.rs](../hifimule-daemon/src/db.rs))
All keyed per `(device_id, server_id)`:

| Table | Columns | Accessors |
|---|---|---|
| `autofill_history` | `track_id, last_synced_at, tier` | `upsert_autofill_history`, `get_autofill_history`, `prune_autofill_history` (~1 yr retention) |
| `autofill_rotation` | `cursor` (default 0) | `get_rotation_cursor`, `advance_rotation_cursor` (+1) |
| `autofill_pity` | `dry_streak` (default 0) | `get_pity_streak`, `set_pity_streak` |

`build_autofill_history` ([rpc.rs:3680](../hifimule-daemon/src/rpc.rs#L3680)) reads all three and
returns `(HistorySnapshot, rotation_cursor, pity_streak)` → packed into `AutoFillParams` →
`PipelineInput`. The cursor advances and the streak resets/increments **after a completed sync**,
gated on the server actually having written a track ([rpc.rs:3901-3940](../hifimule-daemon/src/rpc.rs#L3901)).
`now_civil()` ([rpc.rs:3622](../hifimule-daemon/src/rpc.rs#L3622)) is the single clock site
(`chrono::Local::now()` → hour/month/day/weekday).

---

## 6. Quality & promotion modifiers (Story 13.2 / 13.6)

Both are config-only structs that augment, never replace, the base `unit`/`ordering`/`budget`
machinery; all-default = today's behavior.

### 6.1 Quality (`QualityStage`, [pipeline.rs:250](../hifimule-daemon/src/auto_fill/pipeline.rs#L250))
- **bestVersion (#11)** — `collapse_best_version` ([pipeline.rs:1717](../hifimule-daemon/src/auto_fill/pipeline.rs#L1717)) runs as a pre-pass over all pools so a losing version never occupies budget. "Same logical song" = `logical_key = (normalized_artist, normalized_base_title)` where the base title is `strip_version_markers(title)` (conservatively strips bracketed/`- dash` version markers). The winner is the minimum under `best_version_cmp` ([pipeline.rs:1677](../hifimule-daemon/src/auto_fill/pipeline.rs#L1677)), tiered: budget-fit → version-preference rank → quality rank `(format, bitrate)` → configured ordering → `song.id`.
- **versionPreference (#34)** — closed `VersionTrait` set: studio/live/remastered/remix/acoustic/demo. `detect_version_traits` ([pipeline.rs:1539](../hifimule-daemon/src/auto_fill/pipeline.rs#L1539)) reads title+album text (word-anchored for live/remaster/demo, substring for remix/acoustic); no markers ⇒ Studio. `version_rank` = index of the first matched preferred trait (none ⇒ last). Applied only as the final ordering tiebreak (and tier 1 inside best-version).
- **OrderingKey::Quality (#13)** — `format_quality_rank` ([pipeline.rs:1460](../hifimule-daemon/src/auto_fill/pipeline.rs#L1460)) reads **only** `suffix` then `content_type` mime subtype (never the title): lossless(2)/lossy(1)/unknown(0). Lossless always outranks lossy regardless of bitrate, then bitrate desc within a tier.

### 6.2 Promotion (`PromotionStage`, [pipeline.rs:336](../hifimule-daemon/src/auto_fill/pipeline.rs#L336))
- **(a) Artist Spotlight (#33)** — pre-pass ([pipeline.rs:884-928](../hifimule-daemon/src/auto_fill/pipeline.rs#L884)), gated on `spotlight` + bounded ceiling. `choose_featured_artist` ([pipeline.rs:1062](../hifimule-daemon/src/auto_fill/pipeline.rs#L1062)) picks the artist owning the best-ranked candidate under the configured ordering — so with `Random`/`Rarity` in the ordering the featured artist **varies per seed each sync** with zero new entropy. Reserve = `round(ceiling × spotlightShare.unwrap_or(0.5))`, filled `Unit::Track` retained to that artist. "In depth" is bounded by what the configured sources materialized (no full-discography fetch).
- **(b) albumTrackRatio (#8)** — pre-pass ([pipeline.rs:930-961](../hifimule-daemon/src/auto_fill/pipeline.rs#L930)): reserve = `round(ceiling × ratio)`, filled as `Unit::Album` (atomic) — **complete albums first**, remainder fills as the base unit.
- **(c) promoteAlbumMinFavorites (#9)** — applies **only when base `unit == Track`** ([pipeline.rs:847-854](../hifimule-daemon/src/auto_fill/pipeline.rs#L847)). `unit_stage_promoted` ([pipeline.rs:1245](../hifimule-daemon/src/auto_fill/pipeline.rs#L1245)) counts favorited candidates per album and promotes an album to an atomic unit when its count `>= N`; affinity is per-pool/per-run (counted over candidates present, `is_favorite` the only signal). Structurally inert for Album/Artist base units.
- **(d) coherence (#27)** — `coherence_reorder` ([pipeline.rs:1881](../hifimule-daemon/src/auto_fill/pipeline.rs#L1881)) is a **reorder-only** final pass: stable-sort by artist→album→disc→track→id using first-appearance ranks. The selected id-set and byte total are byte-identical to an un-clustered run.

### 6.3 Budget/unit interaction
The ceiling is computed once. **Both promotion reserves (and best-version's budget-fit tier) require
a bounded ceiling** — they are no-ops on an unbounded budget. Reserves stack via the shared
`Selector` (each adds on top of prior spend, `selector.seen` dedups every later pass); under-filled
reserves spill forward into the primary pass for free. Atomic admission and all budget enforcement
are centralized in `Selector::fill`; coherence touches none of it (pure permutation of `items`).

---

## 7. The builder UI — `AutoFillPanel.ts`

A **modal dialog** scoped to one server ([AutoFillPanel.ts:1-8](../hifimule-ui/src/components/AutoFillPanel.ts#L1)).
It is a plain TS class (NOT a custom element, [AutoFillPanel.ts:113](../hifimule-ui/src/components/AutoFillPanel.ts#L113))
that builds a Shoelace `<sl-dialog>` imperatively and renders by assigning `innerHTML`.

- **Instantiation** — `BasketSidebar.openAutoFillPanel()` ([BasketSidebar.ts:430-471](../hifimule-ui/src/components/BasketSidebar.ts#L430)) resolves the existing pipeline (or a disabled `defaultLegacyPipeline()`), computes `availableBytes`, and constructs the panel with an `onSave` callback delegating to `persistPipeline`.
- **Hydration** — constructor calls `normalizePipeline(opts.pipeline)` ([AutoFillPanel.ts:144](../hifimule-ui/src/components/AutoFillPanel.ts#L144)); capability flags `genresSupported`/`playlistsSupported` come from `modes`. Numeric/byte fields are buffered as strings with `initial*` snapshots.
- **Per-stage editors** — the simple path is enable + size budget + exclude-genres; everything else is behind an "Advanced" disclosure. Each stage has a dedicated `render*Stage` (filter, sources+share, unit, reorderable ordering, memory incl. rotation-tiers editor, budget, fallback, quality incl. version-preference editor, rarity, pity, context rule editor with window kinds, promotion).
- **Serialization & persistence** — `buildPipeline()` ([AutoFillPanel.ts:1121](../hifimule-ui/src/components/AutoFillPanel.ts#L1121)) is shared by Save and Preview; it folds string buffers into the model and calls `serializePipeline` **once**. Save → `onSave` → `BasketSidebar.persistPipeline` → `rpcCall('autoFill.setPipeline', { serverId, pipeline })`. Preview uses a *different* method (`basket.autoFill`) and never persists; only the preview is debounced (≥300 ms).

### UI gotchas a contributor must know
- **innerHTML re-render model, not reactive.** Every structural edit calls `renderBody()`, which blows away the DOM and re-binds events.
- **`captureInputs()` before every re-render.** Buffered text/number fields are not written on each keystroke; any handler that triggers a re-render MUST call `captureInputs()` first or in-progress edits are lost. (Stage editors that re-render on sibling changes instead write straight into the model on every input — two strategies coexist intentionally.)
- **Byte/duration round-trip preservation.** `bytesFromGbInput`/`secondsFromHoursInput` return the *original* byte value when the input string is unchanged from its `initial*` snapshot, avoiding `bytes→GB→bytes` float drift.
- **Hidden/preserved fields.** The UI exposes only `filter.excludeGenres` (not include/tags). Untouched fields are carried through verbatim by normalize/serialize, so editing in the UI won't clobber externally-authored config.
- **Live-preview generation guard.** A `previewGeneration` counter discards stale resolving previews; `renderBody()` bumps it and clears preview state.

---

## What future contributors must know (cross-cutting)

- **Keep the two config representations in sync.** `pipeline.rs` (serde) and `autoFill.ts` must match byte-for-byte: lowercase camelCase enums, playlist field is `ref` (not `refId`), `share` omitted when unset. Drift breaks manifest round-tripping.
- **Never read the clock/RNG inside `pipeline.rs`.** All non-determinism enters via `PipelineInput`/`AutoFillParams` (`now_unix`, `seed`, `pity_streak`, `local` civil time, `history`). This is load-bearing for the unit-test strategy and for deterministic, seed-reproducible output.
- **Preserve omit-when-default on the UI side.** New stages must emit only meaningful fields, or they silently force every pipeline onto the slower engine path.
- **Discovery counters are machine-local DB state**, never stored in the manifest — threaded in via `AutoFillParams` by the RPC layer, and advanced/reset only after a successful sync that wrote a track.
- **`enabled` is not an engine discriminator.** `run_pipeline` runs regardless; enabling is a fetch-layer/caller gate.
- **`pity_reserve_bytes` is the single fire-gate** shared by the engine and the sync-completion path so fire and streak-reset can't drift. Reuse it; don't re-derive the condition.
- **Stale comment:** `fetch.rs`'s module header still claims the history table is "neither read nor written here (Epic 13 wires it)" — Epic 13 has since wired all three counter tables through the RPC layer.

### Verification before changing this area
```bash
rtk cargo test -p hifimule-daemon auto_fill   # pure engine + routing + stage unit tests
rtk cargo test -p hifimule-daemon             # full suite (device manifest round-trips, DB counters)
```
For UI/contract changes, also run the UI typecheck/build so the serde mirror stays valid.

---

## Related code

- **Manifest data model:** [data-models-hifimule-daemon.md](./data-models-hifimule-daemon.md) (DeviceManifest)
- **Provider abstraction & list_* methods:** [integration-architecture.md](./integration-architecture.md)
- **RPC method surface:** [api-contracts-hifimule-daemon.md](./api-contracts-hifimule-daemon.md)
- **UI builder component & mounting:** [component-inventory-hifimule-ui.md](./component-inventory-hifimule-ui.md) (`AutoFillPanel.ts`, `BasketSidebar.ts`)
