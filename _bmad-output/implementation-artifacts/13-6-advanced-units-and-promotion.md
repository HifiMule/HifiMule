---
baseline_commit: 3a46765d5c2ee307154ab00730ddbaeed9bc3eae
---

# Story 13.6: Advanced Units & Promotion

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a HifiMule user who thinks about my fills in **albums and artists**, not just loose tracks — I want one fill to go deep on a single featured artist, or split between complete albums and singles, or pull a whole album onto the device when I've favorited enough of it, and I want the result to **flow like a curated set** rather than a shuffle of fragments,
I want the auto-fill pipeline's **Unit axis** to support advanced granularity and promotion strategies — **Artist Spotlight** (feature one artist in depth), an **album/track space ratio** (budget split between complete albums and loose tracks), **affinity-triggered album promotion** (a track-level fill promotes an album to a full atomic unit once enough of its tracks are favorited), and **coherence ordering** (cluster the selected set by artist→album→track so it plays as a coherent whole),
so that fills respect how I actually listen — while the selection core stays **deterministic-given-its-inputs**, budget-respecting, per-server, and pure (no clock, no RNG read, no network in the engine).

This is the **final story of Epic 13**. It delivers the brainstorm's **Granularity / Unit** ideas — **#33 Artist Spotlight**, **#8 Album/Track Space Ratio**, **#9 Affinity-Triggered Album Promotion** — plus **#27 Coherence-Optimized Fill**, the last item the brainstorm placed in the pipeline grid's **Unit** stage (`Filter → Sources → Unit → Ordering → Memory → Budget → Context`, [brainstorm:138](../brainstorming/brainstorming-session-2026-06-12-1.md)). The base `unit: "track" | "album" | "artist"` field already exists and works ([pipeline.rs:152-160](../../hifimule-daemon/src/auto_fill/pipeline.rs#L152), grouped in [`unit_stage`](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1007)); this story adds the **advanced modifiers** the single `unit` field can't express (mixed/conditional granularity, single-artist depth, flow ordering).

### What this story is (and is not) — read first

Every feature here is an **additive, default-noop modifier** delivered by **reusing the exact disciplines 13.1–13.5 established** — it invents no new infrastructure:

1. **Reserve-slice pre-passes (Spotlight #33, Album/Track ratio #8).** Both reserve a fraction of the byte ceiling and fill it FIRST with a specific scope/granularity, then let the remaining budget fill normally. This is **literally the stable-core (#24) / pity-reserve (#30) mechanism** already in [`run_pipeline`](../../hifimule-daemon/src/auto_fill/pipeline.rs#L802-L848): a temporary `selector.ceiling`, a restricted candidate set, the **shared `Selector`** giving automatic dedup into the primary pass, and automatic spillover (an under-filled reserve just leaves budget for the later passes). **Do not build a new selection loop** — add a pre-pass exactly like the two that exist.

2. **Unit-grouping refinement (Affinity promotion #9).** Promotion changes only **how candidates are grouped into units** inside [`unit_stage`](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1007) / [`build_source_units`](../../hifimule-daemon/src/auto_fill/pipeline.rs#L896): when base `unit == Track`, an album whose materialized candidate set contains **≥ N favorited tracks** is promoted to a single atomic album unit (the rest stay track singletons). The `Selector` already treats a multi-track unit atomically (a whole album fits or stops the source, [pipeline.rs:1640-1714](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1640)). Pure, deterministic — the affinity signal is `Song.is_favorite` ([domain/models.rs:43](../../hifimule-daemon/src/domain/models.rs#L43)), which the engine already reads.

3. **Output-ordering pass (Coherence #27).** Coherence **reorders the final selected `Vec<AutoFillItem>`** — clustering by `artist_id → album_id → disc_number → track_number` so an album's tracks are contiguous and in playback order. It **never changes which tracks are selected** (so every budget/dedup/memory guarantee is untouched) — only the order they come out. Pure, signal-free, deterministic.

The pure-engine invariant ([pipeline.rs:12-27,744-745](../../hifimule-daemon/src/auto_fill/pipeline.rs#L12)) holds throughout: no network, no `async`, no `MediaProvider`, no clock/RNG read inside the core. Spotlight artist selection that varies per sync rides the **existing `seed`-as-value** ([PipelineInput::seed pipeline.rs:712-716](../../hifimule-daemon/src/auto_fill/pipeline.rs#L712)) — minted impurely, consumed purely — exactly like 13.4's draws.

### Scope decision — favorites, not ratings; clustering, not audio-flow (read second)

Two ideas reference signals `Song` does not carry; deliver the feasible version, defer the rest (the same call 13.3/13.4/13.5 made):

- **#9 says "favorited / rated"** ([brainstorm:87](../brainstorming/brainstorming-session-2026-06-12-1.md)). `Song` has **`is_favorite: Option<bool>`** but **no star-rating field** ([domain/models.rs:26-50](../../hifimule-daemon/src/domain/models.rs#L26)). Deliver affinity on **favorites only** (count of `is_favorite == Some(true)` candidates in the album). Ratings-based promotion is **deferred** (no rating signal — same deferral as 13.3 community-rating #15 and 13.4 play-feedback).
- **#27 Coherence** *"optimize the set for flow, not tracks individually (resolved into #28 scoping)"* ([brainstorm:67](../brainstorming/brainstorming-session-2026-06-12-1.md)). True audio-flow (BPM/energy/key transitions) needs a signal `Song` lacks — the same gap that gated #17 energy-curve in 13.5. The brainstorm itself notes #27 "resolved into #28 scoping" (tag pre-filter, shipped in 12.4). The **feasible deterministic coherence** with no audio analysis is **structural clustering**: keep an album's/artist's tracks together and in track order so the fill plays as coherent groups rather than a scattered shuffle. **Audio-analysis-driven flow ordering is deferred.**

**Consciously DEFERRED (bright lines — do NOT implement here):**
- **Rating-based** album promotion or any rating-weighted unit logic — no rating signal on `Song`.
- **Audio-analysis / BPM / key-transition** coherence — no such signal; clustering is the whole delivery.
- **Cross-source artist/album merging beyond what the materialized pools already give** — the engine selects from the caller-materialized candidate pools; it does not fetch an artist's *full* discography on demand. Spotlight "depth" is bounded by what the configured sources surfaced (document this).
- **Any new DB table or new entropy** — Spotlight reuses the existing `seed`; the other three are pure functions of the candidate set. No runtime DB state this story.

This is the same discipline that landed 13.1–13.5 cleanly: **deliver the deterministic, signal-available core of each idea; defer anything that needs a signal or subsystem we don't have.**

## Acceptance Criteria

### A — Config: one additive `PromotionStage`, default = today's behavior

1. **New `PromotionStage` config carries all four advanced-unit modifiers; default ⇒ zero behavior change.** Add `pub promotion: PromotionStage` to [`AutoFillPipeline`](../../hifimule-daemon/src/auto_fill/pipeline.rs#L52-L89) (`#[serde(default)]`, camelCase), modeled on `QualityStage`/`RarityStage`/`ContextStage` (one optional stage, `#[derive(... PartialEq, Default)]`). Recommended shape:
   ```rust
   pub struct PromotionStage {
       /// #33 Artist Spotlight: feature ONE artist in depth.
       pub spotlight: bool,
       /// Share of the byte ceiling reserved for the featured artist (0.0..=1.0). None ⇒ default (e.g. 0.5).
       pub spotlight_share: Option<f32>,
       /// #8 Album/Track space ratio: fraction of the ceiling filled as COMPLETE albums (atomic),
       /// the remainder as loose tracks. None/0 ⇒ no album reserve (today's behavior).
       pub album_track_ratio: Option<f32>,
       /// #9 Affinity promotion: when base unit == Track, an album with >= N favorited candidate
       /// tracks is promoted to a single atomic album unit. None/0 ⇒ no promotion.
       pub promote_album_min_favorites: Option<u32>,
       /// #27 Coherence: reorder the final selection by artist→album→disc→track for flow.
       pub coherence: bool,
   }
   ```
   All-default (`spotlight:false`, all `None`, `coherence:false`) ⇒ today's behavior, exactly like `QualityStage::default()`. The existing `unit: Unit` field is **unchanged** — `promotion` augments it, never replaces it. [Source: AutoFillPipeline + stage-add precedent pipeline.rs:52-89; default-is-noop QualityStage pipeline.rs:245-257; ContextStage pipeline.rs:329-339]

2. **Parse tolerance.** Out-of-range floats are clamped (`spotlight_share`/`album_track_ratio` to `0.0..=1.0`) at consumption, mirroring the existing `.clamp(0.0, 1.0)` discipline ([pipeline.rs:807,987](../../hifimule-daemon/src/auto_fill/pipeline.rs#L807)). A malformed `PromotionStage` block degrades to default (`#[serde(default)]` on each field) and never aborts the pipeline parse. No custom deserializer is needed unless a field is a list (none is here). [Source: clamp discipline pipeline.rs:807,987; serde default pipeline.rs:53]

### B — #33 Artist Spotlight (feature one artist in depth)

3. **A spotlight reserve fills one deterministically-chosen featured artist first.** When `promotion.spotlight` AND the budget is bounded (`ceiling != u64::MAX`), `run_pipeline` runs a **spotlight pre-pass** (mirroring the stable-core/pity pre-passes, [pipeline.rs:802-848](../../hifimule-daemon/src/auto_fill/pipeline.rs#L802)):
   - **Choose the featured artist purely.** Across the primary sources' filtered candidates, pick the single `artist_id` whose **best-ranked candidate** ranks highest under the configured ordering ([`compare_by_ordering`](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1066)) — so the choice composes with the user's ordering and, when `Random`/`Rarity` is in the ordering, varies per `seed` (a different spotlight each sync — the #33 delight). Ties broken by `artist_id` string for determinism. Candidates with no `artist_id` are never featured.
   - **Reserve & fill in depth.** Reserve `round(ceiling × spotlight_share)` bytes (default `spotlight_share = 0.5` when `None`); fill that slice FIRST from **only the featured artist's** candidates, ordered by the pipeline ordering (track-level depth), honoring full Memory rules, via the shared `Selector` (`FillMode::Primary` cap = the reserve, scoped to the featured artist). The featured artist's tracks remain eligible in the later primary pass; dedup is automatic.
   - **Spillover.** If the featured artist has fewer bytes than the reserve, the unused budget simply flows to the normal primary pass (no special back-fill needed — the shared `Selector` and later passes consume the remaining ceiling). `spotlight_share = 0`, an unbounded ceiling, or no artist-bearing candidates ⇒ no-op.
   Deterministic; no clock; entropy only via the existing `seed`. [Source: stable-core/pity pre-pass pattern pipeline.rs:802-848; compare_by_ordering pipeline.rs:1066; seed pipeline.rs:712-716; source_caps pipeline.rs:1576]

### C — #8 Album/Track space ratio (complete albums + loose tracks)

4. **An album-ratio reserve fills complete albums first, then loose tracks.** When `promotion.album_track_ratio = Some(r > 0)` AND the budget is bounded, `run_pipeline` runs an **album pre-pass** before the normal (track) primary pass:
   - Reserve `round(ceiling × r.clamp(0,1))` bytes; fill that slice FIRST using **`Unit::Album` grouping** (complete albums, atomic — a whole album fits or the source stops, exactly as the `Selector` already enforces for multi-track units, [pipeline.rs:1660-1714](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1660)), across the primary sources (split the reserve across sources by share via `source_caps`, like stable-core, [pipeline.rs:814-818](../../hifimule-daemon/src/auto_fill/pipeline.rs#L814)).
   - The remaining budget fills with the pipeline's **base `unit`** (typically `Track`) as today. Dedup against the album pass is automatic (shared `Selector`).
   - This requires [`build_source_units`](../../hifimule-daemon/src/auto_fill/pipeline.rs#L896) to accept a **unit override** (the album pass forces `Unit::Album`; the normal pass keeps `pipeline.unit`) — add a `unit: Unit` (or `Option<Unit>`) parameter rather than reading `pipeline.unit` unconditionally. `r = 0`/unbounded ceiling ⇒ no-op (the album pre-pass is skipped, base unit governs everything). [Source: build_source_units pipeline.rs:896-925; unit_stage pipeline.rs:1007-1013; atomic-unit fit Selector pipeline.rs:1640-1714; source_caps split pipeline.rs:814-818]

### D — #9 Affinity-triggered album promotion

5. **A track-level fill promotes high-affinity albums to atomic album units.** When base `unit == Unit::Track` AND `promotion.promote_album_min_favorites = Some(n > 0)`, `unit_stage` groups candidates as follows: an album (`album_id`) whose materialized candidates include **≥ n** with `is_favorite == Some(true)` becomes a **single atomic album unit** (all its candidates grouped, like `Unit::Album`); every other candidate stays a **track singleton**. The promoted album therefore syncs whole-or-not (atomic) while the rest fill track-by-track. Preserve first-seen order (reuse [`group_by`](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1017)). When base `unit` is `Album` or `Artist`, promotion is a **no-op** (every album is already atomic) — gate on `unit == Track`. `None`/0 ⇒ today's track grouping. Pure; the only signal read is `Song.is_favorite`. [Source: unit_stage pipeline.rs:1007-1013; group_by pipeline.rs:1017-1057; Song.is_favorite domain/models.rs:43; fav_rank pipeline.rs (Favorite ordering key); affinity = favorites only — ratings deferred, see Scope]

### E — #27 Coherence ordering (cluster the final set for flow)

6. **Coherence reorders the final selection by artist→album→disc→track; it never changes the selection.** When `promotion.coherence`, after the full selection is assembled and **before `into_items`** returns (or as a stable reorder of the `items` vec), reorder the selected `AutoFillItem`s into **coherent clusters**: group by `artist` then `album` (preserving the order in which each artist/album first appears in the selected set, for determinism), and within an album sort by `disc_number` then `track_number` then `id`. The set of selected ids and the total bytes are **byte-identical** to the un-clustered run — only the output order changes (so every budget/dedup/memory/spotlight/album guarantee is untouched). `coherence:false` ⇒ today's order. The reorder needs `disc_number`/`track_number`, which live on `Song` — either reorder before items lose the `Song` (cluster the staged selection in the `Selector`/`run_pipeline`) or carry the needed sort fields. Pure, deterministic, signal-free. [Source: into_items pipeline.rs:1717-1719; make_item pipeline.rs:1731 (AutoFillItem carries album/artist but NOT track/disc — see Dev Notes for where to reorder); Song.track_number/disc_number domain/models.rs:37-38]

### F — Routing, UI, i18n, scope

7. **The configurable path recognizes the new stage.** Add `promotion_default` (`p.promotion == PromotionStage::default()`) ANDed into the [`needs_configurable_expansion`](../../hifimule-daemon/src/auto_fill/fetch.rs#L146-L195) discriminator alongside `quality_default`/`rarity_default`/`pity_default`/`context_default`. A promotion-only pipeline (default everything else but a promotion modifier set) must route to the engine path — the fast `run_auto_fill_provider` path can't run spotlight/album-ratio/promotion/coherence. A default `PromotionStage` keeps the fast path. Add a discriminator test. Note: a non-default base `unit` (`Album`/`Artist`) **already** routes via the existing `unit_default` check ([fetch.rs:160](../../hifimule-daemon/src/auto_fill/fetch.rs#L160)) — verify it still does. [Source: discriminator fetch.rs:146-195; quality/rarity/pity/context precedents :167-176]

8. **Configuration UI exposes the promotion controls under Advanced, near the Unit selector.**
   - **Mirror types:** add a `PromotionStage` TS type (`spotlight: boolean`, `spotlightShare?: number`, `albumTrackRatio?: number`, `promoteAlbumMinFavorites?: number`, `coherence: boolean`) and `promotion: PromotionStage` to the `AutoFillPipeline` mirror ([state/autoFill.ts:121-134](../../hifimule-ui/src/state/autoFill.ts#L121)), with **omit-when-default** `normalizePipeline`/`serializePipeline` handling (the pattern `rarity`/`pity`/`context` use — emit the object only when non-default; a default pipeline emits nothing and round-trips byte-identically so the fast path is preserved, [state/autoFill.ts:206-307](../../hifimule-ui/src/state/autoFill.ts#L206)).
   - **Renderer:** add a `renderPromotionStage` (a spotlight enable + share slider, an album/track-ratio slider, an affinity min-favorites number input, and a coherence switch) under the Advanced disclosure ([renderAdvanced AutoFillPanel.ts:303-320](../../hifimule-ui/src/components/AutoFillPanel.ts#L303)), modeled on `renderRarityStage`/`renderPityStage`/`renderContextStage` ([AutoFillPanel.ts:420-470](../../hifimule-ui/src/components/AutoFillPanel.ts#L420)); place it adjacent to the existing Unit selector ([AutoFillPanel.ts:307-311](../../hifimule-ui/src/components/AutoFillPanel.ts#L307)) so "advanced unit" controls cluster. Handlers invalidate the debounced live preview via `invalidatePreview()` ([AutoFillPanel.ts:981-991](../../hifimule-ui/src/components/AutoFillPanel.ts#L981)) like the existing stages. Use a **finer step** on the share/ratio sliders (review 13.5: `step="5"` snapped externally-authored values — prefer `step="1"` or finer). The simple (non-Advanced) default state is unchanged. [Source: state/autoFill.ts:121-307; AutoFillPanel.ts:303-470,680-682,981-991]

9. **i18n parity across all 4 locales.** Add every new `basket.autofill.*` label/hint key to **all 4 locales** (`en`/`fr`/`es`/`de`), mirroring the `snake_case` convention. Current parity is **120×4** (verified; locale blocks en :2 / fr :389 / es :776 / de :1163). New keys cover: a promotion section title, spotlight (enable + share + hint), album/track ratio (+ hint), affinity min-favorites (+ hint), coherence (enable + hint). **Count the new keys precisely and report the new `N×4`.** Keep `rtk cargo test -p hifimule-i18n` green. [Source: catalog.json 120 basket.autofill keys/locale today; 13.5 went 96×4 → 120×4]

10. **Backward compatibility & scope boundary.** A pipeline with a default `PromotionStage` behaves **exactly** as today — zero migration, fast path intact, byte-identical selection. The legacy `run_auto_fill*` path is untouched. Config (the four promotion fields) lives in the **manifest** pipeline, portable, per `(device, serverId)`, round-tripping through serde camelCase like every stage; **no DB table, no new entropy** (Spotlight reuses the existing `seed`). **Do NOT** implement: rating-based promotion (no rating signal), audio-analysis/BPM coherence (no signal), on-demand full-discography fetch for spotlight depth, or any new runtime DB state. This is the **last Epic 13 story** — it owns the remaining Unit/promotion ideas (#33/#8/#9/#27) and nothing else. [Source: epics.md:3103-3105 Story 13.6; sprint-change-proposal:115,117; storage split architecture.md:922; consciously-cut brainstorm:117-120]

11. **Build & tests green.** `rtk cargo test -p hifimule-daemon` passes with no regression (sandbox caveat: if mockito/networking is blocked, run targeted `rtk cargo test -p hifimule-daemon auto_fill::`). New tests must cover:
    - **Spotlight (#33):** the featured artist is chosen deterministically by ordering; the reserve fills that artist in depth; a different `seed` (with `Random`/`Rarity` in ordering) can change the featured artist; under-filled reserve spills to the primary pass; `spotlight:false`/unbounded ceiling ⇒ byte-identical to no-spotlight.
    - **Album/track ratio (#8):** ~`r` fraction of bytes comes from complete (atomic) albums and the remainder from tracks; an album that can't fit the album-reserve doesn't partially leak; `r=0` ⇒ unchanged; the base `unit` still governs the non-reserve pass.
    - **Affinity promotion (#9):** an album with ≥ n favorited candidates is emitted as a whole atomic unit (whole-or-nothing under budget); an album below the threshold stays track-level; promotion is inert when base `unit != Track`; `None`/0 ⇒ today's track grouping.
    - **Coherence (#27):** the **set of selected ids and total bytes are identical** with and without coherence (selection unchanged); the output order clusters by artist→album→disc→track; deterministic.
    - **Routing:** `needs_configurable_expansion` returns `true` for a promotion-only pipeline (each modifier) and `false` for legacy default.
    - **Serde:** round-trip a pipeline carrying all four promotion fields; a default `PromotionStage` omits/round-trips byte-identically.
    - **Persona:** strengthen a persona whose fill should reflect units (e.g. **Antoine** the audiophile — album integrity / spotlight; or a coherence assertion) so a promotion modifier demonstrably changes the result, expressed purely in config — **no `if persona ==` branch** ([persona suite pipeline.rs:1835-2161](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1835)).
    `rtk cargo clippy -p hifimule-daemon --all-targets` adds no new warnings in touched modules. Frontend `rtk npx tsc --noEmit` + `rtk npm run build` stay green; `rtk cargo test -p hifimule-i18n` green.

## Tasks / Subtasks

- [ ] **`PromotionStage` config + routing** (`hifimule-daemon/src/auto_fill/pipeline.rs`, `fetch.rs`) (AC: 1, 2, 7)
  - [ ] Add `PromotionStage { spotlight, spotlight_share, album_track_ratio, promote_album_min_favorites, coherence }` (camelCase, `#[serde(default)]`, `Default`, `PartialEq`); add `pub promotion: PromotionStage` to `AutoFillPipeline` and to `default_legacy()` ([pipeline.rs:583-607](../../hifimule-daemon/src/auto_fill/pipeline.rs#L583)).
  - [ ] Add `promotion_default` to `needs_configurable_expansion`; discriminator test (each modifier routes; default keeps fast path; verify `Unit::Album`/`Artist` still routes via `unit_default`).

- [ ] **#9 Affinity promotion (unit grouping)** (`hifimule-daemon/src/auto_fill/pipeline.rs`) (AC: 5)
  - [ ] Extend `unit_stage`/`build_source_units` to accept the promotion config (or a resolved promotion mode). When base `unit == Track` and `promote_album_min_favorites = Some(n>0)`: promote albums with ≥ n favorited candidates to atomic album units (reuse `group_by`), rest stay singletons. No-op for `Album`/`Artist` base unit.
  - [ ] Tests: promotion threshold met → atomic album; below → track-level; inert for non-Track base unit; None/0 unchanged.

- [ ] **#8 Album/track ratio + #33 Spotlight (reserve pre-passes)** (`hifimule-daemon/src/auto_fill/pipeline.rs`) (AC: 3, 4)
  - [ ] Add a `unit` override parameter to `build_source_units` (album pass forces `Unit::Album`; normal pass keeps `pipeline.unit`). Update existing call sites (pass `pipeline.unit`).
  - [ ] Album-ratio pre-pass: reserve `round(ceiling × r)` bytes, fill with `Unit::Album` across sources (`source_caps` split), restore ceiling, then the normal base-unit primary pass. Mirror the stable-core pre-pass exactly.
  - [ ] Spotlight pre-pass: choose the featured `artist_id` (best-ranked candidate by `compare_by_ordering`, tie-break by id; seed-aware via ordering); reserve `round(ceiling × spotlight_share)` (default 0.5), fill featured-artist candidates in depth, restore ceiling. Mirror the pity reserve pre-pass.
  - [ ] Decide & document the pre-pass order (recommend: stable-core → spotlight → album-ratio → pity → primary → fallback) and that reserves spill to later passes; ensure no double-count and the global ceiling/dedup hold.
  - [ ] Tests: spotlight depth + determinism + seed-variation + spillover + inert; album-ratio fraction + atomicity + r=0 unchanged.

- [ ] **#27 Coherence ordering** (`hifimule-daemon/src/auto_fill/pipeline.rs`) (AC: 6)
  - [ ] After selection assembly, when `coherence`, stable-reorder the selected set into artist→album clusters (first-appearance order), within an album by disc→track→id. Reorder where `Song` fields are still available (see Dev Notes). Selection set & bytes unchanged.
  - [ ] Tests: identical id-set & bytes with/without coherence; clustered order; deterministic.

- [ ] **Frontend: promotion stage** (`hifimule-ui/src/state/autoFill.ts`, `components/AutoFillPanel.ts`) (AC: 8)
  - [ ] Mirror `PromotionStage` + `pipeline.promotion`; omit-when-default normalize/serialize (mirror rarity/pity/context).
  - [ ] `renderPromotionStage` (spotlight enable+share, album/track ratio, affinity min-favorites, coherence switch) under `renderAdvanced` near the Unit selector; finer slider step; handlers invalidate the live preview.

- [ ] **i18n ×4 locales** (`hifimule-i18n/catalog.json`) (AC: 9)
  - [ ] Add all new `basket.autofill.*` keys to en/fr/es/de; report the new `N×4` (was 120×4). Catalog tests green.

- [ ] **Full verification** (AC: 10, 11)
  - [ ] Daemon tests (no regression); clippy clean on touched modules; i18n green; tsc + build green. Strengthen a persona (config-driven, no `if persona` branch).

## Dev Notes

### The Unit axis already exists — this story adds the *advanced* modifiers (read first)

`unit: Unit { Track, Album, Artist }` is shipped and works: [`unit_stage`](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1007) groups by `album_id`/`artist_id` via [`group_by`](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1017), and the `Selector` ([pipeline.rs:1640-1714](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1640)) already treats a multi-track unit **atomically** — it stages all syncable tracks of a unit, and if they don't all fit the cap/ceiling/duration it **stops the source** (smaller later units are not back-filled). That atomicity is exactly what #8 (complete albums) and #9 (promoted albums) need — **you are not building atomic-unit budgeting; it's already there.** This story adds four *modifiers* that the single `unit` field can't express. Keep them as one additive `PromotionStage`, default-noop, just like 13.2's `QualityStage` and 13.4's `RarityStage`/`PityStage`.

### Reserve pre-passes are a solved pattern — copy stable-core/pity (the central design)

`run_pipeline` already has **two** reserve pre-passes you should copy verbatim in structure ([pipeline.rs:802-848](../../hifimule-daemon/src/auto_fill/pipeline.rs#L802)):
- **stable-core (#24):** `selector.ceiling = core_cap` → `source_caps(sources, core_cap)` split → fill restricted candidates → `selector.ceiling = ceiling` restore.
- **pity reserve (#30):** same shape with a discovery-restricted candidate set and the reserve added on top of `cum_bytes`.

**#8 album-ratio** and **#33 spotlight** are the identical move: a temporary ceiling, a restricted scope (album-grouped units for #8; one artist's candidates for #33), the **shared `Selector`** (so dedup into the later primary pass is automatic), then restore. Spillover is free — an under-filled reserve just leaves ceiling for the primary/fallback passes. **Do not write a new selection loop.** The only genuinely new plumbing is letting `build_source_units` take a **unit override** (so #8's pass can force `Unit::Album` while the primary pass keeps `pipeline.unit`).

**Pre-pass ordering (document your choice).** There will be up to four reserve pre-passes (stable-core, spotlight, album-ratio, pity) before the primary pass. Recommend the order **stable-core → spotlight → album-ratio → pity → primary → fallback**, each consuming from the shared `Selector`'s remaining ceiling. The combined reserves must never exceed the global ceiling (each pass is capped by `selector.ceiling`; the primary pass then fills `ceiling - cum_bytes`). Add a test that combines two reserves and asserts total ≤ ceiling and no double-count. If interactions get hairy, keep each reserve modest and lean on the existing global-ceiling guard in `Selector::fill` ([pipeline.rs:1701-1706](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1701)).

### Spotlight: choosing the featured artist purely (seed-as-value, not a clock/RNG read)

Pick the featured artist **deterministically from the candidate set** — never read a clock/RNG. Recommended: group the primary candidates by `artist_id`, and feature the artist owning the **single best-ranked candidate** under `compare_by_ordering` (the same comparator the engine already uses, [pipeline.rs:1066](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1066)). Two payoffs: (a) it respects the user's ordering (favorites/quality/etc.), and (b) when the ordering includes `Random` or `Rarity`, the choice rides the existing `seed` ([PipelineInput::seed pipeline.rs:712-716](../../hifimule-daemon/src/auto_fill/pipeline.rs#L712)) — so "a different artist each sync" emerges from config + seed, with **zero new entropy**. Tie-break by `artist_id` string so equal-rank artists resolve deterministically. "In depth" is bounded by what the configured sources materialized — the engine does **not** fetch the artist's full discography (deferred; document it).

### Affinity promotion: favorites only (no rating signal)

The affinity signal is **`Song.is_favorite`** ([domain/models.rs:43](../../hifimule-daemon/src/domain/models.rs#L43)); there is **no star-rating field** on `Song` (confirmed: fields are id/title/artist/album/duration/bitrate/track_number/disc_number/cover/date_added/last_played_at/play_count/is_favorite/content_type/suffix/size_bytes). Count favorited candidates per `album_id` from the materialized pool and promote when `count >= n`. This is **per-pool / per-run**: an album is promoted based on the favorited tracks **present in the candidate set**, not the album's full track list (the engine doesn't have it). Gate on base `unit == Track` (promotion is meaningless when albums are already atomic). Rating-weighted promotion is **deferred** — same call 13.3 (#15 community-rating) and 13.4 (play-feedback) made.

### Coherence: reorder, never reselect — and mind where `Song` is still available

Coherence is the **safest** of the four: it must produce a **byte-identical selection** to the non-coherence run and only change output order. The trap: `AutoFillItem` ([mod.rs:32-53](../../hifimule-daemon/src/auto_fill/mod.rs#L32)) carries `album`/`artist`/`provider_album_id` but **not** `disc_number`/`track_number`. So either (a) do the clustering reorder **inside the engine while you still hold `Candidate`/`Song`** (cluster the staged/selected `Song`s, then emit items in that order), or (b) sort `AutoFillItem`s by the fields they have (`provider_album_id`, `artist`) and accept that intra-album order can't use disc/track without carrying them. Prefer (a): reorder the selected songs by `artist_id → album_id → disc_number → track_number → id` (first-appearance order for the artist/album cluster keys, to stay deterministic and independent of id sort). **Test that the id-set and total bytes are unchanged** — that invariant is the whole safety guarantee. Do not let coherence interact with budget/dedup.

### Routing gate — add `promotion_default`, verify base-unit routing

New stages **always** need a `*_default` clause in [`needs_configurable_expansion`](../../hifimule-daemon/src/auto_fill/fetch.rs#L146) or a configured pipeline silently takes the legacy fast path (which can't run any of this). Add `promotion_default` (mirror `quality_default`/`rarity_default`/`pity_default`/`context_default`, [fetch.rs:167-176](../../hifimule-daemon/src/auto_fill/fetch.rs#L167)). The base `unit` is already covered: `unit_default = p.unit == Unit::Track` ([fetch.rs:160](../../hifimule-daemon/src/auto_fill/fetch.rs#L160)) routes `Album`/`Artist` to the engine — verify, don't change.

### Frontend patterns (from 13.1–13.5)

- **Object stage needs omit-when-default serialize** — `promotion` (object with all-default fields) must emit nothing when default so the JSON matches the daemon serde and a default pipeline round-trips byte-identically (keeping the fast path). Copy `rarity`/`pity`/`context`'s normalize/serialize ([state/autoFill.ts:206-307](../../hifimule-ui/src/state/autoFill.ts#L206)). Emit the object only when a field is non-default (e.g. `spotlight || coherence || albumTrackRatio || promoteAlbumMinFavorites || spotlightShare`).
- **No UI unit-test framework** — rely on `tsc` + `build` + manual preview. New renderer goes under the Advanced disclosure ([AutoFillPanel.ts:303-320](../../hifimule-ui/src/components/AutoFillPanel.ts#L303)), near the existing Unit selector ([:307-311](../../hifimule-ui/src/components/AutoFillPanel.ts#L307)); model handlers on `renderRarityStage`/`renderPityStage`/`renderContextStage` ([:420-470](../../hifimule-ui/src/components/AutoFillPanel.ts#L420)) and invalidate the debounced live preview ([:981-991](../../hifimule-ui/src/components/AutoFillPanel.ts#L981)).
- **Slider step (review 13.5 patch):** the memory sliders used `step="5"` and snapped externally-authored non-5% values on edit. Use a **finer step** (`step="1"` for percent sliders) on `spotlightShare`/`albumTrackRatio`.

### i18n parity (currently 120×4 — hard gate)

Every new key in all 4 locales or the catalog test breaks. Count this story's keys precisely and report the new total. **Pre-existing gap (do not block on it, flagged in 13.3/13.4/13.5 reviews):** there is still **no automated all-locale key-parity test** in `hifimule-i18n` (only per-translation tests; `translate` silently falls back to English then the raw key). Verify parity by hand/script. Adding a real parity test is a worthwhile out-of-scope cleanup — flag it, don't expand this story for it.

### Persona suite is the engine acceptance bar

The persona suite ([pipeline.rs:1835-2161](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1835)) is the determinism contract. **Antoine** (audiophile, quality-first, [pipeline.rs:1911](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1911)) is the natural home for an album-integrity / spotlight / coherence assertion; **Claire** or **Nadia** also work for a coherence/promotion case. Strengthen one so a promotion modifier demonstrably changes the result under fixed inputs, and is inert under default `PromotionStage`. Behavior must emerge from config — **never** an `if persona ==` branch ([pipeline.rs:1835-1838](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1835)).

### Storage split & architecture compliance (non-negotiable)

- **Config** (the four promotion fields) → **manifest** pipeline, portable, per `(device, serverId)`, round-tripping through serde camelCase like every stage. **No DB table, no new entropy** this story (Spotlight reuses the existing `seed`; the other three are pure functions of the candidate set). [Source: storage split architecture.md:922]
- The pipeline stage model is an **open extension point** ([architecture.md:788-826](../../_bmad-output/planning-artifacts/architecture.md#L788); `unit: "track"|"album"|"artist"` :799); `promotion` is a new optional stage augmenting the `unit` axis in the same additive, default-noop spirit.
- Per-server routing & the legacy fast path untouched; the pure engine still never sees a provider, a clock, or an RNG read. Reuse Epic 12/13 types; only **add** `PromotionStage` and the `AutoFillPipeline::promotion` field + the `build_source_units` unit-override parameter. Do not redefine `AutoFillPipeline`/`Song`/`Unit`/`Selector`.

### Current code being changed (read before writing)

- **Engine** ([pipeline.rs](../../hifimule-daemon/src/auto_fill/pipeline.rs)): `AutoFillPipeline` :52-89 (add `promotion`), `Unit` :152-160 (unchanged), `default_legacy` :583-607 (add `promotion: PromotionStage::default()`), `run_pipeline` :745-873 (add spotlight + album-ratio pre-passes mirroring stable-core :802-821 / pity :823-848; coherence reorder before `into_items`), `build_source_units` :896-925 (add unit-override param), `unit_stage`/`group_by` :1007-1057 (affinity promotion grouping), `compare_by_ordering` :1066+ (reuse for spotlight artist pick — no change), `Selector::fill`/`into_items` :1643-1719 (coherence reorder point; atomicity already correct), `source_caps` :1576-1595 (reuse for reserve splits), persona suite :1835-2161 (Antoine :1911).
- **Async fetch** ([fetch.rs](../../hifimule-daemon/src/auto_fill/fetch.rs)): `needs_configurable_expansion` :146-195 (+ `promotion_default`; verify `unit_default` :160). The fetch/materialization path needs **no new data** — Spotlight/album-ratio/promotion/coherence all run over the already-materialized candidate pools (`artist_id`/`album_id`/`is_favorite`/`track_number`/`disc_number` are on `Song`). Confirm the pools carry these (they're populated from provider `Song`s).
- **Params** ([mod.rs:55-85](../../hifimule-daemon/src/auto_fill/mod.rs#L55)): **no change** — no new caller-supplied runtime value (Spotlight reuses `seed`, already threaded). `AutoFillItem` :32-53 — no new field strictly required (coherence reorders the vec; spotlight/album reasons can reuse `priority_reason`).
- **RPC** ([rpc.rs](../../hifimule-daemon/src/rpc.rs)): **no change expected** — the existing `seed` mint at the engine fill sites already feeds spotlight; no new DB read/write, no new transcode wiring (contrast 13.5). Verify nothing in the RPC layer gates on the stage set in a way that needs updating.
- **Frontend**: [state/autoFill.ts:121-307](../../hifimule-ui/src/state/autoFill.ts#L121) (mirror `PromotionStage` + normalize/serialize), [AutoFillPanel.ts:303-320,420-470,680-682,981-991](../../hifimule-ui/src/components/AutoFillPanel.ts#L303) (renderer + handlers + preview).
- **i18n**: [catalog.json](../../hifimule-i18n/catalog.json) locale blocks en :2 / fr :389 / es :776 / de :1163; `basket.autofill.*` snake_case keys (120/locale today).

### Previous story intelligence (13.1–13.5)

- **13.4/13.5** established **entropy/clock-as-value**: a runtime value (`seed`, `now`, civil time) is minted once at the impure RPC layer and threaded through `AutoFillParams` → `PipelineInput`; the pure engine consumes it, never reads a clock/RNG. Spotlight **reuses the already-threaded `seed`** — no new wiring, no new entropy (the simplest of the entropy-consuming features so far).
- **13.1–13.5** proved: new optional stages are additive + default-noop + need a `*_default` routing clause; default-equivalent stages must round-trip byte-identically (omit-when-default); behavior must emerge from config in the persona suite (no `if persona`). Follow all four.
- **13.5 review patches to honor:** clamp non-positive shares/weights (apply `.clamp(0.0,1.0)` to `spotlight_share`/`album_track_ratio`); avoid coarse slider `step` that snaps authored values (use `step="1"` or finer). [Source: 13-5 Review Findings]
- **Reserve-pass review lessons from 13.1 (apply to spotlight/album-ratio):** a fixed reserve share with no spillover under-fills when the restricted pool is small — the shared `Selector` + later passes already give spillover here, but **test the small-pool case**. Split a reserve across sources by `source_caps` so one source can't monopolize the slice (13.1's stable-core blend-distortion patch). [Source: 13-1 Review Findings, rotation lead-tier & stable-core patches]
- **Sandbox caveat (recurring):** full `rtk cargo test -p hifimule-daemon` may not finish where mockito/networking is blocked. New tests here are pure (`auto_fill::`); run targeted `rtk cargo test -p hifimule-daemon auto_fill::` if blocked.

### Git intelligence

Recent commits (`3a46765 Review 13.5`, `79a021d Dev 13.5`, `6b51767 Review 13.4`, `291e5f6 Dev 13.4`, `0a684c4 Story 13.4`) confirm Epic 13 has progressed 13.1→13.5 cleanly and this is the **final** Epic 13 story. No competing in-flight changes to `auto_fill/`, `AutoFillPanel.ts`, or `catalog.json`. The frozen contract that must survive: the legacy fast path + every default-stage pipeline behaves byte-identically; the pure engine stays clock-/RNG-/provider-free.

### Latest technical context

- **No new crate dependency.** Everything is pure functions over the existing `Song`/`Candidate` fields + the already-present `seed`/`serde`/`serde_json`. `rand`/`chrono` already in `Cargo.toml` are not needed for this story. Rust edition 2024 (`f64::total_cmp` for any float-key comparison — never `partial_cmp().unwrap()`, per 13.4's `Rarity` discipline; let-chains available).
- **No new clock/RNG read, no new network.** Spotlight's only entropy is the existing `seed`; coherence/album-ratio/affinity are deterministic functions of the candidate set.

### Project Structure Notes

- Daemon (Rust): engine in `auto_fill/pipeline.rs`; async/routing in `auto_fill/fetch.rs`; params in `auto_fill/mod.rs`. Binary crate (no `lib.rs`); tests are `#[cfg(test)] mod tests` in each file. `PromotionStage` lives in `pipeline.rs` — **not** `domain/models.rs` (provider-neutral entities only).
- Frontend (TS): `hifimule-ui/src/state/autoFill.ts`, `components/AutoFillPanel.ts`; i18n `hifimule-i18n/catalog.json`. No UI unit-test framework — `tsc` + build + manual preview.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Epic-13 (lines 3079-3105; Story 13.6 line 3103-3105: Artist Spotlight #33, album/track space ratio #8, affinity-triggered album promotion #9, coherence-optimized fill #27)]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-06-14-configurable-auto-fill.md (FR54 line 82; Epic 13 table line 115 "Advanced units & promotion"; conscious cuts line 117)]
- [Source: _bmad-output/brainstorming/brainstorming-session-2026-06-12-1.md (#8 album/track space ratio line 86, #9 affinity-triggered album promotion line 87, #33 Artist Spotlight line 88, #27 coherence-optimized fill line 67 "resolved into #28 scoping"; pipeline grid Unit stage line 138; gap-hunting "Artist Spotlight (unit=artist)" line 60)]
- [Source: _bmad-output/planning-artifacts/architecture.md#Auto-Fill-Pipeline-Model (lines 788-826; `unit: "track"|"album"|"artist"` line 799; open stage extension); #Enforcement (lines 913-922; config-in-manifest / history-in-DB line 922; route per server line 921)]
- [Source: _bmad-output/implementation-artifacts/13-5-context-and-encoding-from-goals.md (additive default-noop stage + routing clause; omit-when-default serialize; persona discipline; i18n parity gate 120×4; clamp & slider-step review patches; entropy/clock-as-value discipline)]
- [Source: _bmad-output/implementation-artifacts/13-1-memory-and-rotation-strategies.md (reserve pre-pass pattern; reserve spillover & source-cap split review patches; pure-function/determinism discipline; persona suite)]
- [Source: hifimule-daemon/src/auto_fill/pipeline.rs:52-89,152-160,245-257,329-339,583-607,712-716,745-873,896-925,1007-1057,1066+,1576-1595,1640-1719,1835-2161]
- [Source: hifimule-daemon/src/auto_fill/fetch.rs:146-195 (needs_configurable_expansion; unit_default :160; quality/rarity/pity/context precedents :167-176)]
- [Source: hifimule-daemon/src/auto_fill/mod.rs:32-85 (AutoFillItem — album/artist but no track/disc; AutoFillParams — seed already threaded)]
- [Source: hifimule-daemon/src/domain/models.rs:26-50 (Song — artist_id :29, album_id :31, track_number :37, disc_number :38, play_count :42, is_favorite :43; NO rating field)]
- [Source: hifimule-ui/src/state/autoFill.ts:8 (Unit type), 121-134 (AutoFillPipeline mirror), 173-307 (normalize/serialize omit-when-default)]
- [Source: hifimule-ui/src/components/AutoFillPanel.ts:82 (UNITS), 303-320 (renderAdvanced), 307-311 (unit selector), 420-470 (rarity/pity renderers), 680-682 (unit handler), 981-991 (invalidatePreview)]
- [Source: hifimule-i18n/catalog.json (en :2 / fr :389 / es :776 / de :1163; 120 basket.autofill keys/locale today; unit keys :117-120)]

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List

## Change Log

| Date | Change |
|------|--------|
| 2026-06-15 | Story 13.6 created via create-story. Scope: the Unit axis's advanced modifiers as one additive default-noop `PromotionStage` — #33 Artist Spotlight (seed-aware spotlight reserve pre-pass), #8 album/track space ratio (album reserve pre-pass), #9 affinity-triggered album promotion (favorites-only unit-grouping promotion), #27 coherence ordering (cluster the final selection by artist→album→disc→track; reorder-only, selection byte-identical). Reuses the stable-core/pity reserve-pass pattern, the existing atomic-unit `Selector`, and the already-threaded `seed`; no new DB table, no new entropy, no clock/RNG read. Deferred: rating-based promotion (no rating signal on `Song`) and audio-analysis/BPM coherence (no signal). Final Epic 13 story. |
