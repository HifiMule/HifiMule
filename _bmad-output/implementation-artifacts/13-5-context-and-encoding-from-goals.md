---
baseline_commit: 6b51767f93383a476f8d31f53c2853a3ac0e6e2e
---

# Story 13.5: Context & Encoding-From-Goals

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a HifiMule user whose listening changes with the **clock and the calendar** — energetic mornings, calm evenings, a Christmas-only mood in December — and who wants the daemon to **hit a listening-hours goal inside a byte budget by choosing the right encoding** instead of guessing a bitrate,
I want the auto-fill pipeline to support a **clock-driven Context stage** (time-of-day / seasonal source-activation and scheduled tag filters — the playlist/tag *proxy* versions) and an **encoding-from-goals budget mode** that derives the transcode bitrate from my duration + size targets,
so that fills follow my daily and yearly rhythm and reliably reach my hours target within my space limit — while the selection core stays **deterministic-given-its-inputs**, budget-respecting, per-server, and clock-free in its pure layer.

This story extends the **pure-function pipeline engine** (Epic 12 → 13.1/13.2/13.3/13.4) with the **Context axis** the brainstorm placed last in the pipeline grid (`Filter → Sources → Unit → Ordering → Memory → Budget → Context`, [brainstorm:138](../brainstorming/brainstorming-session-2026-06-12-1.md)) plus the **encoding-from-goals** corner of the Budget system. It delivers the brainstorm's **Context Strategies** — **#3 Time-of-Day**, **#17 Energy-Curve**, **#32 Seasonal Drift** — as their explicitly-prescribed **cheap proxies** (playlist activation + scheduled tag filter), and **#20 Encoding-from-Goals** ("encoding computed backwards from size/duration goals"), which the epic gates on transcode-on-sync (now shipped).

### What makes this story different from 13.4 (read first)

13.4 introduced the engine's **first entropy** (a caller-supplied `seed`) and its **second DB counter** (`autofill_pity`). 13.5 introduces the engine's **first civil-time awareness** and its **first cross into the transcode/sync layer** — but it does so by **reusing the exact two disciplines 13.4 established**, not inventing new ones:

1. **Clock-as-value, not clock-read (the Context stage).** The engine's defining invariant ([pipeline.rs:12-27, 449-452](../../hifimule-daemon/src/auto_fill/pipeline.rs#L12)) is *"no network, no `async`, no `MediaProvider`, no clock/RNG read inside this core."* Time-of-day and seasonal selection obviously need a clock — but exactly like `seed` and `now`, **the clock enters as a value**, never as a read. `HistorySnapshot::now: i64` already carries Unix seconds ([pipeline.rs:405-409](../../hifimule-daemon/src/auto_fill/pipeline.rs#L405)). This story carries the **local civil fields** (`hour`, `month`, `day`) the same way — minted once at the impure RPC layer (the only place `now_unix_secs()` reads the clock, [rpc.rs:3595-3600](../../hifimule-daemon/src/rpc.rs#L3595)) and consumed purely. Same `(input, civil-time, seed, pipeline)` ⇒ byte-identical output. **Never call `Local::now()`, `SystemTime::now()`, or any clock inside `pipeline.rs`/`fetch.rs` selection** — the civil fields already carry everything.

2. **Pure budget math + one impure wiring point (encoding-from-goals).** The bitrate derivation `target_kbps = effective_bytes × 8 / (target_duration_secs × 1000)` is **pure arithmetic the engine already has both operands for** (the `BudgetStage` carries `max_bytes`, `headroom_bytes`, and `target_duration_secs`). The engine uses it to make the **byte estimate bitrate-aware** so the fill packs to the duration goal within the byte goal. The **only** impure piece is feeding that same derived bitrate to the transcode path at sync as a per-slot `max_bitrate_kbps` override — mirroring how `seed` is minted impurely but consumed purely.

So this story's *new* surface is: one pure `ContextStage`, caller-supplied civil-time fields, one `BudgetStage` flag, and one per-slot transcode-bitrate override at sync. **No new DB table** (contrast 13.1/13.4). No new entropy.

### Scope decision — the *cheap proxy* versions, exactly as the brainstorm prescribes (read second)

Every Context idea in the brainstorm ships with a **deliberate cheap version**, because the "smart" version needs a signal `Song` does not have:

- **#3 Time-of-Day** — *"energetic morning slots, calm evening slots"* ([brainstorm:71](../brainstorming/brainstorming-session-2026-06-12-1.md)). There is **no energy/BPM/mood field on `Song`** ([domain/models.rs:26-50](../../hifimule-daemon/src/domain/models.rs#L26)) — `play_count`/`is_favorite`/`duration`/`bitrate` are the only signals. The cheap, feasible version is **the playlist proxy**: the user maps time-of-day windows to **source activation/weighting** (a "morning" playlist active 6–11, an "evening" playlist active 18–23).
- **#17 Energy-Curve** — *"session-shaped … via BPM/energy **or playlist proxy**"* ([brainstorm:72](../brainstorming/brainstorming-session-2026-06-12-1.md)). Same missing signal ⇒ the **playlist-proxy** version: time-windowed/weighted phase sources. There is no BPM-driven curve here; it folds into the same time-window source-weighting mechanism as #3.
- **#32 Seasonal Drift** — *"high effort; **cheap version = scheduled tag filter**"* ([brainstorm:73](../brainstorming/brainstorming-session-2026-06-12-1.md)). The cheap version is a **calendar-windowed filter/source**: include genre "Christmas" only in December; activate a "Summer" playlist June–August.

**The unifying mechanism is one `ContextStage`**: an ordered list of **context rules**, each a *time predicate* (a time-of-day hour window and/or a calendar month/date window, evaluated purely against the caller-supplied civil-time) plus an *effect* (activate/weight named sources, and/or contribute scheduled include/exclude tags+genres). This single stage expresses all three cheap proxies with no special cases — the same "four-personas-one-model" discipline that gated 12.1.

**#20 Encoding-from-Goals** — *"encoding computed backwards from size/duration goals (depends on transcode-on-sync)"* ([brainstorm:114,166](../brainstorming/brainstorming-session-2026-06-12-1.md)). Transcode-on-sync **is shipped** (device profiles → `TranscodeProfile { container, audio_codec, max_bitrate_kbps }`, [providers/mod.rs:355-359](../../hifimule-daemon/src/providers/mod.rs#L355); selected per device via `transcoding_profile_id`, applied at sync via `load_selected_transcoding_profile`, [rpc.rs:2319-2339, 4940](../../hifimule-daemon/src/rpc.rs#L2319)). So #20 is delivered as a **budget-derived per-slot bitrate**: when enabled, the engine derives the target bitrate from the slot's existing `target_duration_secs` + byte ceiling and packs against a bitrate-aware byte estimate; at sync, that derived bitrate overrides `max_bitrate_kbps` on the selected profile **for that slot's auto-fill downloads only**.

