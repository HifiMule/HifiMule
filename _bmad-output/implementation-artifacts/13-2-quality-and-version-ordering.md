---
baseline_commit: 6b9a90dc4407c7d46bd013f42f0d52964d64cdaf
---

# Story 13.2: Quality & Version Ordering

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a HifiMule user with a library full of duplicate recordings (the same song on a single, an album, a "Best Of", a live record, and a 2011 remaster),
I want the auto-fill pipeline to **rank candidates by audio quality and to keep only my preferred version of each song** — collapsing duplicates so I get the studio FLAC instead of three lossy live copies, ordered however I prefer (lossless first, original over remaster, etc.),
so that the limited space on my device holds the best single copy of each track instead of wasting bytes on redundant or inferior versions.

This story extends the **pure-function pipeline engine** built in Epic 12 with three deterministic, additive modifiers: a **format-aware quality ordering** (#13 — enhances the existing `OrderingKey::Quality`), **version detection + preference** (#34), and **best-version resolution** (#11 — collapse same-song duplicates, keep the best). It is **engine + UI only**: no new DB tables, no new provider calls, no async fetch changes — every signal it needs (`bitrate_kbps`, `suffix`, `content_type`, `title`, `album_title`, `artist_name`) already lives on the `Song` candidates the fetch layer materializes today.

## Acceptance Criteria

### A — Quality modifier (#13): format-aware ordering

1. **`OrderingKey::Quality` becomes lossless-aware.** Today `OrderingKey::Quality` ranks purely by `bitrate_kbps` descending ([pipeline.rs:556-559](../../hifimule-daemon/src/auto_fill/pipeline.rs#L556-L559)), which mis-ranks a 320 kbps MP3 *above* a FLAC that reports a low/absent bitrate. When this story ships, `OrderingKey::Quality` ranks **lossless formats above lossy regardless of bitrate**, then by `bitrate_kbps` descending within each tier. "Lossless" is detected deterministically from `Song.suffix` / `Song.content_type` via a pure helper `format_quality_rank(song) -> u8` (e.g. `flac`/`alac`/`wav`/`aiff`/`ape`/`wavpack` → lossless tier; everything else → lossy tier; unknown → lowest). The helper is case-insensitive and reads only `suffix` then `content_type` (mime subtype) — never the title. [Source: domain/models.rs:36,45,47 (`bitrate_kbps`, `content_type`, `suffix`); architecture.md line 800 (`ordering: [… "quality" …]`)]

2. **Quality ordering composes as a tiebreak, unchanged in position.** `OrderingKey::Quality` remains one entry in the ordered `ordering: Vec<OrderingKey>` multi-key sort ([pipeline.rs:541-568](../../hifimule-daemon/src/auto_fill/pipeline.rs#L541-L568)) — its placement (e.g. `[favorite, quality]` vs `[quality, favorite]`) still determines precedence. The enhancement changes only the *comparison within the Quality key*, not the sort framework. A pipeline that never lists `Quality` in its `ordering` is **byte-for-byte unaffected** — and the legacy default ordering (`[Favorite, PlayCount, DateCreated]`) does not include Quality, so no existing device changes behavior. [Source: pipeline.rs:543-566 loop; needs_configurable_expansion default at fetch.rs:151-156]

### B — Version detection & preference (#34)

3. **Deterministic version-trait detection.** A pure helper classifies each `Song` into a small **closed set** of version traits, detected case-insensitively from `title` and `album_title` markers (no provider call, no clock, no RNG):
   - `Studio` — the **absence** of any other recognized marker (the plain album/studio cut).
   - `Live` — markers: `live`, `(live`, `- live`, `live at`, `live in`, `unplugged`.
   - `Remastered` — markers: `remaster`, `remastered`, `remaster)`, `(YYYY remaster…)`.
   - `Remix` — markers: `remix`, ` rmx`, `re-mix`.
   - `Acoustic` — markers: `acoustic`.
   - `Demo` — markers: `demo`.

   Marker matching is conservative (substring on a lowercased string, anchored to word/parenthesis boundaries where listed) to avoid false positives (e.g. "Alive" must not match `live`; "Demolition" must not match `demo`). A song may match multiple traits (e.g. "Song (Live) [2011 Remaster]") — detection returns the **set**. Document the exact marker list inline. [Source: brainstorm #34 via sprint-change-proposal-2026-06-14-configurable-auto-fill.md line 111; Song has no version field → heuristic from text]

4. **Version preference is an ordered list (config).** A new optional config field `version_preference: Vec<VersionTrait>` (default empty) expresses, in order, which version traits the user prefers (earlier = more preferred). A candidate's **version rank** = the index of the first listed trait it matches; a candidate matching none of the listed traits ranks **last** (worst). `version_preference = []` (default) ⇒ every candidate ties on version (today's behavior). The list is deduplicated and any unknown trait string is dropped on parse (best-effort, logged) so a malformed config never aborts the slot — mirror `parse_tiers`' tolerance ([fetch.rs](../../hifimule-daemon/src/auto_fill/fetch.rs)). Must remain deterministic — no RNG. [Source: 13.1 `parse_tiers` malformed-tolerance precedent, fetch.rs]

5. **Version preference acts as an ordering tiebreak.** When `version_preference` is non-empty, version rank participates in `compare_by_ordering` as a ranking dimension (lower rank = better, placed first). It applies **whether or not** best-version (AC 6) is enabled — i.e. with best-version off, a non-empty `version_preference` simply biases the sort so preferred versions appear earlier. Precedence vs. the other ordering keys: version preference is applied **after** the explicit `ordering` keys as a final tiebreak (so user-chosen favorites/playcount/quality still dominate) — unless the dev's design note (see Dev Notes) justifies otherwise; document the chosen precedence and unit-test it.

### C — Best-version resolution (#11)

6. **Best-version collapses same-song duplicates, keeping the best.** A new optional config flag `best_version: bool` (default `false`). When `true`, the pipeline collapses candidates that represent the **same logical song** down to a single winning version **globally across all source pools** (primary + fallback), before unit grouping and selection. The **logical key** is `(normalized_artist, normalized_base_title)`:
   - `normalized_artist` = `artist_name` lowercased + whitespace-trimmed + internal-whitespace-collapsed; `None`/empty artist ⇒ the candidate is **not** collapsed (treated as unique — never merge across unknown artists).
   - `normalized_base_title` = `title` lowercased, trimmed, with recognized **version markers stripped** (trailing/parenthetical/bracketed `(Live)`, `- 2011 Remaster`, `[Acoustic]`, ` - Live at …`, etc.) and internal whitespace collapsed.
   - The **winner** per logical key is chosen by a deterministic comparator: (1) version preference rank (AC 4, when set), then (2) quality rank (AC 1), then (3) the pipeline's full `ordering` (AC 2/5) as the final tiebreak, then (4) `song.id` lexicographic as the ultimate deterministic tiebreak (never leave it RNG-/order-dependent).
   - Implementation: build the global `logical_key → winning_song_id` map across the union of pools, then drop from **every** pool the candidates whose logical key maps to a *different* id. The winner stays in whichever source(s) it appears in; the existing Selector dedup-by-`song.id` ([pipeline.rs:687](../../hifimule-daemon/src/auto_fill/pipeline.rs#L687)) then handles the winner appearing in more than one pool. `best_version = false` (default) ⇒ no collapse (today's behavior). [Engine change in `run_pipeline` — see Dev Notes "Best-version design note".]

7. **Best-version is conservative and never over-merges.** Distinct songs must never collapse: a `None`/empty artist, or a `normalized_base_title` that differs after stripping, yields distinct logical keys. Best-version operates at **track granularity** and is intended for `Unit::Track`; album/artist-level "best version of the album" is explicitly **out of scope** (a possible future story). When `Unit` is `Album`/`Artist`, best-version still de-dupes by the track-level logical key (documented behavior — note that it may drop a duplicate track shared across album versions); do **not** attempt album-version resolution here. The collapse never produces a 0-byte result and never exceeds the budget ceiling (it only *removes* candidates; the existing budget guarantees are untouched).

### D — Routing, UI, i18n, scope

8. **Configurable-path routing recognizes the new fields.** Given a pipeline whose only non-default aspect is a Quality/Version setting (`OrderingKey::Quality` in `ordering`, a non-empty `version_preference`, or `best_version = true`), `needs_configurable_expansion` ([fetch.rs:144-173](../../hifimule-daemon/src/auto_fill/fetch.rs#L144-L173)) returns `true` so the materialized engine path runs. A non-default `ordering` already triggers this; add a `quality_default` (or equivalent) check for the new `best_version`/`version_preference` config so a *quality-only* pipeline (default ordering but `best_version = true`) still routes correctly. Add a test for each new trigger.

9. **Configuration UI exposes the new controls.** Under the **Advanced** disclosure of the Auto-Fill pipeline builder ([AutoFillPanel.ts](../../hifimule-ui/src/components/AutoFillPanel.ts), mirror the Memory/tiers section at lines 260-318), add a **Quality & Version** stage rendering: (a) a **best-version** switch, and (b) an **ordered version-preference editor** (add/remove/reorder version traits, modeled on the existing ordering editor at [AutoFillPanel.ts:375-396](../../hifimule-ui/src/components/AutoFillPanel.ts#L375-L396) and the tiers editor at `:292-318`). The existing **"Highest quality"** ordering key (`basket.autofill.ordering_quality`) is already addable in the Ordering section — no change there. Each control reads/writes the matching field on the frontend pipeline type ([state/autoFill.ts](../../hifimule-ui/src/state/autoFill.ts)), is captured on save via the input handlers (mirror `#af-cooldown`/`af-tier-*` capture), and round-trips through `autoFill.setPipeline` / `get_daemon_state`. Edits invalidate the debounced live preview ([AutoFillPanel.ts:567-618](../../hifimule-ui/src/components/AutoFillPanel.ts) preview wiring). The simple (non-Advanced) default path is unchanged. [Source: ux-design-specification.md §5.3 (Advanced disclosure, collapsible stage sections)]

10. **i18n parity across all 4 locales.** New UI strings (Quality/Version stage label, best-version label + hint, version-preference label + hint + add label, and a display label per `VersionTrait`) are added to **all 4 locales** (`en`, `fr`, `es`, `de`) under the `basket.autofill.*` snake_case convention (current parity after 13.1: **65×4**). The i18n parity test stays green. [Source: 13.1 completion "65×4 parity"; catalog.json existing `basket.autofill.ordering_quality` at line 125]

11. **Backward compatibility & scope.** A pipeline with default Quality/Version settings (`best_version = false`, empty `version_preference`, and no `Quality` ordering key) behaves **exactly** as today — zero migration. The legacy fast path (`run_auto_fill*`) is untouched. The `OrderingKey::Quality` enhancement (AC 1) only affects pipelines that explicitly list `Quality` in `ordering`. Config stays in the manifest; **no DB, no `autofill_history`/rotation interaction** in this story. Do **NOT** implement other Epic 13 features: memory/rotation (13.1 — done), discovery sources (13.3), rarity draws & pity timer (13.4), context/encoding-from-goals (13.5), advanced units & promotion (13.6).

12. **Build & tests green.** `rtk cargo test -p hifimule-daemon` passes (no regression; if the sandbox blocks mockito/networking run targeted `rtk cargo test -p hifimule-daemon auto_fill::`). New tests cover: `format_quality_rank` (lossless > lossy > unknown; case-insensitive); lossless-aware `OrderingKey::Quality` ordering (FLAC ranks above 320k MP3; bitrate breaks ties within a tier); version-trait detection (each trait + the false-positive guards "Alive"/"Demolition"; multi-trait songs); `version_preference` parse (dedup, unknown dropped, empty = no-op) + ordering tiebreak; best-version logical-key normalization (groups versions; does NOT over-merge distinct titles/artists; `None` artist stays unique); best-version winner selection (preference > quality > ordering > id) and global cross-pool collapse; `best_version = false`/empty preference == today; `needs_configurable_expansion` for each new trigger; serde round-trip for the new config. `rtk cargo clippy -p hifimule-daemon --all-targets` adds no new warnings in touched modules. Frontend `rtk npx tsc --noEmit` + `rtk npm run build` stay green; `rtk cargo test -p hifimule-i18n` parity green.

## Tasks / Subtasks

- [x] **Format-aware quality ordering (#13)** (`hifimule-daemon/src/auto_fill/pipeline.rs`) (AC: 1, 2)
  - [x] Add a pure `format_quality_rank(song: &Song) -> u8` helper: lossless suffix/content-type (`flac`/`alac`/`wav`/`aiff`/`ape`/`wavpack`; mime `audio/flac` etc.) → high tier, lossy → low tier, unknown → lowest. Case-insensitive; read `suffix` then `content_type`; never the title.
  - [x] In `compare_by_ordering` change the `OrderingKey::Quality` arm to compare `(format_quality_rank(b), b.bitrate)` vs `(format_quality_rank(a), a.bitrate)` (lossless-first, then bitrate desc).
  - [x] Tests: FLAC (no/low bitrate) ranks above 320k MP3; two MP3s break by bitrate; pipelines without `Quality` unchanged. (+ Antoine persona strengthened.)

- [x] **Version detection + preference (#34)** (`hifimule-daemon/src/auto_fill/pipeline.rs`) (AC: 3, 4, 5)
  - [x] Add a closed `VersionTrait` enum (`Studio`/`Live`/`Remastered`/`Remix`/`Acoustic`/`Demo`), `#[serde(rename_all = "camelCase")]`, and a pure `detect_version_traits(song: &Song) -> Vec<VersionTrait>` reading `title`+`album_title` with the documented marker list and false-positive guards (`has_word` word-anchoring).
  - [x] Add `version_preference: Vec<VersionTrait>` to the new config (malformed-tolerant deserializer); a `version_rank(song, &[VersionTrait]) -> usize` helper (index of first matched trait; non-match = `len`/worst).
  - [x] Thread `version_preference` into `compare_by_ordering` as a final trailing tiebreak (applied after the explicit keys; documented in AC 5 + code comment). Pure signature kept.
  - [x] Tests: each trait detected; "Alive"/"Demolition"/"Mixtape" not misdetected; multi-trait; album-title read; preference orders preferred-first; empty preference = no-op; favorite dominates the tiebreak; unknown/non-string/dup dropped on parse.

- [x] **Best-version resolution (#11)** (`hifimule-daemon/src/auto_fill/pipeline.rs`) (AC: 6, 7)
  - [x] Add the config field `best_version: bool`. Add pure helpers `logical_key(song) -> Option<(String, String)>` (normalized artist + base title; `None` when artist missing/empty) and `strip_version_markers(title) -> String` (+ `strip_bracketed_markers`, `normalize_ws`).
  - [x] In `run_pipeline`, when `best_version` is set: build the global `logical_key → winning_song` map across the union of all `input.pools` using the AC-6 comparator (`best_version_cmp`: preference → quality → ordering → id), then filter each pool to drop losing-version candidates. Done **before** `build_source_units`. Selector dedup-by-id is the second line of defense.
  - [x] Tests: collapse keeps the FLAC studio over the lossy live; preference flips the winner; distinct titles/artists and `None`-artist candidates survive; cross-pool collapse works; `best_version=false` unchanged; never emits 0-byte / over-budget; `strip_version_markers` normalization.

- [x] **Config shape + routing** (`hifimule-daemon/src/auto_fill/pipeline.rs`, `fetch.rs`) (AC: 8, 11)
  - [x] Added a `QualityStage { best_version: bool, version_preference: Vec<VersionTrait> }` struct on `AutoFillPipeline` with `#[derive(Default)] #[serde(default, rename_all = "camelCase")]` (mirrors `MemoryStage`/`FilterStage`).
  - [x] In `needs_configurable_expansion` added `quality_default = p.quality == QualityStage::default()` to the AND-chain so `best_version`/`version_preference`-only pipelines route to the engine path.
  - [x] Tests: `needs_configurable_expansion` true for `best_version`-only, `version_preference`-only, and `Quality`-ordering-only pipelines; default stays on fast path.

- [x] **Frontend Quality & Version controls** (`hifimule-ui/src/components/AutoFillPanel.ts`, `state/autoFill.ts`) (AC: 9)
  - [x] In `state/autoFill.ts` added a `VersionTrait` string-union type + `VERSION_TRAITS`, a `QualityStage` type (`{ bestVersion?: boolean; versionPreference?: VersionTrait[] }`), its default, hydrated it in `normalizePipeline`, and emit it from `serializePipeline` **only when meaningful** (dedup + omit defaults) — mirrors the Memory-fields pattern.
  - [x] In `AutoFillPanel.ts` render a Quality & Version stage under Advanced: a best-version `sl-switch` + an ordered version-preference editor (add/remove/reorder; mirrors `renderOrderingSection` + tiers editor). Wired input capture + preview invalidation.

- [x] **i18n keys ×4 locales** (`hifimule-i18n/catalog.json`) (AC: 10)
  - [x] Added `basket.autofill.quality_version`, `.best_version` (+`_hint`), `.version_preference` (+`_hint`, +`_add`), `.no_version_preference`, and `.version_trait_studio`/`_live`/`_remastered`/`_remix`/`_acoustic`/`_demo` to `en`/`fr`/`es`/`de` (13 keys ×4 → 65×4 → 78×4). Parity verified.

- [x] **Full verification** (AC: 12)
  - [x] `rtk cargo test -p hifimule-daemon` (557 pass), `rtk cargo clippy -p hifimule-daemon --all-targets` (no new warnings in touched modules), `rtk cargo test -p hifimule-i18n` (6 pass), frontend `rtk npx tsc --noEmit` (clean) + `rtk npm run build` (green).

### Review Findings

_Code review 2026-06-14 (baseline 6b9a90d → HEAD 20cb42c). Layers: Blind Hunter, Edge Case Hunter, Acceptance Auditor. 2 decision-needed (both resolved → patch), 3 patch — **all 3 applied & verified** (83 auto_fill tests green, clippy clean), 5 deferred, 13 dismissed as noise/by-design. All 12 ACs verified satisfied by the Acceptance Auditor (no Critical/High). Status → done._

- [x] [Review][Patch] Tighten merge-key stripping to word-anchored markers (Decision 1 → option 1) [hifimule-daemon/src/auto_fill/pipeline.rs] — FIXED: `segment_has_marker` now word-anchors every marker (incl. remix/acoustic/re-mix) so a marker substring embedded in a larger word no longer strips a parenthetical; `detect_version_traits` keeps its looser substring tagging. Test `strip_version_markers_word_anchors_remix_and_acoustic`.
- [x] [Review][Patch] Make best-version collapse budget-aware (Decision 2 → option 2) [hifimule-daemon/src/auto_fill/pipeline.rs] — FIXED: new `fits_ceiling` helper + a budget-fit tier (0) in `best_version_cmp`; `collapse_best_version`/`run_pipeline` thread the byte ceiling so a winner that can never fit yields to a fitting lesser version. No-op for unbounded budgets. Test `best_version_falls_back_to_a_fitting_version_over_budget`.
- [x] [Review][Patch] `strip_version_markers` strips only the last ` - ` dash-suffix once [hifimule-daemon/src/auto_fill/pipeline.rs] — FIXED: looped the trailing dash-suffix strip (stops at first non-marker tail). Test `strip_version_markers_strips_stacked_dash_suffixes`.
- [x] [Review][Defer] Per-comparison recomputation of `detect_version_traits` [hifimule-daemon/src/auto_fill/pipeline.rs:777-786,858-881] — deferred, performance-only (allocations in O(n log n) sort path; fine at typical pool sizes).
- [x] [Review][Defer] `collapse_best_version` clones the full `PipelineInput` even when nothing collapses [hifimule-daemon/src/auto_fill/pipeline.rs:909] — deferred, performance-only.
- [x] [Review][Defer] Plural/gerund remaster forms not detected [hifimule-daemon/src/auto_fill/pipeline.rs:754] — deferred; `"Remasters"`/`"remastering"` miss `has_word("remaster")`, inconsistent with remix substring matching. Minor missed-detection, not over-merge.
- [x] [Review][Defer] Non-ASCII case folding not handled [hifimule-daemon/src/auto_fill/pipeline.rs:742,793] — deferred; ASCII-only lowercase means accented duplicates/markers ("CAFÉ" vs "café") won't fold. No spec requirement for unicode.
- [x] [Review][Defer] Nested/unbalanced bracket groups leave a stray bracket char [hifimule-daemon/src/auto_fill/pipeline.rs:802-821] — deferred; `"(feat. X (Live))"` strips to a residual `")"`. Rare; proper nested matching is non-trivial.

## Dev Notes

### What this story is (and is not)

A **pure-engine + UI** story. Unlike 13.1 there is **no DB work, no new provider calls, no async fetch logic, no `autofill_history`/rotation interaction**. Every signal comes from `Song` fields already present in the materialized candidate pools (`bitrate_kbps`, `suffix`, `content_type`, `title`, `album_title`, `artist_name` — [domain/models.rs:26-50](../../hifimule-daemon/src/domain/models.rs#L26-L50)). The work is: (1) make the existing `OrderingKey::Quality` format-aware; (2) add deterministic version detection + an ordered preference; (3) add a global best-version collapse pre-pass. All three are **deterministic** and live in `pipeline.rs`; the fetch layer needs only the `needs_configurable_expansion` gate update — it already calls `run_pipeline` ([fetch.rs `expand_with_pipeline`](../../hifimule-daemon/src/auto_fill/fetch.rs)).

### The pure-function discipline (non-negotiable)

The engine ([auto_fill/pipeline.rs](../../hifimule-daemon/src/auto_fill/pipeline.rs)) is **pure, synchronous, deterministic** — no `SystemTime::now()`, no RNG ([pipeline.rs:560-561](../../hifimule-daemon/src/auto_fill/pipeline.rs#L560-L561) — `OrderingKey::Random` is a deliberate no-op). All three modifiers here are deterministic: quality/version detection is string/flag comparison; best-version is a deterministic collapse whose ties are broken down to `song.id` lexicographic. **Never** introduce randomness into version selection. The four-persona suite ([pipeline.rs:787-976](../../hifimule-daemon/src/auto_fill/pipeline.rs#L787-L976)) — including `persona_antoine_audiophile_quality_first` ([pipeline.rs:914-938](../../hifimule-daemon/src/auto_fill/pipeline.rs#L914-L938)) — depends on this. Strengthen Antoine's test with the lossless-aware quality ordering; **do not** add `if persona == …` branches (every behavior must emerge from config composition).

### Best-version design note (highest-risk sub-feature — keep it conservative)

The source brainstorm catalog (#11/#34) is **not in this checkout** (confirmed in 13.1 dev notes), so semantics are defined here — keep them minimal, deterministic, and conservative, exactly as 13.1 did for rotation tiers:

- **Logical key = `(normalized_artist, normalized_base_title)`.** Conservative normalization: lowercase, trim, collapse internal whitespace, strip only the *recognized* version markers from the title. **When in doubt, do NOT merge** — a `None`/empty artist makes the candidate unique; an unrecognized suffix leaves the title intact (so it stays distinct). Over-merging two genuinely different songs is worse than missing a collapse.
- **Global, not per-source.** Build the winner map across the union of all pools so the best version wins even when versions span sources (e.g. live copy in a playlist, studio FLAC in the library). Then remove losing versions from every pool. The winner naturally remains where it appeared; Selector dedup-by-`song.id` ([pipeline.rs:686-688](../../hifimule-daemon/src/auto_fill/pipeline.rs#L686-L688)) covers a winner that appears in several pools.
- **Winner comparator order:** version-preference rank (AC 4) → quality rank (AC 1) → full `ordering` (AC 2) → `song.id` lexicographic (ultimate deterministic tiebreak). This makes the result independent of pool iteration order.
- **Track granularity only.** Album/artist "best version" is out of scope (AC 7). If best-version proves larger than the rest of the story combined, **flag it** (scope note) rather than expanding silently.
- **Observability (optional, recommended):** when best-version drops losers, you may annotate the winner's `priority_reason` (e.g. append `+bestVersion`) like 13.1 tagged tiers — but do not break the existing `priority_reason` shape the preview UI parses ([reason_for, pipeline.rs:753-775](../../hifimule-daemon/src/auto_fill/pipeline.rs#L753-L775)).

### Version-trait detection — false-positive discipline

Marker matching must not fire on substrings of unrelated words. Concretely: `live` must not match "Alive"/"Believe"; `demo` must not match "Demolition"/"Demon"; `remix` is safe but `mix` alone is **not** a marker (would hit "Mixtape"). Anchor markers to parenthesis/bracket/dash boundaries or surrounding whitespace where listed in AC 3, and lowercase once up front. Unit-test the guards explicitly. The detector reads `title` and `album_title` only.

### Where the quality ordering already is

`OrderingKey::Quality` **already exists and is wired end-to-end** (engine [pipeline.rs:161,556-559](../../hifimule-daemon/src/auto_fill/pipeline.rs#L161); frontend type [state/autoFill.ts:7,58](../../hifimule-ui/src/state/autoFill.ts#L7); i18n `basket.autofill.ordering_quality` [catalog.json:125](../../hifimule-i18n/catalog.json#L125); addable in the Ordering UI [AutoFillPanel.ts:375-396](../../hifimule-ui/src/components/AutoFillPanel.ts#L375-L396)). **Do not add a new ordering key for quality** — enhance the existing arm (AC 1). The #13 "quality modifier" deliverable is the lossless-aware enhancement + confirming the existing wiring, not a re-build.

### Current code being changed (read before writing)

- **Engine:** [pipeline.rs:54-73](../../hifimule-daemon/src/auto_fill/pipeline.rs#L54-L73) (`AutoFillPipeline` — add `quality` stage here), `:146-162` (`OrderingKey` — `Quality` arm), `:226-241` (`Candidate`), `:306-365` (`run_pipeline` — best-version pre-pass goes before `build_source_units`), `:384-405` (`build_source_units` — filter→unit→ordering), `:541-572` (`compare_by_ordering`/`fav_rank` — Quality arm + version tiebreak), `:577-587` (`estimated_size`), `:632-728` (`Selector`/`fill` — existing dedup-by-`song.id` at `:686-688`), `:728-976` (fixtures `song_sized`/`song_bitrate`/`cand` + 4-persona suite; Antoine at `:914-938`).
- **Song fields available:** [domain/models.rs:26-50](../../hifimule-daemon/src/domain/models.rs#L26-L50) — `title`, `album_title` (serde `albumName`), `artist_name`, `bitrate_kbps`, `content_type`, `suffix`. No `genre`/version field exists → version traits are text-derived.
- **Routing:** [fetch.rs:144-173](../../hifimule-daemon/src/auto_fill/fetch.rs#L144-L173) (`needs_configurable_expansion` — add `quality_default`); `expand_with_pipeline` already calls `run_pipeline`, so no other fetch change.
- **Frontend:** [state/autoFill.ts:6-7](../../hifimule-ui/src/state/autoFill.ts#L6-L7) (`SourceKind`/`OrderingKey` unions — add `VersionTrait`), `:51` (`ordering` on the pipeline type — add `quality` stage), `:65-94` (default + parser/hydrate), `:107-145` (`serializePipeline` + clone — emit `quality` only when meaningful). [AutoFillPanel.ts:208](../../hifimule-ui/src/components/AutoFillPanel.ts#L208) (`renderStage` helper), `:253-318` (Memory + tiers editor — mirror for the new stage), `:375-396` (ordering editor — mirror for version-preference editor), `:513-536` (ordering/tier handlers — mirror capture), `:567-618` (debounced preview).
- **i18n:** [catalog.json](../../hifimule-i18n/catalog.json) — 4 locales; `basket.autofill.*` snake_case; existing `ordering_quality` at line 125 / 457 / 789 / (de). 65 keys ×4 today.

### Architecture compliance (non-negotiable)

- **Config in the manifest only.** The new `quality` stage is pipeline **config** → lives in `manifest.autoFill : Map<serverId, AutoFillPipeline>`. This story writes **nothing** to the daemon DB ([architecture.md line 922](../../_bmad-output/planning-artifacts/architecture.md) — never put user config in the DB; here there is simply no runtime state). [Source: architecture.md lines 792-807]
- **Reuse Epic 12 types — do not redefine** `AutoFillPipeline`/`OrderingKey`/`Song`/`Candidate`. Add the `QualityStage` struct and `VersionTrait` enum; extend `OrderingKey::Quality`'s comparison; nothing else changes shape.
- **Per-server routing & legacy fast path are untouched** — this story never sees a provider directly; it only adds pure-engine logic + the routing gate. [architecture.md lines 920-921]

### Previous story intelligence (13.1)

- **`needs_configurable_expansion` is the routing gate** — every new non-default pipeline aspect must be added to its AND-chain or the slot silently falls back to the legacy fast path and your feature never runs. 13.1 added the memory check; add the `quality` check here (AC 8) and test it.
- **Malformed-config tolerance pattern:** 13.1's `parse_tiers` ([fetch.rs](../../hifimule-daemon/src/auto_fill/fetch.rs)) logs and ignores malformed input rather than aborting. Apply the same to `version_preference` parsing (unknown traits dropped, deduped) — a bad config must degrade to "no preference", never fail the slot.
- **Serialize-only-when-meaningful:** 13.1's `serializePipeline` emits new Memory fields only when non-default to keep the manifest JSON clean and backward-compatible. Do the same for the `quality` stage (omit when `bestVersion=false` and `versionPreference=[]`).
- **i18n parity is a hard gate (currently 65×4):** every new key in all 4 locales or the parity test fails. 13.1 went 58×4 → 65×4; this story adds ~13 keys → ~78×4 (count precisely).
- **Persona suite is the engine acceptance bar:** Antoine (audiophile, quality-first) already asserts bitrate ordering — extend him to assert lossless-first and, ideally, a best-version collapse. No `if persona` branches.
- **Sandbox caveat (recurring across Epic 12/13):** full `rtk cargo test -p hifimule-daemon` may not finish where mockito/local networking is blocked. New tests here are **pure engine** (`auto_fill::`) + serde — run targeted: `rtk cargo test -p hifimule-daemon auto_fill::`.

### Git intelligence

Recent commits (`6b9a90d Review 13.1`, `b47d693 Dev 13.1`, `0a537f8 Story 13.1`, `8dc855a Fix issue`, `f1790db Review 12.7`) confirm 13.1 is closed and this is the second Epic 13 story. No competing in-flight changes to `auto_fill/pipeline.rs`, `fetch.rs`, `AutoFillPanel.ts`, or `state/autoFill.ts`. The frozen contract from 13.1 holds: legacy fast path + default pipelines behave identically — that invariant must survive this story.

### Latest technical context

- **No new crate dependency.** Version detection is plain string ops; quality ranking reads existing `Song` fields; `version_preference`/`best_version` parse via existing `serde`/`serde_json ~1.0`. Rust edition 2024 (let-chains in use — see [pipeline.rs:462-464](../../hifimule-daemon/src/auto_fill/pipeline.rs#L462-L464)).
- **No clock, no RNG** anywhere in this story — every decision derives from `Song` fields + config.

### Project Structure Notes

- Daemon (Rust): all engine logic in `hifimule-daemon/src/auto_fill/pipeline.rs`; the routing gate in `auto_fill/fetch.rs`. Binary crate (no `lib.rs`); tests are `#[cfg(test)] mod tests` in `pipeline.rs`. Do **not** put version/quality types in `domain/models.rs` (provider-neutral entities only) — they belong in `auto_fill/pipeline.rs`.
- Frontend (TS): `hifimule-ui/src/components/AutoFillPanel.ts`, `state/autoFill.ts`; i18n catalog `hifimule-i18n/catalog.json`. No UI unit-test framework configured — rely on `tsc` + build + manual preview, matching the existing pattern.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Epic-13 (lines 3079-3089, Story 13.2: best-version #11, quality modifier #13, version preference #34)]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-06-14-configurable-auto-fill.md (FR54 line 82; Epic 13 table line 111; ambition-tier cheap-equivalent model)]
- [Source: _bmad-output/planning-artifacts/architecture.md#Auto-Fill-Pipeline-Model (lines 788-826, pipeline shape `ordering: [… "quality" …]` line 800); #Enforcement (lines 913-923, config-in-manifest line 922)]
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md §5.3 (Advanced disclosure, collapsible stage sections)]
- [Source: _bmad-output/implementation-artifacts/13-1-memory-and-rotation-strategies.md (pure-function discipline, `needs_configurable_expansion` gate, malformed-tolerance, serialize-when-meaningful, i18n 65×4, persona suite, sandbox caveat)]
- [Source: _bmad-output/implementation-artifacts/12-1-autofill-pipeline-domain-model-and-engine.md (engine model, OrderingKey::Quality, persona suite, determinism)]
- [Source: hifimule-daemon/src/auto_fill/pipeline.rs:54-73,146-162,226-241,306-365,384-405,541-572,577-587,632-728,787-976]
- [Source: hifimule-daemon/src/auto_fill/fetch.rs:144-173 (needs_configurable_expansion), expand_with_pipeline]
- [Source: hifimule-daemon/src/auto_fill/mod.rs:34-52 (AutoFillItem)]
- [Source: hifimule-daemon/src/domain/models.rs:26-50 (Song fields: bitrate_kbps, content_type, suffix, title, album_title, artist_name)]
- [Source: hifimule-ui/src/state/autoFill.ts:6-7,51,65-145; components/AutoFillPanel.ts:208,253-396,513-536,567-618; hifimule-i18n/catalog.json:121-127]

## Dev Agent Record

### Agent Model Used

claude-opus-4-8 (dev-story workflow)

### Debug Log References

- `rtk cargo test -p hifimule-daemon auto_fill::` → 80 passed (engine + routing)
- `rtk cargo test -p hifimule-daemon` → 557 passed (no regression; full suite ran in-sandbox)
- `rtk cargo clippy -p hifimule-daemon --all-targets` → no new warnings in `pipeline.rs`/`fetch.rs`/`device/tests.rs` (89 pre-existing repo-wide warnings unchanged)
- `rtk cargo test -p hifimule-i18n` → 6 passed; catalog parity 343 keys ×4 (78 `basket.autofill.*` ×4)
- `rtk npx tsc --noEmit` → no errors; `rtk npm run build` → green

### Completion Notes List

- **#13 format-aware quality:** `format_quality_rank(song) -> u8` (lossless 2 > lossy 1 > unknown 0), read from `suffix` then `content_type` mime subtype (with `x-` strip), case-insensitive, never the title. `OrderingKey::Quality` arm now compares `(format_rank, bitrate)` lossless-first. Pipelines without `Quality` in `ordering` are byte-for-byte unchanged (legacy default never lists it).
- **#34 version detection + preference:** closed `VersionTrait` enum; `detect_version_traits` reads title+album with a `has_word` word-anchored matcher so "Alive"/"Believe"/"Demolition"/"Demon"/"Mixtape" don't false-match. `Studio` = absence of any other marker. `version_preference` participates in `compare_by_ordering` as a **trailing tiebreak applied after the explicit ordering keys** (AC-5 decision — explicit favorites/quality/etc. dominate; documented inline). Field has a malformed-tolerant deserializer (unknown/non-string/duplicate entries dropped, never aborts the slot — mirrors 13.1 `parse_tiers`).
- **#11 best-version:** `best_version: bool`. When set, `collapse_best_version` builds a global `logical_key → winning song` map across the union of all pools, then drops losing-version candidates from every pool — before unit grouping. `logical_key = (normalized_artist, normalized_base_title)`; `None`/empty artist or empty base title ⇒ never collapsed (conservative). Winner comparator `best_version_cmp`: preference → quality → full ordering → `song.id` (deterministic, pool-order-independent). Collapse only removes candidates, so budget/0-byte guarantees are untouched; Selector dedup-by-id handles a winner appearing in several pools. Track granularity only (album/artist best-version out of scope per AC 7).
- **Config home:** new `QualityStage` struct on `AutoFillPipeline` (mirrors `MemoryStage`/`FilterStage`; `#[serde(default, rename_all="camelCase")]`). Routing gate `needs_configurable_expansion` gains `quality_default` so a quality-only pipeline (e.g. `best_version=true` with default ordering) reaches the engine path.
- **Frontend:** `VersionTrait`/`VERSION_TRAITS`/`QualityStage` added to `state/autoFill.ts`; `serializePipeline` emits `quality` only when meaningful (dedup + omit defaults) for clean, backward-compatible manifests. New "Quality & Version" stage under Advanced: best-version switch + ordered version-preference editor (add/remove/reorder, mirroring the Ordering editor), wired to capture + preview invalidation.
- **i18n:** 13 new `basket.autofill.*` keys ×4 locales (65×4 → 78×4); parity confirmed (all locales 343 keys, no missing/extra).
- **Backward compatibility:** default `QualityStage` (`best_version=false`, empty `version_preference`, no `Quality` ordering key) behaves exactly as today; legacy fast path untouched; existing serde/round-trip and persona tests still green (Antoine strengthened to assert lossless-first).

### File List

- `hifimule-daemon/src/auto_fill/pipeline.rs` — `VersionTrait` enum, `QualityStage` struct + tolerant `version_preference` deserializer, `quality` field on `AutoFillPipeline` (+`default_legacy`), `format_quality_rank`, `has_word`/`segment_has_marker`/`detect_version_traits`/`version_rank`, `normalize_ws`/`strip_bracketed_markers`/`strip_version_markers`/`logical_key`/`best_version_cmp`/`collapse_best_version`, lossless-aware `OrderingKey::Quality` arm + version tiebreak in `compare_by_ordering`, best-version pre-pass in `run_pipeline`, new test suite + strengthened Antoine persona.
- `hifimule-daemon/src/auto_fill/fetch.rs` — import `QualityStage`; `quality_default` in `needs_configurable_expansion` (+doc); routing test.
- `hifimule-daemon/src/device/tests.rs` — `rich_pipeline` gains a non-default `quality` stage (import `QualityStage`).
- `hifimule-ui/src/state/autoFill.ts` — `VersionTrait` type, `VERSION_TRAITS`, `QualityStage` interface, `quality` on `AutoFillPipeline`, default/normalize/serialize handling.
- `hifimule-ui/src/components/AutoFillPanel.ts` — Quality & Version stage render + version-preference editor + event bindings + `moveVersionPreference`.
- `hifimule-i18n/catalog.json` — 13 new `basket.autofill.*` keys ×4 locales.

## Change Log

- 2026-06-14 — Story 13.2 implemented (dev-story). Engine: lossless-aware `OrderingKey::Quality` (#13), `VersionTrait` detection + ordered `version_preference` tiebreak (#34), global best-version collapse (#11) via new `QualityStage` config; routing gate updated. Frontend Quality & Version controls + 13 i18n keys ×4. 557 daemon tests + 6 i18n + tsc/build all green; clippy clean in touched modules. Status → review.
- 2026-06-14 — Story 13.2 created via create-story workflow (ready-for-dev). Scope: extend the pure-function auto-fill engine with format-aware quality ordering (#13 — lossless-first enhancement of the existing `OrderingKey::Quality`), deterministic version-trait detection + an ordered version-preference config (#34), and global best-version resolution that collapses same-logical-song duplicates keeping the best version (#11). Engine + UI only — no DB, no new provider calls, no `autofill_history`/rotation interaction. New `QualityStage` config in the manifest; new Advanced UI controls (best-version switch + version-preference editor) + ~13 i18n keys ×4 locales. Backward compatible (default Quality/Version == today; legacy fast path untouched; Quality-key enhancement affects only pipelines that list it).
