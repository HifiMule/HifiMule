---
baseline_commit: add47c24a17ec93fd33b52e92ad49effc8f1ac6b
---

# Story 13.3: Curation & Discovery Sources

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a HifiMule user whose library is far bigger than my device and who is tired of the same favorites being copied over and over,
I want the auto-fill pipeline to **surface music I own but rarely reach for** — the deep cuts I've barely played and the tracks I added long ago and forgot — so I can rediscover my own collection on each sync,
so that a fill brings genuine novelty from my own library instead of re-copying the hits, while staying fully deterministic and respecting my budget and per-server config.

This story extends the **pure-function pipeline engine** (Epic 12, then 13.1/13.2) with two deterministic, additive **discovery ordering keys** — a **Deep-Cuts Excavator** (#14: fewer plays first) and a **Rediscovery** key (#31 cheap "musical memories": oldest-added first). It is **engine + UI only**: no new DB tables, no new provider calls, no async fetch changes, no clock/RNG. Every signal already lives on the `Song` candidates the fetch layer materializes today (`play_count`, `date_added`). Both keys slot into the existing multi-key `ordering` sort exactly the way `OrderingKey::Quality` did in 13.2, and surface automatically in the existing ordering editor.

### Scope decision — rating-dependent ideas are deferred (read first)

Story 13.3's epic line names four brainstorm ideas: deep-cuts (#14), acclaimed-classics (#16), community-rating fallback (#15), musical-memories (#31). **Two of them need a rating/acclaim signal that does not exist in this codebase** and are **out of scope** for this story by explicit decision (Alexis, 2026-06-14 — "engine-only + defer ratings"):

- **#15 Community-Rating Fallback — DEFERRED.** The brainstorm defines this as *external* scores (ListenBrainz / Last.fm / Discogs) for unrated items ([brainstorming-session-2026-06-12-1.md:97](../brainstorming/brainstorming-session-2026-06-12-1.md)). `Song` has **no rating field**, neither `JellyfinProvider` nor `SubsonicProvider` maps one ([providers/jellyfin.rs](../../hifimule-daemon/src/providers/jellyfin.rs), [providers/subsonic.rs](../../hifimule-daemon/src/providers/subsonic.rs) — confirmed no `rating`/`communityRating`/`averageRating` mapping), and adding an external-score subsystem (network, caching, auth) is a separate feature, not one story. **Do NOT** implement it here. A future story owns it.
- **#16 Acclaimed-Classics Fill — cheap equivalent only.** The full idea is "owned albums acclaimed *by the community* but never played" ([brainstorm:98](../brainstorming/brainstorming-session-2026-06-12-1.md)) — the "acclaimed" part needs the same absent rating signal. Per the brainstorm's **ambition-tier model** ("every smart strategy has a ~10%-cost playlist version" — [brainstorm:129,147](../brainstorming/brainstorming-session-2026-06-12-1.md)), the cheap equivalent is **already fully expressible today** with shipped pieces: a user-curated "Classics/Acclaimed" **PlaylistSource** (Epic 12.4) blended with **`memory.playedExclusion = true`** (Epic 13.1). This story's only obligation for #16 is to **document and surface that recipe** (AC 6) — there is **no new engine code** for acclaimed-classics. The true community-acclaim version is deferred with #15.

So the engine deliverables of this story are precisely **#14 + #31-cheap as two new `OrderingKey` variants**. Everything else is composition of already-shipped pipeline pieces + documentation. Keep it that tight — the discipline that made 13.1/13.2 land cleanly is "derive from existing `Song` fields, stay pure, defer anything needing new data."

## Acceptance Criteria

### A — Deep-Cuts Excavator (#14): low-play-count ordering

1. **New `OrderingKey::Excavation` ranks fewer-played tracks first.** A new ordering key surfaces "owned-but-barely-played" music: candidates are ranked by `play_count` **ascending** (a never-played track — `play_count` `None` or `0` — is the *deepest* cut and ranks first; a heavily-played hit ranks last). The comparison reads only `Song.play_count` (`None` treated as `0`). It is the inverse of `OrderingKey::PlayCount`. Deterministic — no clock, no RNG. [Source: brainstorm #14 "fill with owned-but-barely-played music" ([brainstorm:96](../brainstorming/brainstorming-session-2026-06-12-1.md)); the pipeline grid lists "excavation" as an Ordering axis ([brainstorm:138](../brainstorming/brainstorming-session-2026-06-12-1.md)); `Song.play_count` at [domain/models.rs:42](../../hifimule-daemon/src/domain/models.rs#L42)]

2. **Excavation composes as one key in the multi-key sort, unchanged in framework.** `OrderingKey::Excavation` is one entry in the ordered `ordering: Vec<OrderingKey>` ([pipeline.rs:615-654 `compare_by_ordering`](../../hifimule-daemon/src/auto_fill/pipeline.rs#L615-L654)); its placement determines precedence vs. other keys, exactly like every existing key. The enhancement adds **only a new match arm** — it does not touch the sort framework, version-preference tiebreak, or any other key. A pipeline that never lists `Excavation` is **byte-for-byte unaffected**. It pairs naturally with `memory.playedExclusion` (deep cuts you've *never* played) but does not require it. [Source: pipeline.rs:622-645 the `for key in keys` loop + `OrderingKey::Quality` precedent added in 13.2]

### B — Rediscovery / cheap Musical Memories (#31): oldest-added ordering

3. **New `OrderingKey::Rediscovery` ranks oldest-added tracks first.** A new ordering key resurfaces music added long ago: candidates are ranked by `Song.date_added` **ascending** (oldest ISO-8601 timestamp first). This is the inverse of the existing `OrderingKey::DateCreated` (newest first, [pipeline.rs:629-633](../../hifimule-daemon/src/auto_fill/pipeline.rs#L629-L633)). **`None`/empty `date_added` sorts LAST** (an unknown add-date is the *worst* rediscovery candidate, not the best — do **not** let empty strings sort to the front the way a naive `unwrap_or("")` ascending would). Implement with an explicit None-last comparison (e.g. compare `Option<&str>` where `None` is "greatest", or map missing to a high sentinel). Deterministic — no clock, no RNG. ISO-8601 strings sort lexicographically (same assumption `DateCreated` already relies on). [Source: brainstorm #31 "Musical Memories Fill … cheap version = date-added by past season" ([brainstorm:100](../brainstorming/brainstorming-session-2026-06-12-1.md)); `Song.date_added` at [domain/models.rs:40](../../hifimule-daemon/src/domain/models.rs#L40); `DateCreated` arm to mirror-and-invert at [pipeline.rs:629-633](../../hifimule-daemon/src/auto_fill/pipeline.rs#L629-L633)]

4. **Rediscovery is the documented cheap version of #31 — no clock, no date math.** This story delivers the **ambition-tier cheap equivalent** the brainstorm prescribes (oldest-first ordering), **not** a true "same season N years ago" window. A real seasonal/anniversary window would need `now` inside the comparator (which `compare_by_ordering` deliberately does not receive — the engine reads `now` only via `HistorySnapshot`, never in the sort) and is the deferred "high effort" version. **Do NOT** thread `now` into the comparator or add date arithmetic. Combined with `memory.playedExclusion`, rediscovery expresses "stuff I added long ago and never played." Document this scoping inline. [Source: pure-function discipline — engine takes `now` only on the snapshot ([pipeline.rs:259-282 PipelineInput/HistorySnapshot](../../hifimule-daemon/src/auto_fill/pipeline.rs#L259-L282)); 13.1/13.2 "cheap equivalent" precedent]

### C — Acclaimed-Classics cheap equivalent (#16): recipe via existing pieces

5. *(intentionally folded into AC 6 — no separate engine AC; see the Scope decision above.)*

6. **Acclaimed-Classics cheap recipe is documented & expressible with shipped pieces — no new engine code.** Verify (and document in Dev Notes + a UI hint) that the #16 cheap equivalent works **today** with no engine changes: a `PlaylistSource` pointing at a user-curated "classics/acclaimed" playlist (Epic 12.4 — `SourceKind::Playlist` with a `ref`) blended with `memory.playedExclusion = true` (Epic 13.1) already yields "owned, curated-as-acclaimed, never-played" tracks. The only deliverable is **discoverability**: a short helper/caption string in the configuration UI (under Sources or the Advanced disclosure) that explains this recipe, added to all 4 locales (AC 9). **Do NOT** add a `SourceKind::Acclaimed`, a rating field, or any new engine path for this. The true community-acclaim version is deferred (Scope decision). [Source: brainstorm ambition tiers ([brainstorm:129,147](../brainstorming/brainstorming-session-2026-06-12-1.md)); PlaylistSource = `SourceKind::Playlist` ([pipeline.rs:133-138](../../hifimule-daemon/src/auto_fill/pipeline.rs#L133-L138)); `playedExclusion` shipped in 13.1]

### D — Routing, UI, i18n, scope

7. **Configurable-path routing recognizes the new ordering keys.** Given a pipeline whose only non-default aspect is `Excavation` or `Rediscovery` in `ordering`, `needs_configurable_expansion` ([fetch.rs:145-180](../../hifimule-daemon/src/auto_fill/fetch.rs#L145-L180)) returns `true` so the materialized engine path runs. This already holds via the `ordering_default` check ([fetch.rs:152-158](../../hifimule-daemon/src/auto_fill/fetch.rs#L152-L158)): the legacy ordering is exactly `[Favorite, PlayCount, DateCreated]`, so any ordering containing a new key is non-legacy → routes. **Verify** with a test for each new key; **no logic change expected** (same situation 13.2 documented for `OrderingKey::Quality`, AC 8). [Source: fetch.rs:152-158; 13.2 AC 8]

8. **Configuration UI exposes the new ordering keys with zero new render code.** The ordering editor ([AutoFillPanel.ts:424-443 `renderOrderingSection`](../../hifimule-ui/src/components/AutoFillPanel.ts#L424-L443)) is fully data-driven: it renders/labels every key in `ORDERING_KEYS` via `t('basket.autofill.ordering_' + key)`. Add `'excavation'` and `'rediscovery'` to the `OrderingKey` union and the `ORDERING_KEYS` array ([state/autoFill.ts:7,71](../../hifimule-ui/src/state/autoFill.ts#L7)), and they appear automatically in the add-dropdown, are reorderable, and round-trip through `serializePipeline`/`normalizePipeline` (which pass `ordering` through verbatim — [state/autoFill.ts:109-111,179](../../hifimule-ui/src/state/autoFill.ts#L109)). No new control, no new handler. Edits already invalidate the debounced live preview via the existing ordering handlers. The simple (non-Advanced) default path is unchanged. [Source: AutoFillPanel.ts:424-443; state/autoFill.ts:6-7,71,99-122,126-185]

9. **i18n parity across all 4 locales.** Add the new label keys `basket.autofill.ordering_excavation` and `basket.autofill.ordering_rediscovery`, plus one acclaimed-classics recipe hint key (AC 6), to **all 4 locales** (`en`, `fr`, `es`, `de`) under the existing `basket.autofill.*` convention — mirroring `ordering_quality` ([catalog.json:125 / 470 / 815 / 1160](../../hifimule-i18n/catalog.json#L125)). Current parity is **78×4**; this adds 3 keys → **81×4**. The i18n parity test stays green. (Ordering keys are labels only — no `_hint` per ordering key today — so each new key is a single string; the acclaimed-classics recipe hint is one additional key.) [Source: catalog.json ordering labels at lines 122-125; 13.2 completion "78×4"; i18n parity test in `hifimule-i18n`]

10. **Backward compatibility & scope.** A pipeline that lists neither new key behaves **exactly** as today — zero migration. The legacy fast path (`run_auto_fill*`) is untouched (it only knows the legacy default ordering). The new keys affect only pipelines that explicitly list them. Config stays in the manifest; **no DB, no `autofill_history`/rotation/cooldown interaction, no new provider call, no `Song` field** in this story. Do **NOT** implement: community-rating (#15 — deferred), true acclaimed-classics (#16 acclaim signal — deferred), rarity draws & pity timer (13.4), context/encoding-from-goals (13.5), advanced units & promotion (13.6). Do **NOT** add a `SourceKind` for deep-cuts/memories/acclaimed — these are ordering keys + a playlist recipe, not provider sources. [Source: 13.2 AC 11 backward-compat precedent]

11. **Build & tests green.** `rtk cargo test -p hifimule-daemon` passes (no regression; if the sandbox blocks mockito/networking run targeted `rtk cargo test -p hifimule-daemon auto_fill::`). New tests cover: `Excavation` ranks low/zero/`None` play_count first and a hit last; `Rediscovery` ranks oldest `date_added` first with **`None` sorting last** (the explicit guard — a candidate with no date must not jump to the front); both compose as one key among others (placement precedence preserved; a pipeline without them is unchanged); determinism (stable on ties, no RNG); `needs_configurable_expansion` returns `true` for an `Excavation`-only and a `Rediscovery`-only pipeline and the legacy default still takes the fast path; serde round-trip of a pipeline whose `ordering` includes the new keys. Strengthen the **Léo** persona ([pipeline.rs:1321 `persona_leo_gym_energy_playlist_tiny_device`](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1321)) — the explorer/discovery persona — to assert excavation surfaces a barely-played track over a hit (no `if persona ==` branch — behavior emerges from config). `rtk cargo clippy -p hifimule-daemon --all-targets` adds no new warnings in touched modules. Frontend `rtk npx tsc --noEmit` + `rtk npm run build` stay green; `rtk cargo test -p hifimule-i18n` parity green.

## Tasks / Subtasks

- [x] **Deep-Cuts Excavator ordering key (#14)** (`hifimule-daemon/src/auto_fill/pipeline.rs`) (AC: 1, 2)
  - [x] Add `Excavation` to the `OrderingKey` enum ([pipeline.rs:153-166](../../hifimule-daemon/src/auto_fill/pipeline.rs#L153-L166)), `#[serde(rename_all = "camelCase")]` → `"excavation"`. Update the doc comment ("fewer plays first — owned-but-barely-played").
  - [x] In `compare_by_ordering` ([pipeline.rs:622-645](../../hifimule-daemon/src/auto_fill/pipeline.rs#L622-L645)) add the arm: `a.play_count.unwrap_or(0).cmp(&b.play_count.unwrap_or(0))` (ascending → fewer-played first; inverse of the `PlayCount` arm at [:627](../../hifimule-daemon/src/auto_fill/pipeline.rs#L627)).
  - [x] Tests: never-played (`None`) and `0` rank before a 50-play hit; ties stable; `[Excavation]` vs `[PlayCount]` produce reversed orders; a pipeline without `Excavation` is unchanged.

- [x] **Rediscovery / cheap Musical-Memories ordering key (#31)** (`hifimule-daemon/src/auto_fill/pipeline.rs`) (AC: 3, 4)
  - [x] Add `Rediscovery` to `OrderingKey` (`"rediscovery"`). Doc: "oldest-added first — resurface music added long ago (cheap musical-memories)".
  - [x] In `compare_by_ordering` add the arm: oldest `date_added` first with **`None` last**. Do NOT use `unwrap_or("")` ascending (that sorts unknowns first). Used an explicit absent-last comparison via the `nonblank_date` helper (folds whitespace-only `Some("")` into "absent" too, per AC 3's "None/empty" wording): `match (nonblank_date(a), nonblank_date(b)) { (Some(x), Some(y)) => x.cmp(y), (Some(_), None) => Less, (None, Some(_)) => Greater, (None, None) => Equal }`.
  - [x] Tests: oldest ISO date first; `None`/empty `date_added` sorts **last** (explicit assertion — the AC 3 guard, incl. whitespace-only); inverse of `DateCreated` on the same fixtures; ties stable; pipeline without it unchanged.

- [x] **Routing verification** (`hifimule-daemon/src/auto_fill/fetch.rs`) (AC: 7)
  - [x] Added `discriminator_new_ordering_keys_force_configurable`: `needs_configurable_expansion` returns `true` for an `Excavation`-only and a `Rediscovery`-only pipeline, and `false` for the legacy default. No logic change — confirmed and locked in.

- [x] **Frontend ordering keys** (`hifimule-ui/src/state/autoFill.ts`, `components/AutoFillPanel.ts`) (AC: 8)
  - [x] In `state/autoFill.ts`: added `'excavation' | 'rediscovery'` to the `OrderingKey` union and appended both to `ORDERING_KEYS`. No serialize/normalize change (`ordering` passes verbatim).
  - [x] No change to `AutoFillPanel.ts` ordering render required (data-driven). The new keys appear in the add-dropdown / reorder / remove automatically via `ORDERING_KEYS` + `t('basket.autofill.ordering_'+key)`. Verified via tsc + build.

- [x] **Acclaimed-Classics cheap recipe — UI hint + docs (#16)** (`hifimule-ui/src/components/AutoFillPanel.ts`, Dev Notes) (AC: 6)
  - [x] Added a caption in the Sources stage (`renderSourcesStage`) keyed on `basket.autofill.acclaimed_classics_hint` explaining the recipe (curated "classics" playlist source + `playedExclusion`). No engine code, no new `SourceKind`.

- [x] **i18n keys ×4 locales** (`hifimule-i18n/catalog.json`) (AC: 9)
  - [x] Added `basket.autofill.ordering_excavation`, `basket.autofill.ordering_rediscovery`, and `basket.autofill.acclaimed_classics_hint` to `en`/`fr`/`es`/`de` (3 keys → **81×4**). Parity test green.

- [x] **Full verification** (AC: 10, 11)
  - [x] `rtk cargo test -p hifimule-daemon auto_fill::` (93 pass) + full `-p hifimule-daemon` (570 pass), `rtk cargo clippy -p hifimule-daemon --all-targets` (no new warnings in touched modules), `rtk cargo test -p hifimule-i18n` (6 pass), frontend `rtk npx tsc --noEmit` (clean) + `rtk npm run build` (green). Strengthened Léo's persona test to assert excavation surfaces a barely-played deep cut over a hit (config-driven, no `if persona` branch).

## Dev Notes

### What this story is (and is not)

A **pure-engine + tiny-UI** story, the smallest in Epic 13. Like 13.2 there is **no DB work, no new provider calls, no async fetch logic, no `autofill_history`/rotation/cooldown interaction**. The entire engine deliverable is **two new `OrderingKey` arms** that read fields already present on every materialized `Song` (`play_count`, `date_added` — [domain/models.rs:40,42](../../hifimule-daemon/src/domain/models.rs#L40)). The UI deliverable is **two union/array entries** (the ordering editor renders them automatically) + **one helper caption** for the acclaimed-classics recipe. Two of the epic line's four ideas (#15 community-rating, #16 true acclaimed-classics) are **deferred** — see the Scope decision at the top; do not build them.

**Why ordering keys, not Sources:** the brainstorm's own pipeline grid files "excavation" under the **Ordering** axis ([brainstorm:138](../brainstorming/brainstorming-session-2026-06-12-1.md)), and `SourceKind` variants each map 1:1 to a provider fetch method ([pipeline.rs:126-138](../../hifimule-daemon/src/auto_fill/pipeline.rs#L126-L138)) — deep-cuts/memories are not separate endpoints, they are the Library re-ranked. Ordering keys are the lowest-risk, most composable home and mirror exactly how 13.2 added quality. Do not invent a `SourceKind`.

### The pure-function discipline (non-negotiable)

The engine ([auto_fill/pipeline.rs](../../hifimule-daemon/src/auto_fill/pipeline.rs)) is **pure, synchronous, deterministic** — no `SystemTime::now()`, no RNG (`OrderingKey::Random` is a deliberate no-op, [pipeline.rs:639-640](../../hifimule-daemon/src/auto_fill/pipeline.rs#L639-L640)). Both new keys are plain field comparisons. **`now` is unavailable in `compare_by_ordering` by design** (the engine reads `now` only off the `HistorySnapshot` — [pipeline.rs:259-282](../../hifimule-daemon/src/auto_fill/pipeline.rs#L259-L282)); that is exactly why Rediscovery is "oldest-first" (no clock) rather than a seasonal window (would need a clock). Do not thread `now` into the sort. The four-persona suite ([pipeline.rs:1212-1410](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1212)) depends on this determinism — strengthen Léo, never add `if persona ==` branches.

### Rediscovery None-handling — the one real footgun

`OrderingKey::DateCreated` sorts newest-first via `b.date_added.as_deref().unwrap_or("").cmp(a…)` — for *descending* (newest first), an empty string is the smallest and naturally sinks to the bottom, which is fine. For **Rediscovery (ascending / oldest first)** the naive mirror `a.unwrap_or("").cmp(b…)` would sort a missing date (`""`) to the **front** — i.e. unknown-date tracks would masquerade as the oldest. That inverts the intent (AC 3). Use an explicit `None`-last comparison (snippet in the task). This is the single subtle correctness point in the story — unit-test it directly.

### Quality/version & memory tiebreaks still apply after the explicit keys

`compare_by_ordering` applies the configured `ordering` keys in order, then the 13.2 version-preference tiebreak ([pipeline.rs:646-652](../../hifimule-daemon/src/auto_fill/pipeline.rs#L646-L652)). The new keys are just additional arms in the `for key in keys` loop — they inherit composition for free (e.g. `[Excavation, Favorite]` = deep cuts, favorites breaking ties). Nothing else in the sort changes.

### Acclaimed-Classics cheap recipe (#16) — composition, not code

The pieces already shipped: `SourceKind::Playlist { ref }` (12.4) + `memory.playedExclusion` (13.1). Pointing a source at a user's "classics" playlist and excluding played tracks **is** "owned, curated-as-acclaimed, never-played." The only gap is that users won't discover the recipe — so add a caption. Resist building a `SourceKind::Acclaimed` or a rating field; the community-acclaim *signal* is the deferred part, and without it a dedicated source kind would be a hollow alias of `Playlist`.

### Deferred (do NOT implement) — and why

- **#15 Community-Rating Fallback:** external scores (ListenBrainz/Last.fm/Discogs) for unrated items. No rating field on `Song`; no provider mapping; needs a network+cache+auth subsystem. Out of scope by explicit decision. A future story owns it (and may first decide whether to map provider-native ratings — Subsonic `averageRating`/`userRating`, Jellyfin `CommunityRating`/`CriticRating` — onto `Song`, which neither adapter does today).
- **#16 true Acclaimed-Classics:** same missing acclaim signal. Cheap recipe only (AC 6).
- If during implementation either new ordering key starts to need `now`, a `Song` field, a provider call, or a DB read — **stop and flag it**; it has left this story's scope.

### Current code being changed (read before writing)

- **Engine:** [pipeline.rs:153-166](../../hifimule-daemon/src/auto_fill/pipeline.rs#L153-L166) (`OrderingKey` enum — add two variants), `:615-654` (`compare_by_ordering` — add two arms; note `PlayCount` arm `:627`, `DateCreated` arm `:629-633`, version tiebreak `:646-652`), `:259-282` (`PipelineInput`/`HistorySnapshot` — confirms `now` is snapshot-only), `:1212-1410` (4-persona suite; **Léo at `:1321`**, fixtures `song_*`/`cand` nearby). No struct shape changes — `OrderingKey` is `Copy`/`PartialEq`/serde camelCase already.
- **Routing:** [fetch.rs:145-180](../../hifimule-daemon/src/auto_fill/fetch.rs#L145-L180) (`needs_configurable_expansion`; `ordering_default` at `:152-158` already excludes any non-legacy ordering). No change — add tests only.
- **Song fields available:** [domain/models.rs:26-50](../../hifimule-daemon/src/domain/models.rs#L26-L50) — `play_count: Option<u32>` (`:42`), `date_added: Option<String>` (`:40`). **No `rating`/`year`/`genre`/version field** — that absence is the whole reason #15/#16 defer.
- **Frontend:** [state/autoFill.ts:7](../../hifimule-ui/src/state/autoFill.ts#L7) (`OrderingKey` union), `:71` (`ORDERING_KEYS` — the dropdown source), `:109-111` & `:179` (ordering passed verbatim through normalize/serialize — no change). [AutoFillPanel.ts:424-443](../../hifimule-ui/src/components/AutoFillPanel.ts#L424-L443) (`renderOrderingSection` — data-driven by `ORDERING_KEYS` + `t('basket.autofill.ordering_'+key)`; no change beyond verifying); add the acclaimed hint caption near the Sources/Advanced section.
- **i18n:** [catalog.json:122-125](../../hifimule-i18n/catalog.json#L122-L125) (en ordering labels) + the fr/es/de blocks at `:467-470` / `:812-815` / `:1157-1160`. `basket.autofill.*` snake_case; 78 keys ×4 today.

### Architecture compliance (non-negotiable)

- **Config in the manifest only.** New ordering keys are pipeline **config** → live in `manifest.autoFill : Map<serverId, AutoFillPipeline>`. This story writes **nothing** to the daemon DB (there is no runtime state). The architecture's `ordering` list already shows `"…"` as an open extension point: `ordering: [ "favorite", "playCount", "dateCreated", "random", "quality", … ]` ([architecture.md:800](../../_bmad-output/planning-artifacts/architecture.md#L800)). [Source: architecture.md:792-807; #Enforcement config-in-manifest at :920-922]
- **Reuse Epic 12/13 types — do not redefine** `AutoFillPipeline`/`OrderingKey`/`Song`/`Candidate`. Only add enum variants + comparator arms.
- **Per-server routing & legacy fast path untouched** — this story never sees a provider directly; it only adds pure-engine ordering logic. [architecture.md:920-921]

### Previous story intelligence (13.1 / 13.2)

- **`needs_configurable_expansion` is the routing gate** — but unlike 13.1's memory fields, ordering keys need **no gate change**: any ordering other than empty-or-legacy already routes. 13.2 documented the identical situation for `OrderingKey::Quality` (its AC 8). Add tests; don't touch the gate logic.
- **Ordering keys are label-only in the UI** — 13.2 added `ordering_quality` as a single label and the editor surfaced it automatically. Same here for `excavation`/`rediscovery`. No `_hint` per ordering key (the acclaimed recipe hint is a separate, non-ordering key).
- **`ordering` round-trips verbatim** — `normalizePipeline`/`serializePipeline` copy `ordering` with `[...]` and never filter it ([state/autoFill.ts:109-111,179](../../hifimule-ui/src/state/autoFill.ts#L109)). New union members "just work" — no serialize change (contrast with the Memory/Quality fields, which needed omit-when-default logic).
- **i18n parity is a hard gate (currently 78×4):** every new key in all 4 locales or the parity test fails. 13.2 went 65×4 → 78×4; this story adds 3 → 81×4. Count precisely.
- **Persona suite is the engine acceptance bar:** Léo ([pipeline.rs:1321](../../hifimule-daemon/src/auto_fill/pipeline.rs#L1321)) is the explorer/discovery persona (the brainstorm's "cheap proxy" voice) — extend him to assert a barely-played deep cut beats a hit under `[Excavation]`. No `if persona` branches — behavior must emerge from config.
- **Sandbox caveat (recurring across Epic 12/13):** full `rtk cargo test -p hifimule-daemon` may not finish where mockito/local networking is blocked. New tests here are **pure engine** (`auto_fill::`) — run targeted: `rtk cargo test -p hifimule-daemon auto_fill::`.

### Git intelligence

Recent commits (`add47c2 Review 13.2`, `20cb42c Dev 13.2`, `6b9a90d Review 13.1`, `b47d693 Dev 13.1`, `0a537f8 Story 13.1`) confirm 13.1 and 13.2 are closed and this is the third Epic 13 story. No competing in-flight changes to `auto_fill/pipeline.rs`, `fetch.rs`, `AutoFillPanel.ts`, or `state/autoFill.ts`. The frozen contract holds: legacy fast path + default pipelines behave identically — that invariant must survive this story (it trivially does — the new keys are opt-in).

### Latest technical context

- **No new crate dependency.** Both keys are plain field comparisons over existing `Song` fields; serde camelCase already covers the new enum variants. Rust edition 2024 (let-chains in use elsewhere — see [pipeline.rs](../../hifimule-daemon/src/auto_fill/pipeline.rs)).
- **No clock, no RNG, no network, no DB, no `Song`-field addition** anywhere in this story.

### Project Structure Notes

- Daemon (Rust): all engine logic in `hifimule-daemon/src/auto_fill/pipeline.rs`; routing gate in `auto_fill/fetch.rs` (tests only). Binary crate (no `lib.rs`); tests are `#[cfg(test)] mod tests` in `pipeline.rs`. Do **not** put ordering/discovery types in `domain/models.rs` (provider-neutral entities only) — `OrderingKey` lives in `auto_fill/pipeline.rs`.
- Frontend (TS): `hifimule-ui/src/components/AutoFillPanel.ts`, `state/autoFill.ts`; i18n catalog `hifimule-i18n/catalog.json`. No UI unit-test framework configured — rely on `tsc` + build + manual preview, matching the existing pattern.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Epic-13 (lines 3079-3093, Story 13.3: deep-cuts #14, acclaimed-classics #16, community-rating fallback #15, musical-memories #31)]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-06-14-configurable-auto-fill.md (FR54 line 82; Epic 13 table line 112; ambition-tier cheap-equivalent model line 122; provider-trait additive note line 40)]
- [Source: _bmad-output/brainstorming/brainstorming-session-2026-06-12-1.md (ideas #14/#15/#16/#31 lines 96-100; pipeline grid "excavation" ordering axis line 138; ambition tiers lines 129/147)]
- [Source: _bmad-output/planning-artifacts/architecture.md#Auto-Fill-Pipeline-Model (lines 788-826; ordering open-extension list line 800); #Enforcement (lines 920-922, config-in-manifest)]
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md §5.3 (Advanced disclosure, collapsible stage sections; ambition-tier inline cheap equivalents)]
- [Source: _bmad-output/implementation-artifacts/13-2-quality-and-version-ordering.md (OrderingKey arm pattern, routing-via-ordering_default, label-only i18n, serialize-verbatim, persona discipline, sandbox caveat)]
- [Source: _bmad-output/implementation-artifacts/13-1-memory-and-rotation-strategies.md (playedExclusion shipped; pure-function/now-on-snapshot discipline; i18n parity gate)]
- [Source: hifimule-daemon/src/auto_fill/pipeline.rs:126-166,259-282,615-654,1212-1410]
- [Source: hifimule-daemon/src/auto_fill/fetch.rs:145-180]
- [Source: hifimule-daemon/src/domain/models.rs:26-50 (Song.play_count :42, Song.date_added :40; no rating field)]
- [Source: hifimule-ui/src/state/autoFill.ts:6-7,71,99-185; components/AutoFillPanel.ts:424-443; hifimule-i18n/catalog.json:122-125,467-470,812-815,1157-1160]

## Dev Agent Record

### Agent Model Used

claude-opus-4-8 (BMad dev-story workflow)

### Debug Log References

None — no failures encountered; all suites passed on first run after implementation.

### Completion Notes List

- **Engine (#14 + #31-cheap):** Added two `OrderingKey` variants — `Excavation` (fewer plays first, the exact inverse of `PlayCount`, `None`→0) and `Rediscovery` (oldest `date_added` first, the inverse of `DateCreated`). Both are pure field comparisons in `compare_by_ordering`; no clock, no RNG, no new `Song` field, no DB, no provider call. They slot into the existing multi-key sort as new match arms only — the framework, version-preference tiebreak, and all other keys are untouched.
- **Rediscovery None-handling (the one footgun, AC 3):** Implemented an explicit absent-last comparison via a new `nonblank_date(&Song) -> Option<&str>` helper. Beyond the `None` case the task snippet covered, the helper also folds whitespace-only `Some("")` into "absent" — AC 3's wording is "None/**empty** sorts last", and a naive `Some("")` would otherwise sort lexicographically to the *front*. Unit-tested directly (`rediscovery_sorts_missing_or_blank_date_last`).
- **Routing (AC 7):** No logic change — any non-legacy `ordering` already trips `ordering_default` in `needs_configurable_expansion`. Added `discriminator_new_ordering_keys_force_configurable` to lock in `true` for Excavation-only / Rediscovery-only and `false` for the legacy default.
- **Frontend (AC 8):** Two union members + two `ORDERING_KEYS` entries. The data-driven ordering editor surfaces them automatically (dropdown / reorder / remove); `ordering` round-trips verbatim through normalize/serialize, so no serialize change. tsc + build green.
- **Acclaimed-Classics cheap recipe (#16, AC 6):** No engine code. Added a caption in the Sources stage (`basket.autofill.acclaimed_classics_hint`) documenting the shipped-pieces recipe: a curated "classics" `PlaylistSource` (12.4) + `memory.playedExclusion` (13.1) = "owned, curated-as-acclaimed, never-played." No `SourceKind::Acclaimed`, no rating field.
- **i18n (AC 9):** 3 keys × 4 locales (en/fr/es/de) → 78×4 → **81×4**. Parity test green.
- **Persona (AC 11):** Strengthened Léo (`persona_leo_gym_energy_playlist_tiny_device`) — now uses an `Excavation` ordering so a 90-play hit is excavated past the tiny budget while the barely-played deep cuts surface. Behavior emerges from config; no `if persona ==` branch.
- **Deferred (untouched, per Scope decision):** #15 community-rating (no rating signal on `Song`/providers), true #16 community-acclaim, 13.4–13.6. No `Song` field, DB, provider, or clock added.
- **Verification:** auto_fill 93 pass; full `-p hifimule-daemon` 570 pass (no regression); i18n 6 pass; tsc clean; build green; clippy adds no new warnings in `pipeline.rs`/`fetch.rs` (remaining warnings are pre-existing in vault/device_io/api/jellyfin).

### File List

- `hifimule-daemon/src/auto_fill/pipeline.rs` (modified — `Excavation`/`Rediscovery` enum variants, two `compare_by_ordering` arms, `nonblank_date` helper, 8 new 13.3 tests, strengthened Léo persona test)
- `hifimule-daemon/src/auto_fill/fetch.rs` (modified — `discriminator_new_ordering_keys_force_configurable` routing test)
- `hifimule-ui/src/state/autoFill.ts` (modified — `OrderingKey` union + `ORDERING_KEYS` array)
- `hifimule-ui/src/components/AutoFillPanel.ts` (modified — acclaimed-classics hint caption in `renderSourcesStage`)
- `hifimule-i18n/catalog.json` (modified — 3 keys × 4 locales)

## Change Log

| Date       | Version | Description                                                                                     | Author |
| ---------- | ------- | ----------------------------------------------------------------------------------------------- | ------ |
| 2026-06-15 | 1.0     | Implemented Story 13.3: `Excavation` (#14) + `Rediscovery` (#31-cheap) ordering keys; acclaimed-classics recipe hint (#16); frontend + i18n (81×4); routing/persona tests. All suites green. | Amelia (dev-story) |
