---
baseline_commit: c3fdaf062cb0fffc4f983ab6e756b17bb03ced2b
---

# Story 13.4: Delight — Rarity Draws & Pity Timer

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a HifiMule user who wants each sync to feel a little different — sometimes a hit, sometimes a buried gem — and who never wants the fill to go stale on the same tracks forever,
I want the auto-fill pipeline to support a **loot-table-style weighted rarity draw** (common/rare/legendary classes that give each sync its own texture) and a **pity timer** that guarantees a quota of fresh discoveries after a run of stale syncs,
so that fills stay surprising and self-refreshing without me re-tuning the config — while the selection core stays deterministic-given-its-inputs, budget-respecting, and per-server.

This story extends the **pure-function pipeline engine** (Epic 12 → 13.1/13.2/13.3) with the **delight layer** the brainstorm flagged for last (loot-table mechanics, [brainstorm:147](../brainstorming/brainstorming-session-2026-06-12-1.md)). It delivers exactly two brainstorm ideas: **#29 Weighted Rarity Draws** and **#30 Pity Timer**.

### What makes this story different from 13.2/13.3 (read first)

13.2 and 13.3 were **engine + tiny-UI** stories with no entropy, no clock, no DB. 13.4 is **not** that — it is the story that finally introduces the two things the engine has deliberately withheld:

1. **The first entropy (a seed).** The engine's `OrderingKey::Random` has shipped as a deliberate no-op since 12.1 with the comment *"deterministic no-op … Epic 13 adds seeding"* ([pipeline.rs:647-648](../../hifimule-daemon/src/auto_fill/pipeline.rs#L647-L648)). **This is that story.** Rarity draws are inherently a *random* draw. The discipline is preserved exactly the way `now` is: **the pure core stays deterministic *given its inputs*** — entropy enters as a caller-supplied **`seed`** carried on `PipelineInput` (mirroring `HistorySnapshot::now`), and the impure RPC layer is the only place a real "random" seed is minted. Same `(input, seed, pipeline)` ⇒ byte-identical output. Fixture tests pass a fixed seed and assert exact order. **Never call `rand::thread_rng()`, `SystemTime::now()`, or any global entropy inside `auto_fill/pipeline.rs`.**