**Consciously DEFERRED (bright lines — do NOT implement here):**
- **Any BPM/energy/mood-signal-driven** energy curve or "smart" time-of-day — no such signal exists on `Song`; the cheap playlist/tag proxy is the whole delivery. [brainstorm:71-72]
- **Device-side "zones"** (multiple device folders per time slot, [brainstorm:71](../brainstorming/brainstorming-session-2026-06-12-1.md)) — the device folder model is out of the pipeline's scope; context drives *selection*, not on-device layout.
- **External/holiday calendars** for seasonal — only user-configured month/date windows; no network calendar lookup (keeps the engine pure & offline).
- **A full Device-Profile-Editor UI with an encoding policy panel** ([brainstorm:114](../brainstorming/brainstorming-session-2026-06-12-1.md)) — #20 here is the *budget-derived bitrate* mechanism + minimal config toggle; the standalone profile-editor surface is a separate UX effort.
- **Anything needing ratings / play-feedback** — already deferred in 13.3/13.4 and unrelated here.

This is the same discipline that landed 13.1–13.4 cleanly: **deliver the deterministic cheap core of the idea; defer anything that needs a signal or subsystem we don't have.**

## Acceptance Criteria

### A — Civil-time foundation (clock-as-value; enables #3/#17/#32; engine stays pure)

1. **Caller-supplied local civil-time carries all clock awareness; the engine stays pure.** Add local civil fields to the engine input alongside `now`. Recommended: extend [`HistorySnapshot`](../../hifimule-daemon/src/auto_fill/pipeline.rs#L405-L409) (the existing home for "the caller's notion of time") with a small caller-supplied struct, e.g. `pub local: CivilTime` where `CivilTime { hour: u8 /*0..=23*/, month: u8 /*1..=12*/, day: u8 /*1..=31*/, weekday: u8 /*0=Mon..=6*/ }`, `#[derive(Default)]` ⇒ all-zero. The pure engine derives **every** time-of-day / calendar decision from these fields — there is **no** `Local::now`, `Utc::now`, `SystemTime`, or `chrono` call anywhere in `pipeline.rs` or the `fetch.rs` selection path. Same `(input, pipeline)` ⇒ identical `Vec<AutoFillItem>`. This mirrors exactly how `seed`/`now` are carried. [Source: clock-as-value discipline pipeline.rs:12-27,403-409,449-452; seed precedent pipeline.rs:420-424; PipelineInput pipeline.rs:412-429]

2. **The impure RPC layer mints civil-time from system local time (the only clock read).** At the two engine fill sites the civil fields are computed once from the system's **local** time and threaded through `AutoFillParams` → `HistorySnapshot::local`. Add a dependency capable of local civil time — recommend **`chrono` with its default `clock` feature** (`chrono::Local::now()` uses `localtime_r`, giving a sound local offset; the `time` crate's `local-offset` feature carries a documented multi-thread soundness caveat — prefer chrono). Add a sibling helper to `now_unix_secs()`, e.g. `fn now_civil() -> CivilTime`, in `rpc.rs` (the single clock-reading module for auto-fill). Default/legacy paths set `CivilTime::default()` (inert — see AC 4). [Source: now_unix_secs single-clock-read rpc.rs:3595-3600; fill sites rpc.rs:2652-2675, 4049-4063; no time crate yet — Cargo.toml has only `rand = "0.8"` :34]

### B — Context stage (#3 Time-of-Day, #17 Energy-Curve, #32 Seasonal — cheap proxies)

3. **New `ContextStage` config.** Add `pub context: ContextStage` to [`AutoFillPipeline`](../../hifimule-daemon/src/auto_fill/pipeline.rs#L52-L84) (`#[serde(default)]`, camelCase), shape:
   ```rust
   pub struct ContextStage {
       pub enabled: bool,            // off ⇒ zero behavior change
       pub rules: Vec<ContextRule>,  // evaluated in order against HistorySnapshot::local
   }
   pub struct ContextRule {
       pub window: ContextWindow,            // when this rule is active
       pub source_refs: Vec<String>,         // ref_ids of pipeline sources this rule activates/boosts
       pub weight: Option<f32>,              // optional share multiplier for activated sources (energy phases)
       pub include_tags: Vec<String>,        // scheduled filter additions while active (seasonal)
       pub exclude_tags: Vec<String>,
       pub include_genres: Vec<String>,
       pub exclude_genres: Vec<String>,
   }
   pub enum ContextWindow {
       TimeOfDay { start_hour: u8, end_hour: u8 },   // #3/#17: inclusive start, exclusive end; wraps past midnight if start>end
       Months { months: Vec<u8> },                   // #32: e.g. [12] = December; [6,7,8] = summer
       DateRange { start: (u8, u8), end: (u8, u8) },  // #32: (month,day)..=(month,day); wraps across year-end
   }
   ```
   All-default (`enabled:false`, no rules) ⇒ today's behavior, exactly like `QualityStage::default()`/`RarityStage::default()`. Use the **parse-tolerant** discipline (like 13.2's `deserialize_version_preference` and 13.1's `parse_tiers`): a malformed rule/window degrades to "no effect", never aborts the pipeline parse. [Source: AutoFillPipeline field-add precedent (rarity/pity) pipeline.rs:77-83; default-is-noop QualityStage pipeline.rs:231-245; parse-tolerance pipeline.rs:247-265]

4. **A rule is *active* iff its window matches the caller-supplied civil-time; `enabled:false` ⇒ total no-op.** Implement a pure `fn context_rule_active(rule: &ContextRule, local: &CivilTime) -> bool`:
   - `TimeOfDay { start_hour, end_hour }`: `start_hour <= local.hour < end_hour`, with **midnight wrap** when `start_hour > end_hour` (e.g. 22→6 means "active if hour ≥ 22 OR hour < 6").
   - `Months { months }`: `months.contains(&local.month)`.
   - `DateRange { start, end }`: `(month,day)` within `[start, end]` inclusive, with **year-end wrap** when `start > end` (e.g. Dec 15 → Jan 5).
   When `context.enabled` is false, **no rule is ever consulted** and behavior is byte-identical to today. A `CivilTime::default()` (all-zero ⇒ month 0, day 0) matches no `Months`/`DateRange` rule and matches a `TimeOfDay` only at hour 0 — acceptable because the stage is gated on `enabled`. [Source: pure-predicate style mirrors `is_on_device` pipeline.rs:553-560 / cooldown eval in Memory]

5. **Active rules drive source activation/weighting (#3/#17) — the playlist proxy.** When `context.enabled`, before source-cap computation in `run_pipeline` ([pipeline.rs:469-543](../../hifimule-daemon/src/auto_fill/pipeline.rs#L469)), the effective source set is adjusted by the active rules:
   - A source whose `ref_id` is named in **any active rule's** `source_refs` is **retained**; a source named **only** in rules whose windows are **all inactive** is **suppressed** for this run (so a "morning playlist" listed in a `TimeOfDay 6-11` rule contributes nothing at 20:00). A source named in **no** context rule is **always active** (context only gates the sources rules mention — it never silently drops un-mentioned sources).
   - An active rule's `weight` (default `1.0`) **multiplies the share** of its activated sources when `source_caps` are computed ([pipeline.rs:1201+ `source_caps`](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1201)) — this is the energy-curve phase emphasis. Multiple active rules touching the same source compose by taking the **max** weight (documented choice; avoids unbounded products).
   This composes with the existing core/pity/primary/fallback passes by adjusting only which sources each pass iterates and their caps; the budget/dedup guarantees are untouched. [Source: source-iteration loops pipeline.rs:494-543; source_caps pipeline.rs:1201+]

6. **Active rules contribute a scheduled filter (#32) — the tag/genre proxy.** When `context.enabled`, the **effective `FilterStage` for this run** is the pipeline's static `filter` ([pipeline.rs:59,93-98](../../hifimule-daemon/src/auto_fill/pipeline.rs#L93)) **unioned** with the `include_tags`/`exclude_tags`/`include_genres`/`exclude_genres` of every **active** rule (exclude wins over include on conflict, matching existing filter semantics). An inactive rule contributes nothing. With `context.enabled:false`, the effective filter is exactly the static `filter` (today's behavior). The filter is applied per-candidate in `build_source_units` ([pipeline.rs:574+](../../hifimule-daemon/src/auto_fill/pipeline.rs#L574)) against the caller-attached `Candidate.genres`/`Candidate.tags` ([pipeline.rs:361-368](../../hifimule-daemon/src/auto_fill/pipeline.rs#L361)) — **reuse the existing filter application**, only the *effective filter value* changes. [Source: FilterStage pipeline.rs:86-98; Candidate genres/tags pipeline.rs:361-368; existing filter application in build_source_units]

### C — Encoding-from-goals (#20): bitrate derived from size + duration goals

7. **New `BudgetStage.encoding_from_goals` flag drives a pure target-bitrate derivation.** Add `pub encoding_from_goals: bool` (`#[serde(default)]`, camelCase) to [`BudgetStage`](../../hifimule-daemon/src/auto_fill/pipeline.rs#L209-L215). Add a pure `fn target_bitrate_kbps(budget: &BudgetStage) -> Option<u32>`: when `encoding_from_goals && max_bytes.is_some() && target_duration_secs.is_some_and(|s| s>0)`, return `Some(clamp(effective_bytes × 8 / (target_duration_secs × 1000)))` where `effective_bytes = budget_ceiling(budget)` ([pipeline.rs:1192-1199](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1192), i.e. `max_bytes - headroom`); clamp to a sane audio range (e.g. `32..=320` kbps) so a tiny/huge goal can't produce a nonsense encode. Returns `None` (no derivation) when the flag is off or either goal is missing — **today's behavior**. Pure, unit-testable, no I/O. [Source: BudgetStage pipeline.rs:209-215; budget_ceiling pipeline.rs:1192-1199; Selector duration_target pipeline.rs:1235-1280]

8. **The byte estimate becomes bitrate-aware when encoding-from-goals is active, so the fill packs to the duration goal.** `estimated_size` ([pipeline.rs:1177-1190](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1177)) today prefers `Song.size_bytes` then derives from source `bitrate_kbps`. When `target_bitrate_kbps(&pipeline.budget)` is `Some(b)`, the engine must estimate each candidate's **post-transcode** size as `min(source_estimate, b×1000/8 × duration_seconds)` — never larger than the source (transcoding down only shrinks; a source already below the target is unchanged). Thread the optional target bitrate into the size estimation used by the `Selector` so the budget math and the duration target stay self-consistent (target hours fit the byte ceiling at the derived bitrate). When `None`, `estimated_size` is byte-for-byte unchanged. [Source: estimated_size pipeline.rs:1177-1190; its callers `can_ever_fit` :1084-1090 and Selector size use :1308; Selector::new(ceiling, duration_target, …) :1246]

9. **At sync, the derived bitrate overrides the transcode profile for that slot's auto-fill downloads only.** The same `target_bitrate_kbps` (computed from the slot's pipeline budget) is applied as a **per-slot `max_bitrate_kbps` override** on the `TranscodeProfile` used for that server's auto-fill tracks — layering on top of the user-selected device profile's `container`/`audio_codec` ([providers/mod.rs:355-359](../../hifimule-daemon/src/providers/mod.rs#L355); profile selected via `load_selected_transcoding_profile`, [rpc.rs:2319-2339](../../hifimule-daemon/src/rpc.rs#L2319), applied at sync [rpc.rs:4940-4942, 5081-5082, 5207-5208](../../hifimule-daemon/src/rpc.rs#L4940)). Scope the override to **auto-fill items of the slot whose pipeline set the flag** — do **not** mutate the device-wide `transcoding_profile_id` (one device may have several slots with different budgets; manual selections are unaffected). If no transcode profile is selected (`passthrough`), encoding-from-goals still informs the byte estimate (AC 8) but cannot force a re-encode — document that the byte estimate then over-reserves vs the actual passthrough size, and gate the bitrate-aware estimate on "a transcoding profile is active" so estimate and reality stay aligned. [Source: transcode selection/application rpc.rs:2319-2339,4940,5081,5207; per-device transcoding_profile_id rpc.rs:1934,5768-5791]

### D — Routing, params/RPC wiring, UI, i18n, scope

10. **The configurable path recognizes the new stage and flag.** Extend `needs_configurable_expansion` ([fetch.rs:145-187](../../hifimule-daemon/src/auto_fill/fetch.rs#L145)) with `context_default` (`p.context == ContextStage::default()`) ANDed into the discriminator alongside `quality_default`/`rarity_default`/`pity_default`. For `encoding_from_goals`: it only matters together with a duration target, which **already** forces the configurable path via `budget_default` ([fetch.rs:172-175](../../hifimule-daemon/src/auto_fill/fetch.rs#L172) — any `target_duration_secs > 0` is non-legacy); **verify with a test** that an `encoding_from_goals` pipeline routes to the engine, and add an explicit clause if a future default could slip through. A default-legacy pipeline still takes the fast path. [Source: discriminator fetch.rs:145-187; quality/rarity/pity precedents :166-171]

11. **`AutoFillParams` + RPC wiring threads civil-time and the per-slot transcode bitrate.**
    - **Civil-time:** add `pub local: CivilTime` (or the chosen carrier) to [`AutoFillParams`](../../hifimule-daemon/src/auto_fill/mod.rs#L55-L81); set it from `now_civil()` at the **two engine fill sites** ([rpc.rs:2664-2675](../../hifimule-daemon/src/rpc.rs#L2664), [rpc.rs:4055-4063](../../hifimule-daemon/src/rpc.rs#L4055)) and `CivilTime::default()` on the **legacy/Jellyfin paths** (the same sites that set `seed: now as u64` vs `seed: 0` — there are five `AutoFillParams` construction sites plus two in `main.rs`, per 13.4's File List). Thread into `HistorySnapshot::local` inside `expand_with_pipeline` ([fetch.rs:189-357](../../hifimule-daemon/src/auto_fill/fetch.rs#L189), at the `PipelineInput` build). [Source: 13.4 wiring of seed/pity_streak across all sites — story 13.4 AC 9 / File List]
    - **Transcode bitrate:** at the sync expansion path, after computing each slot's items, derive `target_bitrate_kbps` from that slot's pipeline budget and apply it as the `max_bitrate_kbps` override on the transcode profile used for those downloads (AC 9). Best-effort; never fail the sync.

12. **Configuration UI exposes Context & encoding-from-goals under Advanced.**
    - **Mirror types:** add `ContextStage`/`ContextRule`/`ContextWindow` to the TS `AutoFillPipeline` mirror ([state/autoFill.ts:87-99](../../hifimule-ui/src/state/autoFill.ts#L87)) and `encodingFromGoals?: boolean` to the `BudgetStage` mirror, with **omit-when-default** `normalizePipeline`/`serializePipeline` handling (the pattern `quality`/`rarity`/`pity` use, [state/autoFill.ts:134-185](../../hifimule-ui/src/state/autoFill.ts#L134)) so a default pipeline emits nothing and round-trips byte-identically (default-equivalent must keep the fast path).
    - **Renderers:** add a `renderContextStage` (a small rule editor: per rule a window-type selector + bounds + the source-refs/tags it activates) and an encoding-from-goals checkbox in the budget area, under the Advanced disclosure ([renderAdvanced AutoFillPanel.ts:246-259](../../hifimule-ui/src/components/AutoFillPanel.ts#L246)), modeled on `renderMemoryStage`/`renderQualityStage`/`renderRarityStage` ([AutoFillPanel.ts:266-372](../../hifimule-ui/src/components/AutoFillPanel.ts#L266)), handlers invalidating the debounced live preview like the existing stages. The simple (non-Advanced) default state is unchanged. [Source: state/autoFill.ts:87-185; AutoFillPanel.ts:246-372]

13. **i18n parity across all 4 locales.** Add every new `basket.autofill.*` label/hint key to **all 4 locales** (`en`/`fr`/`es`/`de`), mirroring the existing convention. Current parity is **96×4** ([catalog.json](../../hifimule-i18n/catalog.json), verified). New keys cover: the Context stage (title + enable + rule/window labels: time-of-day start/end, months, date-range, the activate-sources + scheduled-tags fields + a `context_hint`) and the encoding-from-goals toggle (+ `encoding_from_goals_hint` explaining it needs both a size and a duration target). Count the new keys precisely and report the new `N×4`. Keep `rtk cargo test -p hifimule-i18n` green. [Source: 13.4 went 81×4 → 96×4; locale blocks en :2 / fr ~:350 / es ~:698 / de ~:1046]

14. **Backward compatibility & scope boundary.** A pipeline that sets neither `context` (default `enabled:false`, no rules) nor `budget.encoding_from_goals` behaves **exactly** as today — zero migration, fast path intact. `CivilTime` defaults to all-zero and is never consulted unless `context.enabled`. The legacy `run_auto_fill*` path is untouched. **Do NOT** implement, in this story: BPM/energy/mood-signal selection, device "zones"/folder layout, external/holiday calendars, a standalone device-profile-editor UI, play-feedback/ratings (deferred — see Scope decision), or any of 13.6's ideas (Artist Spotlight #33, album/track ratio #8, album promotion #9, coherence #27). **Do NOT** add a DB table — Context is pure-from-civil-time and encoding-from-goals is pure-arithmetic + a transcode override; neither needs runtime DB state. [Source: epics.md:3099-3105 Story 13.5/13.6 boundaries; sprint-change-proposal line 117 conscious cuts]

15. **Build & tests green.** `rtk cargo test -p hifimule-daemon` passes with no regression (sandbox caveat: if mockito/networking is blocked, run targeted `rtk cargo test -p hifimule-daemon auto_fill:: db::`). New tests must cover:
    - **Context purity/determinism:** a fixed `CivilTime` yields a stable, asserted selection; `context.enabled:false` is byte-identical to no-context; two different civil-times (likely) differ when rules gate sources/filter.
    - **Window predicates:** `TimeOfDay` normal + midnight-wrap; `Months`; `DateRange` normal + year-end-wrap; an inactive rule contributes nothing.
    - **Source activation/weighting:** a source named only in an inactive rule is suppressed; a source in no rule always runs; an active rule's `weight` boosts its sources' share; max-compose across overlapping active rules.
    - **Scheduled filter:** an active rule's include/exclude tags+genres union with the static filter; exclude beats include; inactive rules add nothing.
    - **Encoding-from-goals:** `target_bitrate_kbps` returns `None` when off/missing-goal and a clamped value otherwise; the bitrate-aware `estimated_size` shrinks oversize candidates to the derived bitrate, never enlarges a smaller source, and lets a duration goal fit the byte ceiling; the per-slot transcode override is applied (RPC-level test) and does not mutate the device-wide profile.
    - **Routing:** `needs_configurable_expansion` returns `true` for a `context.enabled`-only pipeline and for an `encoding_from_goals`+duration pipeline; `false` for legacy default.
    - **Serde:** round-trip a pipeline carrying `context` rules (all three window kinds) and `encoding_from_goals`.
    - **Persona:** strengthen a persona whose fill should follow the clock/calendar (e.g. **Léo**'s gym "energy" or a seasonal case) so a context-gated source/filter changes the result — behavior must emerge from config, **no `if persona ==` branch** ([pipeline.rs persona suite :1212-1410](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1212)).
    `rtk cargo clippy -p hifimule-daemon --all-targets` adds no new warnings in touched modules. Frontend `rtk npx tsc --noEmit` + `rtk npm run build` stay green; `rtk cargo test -p hifimule-i18n` green.

## Tasks / Subtasks

- [x] **Civil-time foundation (clock-as-value)** (`hifimule-daemon/src/auto_fill/pipeline.rs`, `mod.rs`, `rpc.rs`, `Cargo.toml`) (AC: 1, 2)
  - [x] Add `CivilTime { hour, month, day, weekday }` (`#[derive(Default)]`) and `pub local: CivilTime` to `HistorySnapshot`. No clock call in `pipeline.rs`.
  - [x] Add `chrono` (default `clock` feature) to `hifimule-daemon/Cargo.toml`; add `fn now_civil() -> CivilTime` next to `now_unix_secs()` in `rpc.rs` (the single clock-reading site). Note the `time`-crate `local-offset` soundness caveat in a comment; prefer `chrono::Local::now()`.
  - [x] Add `pub local: CivilTime` to `AutoFillParams`; set from `now_civil()` at the three engine fill sites (basket 2664, multi-server delta, preview), `CivilTime::default()` on legacy/Jellyfin paths + both `main.rs` sites (mirror the seed `now as u64` vs `0` split). Thread into `HistorySnapshot::local` in `expand_with_pipeline`.

- [x] **ContextStage + pure window predicates (#3/#17/#32)** (`hifimule-daemon/src/auto_fill/pipeline.rs`) (AC: 3, 4, 5, 6)
  - [x] Add `ContextStage`/`ContextRule`/`ContextWindow` (camelCase, `#[serde(default)]`, parse-tolerant `deserialize_context_rules`); add `pub context: ContextStage` to `AutoFillPipeline`.
  - [x] Add `fn context_rule_active(rule, local) -> bool` (TimeOfDay + midnight-wrap; Months; DateRange + year-end-wrap).
  - [x] In `run_pipeline`: compute the effective source set (suppress sources only-in-inactive-rules; weight-boost active rules' sources, max-compose) and the effective `FilterStage` (static ∪ active rules). Apply via existing source-cap/filter machinery; budget/dedup untouched. Gate everything on `context.enabled`.
  - [x] Tests: purity/determinism, window predicates (incl. both wraps), source activation/weighting, scheduled-filter union.

- [x] **Encoding-from-goals (#20)** (`hifimule-daemon/src/auto_fill/pipeline.rs`, `rpc.rs`/`sync.rs`) (AC: 7, 8, 9)
  - [x] Add `pub encoding_from_goals: bool` to `BudgetStage`; add pure `fn target_bitrate_kbps(&BudgetStage) -> Option<u32>` (clamped 32..=320).
  - [x] Make `estimated_size` bitrate-aware when a target bitrate is active (`min(source, derived)`); thread the optional target into the Selector's size use. Gate the bitrate-aware estimate on "a transcoding profile is active" (impure layer clears the flag in passthrough).
  - [x] At sync, apply the derived bitrate as a per-slot `max_bitrate_kbps` override on the transcode profile for that slot's auto-fill downloads only (carried per-item via `SyncAddItem.max_bitrate_override_kbps`, patched like tiers); never touch the device-wide `transcoding_profile_id`. Best-effort.
  - [x] Tests: `target_bitrate_kbps` on/off/clamp; bitrate-aware estimate shrinks-not-enlarges + duration fits ceiling; per-slot override applied + scoped to auto-fill items (RPC test), device profile unmutated.

- [x] **Routing gate** (`hifimule-daemon/src/auto_fill/fetch.rs`) (AC: 10)
  - [x] Add `context_default` to `needs_configurable_expansion`; verified `encoding_from_goals`+duration routes via `budget_default`. Discriminator test.

- [x] **Frontend: context stage + encoding toggle** (`hifimule-ui/src/state/autoFill.ts`, `components/AutoFillPanel.ts`) (AC: 12)
  - [x] Mirror `ContextStage`/`ContextRule`/`ContextWindow` + `budget.encodingFromGoals`; omit-when-default normalize/serialize (mirror quality/rarity/pity).
  - [x] `renderContextStage` (rule editor) + encoding-from-goals checkbox under `renderAdvanced`; handlers invalidate the live preview / re-render on structural change.

- [x] **i18n ×4 locales** (`hifimule-i18n/catalog.json`) (AC: 13)
  - [x] Add all 24 new `basket.autofill.*` keys to en/fr/es/de; parity now **120×4** (was 96×4). Catalog tests green.

- [x] **Full verification** (AC: 14, 15)
  - [x] Daemon tests (599 pass); clippy clean on touched modules; i18n green (6); tsc + build green. Strengthened the Léo persona with a clock-gated energy playlist (config-driven, no `if persona` branch).

## Dev Notes

### What this story is (and is not)

It delivers the brainstorm's **Context axis** (#3 time-of-day, #17 energy-curve, #32 seasonal) as their **prescribed cheap proxies** — a single clock-driven `ContextStage` (playlist activation + scheduled tag filter) — and **#20 encoding-from-goals** (budget-derived transcode bitrate). It introduces the engine's **first civil-time awareness** (carried as a value, exactly like `seed`/`now`) and its **first reach into the transcode/sync layer** (a per-slot bitrate override). It adds **no DB table** and **no new entropy**. Everything "smart" that needs a signal `Song` lacks (BPM/energy/mood) or a subsystem we don't build here (device zones, holiday calendars, a profile-editor UI) is **deferred** — see the Scope decision. 13.6 owns the remaining units/promotion ideas.

### Civil-time: how the clock enters a deliberately clock-free engine (the central design)

The engine invariant ([pipeline.rs:12-27,449-452](../../hifimule-daemon/src/auto_fill/pipeline.rs#L12)) forbids a clock read inside the pure core. 13.4 threaded `now`/`seed` as **values**; 13.5 does the identical move for civil time. The pure core consumes `HistorySnapshot::local` (`hour`/`month`/`day`/`weekday`); it never *computes* them. The single place they are minted is `rpc.rs`'s `now_civil()`, right next to the existing `now_unix_secs()` — the only clock-reading function in the auto-fill path. **If you feel the urge to call `Local::now()` inside `pipeline.rs` or `fetch.rs` selection, stop** — the civil fields already carry everything, and the entire `auto_fill::` test suite stays a pure fixture suite (pass a fixed `CivilTime`, assert an exact selection).

**Why local, not UTC:** "morning" and "December" are civil-local concepts. `now` (Unix seconds, UTC) can't answer "is it morning?" without an offset. Hence the impure layer computes *local* civil fields. No `time`/`chrono` crate exists yet ([Cargo.toml:34](../../hifimule-daemon/Cargo.toml#L34) has only `rand`); add `chrono` with its default `clock` feature (`Local::now()` → `localtime_r`, a sound local offset). Avoid the `time` crate's `local-offset` feature — it returns `None`/errors in multi-threaded processes by design (a documented soundness guard), and the daemon is multi-threaded.

### The Context stage is one mechanism for three ideas (avoid three special cases)

All three Context ideas reduce to **"when condition C holds (per the clock/calendar), prefer these sources / apply this filter."** Resist building three stages. One `ContextStage` of ordered rules — each a window predicate + an effect — expresses time-of-day phases (#3/#17, `TimeOfDay` windows, optional `weight` for energy emphasis) and seasonal drift (#32, `Months`/`DateRange` windows with scheduled tags). This is the same algebra-minimalism that gated 12.1 ("four personas, one model"). The effect only ever (a) gates/weights which **already-configured** sources run and (b) augments the **effective filter** — it never invents candidates, so every budget/dedup/memory guarantee downstream is unchanged.

**Source gating rule to get right:** context gates only sources its rules *mention*. A source named in **no** context rule always runs (context is additive intent, not a whitelist). A source mentioned **only** in currently-inactive rules is suppressed this run. A source mentioned in **any** active rule runs (and gets the max active weight). Unit-test all three cases — getting "un-mentioned sources still run" wrong would silently break every pipeline that adds a context rule.

### Encoding-from-goals: pure math, one impure override (the cross-layer piece)

The derivation is pure arithmetic the engine already has the operands for: `target_kbps = (max_bytes − headroom) × 8 / (target_duration_secs × 1000)`, clamped to a sane audio range. Two consumers:
1. **Pure (budget honesty):** make `estimated_size` ([pipeline.rs:1177-1190](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1177)) bitrate-aware — `min(source_estimate, derived_bytes_per_sec × duration)`. Transcoding only shrinks; never enlarge a source already below target. This keeps "X hours fits Y bytes" self-consistent with the `Selector`'s `duration_target` ([pipeline.rs:1235-1280](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1235)).
2. **Impure (the actual encode):** apply the same `target_kbps` as `TranscodeProfile.max_bitrate_kbps` ([providers/mod.rs:355-359](../../hifimule-daemon/src/providers/mod.rs#L355)) for that slot's auto-fill downloads only.

**Per-slot, not device-wide.** The device has one `transcoding_profile_id` ([rpc.rs:1934,5768-5791](../../hifimule-daemon/src/rpc.rs#L1934)) but possibly several auto-fill slots with different budgets. Override `max_bitrate_kbps` only for the slot's downloads; never write the device-wide profile, never touch manual selections. **Passthrough caveat:** if no transcode profile is selected, the bytes can't actually be re-encoded — gate the bitrate-aware *estimate* on "a transcoding profile is active" so the estimate never claims a track will shrink when it won't. Document this clearly; it's the one place estimate and reality could diverge.

### Routing gate (`needs_configurable_expansion`) — add `context_default`, verify encoding

New pipeline stages **always** need a `*_default` clause in the discriminator or a configured pipeline silently takes the legacy fast path (which can't run context). Add `context_default` (mirror `quality_default`/`rarity_default`/`pity_default`, [fetch.rs:166-171](../../hifimule-daemon/src/auto_fill/fetch.rs#L166)). `encoding_from_goals` is only meaningful with a `target_duration_secs`, which **already** forces the engine path via `budget_default` ([fetch.rs:172-175](../../hifimule-daemon/src/auto_fill/fetch.rs#L172)) — that part is verify-only (add a test; add an explicit clause only if you find a default that slips through).

### Frontend patterns (from 13.1–13.4)

- **Object stages need omit-when-default serialize** — `context` (object with `enabled:false`/no rules) and `budget.encodingFromGoals` (false) must emit nothing when default so the JSON matches the daemon serde and a default pipeline round-trips byte-identically (keeping the fast path). Copy `quality`/`rarity`/`pity`'s normalize/serialize ([state/autoFill.ts:134-185](../../hifimule-ui/src/state/autoFill.ts#L134)).
- **No UI unit-test framework** — rely on `tsc` + `build` + manual preview. New renderers go under the Advanced disclosure ([AutoFillPanel.ts:246-259](../../hifimule-ui/src/components/AutoFillPanel.ts#L246)); model handlers on `renderMemoryStage`/`renderQualityStage`/`renderRarityStage` and invalidate the debounced live preview.
- The context **rule editor** is more complex than prior stages (a list of rules, each with a window-type discriminated union). Keep it minimal and data-driven; seed sensible defaults on "add rule" (e.g. a `TimeOfDay 6-11` morning rule) and validate hour 0–23 / month 1–12 in the handlers.

### i18n parity (currently 96×4 — hard gate)

Every new key in all 4 locales or the catalog test breaks. Count this story's keys precisely and report the new total. **Pre-existing gap (do not block on it, flagged in 13.3 & 13.4 reviews):** there is still **no automated all-locale key-parity test** in `hifimule-i18n` (only 6 per-translation tests; `translate` silently falls back to English then the raw key). Verify parity by hand/script. Adding a real parity test is a worthwhile out-of-scope cleanup — flag it, don't expand this story for it.

### Persona suite is the engine acceptance bar

The four-persona suite ([pipeline.rs:1212-1410](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1212)) is the determinism contract. Léo (gym/energy, [pipeline.rs:1321](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1321)) is the natural home for a time-of-day/energy assertion; a seasonal case fits any persona. Strengthen one so a context-gated source or scheduled filter demonstrably changes the result under a fixed `CivilTime`, and is inert under `enabled:false`. Behavior must emerge from config — **never** an `if persona ==` branch.

### Storage split & architecture compliance (non-negotiable)

- **Config** (context rules, `encoding_from_goals`) → **manifest** pipeline, portable, per `(device, serverId)`, round-tripping through serde camelCase like every stage. **Runtime time** (civil fields) → **caller-supplied** at the impure layer, never persisted. **No DB table** this story. [Source: storage split architecture.md:920-922]
- The `ordering`/stage model is an **open extension point** ([architecture.md:788-826](../../_bmad-output/planning-artifacts/architecture.md#L788), ordering list :800); `context` is a new optional stage in the same spirit — additive, default-noop.
- Per-server routing & the legacy fast path untouched; the pure engine still never sees a provider or a clock. The transcode override rides the **existing** `TranscodeProfile`/`load_selected_transcoding_profile` machinery — do not build a new transcode path.
- Reuse Epic 12/13 types; only **add** `ContextStage`/`ContextRule`/`ContextWindow`/`CivilTime`, the `BudgetStage.encoding_from_goals` flag, and the `AutoFillParams.local`/`HistorySnapshot.local` fields. Do not redefine `AutoFillPipeline`/`Song`/`TranscodeProfile`/`BudgetStage`.

### Current code being changed (read before writing)

- **Engine** ([pipeline.rs](../../hifimule-daemon/src/auto_fill/pipeline.rs)): `AutoFillPipeline` :52-84 (add `context`), `BudgetStage` :209-215 (add `encoding_from_goals`), `FilterStage` :86-98, `OrderingKey` :160-188 (no change — context is not an ordering key), `HistorySnapshot` :405-409 (add `local`), `PipelineInput` :412-429, `run_pipeline` :453-552 (effective-sources/effective-filter computation; pass order unchanged), `build_source_units` :574+ (existing filter application — only the value changes), `estimated_size` :1177-1190 (+ bitrate-aware), `budget_ceiling` :1192-1199, `source_caps` :1201+ (weight multiply), `Selector` :1235-1349 (`duration_target` :1237), persona suite :1212-1410 (Léo :1321). `Song` has **no** genre/BPM/energy field — context filters use caller-attached `Candidate.genres`/`tags` :361-368.
- **Async fetch** ([fetch.rs](../../hifimule-daemon/src/auto_fill/fetch.rs)): `needs_configurable_expansion` :145-187 (+ `context_default`; verify encoding via `budget_default` :172-175), `expand_with_pipeline` :189-357 (set `HistorySnapshot::local` at the `PipelineInput` build), test builders ~:790-819.
- **Params** ([mod.rs:55-81](../../hifimule-daemon/src/auto_fill/mod.rs#L55)): add `local: CivilTime`. `AutoFillItem` :32-53 — no new field needed.
- **RPC** ([rpc.rs](../../hifimule-daemon/src/rpc.rs)): `now_unix_secs` :3595-3600 (+ sibling `now_civil`), fill sites :2652-2675 / :4049-4063 (+ `local`, + per-slot transcode bitrate), `build_autofill_history` :3605+ (civil-time is from `now`, not the DB — set it at the call site), transcode selection `load_selected_transcoding_profile` :2319-2339 and application :4940-4942/5081-5082/5207-5208, device `transcoding_profile_id` :1934/5768-5791 (do NOT mutate).
- **Providers** ([providers/mod.rs:355-359](../../hifimule-daemon/src/providers/mod.rs#L355)): `TranscodeProfile { container, audio_codec, max_bitrate_kbps }` — set `max_bitrate_kbps` for the per-slot override.
- **Cargo** ([Cargo.toml:34](../../hifimule-daemon/Cargo.toml#L34)): add `chrono` (default `clock` feature).
- **Frontend**: [state/autoFill.ts:87-185](../../hifimule-ui/src/state/autoFill.ts#L87) (mirror types + normalize/serialize), [AutoFillPanel.ts:246-372,481-483](../../hifimule-ui/src/components/AutoFillPanel.ts#L246) (renderers).
- **i18n**: [catalog.json](../../hifimule-i18n/catalog.json) locale blocks en :2 / fr ~:350 / es ~:698 / de ~:1046; `basket.autofill.*` snake_case keys (96/locale today).

### Previous story intelligence (13.1 / 13.2 / 13.3 / 13.4)

- **13.4** established the **clock/entropy-as-value** discipline this story copies for civil-time: a runtime value (`seed`, `now`) is minted once at the impure RPC layer and threaded through `AutoFillParams` → `PipelineInput`; the pure engine consumes it and never reads a clock/RNG. Apply identically to `CivilTime`. 13.4 also proved the **five-fill-site wiring** discipline (engine sites get the live value; legacy sites get the default) — replicate for `local`.
- **13.2/13.3/13.4** proved: new optional stages are additive + default-noop + need a `*_default` routing clause; default-equivalent stages must round-trip byte-identically (omit-when-default); behavior must emerge from config in the persona suite (no `if persona`). Follow all four.
- **Sandbox caveat (recurring):** full `rtk cargo test -p hifimule-daemon` may not finish where mockito/networking is blocked. New context tests are pure (`auto_fill::`); run targeted `rtk cargo test -p hifimule-daemon auto_fill::` if blocked.

### Latest technical context

- Add `chrono` (default `clock` feature) — `Local::now()` uses `localtime_r` for a sound local offset. **Do not** use the `time` crate's `local-offset` feature in this multi-threaded daemon (it returns `None`/errs by design). Rust edition 2024 (`f64::total_cmp`, let-chains available). `rand = "0.8"` already present (not needed here).
- No new network, no clock inside the pure core. The only new `SystemTime`/local-time read is `now_civil()` in `rpc.rs`, beside the existing `now_unix_secs()`.

### Project Structure Notes

- Daemon (Rust): engine in `auto_fill/pipeline.rs`; async/routing in `auto_fill/fetch.rs`; params in `auto_fill/mod.rs`; RPC + transcode wiring in `rpc.rs`; transcode profile types in `providers/mod.rs`. Binary crate (no `lib.rs`); tests are `#[cfg(test)] mod tests` in each file. `ContextStage`/`ContextRule`/`ContextWindow`/`CivilTime` live in `pipeline.rs` — **not** `domain/models.rs` (provider-neutral entities only).
- Frontend (TS): `hifimule-ui/src/state/autoFill.ts`, `components/AutoFillPanel.ts`; i18n `hifimule-i18n/catalog.json`. No UI unit-test framework — `tsc` + build + manual preview.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Epic-13 (lines 3079-3105; Story 13.5 line 3099-3101: time-of-day #3, energy-curve #17, seasonal #32, encoding-from-goals #20 "depends on transcode-on-sync"; Story 13.6 boundary lines 3103-3105)]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-06-14-configurable-auto-fill.md (FR54 line 82 "context-aware"; FR52 budget line 80; Epic 13 table line 114; conscious cuts line 117)]
- [Source: _bmad-output/brainstorming/brainstorming-session-2026-06-12-1.md (#3 line 71, #17 line 72, #32 line 73, #20 line 114; pipeline grid with Context stage line 138; Budget System #20/#21/#35 lines 145,163-167 "encoding-from-goals depends on transcode-on-sync")]
- [Source: _bmad-output/planning-artifacts/architecture.md#Auto-Fill-Pipeline-Model (lines 788-826; stage open-extension; runtime-state vs config) ; #Enforcement (920-922 config-in-manifest / history-in-DB / route per server)]
- [Source: _bmad-output/implementation-artifacts/13-4-delight-rarity-draws-and-pity-timer.md (clock/entropy-as-value discipline; five-fill-site wiring; default-noop stage + routing clause; omit-when-default serialize; persona discipline; i18n parity gate + missing parity-test note)]
- [Source: hifimule-daemon/src/auto_fill/pipeline.rs:12-27,52-98,160-215,361-368,405-429,449-552,574,1084-1090,1177-1280,1201,1212-1410]
- [Source: hifimule-daemon/src/auto_fill/fetch.rs:145-187,189-357,790-819]
- [Source: hifimule-daemon/src/auto_fill/mod.rs:32-81]
- [Source: hifimule-daemon/src/rpc.rs:1934,2319-2339,2652-2675,3595-3610,4049-4063,4940-4942,5081-5082,5207-5208,5768-5791]
- [Source: hifimule-daemon/src/providers/mod.rs:355-359 (TranscodeProfile)]
- [Source: hifimule-daemon/src/domain/models.rs:26-50 (Song — no genre/BPM/energy/rating field; play_count :48, duration :34-35, bitrate :36, size :52)]
- [Source: hifimule-daemon/Cargo.toml:34 (only rand; add chrono)]
- [Source: hifimule-ui/src/state/autoFill.ts:87-185; components/AutoFillPanel.ts:246-372,481-483; hifimule-i18n/catalog.json (en :2 / fr ~:350 / es ~:698 / de ~:1046; 96 basket.autofill keys/locale today)]

## Dev Agent Record

### Agent Model Used

claude-opus-4-8 (Opus 4.8, 1M context) — bmad-dev-story workflow.

### Debug Log References

- `rtk cargo test -p hifimule-daemon` → 599 passed (was 586 in 13.4; +13 new tests).
- `rtk cargo test -p hifimule-daemon --bin hifimule-daemon auto_fill::` → 119 passed.
- `rtk cargo clippy -p hifimule-daemon --all-targets` → no new warnings in touched modules.
- `rtk cargo test -p hifimule-i18n` → 6 passed; parity verified 120×4 across en/fr/es/de.
- `rtk npx tsc --noEmit` (hifimule-ui) → no errors; `rtk npm run build` → green.

### Completion Notes List

**Civil-time foundation (AC 1, 2).** Added `CivilTime { hour, month, day, weekday }` (`#[derive(Default)]`) and `HistorySnapshot::local`. The pure engine has **no** clock/`chrono` call — civil time enters as a value exactly like `seed`/`now`. Added `chrono = "0.4"` (default `clock` feature) and `rpc::now_civil()` (the single new clock read, beside `now_unix_secs()`; documents why the `time` crate's `local-offset` is avoided). `AutoFillParams::local` is set from `now_civil()` at the **three** engine fill sites (basket expansion, multi-server sync delta, preview-with-serverId) and `CivilTime::default()` at the four legacy sites (Jellyfin sync, Jellyfin preview, both `main.rs` auto-sync paths) — mirroring the `seed: now as u64` vs `seed: 0` split. Threaded into `HistorySnapshot::local` in `expand_with_pipeline`.

**Context stage (#3/#17/#32) (AC 3-6).** One `ContextStage { enabled, rules }` of ordered `ContextRule`s; each a `ContextWindow` (`TimeOfDay`/`Months`/`DateRange`) + source-activation/weighting + scheduled tag/genre filter. Parse-tolerant `deserialize_context_rules` (a malformed rule degrades to "no effect"). `context_rule_active` handles midnight-wrap (TimeOfDay) and year-end-wrap (DateRange). `run_pipeline` computes the effective source set (`effective_sources`: unmentioned sources always run; only-in-inactive suppressed; active rules' sources weighted by **max**, normalized to shares) and effective filter (`effective_filter_with`: static ∪ active rules; exclude-wins preserved by the existing `filter_stage`). Reuses the existing `source_caps`/`build_source_units` machinery — budget/dedup untouched. `enabled:false` ⇒ byte-identical.

**Encoding-from-goals (#20) (AC 7-9).** `BudgetStage.encoding_from_goals` + pure `target_bitrate_kbps` (`effective_bytes×8/(secs×1000)`, clamped 32..=320 kbps; `None` when off/missing goal). `estimated_size` is bitrate-aware (`min(source, derived)` — transcoding only shrinks; threaded through `Selector`/`fits_ceiling`/`best_version_cmp`). The per-slot transcode override travels with the item via `SyncAddItem.max_bitrate_override_kbps`, stamped at the sync engine fill sites and patched onto `delta.adds` by `patch_delta_bitrate_overrides` (mirrors `patch_delta_tiers`); applied at the per-track transcode decision in `execute_provider_sync` — so it is naturally scoped to that slot's auto-fill items (manual items carry `None`) and never mutates the device-wide `transcoding_profile_id`. Passthrough gating: the impure layer (`encoding_passthrough_clear`/`transcode_profile_active`) clears the flag when no transcode profile is active, keeping the byte estimate aligned with reality; the override is only derived (`encoding_override_kbps`) when a profile is active.

**Implementation notes / scope.**
- The override is applied at the **provider-sync** seam (`execute_provider_sync`, where `providers::TranscodeProfile.max_bitrate_kbps` lives). The legacy Jellyfin single-server stream path (`get_item_stream`, raw device-profile `Value`) predates per-provider `TranscodeProfile` and is **not** modified — items carrying an override still expand/estimate correctly; only the forced re-encode rides the modern path. Best-effort throughout; never fails the sync.
- No DB table, no new entropy (as scoped). Deferred per Scope decision: BPM/energy-signal selection, device zones, holiday calendars, standalone device-profile-editor UI, ratings/play-feedback.
- **Pre-existing gap carried forward** (flagged in 13.3/13.4 reviews, out of scope here): there is still no automated all-locale i18n key-parity test in `hifimule-i18n` (parity was verified by script for this story).

### File List

- `hifimule-daemon/Cargo.toml` — add `chrono = "0.4"` (default `clock` feature).
- `hifimule-daemon/src/auto_fill/pipeline.rs` — `CivilTime`; `HistorySnapshot::local`; `ContextStage`/`ContextRule`/`ContextWindow` + `deserialize_context_rules`; `context_rule_active`/`active_context_rules`/`effective_filter_with`/`effective_sources`; `AutoFillPipeline::context`; `BudgetStage::encoding_from_goals`; `target_bitrate_kbps`; bitrate-aware `estimated_size`/`fits_ceiling`/`best_version_cmp`/`Selector`; context+encoding tests; Léo persona clock-gated assertion.
- `hifimule-daemon/src/auto_fill/mod.rs` — `AutoFillParams::local`.
- `hifimule-daemon/src/auto_fill/fetch.rs` — `context_default` in `needs_configurable_expansion`; thread `params.local` into `HistorySnapshot::local`; test param builders + routing test.
- `hifimule-daemon/src/rpc.rs` — `now_civil()`; `transcode_profile_active`/`encoding_passthrough_clear`/`encoding_override_kbps`; `patch_delta_bitrate_overrides`; `local`/override wiring at the engine fill sites; `build_autofill_history` snapshot field; rpc-level tests.
- `hifimule-daemon/src/sync.rs` — `SyncAddItem.max_bitrate_override_kbps`; per-track override application in `execute_provider_sync`; constructors/tests updated.
- `hifimule-daemon/src/main.rs` — `local: CivilTime::default()` at both legacy auto-sync `AutoFillParams` sites.
- `hifimule-daemon/src/device/tests.rs` — `rich_pipeline()` enriched with `context` rules + `encoding_from_goals` for round-trip coverage.
- `hifimule-ui/src/state/autoFill.ts` — `ContextWindow`/`ContextRule`/`ContextStage`/`ContextWindowKind` mirror types; `BudgetStage.encodingFromGoals`; `AutoFillPipeline.context`; default/normalize/serialize (omit-when-default).
- `hifimule-ui/src/components/AutoFillPanel.ts` — `renderContextStage` rule editor + encoding-from-goals checkbox; bind handlers; comma-list/window helpers.
- `hifimule-i18n/catalog.json` — 24 new `basket.autofill.*` keys ×4 locales (96×4 → 120×4).

### Change Log

| Date | Change |
|------|--------|
| 2026-06-15 | Story 13.5 created via create-story. Scope: Context axis cheap proxies (#3 time-of-day, #17 energy-curve, #32 seasonal) via one clock-driven `ContextStage` + #20 encoding-from-goals (budget-derived per-slot transcode bitrate). Civil-time carried as a value (chrono at impure layer); no DB table. Deferred: BPM/energy-signal selection, device zones, holiday calendars, device-profile-editor UI, ratings/play-feedback. |
| 2026-06-15 | Dev 13.5: implemented civil-time foundation (clock-as-value via `chrono`/`now_civil`), the `ContextStage` (one mechanism, three cheap proxies; pure window predicates + effective source/filter computation), and encoding-from-goals (pure `target_bitrate_kbps` + bitrate-aware estimate + per-slot transcode override carried via `SyncAddItem.max_bitrate_override_kbps`). Routing gate `context_default` added. Frontend mirror types + context rule editor + encoding toggle (omit-when-default serialize). i18n 96×4 → 120×4. 599 daemon tests pass, clippy clean on touched modules, i18n + tsc + build green. Status → review. |