2. **A new machine-local DB counter (the pity streak).** The pity timer needs runtime state across syncs — a per-`(device, server)` "dry-streak" counter — exactly analogous to the **rotation cursor** Story 13.1 added (`autofill_rotation`, [db.rs:237-246](../../hifimule-daemon/src/db.rs#L237-L246)). It is **machine-local runtime state**, stored in the daemon DB, **never** in the portable manifest (storage split, [architecture.md:922](../../_bmad-output/planning-artifacts/architecture.md#L922)). The pity *config* (enabled/threshold/ratio) lives in the manifest pipeline like every other stage.

So unlike 13.3, this story has **real wiring across all layers**: engine (`pipeline.rs`), async fetch (`fetch.rs` — thread the seed + the pity reserve, extend the routing gate), params (`mod.rs`), RPC (`rpc.rs` — mint the seed, read/reset/increment the pity counter), DB (`db.rs` — new counter table + accessors), frontend (`state/autoFill.ts` + `AutoFillPanel.ts`), and i18n.

### Scope decision — the deterministic cheap version of "self-adjusts from behavior" (read second)

Brainstorm #30 is literally *"discovery ratio **self-adjusts from behavior** (guaranteed finds after dry spells)"* ([brainstorm:99](../brainstorming/brainstorming-session-2026-06-12-1.md)). A *true* behavioral self-adjustment would watch whether the user **actually played** the discoveries we last forced through, and tune the ratio up/down from that signal. **That play-feedback loop is DEFERRED** — for the same reason 13.3 deferred ratings: it needs a play-tracking-over-time subsystem we don't have, and the brainstorm itself **consciously cut** "skip-based negative feedback" and "smart refill triggers" ([brainstorm:60,117 / sprint-change-proposal line 117](../../_bmad-output/planning-artifacts/sprint-change-proposal-2026-06-14-configurable-auto-fill.md)). 

This story ships the **deterministic, periodic-guarantee** version, which fully satisfies *"guaranteed finds after dry spells"*: the dry-streak counter advances each completed sync; once it reaches the configured threshold the fill **reserves a discovery quota** (a budget pre-pass, identical in shape to the stable-core pre-pass, [pipeline.rs:398-417](../../hifimule-daemon/src/auto_fill/pipeline.rs#L398-L417)) and then **resets the streak to 0**. The user is guaranteed fresh discoveries every `threshold + 1` syncs, with no manual re-tuning. The "did they listen?" behavioral closed loop is a future story. Keep that line bright — do **NOT** add play-feedback, skip demotion, or any new play-history table here.

This is the same discipline that made 13.1/13.2/13.3 land cleanly: **deliver the deterministic core of the idea; defer anything that needs a new data subsystem.** This story's *new* surface is the seed and one DB counter — nothing more.

## Acceptance Criteria

### A — Seeded entropy foundation (enables #29; activates the long-reserved `Random`)

1. **A caller-supplied `seed` carries all entropy; the engine stays pure.** Add `pub seed: u64` to [`PipelineInput`](../../hifimule-daemon/src/auto_fill/pipeline.rs#L336-L345) (`#[derive(Default)]` ⇒ defaults to `0`). The pure engine derives **every** random decision from `input.seed` — there is **no** `thread_rng`, `SystemTime`, or global entropy anywhere in `pipeline.rs`. Same `(input, seed, pipeline)` ⇒ identical `Vec<AutoFillItem>`. This mirrors exactly how `HistorySnapshot::now` carries "now" ([pipeline.rs:328-334](../../hifimule-daemon/src/auto_fill/pipeline.rs#L328-L334)). [Source: pipeline.rs:336-345; pure-function discipline pipeline.rs:12-27; the `Random` "Epic 13 adds seeding" note pipeline.rs:647]

2. **`OrderingKey::Random` becomes a functional seeded shuffle.** The reserved no-op arm ([pipeline.rs:647-648](../../hifimule-daemon/src/auto_fill/pipeline.rs#L647-L648)) is replaced by a **seeded uniform shuffle**: each song gets a stable per-song draw key derived from `(seed, song.id)`, and candidates compare by that key. Deterministic given seed; a different seed (likely) yields a different order; a pipeline that never lists `Random` is byte-for-byte unaffected. Implement the per-song key with an **explicit** integer mix (e.g. a splitmix64-style hash of `seed` combined with a stable hash of `song.id`) mapped to a `f64` in `[0,1)` — do **not** rely on `DefaultHasher`'s unspecified internals for the comparison value (use an explicit, unit-testable mix so the order is reproducible and asserted in a test with a fixed seed). Compare floats with `f64::total_cmp` (never `partial_cmp().unwrap()`). [Source: compare_by_ordering pipeline.rs:623-678; the `Random` arm pipeline.rs:647-648]

### B — Weighted Rarity Draws (#29): loot-table classes

3. **New `RarityStage` config defines common/rare/legendary classes and their draw weights.** Add an optional stage to `AutoFillPipeline` (`pub rarity: RarityStage`, `#[serde(default)]`, camelCase) with shape:
   ```rust
   pub struct RarityStage {
       pub enabled: bool,           // off ⇒ zero behavior change
       pub legendary_weight: f32,   // class for never-/0-played ("deepest" gems)
       pub rare_weight: f32,        // class for 1..=rare_max_plays
       pub common_weight: f32,      // class for > rare_max_plays (the hits)
       pub rare_max_plays: u32,     // boundary between rare and common (default 5)
   }
   ```
   All-default (`enabled:false`, weights `0.0`, `rare_max_plays:0`) ⇒ today's behavior, exactly like `QualityStage::default()`. The rarity **class** of a candidate is derived from `Song.play_count` (the only universal signal — there is no rating field; [domain/models.rs:42](../../hifimule-daemon/src/domain/models.rs#L42)): `None`/`0` → legendary, `1..=rare_max_plays` → rare, else → common. [Source: brainstorm #29 "common/rare/legendary classes give each sync texture (loot-table draw)" ([brainstorm:68](../brainstorming/brainstorming-session-2026-06-12-1.md)); QualityStage default-is-noop precedent pipeline.rs:166,221-228]

4. **New `OrderingKey::Rarity` performs the seeded *weighted* draw.** Add a `Rarity` variant to `OrderingKey` (camelCase `"rarity"`). Its comparator arm computes a **weighted** draw key per song using the Efraimidis–Spirakis weighted-reservoir formula — `key = u^(1/w)` where `u = unit01(mix(seed, hash(id)))` (the same uniform from AC 2) and `w` = the song's rarity-class weight from `RarityStage` — and compares **descending** (higher key first). Effect: a class with a higher weight tends to be drawn earlier, giving each sync its loot-table texture, while staying a **stable, pure, seeded** sort. A class weight of `0.0` forces that class's key to the bottom (effectively excluded from the draw unless nothing else remains) — handle `w == 0` explicitly (no divide-by-zero / `1/0`). Like every key, `Rarity` is one entry in the ordered `ordering` list: `[Rarity]` alone = a pure loot draw; `[Favorite, Rarity]` = favorites first, then loot-shuffle the rest. [Source: pipeline.rs:623-678 `compare_by_ordering`; ES weighted sampling is the standard one-pass weighted random permutation]

5. **`Rarity` reads its weights from `pipeline.rarity`; `Random` uses uniform weight 1.** Thread `input.seed` and `&pipeline.rarity` into `compare_by_ordering`. `Random` is the special case `w = 1.0` for every song (uniform shuffle); `Rarity` uses the per-class weight. When `rarity.enabled` is `false`, an `OrderingKey::Rarity` in the list falls back to **uniform** weights (so it degrades to a plain seeded shuffle, never a panic). **There are THREE call sites — a signature change must update all three or it won't compile**, and each must receive a seed: the two in `build_source_units` ([pipeline.rs:475,478](../../hifimule-daemon/src/auto_fill/pipeline.rs#L475), both have `input`/`pipeline` in scope) **and** the one inside `best_version_cmp` ([pipeline.rs:930](../../hifimule-daemon/src/auto_fill/pipeline.rs#L930), reached from `collapse_best_version` which runs in `run_pipeline` with `input` in scope — thread the seed down through `best_version_cmp`'s signature). Note the best-version pre-pass is about choosing the best *duplicate version* deterministically; if a randomized key reaches it the collapse becomes seed-dependent — acceptable (still deterministic given seed) but call it out in a doc comment so the behavior is intentional, not surprising. [Source: build_source_units pipeline.rs:471-480; best_version_cmp pipeline.rs:907-942 (its `compare_by_ordering` call :930)]

### C — Pity Timer (#30): guaranteed discoveries after dry spells (deterministic version)

6. **New `PityStage` config.** Add `pub pity: PityStage` to `AutoFillPipeline` (`#[serde(default)]`, camelCase):
   ```rust
   pub struct PityStage {
       pub enabled: bool,            // off ⇒ no reserve, no counter interaction
       pub threshold_syncs: u32,     // dry syncs before the guarantee fires (default 3)
       pub guaranteed_ratio: f32,    // fraction of budget reserved for discovery when it fires (0.0..=1.0)
       pub discovery_max_plays: u32, // a "discovery" candidate has play_count <= this (default 0 = never-played)
   }
   ```
   All-default (`enabled:false`) ⇒ today's behavior. [Source: brainstorm #30 ([brainstorm:99](../brainstorming/brainstorming-session-2026-06-12-1.md)); storage-split architecture.md:809-812,922]

7. **The pity reserve is a budget pre-pass that fires only on a dry streak.** Carry the streak into the engine: add `pub pity_streak: i64` to `PipelineInput` (caller-supplied, exactly like the rotation cursor is carried via `AutoFillParams`). In `run_pipeline`, **before** the primary fill (and after the stable-core pre-pass, with which it composes), when `pity.enabled && input.pity_streak >= pity.threshold_syncs as i64 && ceiling != u64::MAX`: reserve `round(ceiling × guaranteed_ratio)` bytes and fill that reserve **first**, drawing only **discovery-class** candidates — `play_count <= discovery_max_plays` **AND** not currently on the device (`!is_on_device`, [pipeline.rs:555-560](../../hifimule-daemon/src/auto_fill/pipeline.rs#L555-L560)) so the guarantee surfaces genuinely *new* gems, not residents. The remaining budget then fills normally; dedup against the reserve is automatic via the shared `Selector` (same mechanism the stable-core pass relies on). `guaranteed_ratio = 0`, an unbounded ceiling, or `pity_streak < threshold` ⇒ no-op. [Source: stable-core pre-pass shape pipeline.rs:398-417; `is_on_device` pipeline.rs:553-560; FillMode pipeline.rs:447-451 — add or reuse a fill mode for the discovery reserve]

8. **A new DB counter tracks the dry streak (machine-local, never in the manifest).** Add an `autofill_pity(device_id TEXT, server_id TEXT, dry_streak INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(device_id, server_id))` table (`CREATE TABLE IF NOT EXISTS`, additive — no migration of existing rows), mirroring `autofill_rotation` ([db.rs:237-246](../../hifimule-daemon/src/db.rs#L237-L246)). Add `Database` methods `get_pity_streak(device_id, server_id) -> Result<i64>` (defaults to 0 on no row, like [`get_rotation_cursor` db.rs:850-861](../../hifimule-daemon/src/db.rs#L850)) and `set_pity_streak(device_id, server_id, value)`. Round-trip unit tests in `db.rs` mirroring the existing rotation/history tests. [Source: db.rs:237-246 autofill_rotation table; db.rs:850-880 cursor accessors; db.rs:784-848 history accessors + tests at :953-1009]

9. **RPC mints the seed and wires the pity streak read + reset/increment.** 
   - **Seed:** add `pub seed: u64` to [`AutoFillParams`](../../hifimule-daemon/src/auto_fill/mod.rs#L55-L75). At both fill call sites ([rpc.rs:2659 and rpc.rs:4003](../../hifimule-daemon/src/rpc.rs#L2659)), set `seed` from the already-computed `now` (e.g. `now as u64`) so it varies per run but is deterministic within a run; thread it into `PipelineInput.seed` inside `expand_with_pipeline`. [Source: rpc.rs:2646-2666, rpc.rs:3999-4011; now_unix_secs rpc.rs:3574-3579]
   - **Streak read:** extend `build_autofill_history` ([rpc.rs:3584-3608](../../hifimule-daemon/src/rpc.rs#L3584-L3608)) to also read `get_pity_streak` (best-effort, default 0) and carry it through `AutoFillParams.pity_streak` → `PipelineInput.pity_streak`.
   - **Reset/increment (at sync completion):** in `record_autofill_history_after_sync` ([rpc.rs:3631-3790](../../hifimule-daemon/src/rpc.rs#L3631-L3790)), per touched server whose pipeline has `pity.enabled` and that **actually wrote a track this run** (`servers_wrote`, the same gate the rotation-cursor advance uses, [rpc.rs:3783-3788](../../hifimule-daemon/src/rpc.rs#L3783)): if the streak read for this run was `>= threshold_syncs` (the guarantee fired) → `set_pity_streak(.., 0)`; else → `set_pity_streak(.., streak + 1)`. Best-effort (never fail the sync). Note: this reads `threshold_syncs` from `manifest.auto_fill.pipeline_for(server_id)` exactly like `pipeline_uses_tiers` does for rotation. [Source: rpc.rs:3771-3789 the rotation-advance block is the precise template; `pipeline_for` usage rpc.rs:3779-3782]

### D — Routing, UI, i18n, scope

10. **The configurable path recognizes the new stages and keys.** Extend `needs_configurable_expansion` ([fetch.rs:145-180](../../hifimule-daemon/src/auto_fill/fetch.rs#L145-L180)) with `rarity_default` (`p.rarity == RarityStage::default()`) and `pity_default` (`p.pity == PityStage::default()`) checks, ANDed into the discriminator alongside the existing `quality_default`. `OrderingKey::Random`/`Rarity` in `ordering` already force the configurable path via `ordering_default` (any non-`[Favorite,PlayCount,DateCreated]` ordering is non-legacy) — **verify** with a test, no logic change for that part. A default-legacy pipeline still takes the fast path. [Source: fetch.rs:145-180; the `quality_default` precedent fetch.rs:162-166; 13.3 AC 7 / 13.2 AC 8]

11. **Configuration UI exposes rarity & pity under Advanced, plus the two new ordering keys.** 
    - **Ordering:** add `'random'` and `'rarity'` to the `OrderingKey` union and to `ORDERING_KEYS` ([state/autoFill.ts:7,71](../../hifimule-ui/src/state/autoFill.ts#L7)). They then appear automatically in the data-driven ordering editor ([AutoFillPanel.ts:425-443 `renderOrderingSection`](../../hifimule-ui/src/components/AutoFillPanel.ts#L425)) via `t('basket.autofill.ordering_' + key)` — no new render code for the dropdown (same as 13.3's `excavation`/`rediscovery`). Note `random` was previously intentionally hidden ([state/autoFill.ts:71 comment](../../hifimule-ui/src/state/autoFill.ts#L71)); now that it is functional it is surfaced.
    - **Rarity & Pity stages:** add `RarityStage`/`PityStage` to the TS `AutoFillPipeline` mirror, with `normalizePipeline`/`serializePipeline` **omit-when-default** handling (the same pattern `quality` and the reserved memory fields use — emit nothing when disabled so the JSON matches the daemon serde and default pipelines round-trip byte-identically). Add two new stage renderers under the Advanced disclosure ([renderAdvanced AutoFillPanel.ts:246-262](../../hifimule-ui/src/components/AutoFillPanel.ts#L246)), modeled on `renderMemoryStage`/`renderQualityStage` ([AutoFillPanel.ts:264-340](../../hifimule-ui/src/components/AutoFillPanel.ts#L264)), with handlers that invalidate the debounced live preview like the existing stage handlers. The simple (non-Advanced) default state is unchanged. [Source: state/autoFill.ts:6-7,60-73,99-185; AutoFillPanel.ts:246-340,425-443]

12. **i18n parity across all 4 locales.** Add every new label/hint key under `basket.autofill.*` to **all 4 locales** (`en`/`fr`/`es`/`de`), mirroring the existing convention ([catalog.json en block, fr ~:350, es ~:698, de ~:1046](../../hifimule-i18n/catalog.json#L350)). New keys: `ordering_random`, `ordering_rarity`, the Rarity stage (title + enable + the three class-weight labels + `rare_max_plays` + a `rarity_hint`), and the Pity stage (title + enable + `pity_threshold` + `pity_ratio` + `pity_discovery_max_plays` + a `pity_hint`). Current parity is **81×4**; count the new keys precisely and report the new `N×4`. Keep the catalog tests green. [Source: catalog.json basket.autofill.* (81 keys/locale today); 13.3 completion "81×4"; locale blocks at the four offsets above]

13. **Backward compatibility & scope boundary.** A pipeline that sets neither `rarity` nor `pity` and lists neither `Random` nor `Rarity` behaves **exactly** as today — zero migration, fast path intact. The legacy `run_auto_fill*` path is untouched (it knows only the legacy default). The `seed`/`pity_streak` fields default to `0`/`0` and have no effect when the features are off. **Do NOT** implement, in this story: play-feedback / behavioral ratio adjustment / skip demotion (deferred — see Scope decision), context/time-of-day/energy/seasonal (#3/#17/#32 → 13.5), encoding-from-goals (#20 → 13.5), Artist Spotlight / album-ratio / album promotion / coherence (#33/#8/#9/#27 → 13.6), and anything needing a rating field (#15/#16 — already deferred in 13.3). **Do NOT** add a play-history table; the only new DB object is `autofill_pity`. [Source: epics.md:3095-3105 Epic 13 story boundaries; sprint-change-proposal line 117 conscious cuts]

14. **Build & tests green.** `rtk cargo test -p hifimule-daemon` passes (no regression; if the sandbox blocks mockito/networking run targeted `rtk cargo test -p hifimule-daemon auto_fill::` + `rtk cargo test -p hifimule-daemon db::`). New tests must cover:
    - **Determinism:** a fixed `seed` produces a stable, asserted order for `[Random]` and `[Rarity]`; two different seeds (likely) differ; re-running the same input twice is identical.
    - **Rarity weighting:** with `legendary_weight ≫ common_weight`, over a fixed seed a legendary (never-played) candidate is reliably drawn before a common (heavily-played) one; weight `0.0` for a class sinks it; `Rarity` with `rarity.enabled=false` degrades to a uniform shuffle (no panic).
    - **`Rarity` composes** as one key among others (`[Favorite, Rarity]` keeps favorites ahead of non-favorites; placement precedence preserved; a pipeline without it is unchanged).
    - **Pity reserve:** `pity_streak >= threshold` reserves the discovery quota and surfaces ≥1 never-played, not-on-device track it otherwise wouldn't; `pity_streak < threshold` is a no-op; `guaranteed_ratio` bounds the reserve bytes; unbounded ceiling is a no-op; the reserve composes with stable-core.
    - **DB counter:** `autofill_pity` round-trip (default 0, set/read), reset-vs-increment semantics covered in an RPC-or-db-level test.
    - **Routing:** `needs_configurable_expansion` returns `true` for a `rarity.enabled`-only, a `pity.enabled`-only, a `Random`-only-ordering, and a `Rarity`-only-ordering pipeline; `false` for legacy default.
    - **Serde:** round-trip a pipeline carrying `rarity`/`pity` and `Random`/`Rarity` ordering keys.
    - **Persona:** strengthen the **Léo** explorer/discovery persona ([pipeline.rs:1321 `persona_leo_gym_energy_playlist_tiny_device`](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1321)) to assert a pity-reserve or rarity draw surfaces a discovery — behavior must emerge from config, **no `if persona ==` branch**.
    `rtk cargo clippy -p hifimule-daemon --all-targets` adds no new warnings in touched modules. Frontend `rtk npx tsc --noEmit` + `rtk npm run build` stay green; `rtk cargo test -p hifimule-i18n` green.

## Tasks / Subtasks

- [ ] **Seed foundation + activate `Random` (#29 entropy)** (`hifimule-daemon/src/auto_fill/pipeline.rs`) (AC: 1, 2)
  - [ ] Add `pub seed: u64` to `PipelineInput` ([:336-345](../../hifimule-daemon/src/auto_fill/pipeline.rs#L336)). `#[derive(Default)]` already present ⇒ defaults to 0; confirm no other constructor needs updating.
  - [ ] Add an explicit, unit-testable per-song uniform helper: `fn draw_unit01(seed: u64, id: &str) -> f64` using a splitmix64-style mix of `seed` and a stable hash of `id`, mapped into `[0,1)`. No `DefaultHasher`-internal reliance in the *comparison value*; no global entropy.
  - [ ] Replace `OrderingKey::Random => Ordering::Equal` with the seeded uniform shuffle (descending by `draw_unit01`, compared via `f64::total_cmp`). Update the doc comment (drop "no-op"; now "seeded uniform shuffle — Epic 13.4").
  - [ ] Tests: fixed seed ⇒ stable asserted order; different seed ⇒ (likely) different; pipeline without `Random` unchanged.

- [ ] **Rarity stage + `OrderingKey::Rarity` (#29)** (`hifimule-daemon/src/auto_fill/pipeline.rs`) (AC: 3, 4, 5)
  - [ ] Add `RarityStage` struct (fields per AC 3) with `#[derive(..., Default)]` + `#[serde(rename_all="camelCase", default)]`; add `pub rarity: RarityStage` to `AutoFillPipeline` ([:54-77](../../hifimule-daemon/src/auto_fill/pipeline.rs#L54)).
  - [ ] Add `Rarity` to `OrderingKey` (`"rarity"`); doc-comment the loot-table behavior.
  - [ ] Add helpers: `fn rarity_class_weight(song, &RarityStage) -> f32` (legendary/rare/common by `play_count` vs `rare_max_plays`) and `fn es_draw_key(seed, id, weight) -> f64` (`u^(1/w)`, `w==0 ⇒ key 0.0`).
  - [ ] Thread `seed: u64` + `rarity: &RarityStage` into `compare_by_ordering` ([:623-678](../../hifimule-daemon/src/auto_fill/pipeline.rs#L623)); update **all THREE** call sites: `build_source_units` ([:475,478](../../hifimule-daemon/src/auto_fill/pipeline.rs#L475)) and `best_version_cmp` ([:930](../../hifimule-daemon/src/auto_fill/pipeline.rs#L930), thread seed through its signature). `Random` arm uses weight 1.0; `Rarity` arm uses class weight (uniform fallback when `!rarity.enabled`). Compare descending via `total_cmp`.
  - [ ] Tests per AC 14 (rarity weighting, composition, disabled-fallback, determinism).

- [ ] **Pity stage + discovery-reserve pre-pass (#30 engine)** (`hifimule-daemon/src/auto_fill/pipeline.rs`) (AC: 6, 7)
  - [ ] Add `PityStage` struct (fields per AC 6) + `pub pity: PityStage` to `AutoFillPipeline`; add `pub pity_streak: i64` to `PipelineInput`.
  - [ ] In `run_pipeline` ([:369-441](../../hifimule-daemon/src/auto_fill/pipeline.rs#L369)), after the stable-core pre-pass and before the primary fill, add the discovery reserve: gated on `pity.enabled && pity_streak >= threshold && ceiling != u64::MAX`; reserve `round(ceiling × guaranteed_ratio)`; fill from discovery-class candidates only (`play_count <= discovery_max_plays && !is_on_device`). Reuse/extend `FillMode` and the `Selector` so dedup carries into the normal pass.
  - [ ] Tests per AC 14 (reserve fires/no-op, ratio bound, unbounded no-op, composes with stable-core, surfaces a new gem).

- [ ] **DB pity counter** (`hifimule-daemon/src/db.rs`) (AC: 8)
  - [ ] `CREATE TABLE IF NOT EXISTS autofill_pity (...)` in the migration block next to `autofill_rotation` ([:237-246](../../hifimule-daemon/src/db.rs#L237)).
  - [ ] `get_pity_streak` (default 0) + `set_pity_streak` accessors, modeled on the rotation-cursor pair ([:850-880](../../hifimule-daemon/src/db.rs#L850)).
  - [ ] Round-trip + reset/increment unit tests mirroring [:953-1009](../../hifimule-daemon/src/db.rs#L953).

- [ ] **AutoFillParams + RPC wiring (seed + pity read/reset)** (`hifimule-daemon/src/auto_fill/mod.rs`, `hifimule-daemon/src/rpc.rs`, `hifimule-daemon/src/auto_fill/fetch.rs`) (AC: 9)
  - [ ] Add `pub seed: u64` + `pub pity_streak: i64` to `AutoFillParams` ([mod.rs:55-75](../../hifimule-daemon/src/auto_fill/mod.rs#L55)); update the two test builders in `fetch.rs` ([:790-819](../../hifimule-daemon/src/auto_fill/fetch.rs#L790)).
  - [ ] In `expand_with_pipeline` ([fetch.rs:343-347](../../hifimule-daemon/src/auto_fill/fetch.rs#L343)), set `PipelineInput.seed = params.seed` and `pity_streak = params.pity_streak`.
  - [ ] `build_autofill_history` ([rpc.rs:3584-3608](../../hifimule-daemon/src/rpc.rs#L3584)) also reads `get_pity_streak`; both fill call sites ([rpc.rs:2659,4003](../../hifimule-daemon/src/rpc.rs#L2659)) set `seed = now as u64` and pass the streak.
  - [ ] `record_autofill_history_after_sync` ([rpc.rs:3631-3790](../../hifimule-daemon/src/rpc.rs#L3631)): per touched server with `pity.enabled` that wrote a track, reset to 0 if the read streak `>= threshold` else increment (best-effort), mirroring the rotation-cursor advance block.

- [ ] **Routing gate** (`hifimule-daemon/src/auto_fill/fetch.rs`) (AC: 10)
  - [ ] Add `rarity_default`/`pity_default` to `needs_configurable_expansion` ([:145-180](../../hifimule-daemon/src/auto_fill/fetch.rs#L145)). Add a discriminator test for rarity-only / pity-only / `Random`-ordering / `Rarity`-ordering forcing configurable, and legacy default staying fast.

- [ ] **Frontend: ordering keys + rarity/pity stages** (`hifimule-ui/src/state/autoFill.ts`, `components/AutoFillPanel.ts`) (AC: 11)
  - [ ] `state/autoFill.ts`: add `'random' | 'rarity'` to `OrderingKey` + `ORDERING_KEYS`; add `RarityStage`/`PityStage` interfaces to `AutoFillPipeline`; add omit-when-default normalize/serialize (mirror `quality`).
  - [ ] `AutoFillPanel.ts`: add `renderRarityStage`/`renderPityStage` under `renderAdvanced`, modeled on `renderMemoryStage`/`renderQualityStage`; wire handlers to invalidate the live preview. Verify dropdown surfaces `random`/`rarity` automatically.

- [ ] **i18n ×4 locales** (`hifimule-i18n/catalog.json`) (AC: 12)
  - [ ] Add all new `basket.autofill.*` keys (ordering_random, ordering_rarity, rarity_* , pity_*) to en/fr/es/de. Report exact new `N×4`. Catalog tests green.

- [ ] **Full verification** (AC: 13, 14)
  - [ ] `rtk cargo test -p hifimule-daemon` (targeted `auto_fill::` + `db::` if sandbox-blocked), `rtk cargo clippy -p hifimule-daemon --all-targets` (no new warnings in touched modules), `rtk cargo test -p hifimule-i18n`, frontend `rtk npx tsc --noEmit` + `rtk npm run build`. Strengthen Léo persona for a config-driven rarity/pity discovery.

## Dev Notes

### What this story is (and is not)

The **biggest** Epic 13 story and the **only** one with cross-layer wiring (engine → fetch → RPC → DB → UI → i18n). It delivers exactly two ideas — **#29 weighted rarity draws** and **#30 pity timer (deterministic guarantee version)**. It introduces the engine's **first seed** and its **second** machine-local DB counter (`autofill_pity`, sibling of `autofill_rotation`). Everything else in the brainstorm catalog is another story (13.5/13.6) or already deferred (ratings #15/#16, play-feedback). Resist scope creep: no play-history, no skip demotion, no context/time strategies.

### The seed: how entropy enters a deliberately-pure engine (the central design)

The engine's defining invariant ([pipeline.rs:12-27](../../hifimule-daemon/src/auto_fill/pipeline.rs#L12)) is *"no network, no `async`, no `MediaProvider` call, and no clock/RNG read inside this core."* 13.4 does **not** break that — it threads a **value** (`seed: u64`) the same way `now` is threaded onto `HistorySnapshot`. The pure core consumes the seed; it never *generates* one. The single place a real seed is minted is the impure RPC layer (`seed = now as u64`), exactly where `now_unix` is already computed. This keeps the whole `auto_fill::` test suite a pure fixture suite: pass a fixed seed, assert an exact permutation. **If you ever feel the urge to call `thread_rng()` or `SystemTime::now()` inside `pipeline.rs` or `fetch.rs`'s selection, stop — the seed already carries everything.**

Why Efraimidis–Spirakis (`u^(1/w)`) and not a stateful `StdRng` shuffle: ES gives a **weighted random permutation in one stateless pass** computable per-element from `(seed, id, weight)`, so it slots into the existing pairwise `compare_by_ordering` framework with zero new control flow and stays trivially reproducible/testable. A stateful RNG shuffle would force a separate ordering stage and a captured RNG, fighting the comparator model. `rand = "0.8"` is already a dep ([Cargo.toml:34](../../hifimule-daemon/Cargo.toml#L34)) if you want its uniform helpers, but a hand-rolled splitmix64 mix is clearer and dependency-free for the unit01 step — your call, but make the mix **explicit and tested**, not `DefaultHasher`'s opaque output.

### Floats in a comparator — the correctness footgun

Draw keys are `f64`. **Never** `partial_cmp().unwrap()` (NaN panics). Use `f64::total_cmp` (stable, edition 2024). Guard `w == 0.0` before `1.0/w` (would be `inf`); define weight-0 ⇒ key `0.0` (sorts last) explicitly. This is the single subtle correctness point on the rarity side — unit-test it (a 0-weight class must sink, no panic, no NaN).

### The pity reserve mirrors stable-core exactly — copy that shape

The stable-core pre-pass ([pipeline.rs:398-417](../../hifimule-daemon/src/auto_fill/pipeline.rs#L398)) is the precise template: it temporarily sets `selector.ceiling` to a reserved cap, fills from a restricted candidate set, then restores the full ceiling and runs the normal passes. The pity reserve is the same move with a different restriction (discovery-class, not on-device) and a different gate (`pity_streak >= threshold`). Dedup carries forward automatically through the shared `Selector.seen` set. Order matters: stable-core (keep) → pity reserve (force-new) → primary (normal) → fallback. Document the ordering inline.

### Pity streak semantics — deterministic, no play-feedback (scope boundary)

The streak is a **periodic counter**, not a behavioral inference. Read it at fill time; if `>= threshold`, the reserve fires this run and the streak **resets to 0** at sync completion; otherwise it **increments**. Net effect: a guaranteed discovery injection every `threshold + 1` syncs. This is the honest deterministic core of "guaranteed finds after dry spells." The *true* "self-adjusts from behavior" (watch whether the user played the forced discoveries, tune the ratio) needs a play-feedback subsystem we don't have and the brainstorm consciously cut adjacent ideas — **deferred**. Do not add a play-history table or a per-track "was it played" round-trip. The reset/increment decision is computable from the streak the RPC already read + the `servers_wrote` gate; **no new field on `AutoFillItem` or `SyncDelta` is required** (contrast 13.1's `tier`, which did need propagation because it's per-track — the pity reset is per-server).

### The storage split is law (architecture.md:922)

- **Config** (rarity weights, pity threshold/ratio) → **manifest** pipeline, portable, per `(device, serverId)`. Round-trips through serde camelCase like every stage.
- **Runtime state** (the dry-streak counter, the seed) → **daemon DB / caller-supplied**, machine-local, **never** in the manifest. `autofill_pity` is keyed by `(device_id, server_id)` with the **portable** server id, identical to `autofill_rotation` and `autofill_history`.
- Never mix them. [Source: architecture.md:809-812,920-922]

### Routing gate (`needs_configurable_expansion`) — add two fields, verify the keys

New pipeline fields **always** need a `*_default` clause in the discriminator or a configured pipeline would silently take the legacy fast path (which can't run rarity/pity). Add `rarity_default`/`pity_default` (mirror `quality_default`, [fetch.rs:162-166](../../hifimule-daemon/src/auto_fill/fetch.rs#L162)). The `Random`/`Rarity` *ordering keys* are already caught by `ordering_default` (any non-legacy ordering routes) — that part is verify-only, exactly as 13.2/13.3 documented for `Quality`/`Excavation`/`Rediscovery`.

### Frontend patterns (from 13.1/13.2/13.3)

- **Ordering keys are label-only and data-driven** — adding `'random'`/`'rarity'` to `ORDERING_KEYS` surfaces them in the dropdown/reorder/remove automatically via `t('basket.autofill.ordering_'+key)`; `ordering` round-trips verbatim through normalize/serialize (no per-key serialize logic). `random` was deliberately hidden before ([state/autoFill.ts:71](../../hifimule-ui/src/state/autoFill.ts#L71)) — now it's functional, surface it.
- **Object stages (rarity/pity) need omit-when-default serialize** — unlike `ordering`, these are objects with defaults; copy the `quality` stage's normalize/serialize handling so a default pipeline emits nothing for them and round-trips byte-identically (a default-equivalent pipeline must keep the fast path). The reserved memory fields and `quality` are the precedents.
- **No UI unit-test framework** — rely on `tsc` + `build` + manual preview, matching the existing pattern. New stage renderers go under the Advanced disclosure ([AutoFillPanel.ts:246-262](../../hifimule-ui/src/components/AutoFillPanel.ts#L246)); model handlers on `renderMemoryStage`/`renderQualityStage` and invalidate the debounced live preview like the other stage edits.

### i18n parity (currently 81×4 — hard gate)

Every new key in all 4 locales or the catalog breaks. 13.3 went 78×4 → 81×4. Count this story's keys precisely and report the new total. **Note (pre-existing gap, do not block on it):** the 13.3 review confirmed there is **no automated all-locale key-parity test** in `hifimule-i18n` — only 6 per-translation tests. Verify parity by hand (every new key present in en/fr/es/de). Adding a real key-parity test is a worthwhile but out-of-scope cleanup; flag it if you have spare cycles, don't let it expand this story.

### Persona suite is the engine acceptance bar

Léo ([pipeline.rs:1321](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1321)) is the explorer/discovery persona — the natural home for a rarity/pity assertion. Strengthen him so that under a `Rarity`-weighted or pity-reserved config a discovery (never-played) track surfaces it otherwise wouldn't. Behavior must emerge from config; **never** add an `if persona ==` branch — the four-persona suite ([pipeline.rs:1212-1410](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1212)) is the determinism contract.

### Current code being changed (read before writing)

- **Engine** ([pipeline.rs](../../hifimule-daemon/src/auto_fill/pipeline.rs)): `AutoFillPipeline` :54-77 (add `rarity`/`pity`), `OrderingKey` :155-174 (add `Rarity`; activate `Random` :647-648), `MemoryStage`/`QualityStage` :176-228 (the default-is-noop + parse-tolerant precedent), `PipelineInput` :336-345 (add `seed`/`pity_streak`), `run_pipeline` :369-441 (stable-core pre-pass :398-417 = the reserve template; pass order), `FillMode` :447-451, `compare_by_ordering` :623-678 (+ both call sites :475,478), `is_on_device` :553-560 (discovery = `!is_on_device`), `Selector` :1036-1135, persona suite :1212-1410 (Léo :1321). No change to `Song` — `play_count: Option<u32>` :42 is the only rarity/discovery signal; there is no rating/genre field.
- **Async fetch** ([fetch.rs](../../hifimule-daemon/src/auto_fill/fetch.rs)): `needs_configurable_expansion` :145-180 (+ `rarity_default`/`pity_default`), `expand_with_pipeline` :188-357 (set `input.seed`/`pity_streak` at the `PipelineInput` build :343-347), test builders :790-819.
- **Params** ([mod.rs:55-75](../../hifimule-daemon/src/auto_fill/mod.rs#L55)): add `seed`/`pity_streak`. `AutoFillItem` :32-53 — **no new field needed** (pity reset is per-server, not per-track).
- **RPC** ([rpc.rs](../../hifimule-daemon/src/rpc.rs)): `now_unix_secs` :3574, `build_autofill_history` :3584-3608 (+ pity read), fill sites :2646-2666 / :3999-4011 (+ `seed`), `record_autofill_history_after_sync` :3631-3790 (reset/increment block, template at :3771-3789), `patch_delta_tiers` :3613-3625 (tier precedent, not reused here).
- **DB** ([db.rs](../../hifimule-daemon/src/db.rs)): migration block :220-247 (`autofill_history` :221, `autofill_rotation` :238 — add `autofill_pity`), accessors `upsert/get/prune_autofill_history` :784-848, `get/advance_rotation_cursor` :850-880, tests :953-1009.
- **Frontend**: [state/autoFill.ts:6-7,60-73,99-185](../../hifimule-ui/src/state/autoFill.ts#L6); [AutoFillPanel.ts:246-340,376-443](../../hifimule-ui/src/components/AutoFillPanel.ts#L246).
- **i18n**: [catalog.json](../../hifimule-i18n/catalog.json) locale blocks at en :2, fr :350, es :698, de :1046; `basket.autofill.*` snake_case keys.

### Architecture compliance (non-negotiable)

- Config in the manifest, runtime state (streak/seed) in the DB / caller — never crossed ([architecture.md:922](../../_bmad-output/planning-artifacts/architecture.md#L922)).
- Reuse Epic 12/13 types; only **add** `RarityStage`/`PityStage`/`OrderingKey::Rarity` and the `seed`/`pity_streak` fields. Do not redefine `AutoFillPipeline`/`Song`/`Candidate`.
- Per-server routing & the legacy fast path untouched; the pure engine still never sees a provider. The `ordering` open-extension point already anticipates new keys ([architecture.md:800](../../_bmad-output/planning-artifacts/architecture.md#L800)).
- `autofill_pity` keyed by **portable** `server_id`, same as `autofill_history`/`autofill_rotation` ([db.rs:217-218,235](../../hifimule-daemon/src/db.rs#L217)).

### Previous story intelligence (13.1 / 13.2 / 13.3)

- **13.1** established the DB-history + rotation-cursor pattern this story extends: `build_autofill_history` reads DB → `AutoFillParams` → `PipelineInput`; `record_autofill_history_after_sync` writes/advances at sync completion gated on `servers_wrote`. The pity counter is a **direct sibling** of the rotation cursor — read it in the same builder, advance/reset it in the same recorder block. Mirror its best-effort (never-fail-the-sync) discipline.
- **13.2/13.3** proved: new `OrderingKey` variants are additive comparator arms; non-legacy `ordering` auto-routes (gate is verify-only for keys); ordering keys are label-only/data-driven in the UI; default-equivalent stages must round-trip byte-identically (omit-when-default). Follow all four.
- **Sandbox caveat (recurring):** full `rtk cargo test -p hifimule-daemon` may not finish where mockito/networking is blocked. Your new engine tests are pure (`auto_fill::`) and the DB tests are local SQLite (`db::`) — run targeted: `rtk cargo test -p hifimule-daemon auto_fill:: db::`.

### Latest technical context

- `rand = "0.8"` already a dep — usable for uniform helpers, but the seeded draw is cleanest as an explicit stateless `u^(1/w)` per-element key (no captured RNG). Rust edition 2024 (`f64::total_cmp`, let-chains both available).
- No new crate, no network, no clock inside the pure core. The only `SystemTime` read stays in `now_unix_secs` (RPC), feeding both `now` and the new `seed`.

### Project Structure Notes

- Daemon (Rust): engine in `auto_fill/pipeline.rs`; async/routing in `auto_fill/fetch.rs`; params in `auto_fill/mod.rs`; DB in `db.rs`; RPC wiring in `rpc.rs`. Binary crate (no `lib.rs`); tests are `#[cfg(test)] mod tests` in each file. `OrderingKey`/`RarityStage`/`PityStage` live in `pipeline.rs` — **not** `domain/models.rs` (provider-neutral entities only).
- Frontend (TS): `hifimule-ui/src/state/autoFill.ts`, `components/AutoFillPanel.ts`; i18n `hifimule-i18n/catalog.json`. No UI unit-test framework — `tsc` + build + manual preview.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Epic-13 (lines 3079-3105; Story 13.4 line 3095-3097: weighted rarity draws #29, pity timer #30)]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-06-14-configurable-auto-fill.md (FR54 line 82 "rarity draws, pity timer"; Epic 13 table line 113; conscious cuts — skip-based negative feedback / smart refill triggers line 117)]
- [Source: _bmad-output/brainstorming/brainstorming-session-2026-06-12-1.md (#29 line 68 loot-table draw; #30 line 99 "self-adjusts from behavior, guaranteed finds after dry spells"; "loot-table mechanics … as the memorable-delight layer for later" line 147; pipeline grid "rarity" Ordering axis line 138)]
- [Source: _bmad-output/planning-artifacts/architecture.md#Auto-Fill-Pipeline-Model (lines 788-826; ordering open-extension list line 800; runtime-state-in-DB line 809-812); #Enforcement (lines 920-922, config-in-manifest / history-in-DB)]
- [Source: _bmad-output/implementation-artifacts/13-1-memory-and-rotation-strategies.md (DB-history + rotation-cursor wiring; build/record pattern; tier propagation precedent; best-effort discipline; sandbox caveat)]
- [Source: _bmad-output/implementation-artifacts/13-3-curation-and-discovery-sources.md (OrderingKey arm pattern; routing-via-ordering_default; label-only/data-driven UI; serialize-verbatim vs omit-when-default; persona discipline; i18n parity gate + missing parity-test note)]
- [Source: hifimule-daemon/src/auto_fill/pipeline.rs:12-27,54-77,155-228,336-345,369-441,447-451,553-560,623-678,1036-1135,1212-1410 (Léo :1321)]
- [Source: hifimule-daemon/src/auto_fill/fetch.rs:145-180,188-357,790-819]
- [Source: hifimule-daemon/src/auto_fill/mod.rs:32-75]
- [Source: hifimule-daemon/src/rpc.rs:2646-2666,3574-3608,3631-3790,3999-4011]
- [Source: hifimule-daemon/src/db.rs:217-247,784-880,953-1009]
- [Source: hifimule-daemon/src/domain/models.rs:26-50 (Song.play_count :42; no rating/genre field)]
- [Source: hifimule-ui/src/state/autoFill.ts:6-7,60-73,99-185; components/AutoFillPanel.ts:246-340,376-443; hifimule-i18n/catalog.json (en :2 / fr :350 / es :698 / de :1046)]

## Dev Agent Record

### Agent Model Used

{{agent_model_name_version}}

### Debug Log References

### Completion Notes List

### File List
