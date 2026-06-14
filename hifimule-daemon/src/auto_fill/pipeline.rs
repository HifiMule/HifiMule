//! Configurable auto-fill pipeline (Epic 12) — pure-function selection algebra.
//!
//! The unifying design insight: an auto-fill definition is one **pipeline config** per
//! `(device, portable serverId)` pair, and today's hardcoded
//! favorites→playCount→dateCreated algorithm is just the **default single-Ordering-stage
//! pipeline**. This module expresses the whole selection as composable pure functions:
//!
//! ```text
//! filter → source-blend(by share) → unit → ordering → dedupe-vs-memory → budget(+fallback)
//! ```
//!
//! ## Fetching vs selection — the non-negotiable split
//!
//! The single most important architectural decision is that **fetching** (async, impure,
//! provider-bound) is separate from **selection** (sync, pure, fixture-testable). The pure
//! engine here receives *already-materialized* song pools plus a [`HistorySnapshot`] supplied
//! by the caller and returns the ordered selection. There is **no network, no `async`, no
//! `MediaProvider` call, and no clock/RNG read** inside this core. The async layer that
//! materializes pools from a `MediaProvider` and the per-server routing via
//! `get_provider_by_server_id` belong to Stories 12.3/12.4, never here.
//!
//! ## Config vs history — the storage split (enforced even though 12.1 persists nothing)
//!
//! Pipeline [`AutoFillPipeline`] **config** is portable manifest data (persisted by Story 12.2);
//! runtime **history** (cooldown windows, stable-core, pity-timer) is daemon-DB, machine-local.
//! In 12.1 the [`MemoryStage`] engine logic only *consumes* a supplied [`HistorySnapshot`] — it
//! never reads a DB or a system clock. "Now" is a value carried on the snapshot.
//!
//! Story 12.1 deliberately ships the model + engine + tests with **no wiring** (AC #6): the
//! async fetch layer (12.3/12.4), RPC contracts (12.7), and UI (12.6) consume this surface in
//! later stories. Until then the public API is unreferenced by the binary, hence the
//! module-level `dead_code` allow below — the exhaustive unit tests exercise every path.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use super::AutoFillItem;
use crate::domain::models::Song;

// ---------------------------------------------------------------------------
// Config domain model (portable manifest shape — Story 12.2 persists this verbatim).
// Field names MUST match architecture.md#Auto-Fill-Pipeline-Model exactly.
// ---------------------------------------------------------------------------

/// A complete auto-fill pipeline configuration for one `(device, serverId)` pair.
///
/// Every stage is independently optional; a pipeline with as little as one stage configured
/// is valid. `#[serde(default)]` on every field means an empty or partial pipeline
/// deserializes cleanly, which is also how a legacy `{ enabled, maxBytes }` block is read once
/// it is mapped through [`AutoFillPipeline::default_legacy`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AutoFillPipeline {
    /// Whether auto-fill is active. The *engine* does not gate on this — enabling is a
    /// fetch-layer/caller concern (Story 12.3); [`run_pipeline`] runs regardless.
    pub enabled: bool,
    /// Tag/genre include-exclude filter applied per candidate. Empty = pass-through.
    pub filter: FilterStage,
    /// Ordered list of sources to draw from, optionally blended by `share`.
    pub sources: Vec<SourceEntry>,
    /// Selection granularity. Defaults to [`Unit::Track`].
    pub unit: Unit,
    /// Ordered ranking keys applied as a stable multi-key sort.
    pub ordering: Vec<OrderingKey>,
    /// Cooldown / played-exclusion / rotation modifiers. Consumes a supplied history snapshot.
    pub memory: MemoryStage,
    /// Byte / duration budget bounding the selection.
    pub budget: BudgetStage,
    /// Terminal fallback sources, applied in order to reach the budget once primary sources
    /// are exhausted.
    pub fallback: Vec<SourceEntry>,
    /// Quality & version modifiers (Story 13.2): lossless-aware quality ordering (#13 — see the
    /// [`OrderingKey::Quality`] arm), an ordered version-trait preference (#34), and best-version
    /// collapse (#11). All default ⇒ zero behavior change.
    pub quality: QualityStage,
}

/// Tag/genre filter. All fields default to empty, which means "pass everything through".
///
/// `Song` carries no genre/tag field today, so in 12.1 the filter operates against whatever
/// genres/tags the caller attaches to each [`Candidate`]. Full provider-driven genre/tag
/// enumeration is Story 12.4 — only the *config shape* is fixed here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct FilterStage {
    pub include_tags: Vec<String>,
    pub exclude_tags: Vec<String>,
    pub include_genres: Vec<String>,
    pub exclude_genres: Vec<String>,
}

/// One source to draw candidates from, with an optional proportional blend `share`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceEntry {
    pub kind: SourceKind,
    /// Identifies the concrete source instance when needed (e.g. a playlist id). Serialized
    /// as `ref` to match the manifest JSON shape.
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub ref_id: Option<String>,
    /// Blend weight in `0.0..=1.0`. `None`/unset sources split the remainder equally.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub share: Option<f32>,
}

impl SourceEntry {
    /// Convenience constructor for a bare source with no ref/share.
    pub fn new(kind: SourceKind) -> Self {
        Self {
            kind,
            ref_id: None,
            share: None,
        }
    }

    /// Lookup key into [`PipelineInput::pools`] — `(kind, ref_id)`.
    fn key(&self) -> SourceKey {
        SourceKey {
            kind: self.kind,
            ref_id: self.ref_id.clone(),
        }
    }
}

/// The kinds of source a pipeline can draw from. Each maps cleanly to one `MediaProvider`
/// method that the async fetch layer (Story 12.3/12.4) will call:
/// `Library`→`list_all_songs_page`, `Favorites`→`list_favorites`,
/// `History`→`list_recently_played`, `Playlist`→`get_playlist`.
/// Extensible for Epic 13 (e.g. genre / smart sources).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    Library,
    Favorites,
    History,
    Playlist,
}

/// Selection granularity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Unit {
    #[default]
    Track,
    Album,
    Artist,
}

/// A single ranking key. The pipeline `ordering` is an **ordered list** of these, applied as a
/// stable multi-key sort. Keys whose data lives on `Song` today are implemented; the rest are
/// reserved variants for Epic 13.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OrderingKey {
    /// Favorites first.
    Favorite,
    /// Higher play count first.
    PlayCount,
    /// More-recently-added first.
    DateCreated,
    /// Reserved for Epic 13 — a deterministic no-op in 12.1 (no entropy in the pure core).
    Random,
    /// Higher bitrate first.
    Quality,
}

/// Cooldown / rotation modifiers. In 12.1 only `cooldown_weeks` and `played_exclusion` are
/// consumed (against the supplied [`HistorySnapshot`]); the rest are reserved for Epic 13.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct MemoryStage {
    /// Exclude tracks synced within this many weeks (relative to `HistorySnapshot::now`).
    pub cooldown_weeks: Option<u32>,
    /// Exclude any track that has a recorded play in the snapshot.
    pub played_exclusion: bool,
    /// Reserved (Epic 13): fraction of the selection kept stable across runs.
    pub stable_core_pct: Option<f32>,
    /// Reserved (Epic 13): how aggressively repeats are tolerated.
    pub repeat_tolerance: Option<f32>,
    /// Reserved (Epic 13): tiered rotation buckets. Persisted verbatim; shape formalized later.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tiers: Option<serde_json::Value>,
}

/// Byte / duration budget. An unset `max_bytes` means "no byte ceiling" (duration may still cap).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct BudgetStage {
    pub max_bytes: Option<u64>,
    pub target_duration_secs: Option<u64>,
    pub headroom_bytes: Option<u64>,
}

/// A recording-version trait, detected purely from a song's title/album text (Story 13.2 #34).
/// `Song` carries no version field, so this is a heuristic over a **closed** marker set. A song may
/// match several traits at once. `Studio` is the *absence* of any other recognized marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VersionTrait {
    Studio,
    Live,
    Remastered,
    Remix,
    Acoustic,
    Demo,
}

/// Quality & version modifiers (Story 13.2). Both fields default to "off", so a default
/// `QualityStage` is byte-for-byte today's behavior and is omitted from the routing discriminator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct QualityStage {
    /// #11 — collapse same-logical-song duplicates to a single winning version globally across all
    /// pools (primary + fallback) before unit grouping. `false` ⇒ no collapse (today's behavior).
    pub best_version: bool,
    /// #34 — ordered version-trait preference (earlier = more preferred). A candidate's version rank
    /// is the index of the first listed trait it matches; non-matches rank last. Empty ⇒ no
    /// preference (every candidate ties on version). Deduplicated; unknown traits are dropped on
    /// parse (best-effort, like 13.1's `parse_tiers`) — a malformed entry never aborts the slot.
    #[serde(default, deserialize_with = "deserialize_version_preference")]
    pub version_preference: Vec<VersionTrait>,
}

/// Malformed-tolerant deserializer for [`QualityStage::version_preference`] (Story 13.2 #34, AC 4),
/// mirroring 13.1's `parse_tiers` discipline: read a list of arbitrary JSON values, keep the ones
/// that name a known [`VersionTrait`], silently drop the rest, and de-duplicate preserving order. A
/// bad config therefore degrades to "no preference" instead of aborting the whole pipeline parse.
fn deserialize_version_preference<'de, D>(deserializer: D) -> Result<Vec<VersionTrait>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Vec::<serde_json::Value>::deserialize(deserializer)?;
    let mut out: Vec<VersionTrait> = Vec::new();
    for value in raw {
        if let Ok(trait_) = serde_json::from_value::<VersionTrait>(value)
            && !out.contains(&trait_)
        {
            out.push(trait_);
        }
    }
    Ok(out)
}

impl AutoFillPipeline {
    /// The backward-compatibility mapping for a legacy `{ enabled, maxBytes }` block.
    ///
    /// Returns a pipeline that behaves identically to today's algorithm: a single ordering
    /// stage `[Favorite, PlayCount, DateCreated]` over the `Library` source, byte-budgeted to
    /// `max_bytes`. Existing devices select the same tracks with zero behavior change.
    pub fn default_legacy(max_bytes: Option<u64>) -> Self {
        Self {
            enabled: true,
            filter: FilterStage::default(),
            sources: vec![SourceEntry::new(SourceKind::Library)],
            unit: Unit::Track,
            ordering: vec![
                OrderingKey::Favorite,
                OrderingKey::PlayCount,
                OrderingKey::DateCreated,
            ],
            memory: MemoryStage::default(),
            budget: BudgetStage {
                max_bytes,
                target_duration_secs: None,
                headroom_bytes: None,
            },
            fallback: Vec::new(),
            quality: QualityStage::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Pure engine inputs (NOT persisted — supplied by the caller / fetch layer).
// ---------------------------------------------------------------------------

/// A library candidate: a `Song` plus the genres/tags the caller has attached. `Song` itself
/// carries no genre/tag field, so [`FilterStage`] operates on this engine-internal wrapper.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub song: Song,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
}

impl Candidate {
    /// A candidate with no genres/tags attached.
    pub fn new(song: Song) -> Self {
        Self {
            song,
            genres: Vec::new(),
            tags: Vec::new(),
        }
    }
}

/// Identifies a materialized pool: a `(kind, ref_id)` pair matching a [`SourceEntry`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceKey {
    pub kind: SourceKind,
    pub ref_id: Option<String>,
}

impl SourceKey {
    pub fn new(kind: SourceKind, ref_id: Option<String>) -> Self {
        Self { kind, ref_id }
    }
}

/// Per-track runtime history supplied by the caller (sourced from the daemon DB in 12.3+,
/// never read here). Times are caller-supplied Unix seconds.
#[derive(Debug, Clone, Default)]
pub struct TrackHistory {
    pub last_synced_at: Option<i64>,
    pub last_played_at: Option<i64>,
    pub tier: Option<String>,
}

/// A snapshot of runtime history plus the caller's notion of "now" (Unix seconds). The engine
/// derives every time-based decision from this snapshot — never from the system clock.
#[derive(Debug, Clone, Default)]
pub struct HistorySnapshot {
    pub now: i64,
    pub entries: HashMap<String, TrackHistory>,
}

/// Everything the pure core needs, materialized up front so it can run without a provider.
#[derive(Debug, Clone, Default)]
pub struct PipelineInput {
    /// Materialized candidate pools keyed by `(kind, ref_id)`. The fetch layer fills these.
    pub pools: HashMap<SourceKey, Vec<Candidate>>,
    /// Runtime history snapshot (cooldown/played info + "now").
    pub history: HistorySnapshot,
    /// Manually-selected item ids that auto-fill must never re-emit.
    pub exclude_item_ids: Vec<String>,
}

impl PipelineInput {
    /// Insert (or replace) a materialized pool for a source kind/ref.
    pub fn with_pool(
        mut self,
        kind: SourceKind,
        ref_id: Option<&str>,
        pool: Vec<Candidate>,
    ) -> Self {
        self.pools
            .insert(SourceKey::new(kind, ref_id.map(str::to_string)), pool);
        self
    }
}

// ---------------------------------------------------------------------------
// The engine: run_pipeline = filter → source-blend → unit → ordering → dedupe-vs-memory → budget.
// ---------------------------------------------------------------------------

/// Run the auto-fill pipeline over already-materialized inputs.
///
/// Returns a deterministic, budget-bounded, dedup'd `Vec<AutoFillItem>` produced entirely by
/// synchronous pure functions — no network, no `async`, no `MediaProvider`, no clock/RNG.
pub fn run_pipeline(input: &PipelineInput, pipeline: &AutoFillPipeline) -> Vec<AutoFillItem> {
    // Best-version collapse (#11) is a pre-pass over the materialized pools: it only *removes*
    // losing-version candidates, so every downstream budget/dedup guarantee is untouched. Done
    // before any unit grouping/selection so a loser never occupies budget. Default (`false`) keeps
    // the borrowed input as-is — zero clone, zero behavior change.
    let collapsed;
    let input: &PipelineInput = if pipeline.quality.best_version {
        collapsed = collapse_best_version(input, pipeline);
        &collapsed
    } else {
        input
    };

    let ceiling = budget_ceiling(&pipeline.budget);

    // A one-stage pipeline with no `sources` still draws from the Library source so that the
    // legacy mapping (ordering + budget only) works.
    let default_sources;
    let sources: &[SourceEntry] = if pipeline.sources.is_empty() {
        default_sources = [SourceEntry::new(SourceKind::Library)];
        &default_sources
    } else {
        &pipeline.sources
    };

    let exclude: HashSet<String> = input.exclude_item_ids.iter().cloned().collect();
    let mut selector = Selector::new(ceiling, pipeline.budget.target_duration_secs, exclude);

    // Stable-core (#24, AC 6): when `stable_core_pct = p > 0` and the budget is bounded, fill up to
    // `round(ceiling × p)` bytes FIRST from candidates already on the device (have a `last_synced_at`
    // row), exempt from cooldown — the *stable core*. The remaining budget then fills as the *delta*
    // from all candidates honoring full memory rules. Same Filter/Ordering/Unit/dedup as the delta;
    // dedup against the core is automatic via the shared selector. `p = 0`/unbounded ceiling = no-op.
    let core_pct = pipeline.memory.stable_core_pct.unwrap_or(0.0).clamp(0.0, 1.0);
    if core_pct > 0.0 && ceiling != u64::MAX {
        let core_cap = ((ceiling as f64) * f64::from(core_pct)).round() as u64;
        if core_cap > 0 {
            selector.ceiling = core_cap;
            // Split the core budget across sources by their share so one source can't monopolize the
            // whole core allocation (otherwise every source got the full `core_cap` cap).
            let core_caps = source_caps(sources, core_cap);
            for (source, cap) in sources.iter().zip(core_caps) {
                let units = build_source_units(input, pipeline, source);
                selector.fill(units, source, cap, &pipeline.memory, &input.history, FillMode::Core);
            }
            selector.ceiling = ceiling; // restore the full ceiling for the delta pass
        }
    }

    // Primary sources (delta), each capped by its share of the budget *remaining* after the core
    // pass. Computing caps against the full ceiling would let early sources spend the bytes the core
    // already consumed and starve later sources; with no core (p = 0) `remaining == ceiling`, so
    // legacy multi-source behavior is unchanged.
    let remaining = if ceiling == u64::MAX {
        u64::MAX
    } else {
        ceiling.saturating_sub(selector.cum_bytes)
    };
    let caps = source_caps(sources, remaining);
    for (source, cap) in sources.iter().zip(caps) {
        let units = build_source_units(input, pipeline, source);
        selector.fill(units, source, cap, &pipeline.memory, &input.history, FillMode::Primary);
    }

    // Terminal fallback chain — only reached once primary sources can't fill the budget.
    for source in &pipeline.fallback {
        let units = build_source_units(input, pipeline, source);
        selector.fill(units, source, ceiling, &pipeline.memory, &input.history, FillMode::Fallback);
    }

    selector.into_items()
}

/// Which fill pass is running. `Core` (stable-core, AC 6) restricts to on-device candidates and
/// exempts them from cooldown; `Primary` and `Fallback` apply the full Memory rules and differ only
/// in how the source reason is tagged (`Fallback` items are prefixed `fallback:`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FillMode {
    Core,
    Primary,
    Fallback,
}

/// A selection unit: one or more candidates that are added to the budget atomically (a single
/// track for [`Unit::Track`]; a whole album/artist otherwise).
type UnitGroup = Vec<Candidate>;

/// filter → unit → ordering for a single source's materialized pool.
/// Memory/dedup is intentionally applied later by the selector so the full pipeline order stays
/// `filter → source-blend → unit → ordering → dedupe-vs-memory → budget`.
fn build_source_units(
    input: &PipelineInput,
    pipeline: &AutoFillPipeline,
    source: &SourceEntry,
) -> Vec<UnitGroup> {
    let pool = input.pools.get(&source.key()).cloned().unwrap_or_default();

    // filter (genres/tags)
    let filtered = filter_stage(pool, &pipeline.filter);
    // unit grouping
    let mut units = unit_stage(filtered, pipeline.unit);
    // ordering — sort within each unit (so its first track is its best), then sort units by their
    // best track. For Unit::Track this reduces to a single stable global sort.
    let version_pref = &pipeline.quality.version_preference;
    for unit in units.iter_mut() {
        unit.sort_by(|a, b| compare_by_ordering(&a.song, &b.song, &pipeline.ordering, version_pref));
    }
    units.sort_by(|a, b| match (a.first(), b.first()) {
        (Some(x), Some(y)) => compare_by_ordering(&x.song, &y.song, &pipeline.ordering, version_pref),
        _ => std::cmp::Ordering::Equal,
    });
    units
}

/// Keep candidates that pass the include/exclude genre+tag filter. Empty include lists mean
/// "no include constraint"; any exclude match rejects the candidate.
fn filter_stage(cands: Vec<Candidate>, f: &FilterStage) -> Vec<Candidate> {
    cands
        .into_iter()
        .filter(|c| {
            if !f.include_genres.is_empty()
                && !c.genres.iter().any(|g| f.include_genres.contains(g))
            {
                return false;
            }
            if c.genres.iter().any(|g| f.exclude_genres.contains(g)) {
                return false;
            }
            if !f.include_tags.is_empty() && !c.tags.iter().any(|t| f.include_tags.contains(t)) {
                return false;
            }
            if c.tags.iter().any(|t| f.exclude_tags.contains(t)) {
                return false;
            }
            true
        })
        .collect()
}

/// Apply the consumed memory modifiers: drop tracks still in their cooldown window and, when
/// `played_exclusion` is set, drop any track with a recorded play. Time math derives "now" from
/// the snapshot — never from a clock.
fn memory_stage(
    cands: Vec<Candidate>,
    mem: &MemoryStage,
    hist: &HistorySnapshot,
) -> Vec<Candidate> {
    if mem.cooldown_weeks.is_none() && !mem.played_exclusion {
        return cands;
    }
    cands
        .into_iter()
        .filter(|c| memory_allows(&c.song, mem, hist, false))
        .collect()
}

/// Whether a candidate survives the Memory stage. `skip_cooldown` exempts the candidate from the
/// cooldown window (used by the stable-core pass, AC 6) while still honoring played-exclusion.
///
/// Cooldown window (AC 4) is `cooldown_weeks × 7 × 86400` seconds, scaled by the repeat-tolerance
/// dial (AC 7): `effective = window × (1 − repeat_tolerance)`. `repeat_tolerance` only modulates
/// cooldown (no effect when `cooldown_weeks` is `None`). Deterministic — no clock/RNG.
fn memory_allows(song: &Song, mem: &MemoryStage, hist: &HistorySnapshot, skip_cooldown: bool) -> bool {
    let Some(h) = hist.entries.get(&song.id) else {
        return true; // no history → never cooled down
    };
    if mem.played_exclusion && h.last_played_at.is_some() {
        return false;
    }
    if !skip_cooldown
        && let Some(weeks) = mem.cooldown_weeks
        && let Some(synced) = h.last_synced_at
    {
        let base = (i64::from(weeks) * 7 * 86_400) as f64;
        let tolerance = f64::from(mem.repeat_tolerance.unwrap_or(0.0).clamp(0.0, 1.0));
        let window_secs = (base * (1.0 - tolerance)) as i64;
        if hist.now.saturating_sub(synced) < window_secs {
            return false; // synced too recently (within the tolerance-scaled window)
        }
    }
    true
}

/// True when the candidate is currently on the device (has a recorded `last_synced_at`). Drives the
/// stable-core partition (AC 6) — only on-device tracks are eligible for the core.
fn is_on_device(song: &Song, hist: &HistorySnapshot) -> bool {
    hist.entries
        .get(&song.id)
        .and_then(|h| h.last_synced_at)
        .is_some()
}

/// Group candidates into selection units. `Track` = one unit per song; `Album`/`Artist` group by
/// id (tracks lacking that id become singletons), preserving first-seen order.
fn unit_stage(cands: Vec<Candidate>, unit: Unit) -> Vec<UnitGroup> {
    match unit {
        Unit::Track => cands.into_iter().map(|c| vec![c]).collect(),
        Unit::Album => group_by(cands, |c| c.song.album_id.clone()),
        Unit::Artist => group_by(cands, |c| c.song.artist_id.clone()),
    }
}

/// Group candidates by an optional key, preserving first-seen group order. Candidates whose key
/// is `None` each become their own singleton group.
fn group_by(
    cands: Vec<Candidate>,
    key_of: impl Fn(&Candidate) -> Option<String>,
) -> Vec<UnitGroup> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, UnitGroup> = HashMap::new();
    let mut singletons: Vec<UnitGroup> = Vec::new();
    let mut idx = 0usize;
    // Track interleaving of keyed groups and singletons by recording emission slots in order.
    let mut slots: Vec<Slot> = Vec::new();
    enum Slot {
        Group(String),
        Single(usize),
    }
    for c in cands {
        match key_of(&c).filter(|k| !k.trim().is_empty()) {
            Some(k) => {
                if !groups.contains_key(&k) {
                    order.push(k.clone());
                    slots.push(Slot::Group(k.clone()));
                    groups.insert(k.clone(), Vec::new());
                }
                groups.get_mut(&k).unwrap().push(c);
            }
            None => {
                slots.push(Slot::Single(idx));
                singletons.push(vec![c]);
                idx += 1;
            }
        }
    }
    let _ = order;
    slots
        .into_iter()
        .map(|slot| match slot {
            Slot::Group(k) => groups.remove(&k).unwrap_or_default(),
            Slot::Single(i) => std::mem::take(&mut singletons[i]),
        })
        .filter(|g| !g.is_empty())
        .collect()
}

/// Compare two songs by the ordered ranking keys, then by version preference as a final tiebreak.
/// Returns the ordering that places the "better" song first (i.e. ascending sort yields the desired
/// ranking). Stable on full ties.
///
/// Version preference (Story 13.2 #34, AC 5) is applied **after** the explicit `keys` so the
/// user-chosen ordering (favorites/playCount/quality/…) still dominates; it only breaks ties the
/// configured keys leave open. An empty `version_pref` makes this a no-op (today's behavior).
fn compare_by_ordering(
    a: &Song,
    b: &Song,
    keys: &[OrderingKey],
    version_pref: &[VersionTrait],
) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    for key in keys {
        let ord = match key {
            // favorites first
            OrderingKey::Favorite => fav_rank(b).cmp(&fav_rank(a)),
            // higher play count first
            OrderingKey::PlayCount => b.play_count.unwrap_or(0).cmp(&a.play_count.unwrap_or(0)),
            // newer first (ISO-8601 strings sort lexicographically)
            OrderingKey::DateCreated => b
                .date_added
                .as_deref()
                .unwrap_or("")
                .cmp(a.date_added.as_deref().unwrap_or("")),
            // Story 13.2 #13: lossless formats rank above lossy regardless of bitrate, then by
            // bitrate descending within each tier (so a FLAC with no reported bitrate still beats a
            // 320 kbps MP3). The format tier comes from `suffix`/`content_type`, never the title.
            OrderingKey::Quality => (format_quality_rank(b), b.bitrate_kbps.unwrap_or(0))
                .cmp(&(format_quality_rank(a), a.bitrate_kbps.unwrap_or(0))),
            // deterministic no-op in 12.1 — no entropy in the pure core (Epic 13 adds seeding)
            OrderingKey::Random => Ordering::Equal,
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    // Version preference: lower rank = more preferred = sorts first.
    if !version_pref.is_empty() {
        let ord = version_rank(a, version_pref).cmp(&version_rank(b, version_pref));
        if ord != Ordering::Equal {
            return ord;
        }
    }
    Ordering::Equal
}

fn fav_rank(s: &Song) -> u8 {
    u8::from(s.is_favorite == Some(true))
}

/// Format-aware quality tier (Story 13.2 #13): lossless (2) > lossy (1) > unknown (0). Read
/// case-insensitively from `suffix` first, then the `content_type` mime subtype — **never** the
/// title. A present-but-unrecognized format string is "lossy" (1); only the total absence of any
/// format hint is "unknown" (0), so a known FLAC always outranks a bare MP3 which outranks a song
/// with no format metadata at all.
fn format_quality_rank(song: &Song) -> u8 {
    /// Lossless container/codec tokens (suffixes and mime subtypes, sans any `x-` mime prefix).
    const LOSSLESS: &[&str] = &[
        "flac", "alac", "wav", "wave", "aiff", "aif", "ape", "wavpack", "wv",
    ];

    let suffix = song
        .suffix
        .as_deref()
        .map(|s| s.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if !suffix.is_empty() {
        return if LOSSLESS.contains(&suffix.as_str()) { 2 } else { 1 };
    }

    // Fall back to the mime subtype (`audio/flac` → `flac`, `audio/x-flac` → `flac`).
    let mime_sub = song
        .content_type
        .as_deref()
        .and_then(|c| c.rsplit('/').next())
        .map(|s| s.trim().trim_start_matches("x-").to_ascii_lowercase())
        .unwrap_or_default();
    if !mime_sub.is_empty() {
        return if LOSSLESS.contains(&mime_sub.as_str()) { 2 } else { 1 };
    }

    0 // no format metadata at all → unknown, ranked last
}

// ---------------------------------------------------------------------------
// Version-trait detection (Story 13.2 #34) — pure text heuristics over a closed marker set.
// ---------------------------------------------------------------------------

/// True when `needle` (ASCII) appears in `hay` bounded by a non-alphanumeric byte (or a string
/// edge) on both sides — so `live` matches `(live)`, `- live`, `live at`, but **not** `alive` or
/// `believe`, and `demo` matches `(demo)` but not `demolition`/`demon`. `hay` is expected
/// pre-lowercased. Multibyte (≥0x80) neighbors count as boundaries, which only ever loosens toward
/// "not a word char" — acceptable for these conservative markers.
fn has_word(hay: &str, needle: &str) -> bool {
    let bytes = hay.as_bytes();
    let nlen = needle.len();
    if nlen == 0 {
        return false;
    }
    let mut start = 0;
    while let Some(rel) = hay[start..].find(needle) {
        let i = start + rel;
        let before_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        let after = i + nlen;
        let after_ok = after >= bytes.len() || !bytes[after].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return true;
        }
        start = i + 1;
    }
    false
}

/// Whether a (lowercased) text segment carries any recognized version marker. Shared by
/// [`detect_version_traits`] and [`strip_version_markers`] so detection and stripping agree.
/// `remix`/`acoustic` use substring matching (`remix` is safe per the false-positive analysis;
/// `mix` alone is intentionally **not** a marker); the rest are word-anchored.
fn segment_has_marker(low: &str) -> bool {
    has_word(low, "live")
        || has_word(low, "unplugged")
        || has_word(low, "remaster")
        || has_word(low, "remastered")
        || low.contains("remix")
        || has_word(low, "rmx")
        || low.contains("re-mix")
        || low.contains("acoustic")
        || has_word(low, "demo")
}

/// Classify a song into the closed set of version traits (Story 13.2 #34), reading only `title` and
/// `album_title`, case-insensitively. Returns the **set** of matched traits; `Studio` is returned
/// (alone) when no other marker is recognized. Deterministic — no clock, no RNG.
fn detect_version_traits(song: &Song) -> Vec<VersionTrait> {
    let mut hay = song.title.to_ascii_lowercase();
    if let Some(album) = song.album_title.as_deref() {
        hay.push(' ');
        hay.push_str(&album.to_ascii_lowercase());
    }

    let mut traits = Vec::new();
    // Live: `live` (word-anchored, so "Alive"/"Believe" don't match) or `unplugged`.
    if has_word(&hay, "live") || has_word(&hay, "unplugged") {
        traits.push(VersionTrait::Live);
    }
    // Remastered: `remaster` (covers `(2011 Remaster)`, `remaster)`) or the `-ed` form.
    if has_word(&hay, "remaster") || has_word(&hay, "remastered") {
        traits.push(VersionTrait::Remastered);
    }
    // Remix: `remix` substring (catches remixed/remixes), `rmx` word, or `re-mix`.
    if hay.contains("remix") || has_word(&hay, "rmx") || hay.contains("re-mix") {
        traits.push(VersionTrait::Remix);
    }
    if hay.contains("acoustic") {
        traits.push(VersionTrait::Acoustic);
    }
    // Demo: word-anchored so "Demolition"/"Demon" don't match.
    if has_word(&hay, "demo") {
        traits.push(VersionTrait::Demo);
    }
    if traits.is_empty() {
        traits.push(VersionTrait::Studio);
    }
    traits
}

/// A song's version rank against an ordered preference list (Story 13.2 #34): the index of the first
/// listed trait the song matches; a song matching none of the listed traits ranks **last**
/// (`prefs.len()`, the worst). An empty preference list makes every song tie (rank 0).
fn version_rank(song: &Song, prefs: &[VersionTrait]) -> usize {
    if prefs.is_empty() {
        return 0;
    }
    let traits = detect_version_traits(song);
    prefs
        .iter()
        .position(|p| traits.contains(p))
        .unwrap_or(prefs.len())
}

// ---------------------------------------------------------------------------
// Best-version resolution (Story 13.2 #11) — collapse same-logical-song duplicates, keep the best.
// ---------------------------------------------------------------------------

/// Lowercase, trim, and collapse internal whitespace runs to single spaces.
fn normalize_ws(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

/// Remove `open…close` bracket groups whose inner text carries a recognized version marker, keeping
/// every other (non-version) bracket group intact. `open`/`close` are single ASCII chars.
fn strip_bracketed_markers(s: &str, open: char, close: char) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find(open) {
        match rest[start..].find(close) {
            Some(end_rel) => {
                let end = start + end_rel; // byte index of the closing char (ASCII, 1 byte)
                out.push_str(&rest[..start]);
                let inner = &rest[start + 1..end];
                if !segment_has_marker(&inner.to_ascii_lowercase()) {
                    out.push_str(&rest[start..=end]); // not a version marker → keep verbatim
                }
                rest = &rest[end + 1..];
            }
            None => break, // unbalanced bracket — leave the remainder untouched
        }
    }
    out.push_str(rest);
    out
}

/// Strip recognized version markers from a title so two recordings of the same song share a base
/// title: parenthetical/bracketed marker groups (`(Live)`, `[Acoustic]`, `(2011 Remaster)`) and a
/// trailing ` - <…marker…>` dash suffix (`- 2011 Remaster`, `- Live at Wembley`). Conservative — an
/// unrecognized suffix is left intact so distinct songs stay distinct. Whitespace is **not**
/// collapsed here (the caller normalizes).
fn strip_version_markers(title: &str) -> String {
    let mut s = strip_bracketed_markers(title, '(', ')');
    s = strip_bracketed_markers(&s, '[', ']');
    if let Some(idx) = s.rfind(" - ")
        && segment_has_marker(&s[idx + 3..].to_ascii_lowercase())
    {
        s.truncate(idx);
    }
    s
}

/// The logical-song key for best-version collapse (Story 13.2 #11): `(normalized_artist,
/// normalized_base_title)`. Returns `None` — meaning "never collapse this candidate" — when the
/// artist is missing/empty (never merge across unknown artists) or the base title is empty after
/// stripping (nothing left to match on). Conservative by design: when in doubt, don't merge.
fn logical_key(song: &Song) -> Option<(String, String)> {
    let artist = normalize_ws(song.artist_name.as_deref().unwrap_or(""));
    if artist.is_empty() {
        return None;
    }
    let base = normalize_ws(&strip_version_markers(&song.title));
    if base.is_empty() {
        return None;
    }
    Some((artist, base))
}

/// Deterministic best-version comparator (Story 13.2 #11, AC 6): version-preference rank → quality
/// rank → the full `ordering` → `song.id` lexicographic (the ultimate tiebreak that makes the winner
/// independent of pool iteration order). Returns `Less` when `a` is the better version.
fn best_version_cmp(a: &Song, b: &Song, pipeline: &AutoFillPipeline) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let prefs = &pipeline.quality.version_preference;
    // (1) version preference rank (lower = more preferred)
    if !prefs.is_empty() {
        let ord = version_rank(a, prefs).cmp(&version_rank(b, prefs));
        if ord != Ordering::Equal {
            return ord;
        }
    }
    // (2) quality rank (lossless-first, then bitrate desc) — higher quality is better
    let ord = (format_quality_rank(b), b.bitrate_kbps.unwrap_or(0))
        .cmp(&(format_quality_rank(a), a.bitrate_kbps.unwrap_or(0)));
    if ord != Ordering::Equal {
        return ord;
    }
    // (3) the configured ordering keys (version preference already applied above → pass empty)
    let ord = compare_by_ordering(a, b, &pipeline.ordering, &[]);
    if ord != Ordering::Equal {
        return ord;
    }
    // (4) ultimate deterministic tiebreak
    a.id.cmp(&b.id)
}

/// Collapse same-logical-song duplicates across **all** pools, keeping the single best version
/// (Story 13.2 #11). Builds the global `logical_key → winning song id` map over the union of pools
/// (so the winner is chosen even when versions span sources), then drops from every pool any
/// candidate whose logical key resolves to a *different* winner. Candidates with no logical key
/// (`None` — missing artist / empty base title) are always kept. The winner survives wherever it
/// appeared; the Selector's dedup-by-`song.id` then collapses it to a single emission.
fn collapse_best_version(input: &PipelineInput, pipeline: &AutoFillPipeline) -> PipelineInput {
    // Pick the winning Song per logical key. Iteration order over pools/candidates is irrelevant:
    // `best_version_cmp` is a total order (ties broken by id), so the minimum is deterministic.
    let mut winners: HashMap<(String, String), Song> = HashMap::new();
    for pool in input.pools.values() {
        for cand in pool {
            let Some(key) = logical_key(&cand.song) else {
                continue;
            };
            match winners.get(&key) {
                Some(current)
                    if best_version_cmp(&cand.song, current, pipeline)
                        != std::cmp::Ordering::Less => {}
                _ => {
                    winners.insert(key, cand.song.clone());
                }
            }
        }
    }

    let mut out = input.clone();
    for pool in out.pools.values_mut() {
        pool.retain(|cand| match logical_key(&cand.song) {
            Some(key) => winners.get(&key).is_none_or(|w| w.id == cand.song.id),
            None => true, // no logical key → never collapsed
        });
    }
    out
}

/// Estimated playable size in bytes: prefer `size_bytes`, else `(bitrate_kbps*1000/8)*duration`.
/// Returns `None` for unknown/zero size so the caller skips the track (never a 0-byte filler).
/// Mirrors `ProviderFillState::try_add` / `rank_and_truncate`.
fn estimated_size(song: &Song) -> Option<u64> {
    if let Some(sz) = song.size_bytes {
        return (sz > 0).then_some(sz);
    }
    let kbps = song.bitrate_kbps?;
    let est = u64::from(kbps)
        .checked_mul(1_000)?
        .checked_div(8)?
        .checked_mul(u64::from(song.duration_seconds))?;
    (est > 0).then_some(est)
}

/// The effective byte ceiling: `max_bytes - headroom_bytes`, or unbounded when no byte budget is
/// set (a duration target may still cap the run).
fn budget_ceiling(b: &BudgetStage) -> u64 {
    match b.max_bytes {
        Some(m) => m.saturating_sub(b.headroom_bytes.unwrap_or(0)),
        None => u64::MAX,
    }
}

/// Per-source byte caps derived from `share`. With no shares anywhere, sources split the global
/// ceiling equally. With shares, shared sources get `share * ceiling` and unshared sources split
/// the remainder equally.
fn source_caps(sources: &[SourceEntry], ceiling: u64) -> Vec<u64> {
    if sources.is_empty() {
        return Vec::new();
    }
    let any_share = sources.iter().any(|s| s.share.is_some());
    if !any_share {
        return vec![ceiling / sources.len() as u64; sources.len()];
    }
    let explicit: f32 = sources.iter().filter_map(|s| s.share).sum();
    let n_unshared = sources.iter().filter(|s| s.share.is_none()).count();
    let remainder = (1.0 - explicit).max(0.0);
    sources
        .iter()
        .map(|s| match s.share {
            Some(sh) => frac_bytes(sh, ceiling),
            None if n_unshared > 0 => frac_bytes(remainder / n_unshared as f32, ceiling),
            None => 0,
        })
        .collect()
}

fn frac_bytes(frac: f32, ceiling: u64) -> u64 {
    if ceiling == u64::MAX {
        return u64::MAX;
    }
    ((ceiling as f64) * (frac.clamp(0.0, 1.0) as f64)) as u64
}

/// Accumulates the selection across sources: enforces the global ceiling, per-source caps, the
/// optional duration target, manual-exclude ids, and within-run dedup. Mirrors the
/// stop-on-first-oversized semantics of the legacy `ProviderFillState`/`rank_and_truncate`.
struct Selector {
    ceiling: u64,
    duration_target: Option<u64>,
    exclude: HashSet<String>,
    seen: HashSet<String>,
    items: Vec<AutoFillItem>,
    cum_bytes: u64,
    cum_secs: u64,
}

impl Selector {
    fn new(ceiling: u64, duration_target: Option<u64>, exclude: HashSet<String>) -> Self {
        Self {
            ceiling,
            duration_target,
            exclude,
            seen: HashSet::new(),
            items: Vec::new(),
            cum_bytes: 0,
            cum_secs: 0,
        }
    }

    /// Add units from one source (in order) until the source cap, the global ceiling, or the
    /// duration target stops us. Units are atomic: a unit whose syncable tracks don't all fit
    /// stops this source (smaller later units are not back-filled — matching legacy semantics).
    fn fill(
        &mut self,
        units: Vec<UnitGroup>,
        source: &SourceEntry,
        cap: u64,
        memory: &MemoryStage,
        history: &HistorySnapshot,
        mode: FillMode,
    ) {
        let core = mode == FillMode::Core;
        let is_fallback = mode == FillMode::Fallback;
        let mut source_bytes: u64 = 0;
        for unit in units {
            if let Some(target) = self.duration_target
                && self.cum_secs >= target
            {
                break;
            }
            // Stage the syncable, non-excluded, not-yet-seen tracks of this unit.
            let mut staged: Vec<(AutoFillItem, u64)> = Vec::new();
            let mut local_seen: HashSet<String> = HashSet::new();
            let mut unit_bytes: u64 = 0;
            let mut unit_secs: u64 = 0;
            for cand in &unit {
                let song = &cand.song;
                // The core pass only draws candidates already on the device; cooldown is skipped for
                // them (they are kept on purpose) but played-exclusion still applies.
                if (core && !is_on_device(song, history))
                    || self.exclude.contains(&song.id)
                    || self.seen.contains(&song.id)
                    || !local_seen.insert(song.id.clone())
                    || !memory_allows(song, memory, history, core)
                {
                    continue;
                }
                let Some(size) = estimated_size(song) else {
                    continue; // unknown/zero size — never a 0-byte filler
                };
                unit_bytes = unit_bytes.saturating_add(size);
                unit_secs = unit_secs.saturating_add(u64::from(song.duration_seconds));
                let reason = reason_for(song, source, is_fallback);
                staged.push((make_item(song, size, reason), size));
            }
            if staged.is_empty() {
                continue; // whole unit unsyncable/duplicate/excluded — skip, keep going
            }
            if exceeds(self.cum_bytes, unit_bytes, self.ceiling)
                || exceeds(source_bytes, unit_bytes, cap)
                || self.would_exceed_duration(unit_secs)
            {
                break; // would exceed global ceiling or this source's allocation
            }
            for (item, size) in staged {
                self.seen.insert(item.id.clone());
                self.cum_bytes = self.cum_bytes.saturating_add(size);
                source_bytes = source_bytes.saturating_add(size);
                self.items.push(item);
            }
            self.cum_secs = self.cum_secs.saturating_add(unit_secs);
        }
    }

    fn into_items(self) -> Vec<AutoFillItem> {
        self.items
    }

    fn would_exceed_duration(&self, unit_secs: u64) -> bool {
        self.duration_target
            .is_some_and(|target| exceeds(self.cum_secs, unit_secs, target))
    }
}

fn exceeds(current: u64, add: u64, ceiling: u64) -> bool {
    current.checked_add(add).is_none_or(|total| total > ceiling)
}

fn make_item(song: &Song, size_bytes: u64, reason: String) -> AutoFillItem {
    AutoFillItem {
        id: song.id.clone(),
        name: song.title.clone(),
        album: song.album_title.clone(),
        artist: song.artist_name.clone(),
        provider_album_id: song.album_id.clone(),
        provider_content_type: song.content_type.clone(),
        provider_suffix: song.suffix.clone(),
        size_bytes,
        priority_reason: reason,
        // Tier is assigned by the fetch layer (which owns the rotated tier→pool mapping); the pure
        // engine never knows about tiers, so it always emits `None` here.
        tier: None,
    }
}

/// Build the `priority_reason` describing the winning source/stage for downstream/preview UX.
fn reason_for(song: &Song, source: &SourceEntry, is_fallback: bool) -> String {
    let base = match source.kind {
        SourceKind::Library => {
            if song.is_favorite == Some(true) {
                "favorite".to_string()
            } else if song.play_count.unwrap_or(0) > 0 {
                format!("playCount:{}", song.play_count.unwrap_or(0))
            } else {
                "library".to_string()
            }
        }
        SourceKind::Favorites => "favorite".to_string(),
        SourceKind::History => "history".to_string(),
        SourceKind::Playlist => {
            format!("playlist:{}", source.ref_id.clone().unwrap_or_default())
        }
    };
    if is_fallback {
        format!("fallback:{base}")
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------
    // Fixtures — hand-built `domain::models::Song` values, mirroring the style of
    // `auto_fill::tests::make_track`. No async, no provider mocks, no clock.
    // -------------------------------------------------------------------

    /// Build a `Song` with an explicit `size_bytes`, so budget math is exact in tests.
    fn song_sized(id: &str, fav: bool, play_count: u32, date_added: &str, size_bytes: u64) -> Song {
        Song {
            id: id.to_string(),
            title: format!("Track {id}"),
            artist_id: Some(format!("artist-{id}")),
            artist_name: Some(format!("Artist {id}")),
            album_id: Some(format!("album-{id}")),
            album_title: Some(format!("Album {id}")),
            duration_seconds: 180,
            bitrate_kbps: Some(256),
            track_number: Some(1),
            disc_number: None,
            cover_art_id: None,
            date_added: Some(date_added.to_string()),
            last_played_at: None,
            play_count: Some(play_count),
            is_favorite: Some(fav),
            content_type: Some("audio/mpeg".to_string()),
            suffix: Some("mp3".to_string()),
            size_bytes: Some(size_bytes),
        }
    }

    /// Build a `Song` whose size is derived from its bitrate (no explicit `size_bytes`).
    fn song_bitrate(id: &str, bitrate_kbps: u32, duration_seconds: u32) -> Song {
        Song {
            bitrate_kbps: Some(bitrate_kbps),
            duration_seconds,
            size_bytes: None,
            ..song_sized(id, false, 0, "2024-01-01", 0)
        }
    }

    fn cand(song: Song) -> Candidate {
        Candidate::new(song)
    }

    fn cand_meta(song: Song, genres: &[&str], tags: &[&str]) -> Candidate {
        Candidate {
            song,
            genres: genres.iter().map(|s| s.to_string()).collect(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn ids(items: &[AutoFillItem]) -> Vec<String> {
        items.iter().map(|i| i.id.clone()).collect()
    }

    // ===================================================================
    // FOUR PERSONAS, ONE MODEL — the Story-12.1 success gate.
    //
    // Each persona's intent is expressed purely by composing the stage algebra. There are NO
    // `if persona == ...` branches in the engine: every assertion below is satisfied by the same
    // `run_pipeline`. If a persona ever forced a special case in the engine, the algebra would be
    // wrong, not the persona. (Over-abstraction risk mitigation —
    // sprint-change-proposal-2026-06-14-configurable-auto-fill.md#Section-5.)
    // ===================================================================

    #[test]
    fn persona_claire_commuter_hates_repeats() {
        // Claire: commuter, ~small budget, hates repeats. Favorites + Library sources, low
        // cooldown. Recently-synced tracks must be excluded; the set fits the small budget.
        let now = 1_000_000_000i64;
        let week = 7 * 86_400i64;

        let favorites = vec![
            cand(song_sized("fav-fresh", true, 0, "2024-03-01", 2_000_000)),
            cand(song_sized("fav-recent", true, 0, "2024-03-02", 2_000_000)),
        ];
        let library = vec![
            cand(song_sized("lib-a", false, 5, "2024-02-01", 2_000_000)),
            cand(song_sized("lib-b", false, 1, "2024-01-01", 2_000_000)),
        ];

        let mut history = HistorySnapshot {
            now,
            ..Default::default()
        };
        // fav-recent was synced 1 week ago — inside the 2-week cooldown → excluded.
        history.entries.insert(
            "fav-recent".to_string(),
            TrackHistory {
                last_synced_at: Some(now - week),
                ..Default::default()
            },
        );

        let input = PipelineInput {
            history,
            ..Default::default()
        }
        .with_pool(SourceKind::Favorites, None, favorites)
        .with_pool(SourceKind::Library, None, library);

        let pipeline = AutoFillPipeline {
            sources: vec![
                SourceEntry::new(SourceKind::Favorites),
                SourceEntry::new(SourceKind::Library),
            ],
            ordering: vec![OrderingKey::Favorite, OrderingKey::PlayCount],
            memory: MemoryStage {
                cooldown_weeks: Some(2),
                repeat_tolerance: Some(0.0),
                ..Default::default()
            },
            // 5 MB budget — fits at most 2 of the 2 MB tracks (cooled-down one removed anyway).
            budget: BudgetStage {
                max_bytes: Some(5_000_000),
                ..Default::default()
            },
            ..Default::default()
        };

        let result = run_pipeline(&input, &pipeline);
        let result_ids = ids(&result);

        assert!(
            !result_ids.contains(&"fav-recent".to_string()),
            "recently-synced (cooled-down) track must be excluded"
        );
        let total: u64 = result.iter().map(|i| i.size_bytes).sum();
        assert!(total <= 5_000_000, "selection must fit the small budget");
        // fav-fresh (favorite, not cooled down) is the top pick.
        assert_eq!(result_ids.first().map(String::as_str), Some("fav-fresh"));
    }

    #[test]
    fn persona_antoine_audiophile_quality_first() {
        // Antoine: 512 GB DAP, quality-first. Large budget, ordering [Quality]. Story 13.2 makes
        // Quality lossless-aware: a FLAC ranks above every lossy file regardless of bitrate, then
        // bitrate breaks ties within a tier. So a 900 kbps FLAC beats a 1411 kbps "HD" MP3.
        let mut flac = song_bitrate("flac-lo", 900, 200);
        flac.suffix = Some("flac".to_string());
        flac.content_type = Some("audio/flac".to_string());
        let library = vec![
            cand(song_bitrate("low", 128, 200)),
            cand(song_bitrate("hi", 1_411, 200)), // high-bitrate MP3 (still lossy)
            cand(song_bitrate("mid", 320, 200)),
            cand(flac),
        ];
        let input = PipelineInput::default().with_pool(SourceKind::Library, None, library);

        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            ordering: vec![OrderingKey::Quality],
            budget: BudgetStage {
                max_bytes: Some(512u64 * 1_000 * 1_000 * 1_000),
                ..Default::default()
            },
            ..Default::default()
        };

        let result = run_pipeline(&input, &pipeline);
        assert_eq!(
            ids(&result),
            vec!["flac-lo", "hi", "mid", "low"],
            "lossless ranks above lossy regardless of bitrate, then bitrate breaks ties within a tier"
        );
    }

    #[test]
    fn persona_leo_gym_energy_playlist_tiny_device() {
        // Léo: tiny device, energy-driven. A single Playlist source ("energy") and a tiny budget.
        // Only the playlist pool's tracks are picked, truncated to the tiny budget. The library
        // pool is present but never referenced — so it must not leak into the result.
        let energy = vec![
            cand(song_sized("e1", false, 0, "2024-01-01", 3_000_000)),
            cand(song_sized("e2", false, 0, "2024-01-01", 3_000_000)),
            cand(song_sized("e3", false, 0, "2024-01-01", 3_000_000)),
        ];
        let library = vec![cand(song_sized("lib-x", true, 99, "2024-01-01", 1_000_000))];

        let input = PipelineInput::default()
            .with_pool(SourceKind::Playlist, Some("energy"), energy)
            .with_pool(SourceKind::Library, None, library);

        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry {
                kind: SourceKind::Playlist,
                ref_id: Some("energy".to_string()),
                share: None,
            }],
            // 7 MB budget fits only 2 of the 3 MB playlist tracks.
            budget: BudgetStage {
                max_bytes: Some(7_000_000),
                ..Default::default()
            },
            ..Default::default()
        };

        let result = run_pipeline(&input, &pipeline);
        let result_ids = ids(&result);
        assert_eq!(
            result_ids,
            vec!["e1", "e2"],
            "only playlist tracks, truncated to budget"
        );
        assert!(
            !result_ids.iter().any(|id| id == "lib-x"),
            "unreferenced library source must not leak in"
        );
        assert!(
            result
                .iter()
                .all(|i| i.priority_reason == "playlist:energy")
        );
    }

    #[test]
    fn persona_nadia_kids_player_filtered() {
        // Nadia: parent filling a kid's player. Filter to kids genres, exclude explicit tag.
        // Filtered-out tracks must never appear.
        let library = vec![
            cand_meta(
                song_sized("kids-clean", false, 0, "2024-01-01", 1_000_000),
                &["kids"],
                &[],
            ),
            cand_meta(
                song_sized("kids-explicit", false, 0, "2024-01-01", 1_000_000),
                &["kids"],
                &["explicit"],
            ),
            cand_meta(
                song_sized("metal", false, 0, "2024-01-01", 1_000_000),
                &["metal"],
                &[],
            ),
        ];
        let input = PipelineInput::default().with_pool(SourceKind::Library, None, library);

        let pipeline = AutoFillPipeline {
            filter: FilterStage {
                include_genres: vec!["kids".to_string()],
                exclude_tags: vec!["explicit".to_string()],
                ..Default::default()
            },
            sources: vec![SourceEntry::new(SourceKind::Library)],
            budget: BudgetStage {
                max_bytes: Some(100_000_000),
                ..Default::default()
            },
            ..Default::default()
        };

        let result = run_pipeline(&input, &pipeline);
        let result_ids = ids(&result);
        assert_eq!(
            result_ids,
            vec!["kids-clean"],
            "only non-explicit kids tracks survive the filter"
        );
    }

    // ===================================================================
    // Story 13.1 — repeat-tolerance dial (#23) & stable-core (#24).
    // ===================================================================

    #[test]
    fn repeat_tolerance_scales_the_cooldown_window() {
        let now = 1_000_000_000i64;
        let week = 7 * 86_400i64;
        let song = song_sized("t", false, 0, "2024-01-01", 1_000_000);

        let mut hist = HistorySnapshot {
            now,
            ..Default::default()
        };
        hist.entries.insert(
            "t".to_string(),
            TrackHistory {
                last_synced_at: Some(now - week), // synced one week ago
                ..Default::default()
            },
        );

        // t = 0 → full 2-week window → a 1-week-old sync is still cooled down (current behavior).
        let strict = MemoryStage {
            cooldown_weeks: Some(2),
            repeat_tolerance: Some(0.0),
            ..Default::default()
        };
        assert!(!memory_allows(&song, &strict, &hist, false), "t=0 strict");

        // t = 1 → zero window → recently-synced tracks fully allowed.
        let lax = MemoryStage {
            cooldown_weeks: Some(2),
            repeat_tolerance: Some(1.0),
            ..Default::default()
        };
        assert!(memory_allows(&song, &lax, &hist, false), "t=1 no cooldown");

        // t = 0.5 → 1-week effective window. Synced exactly one week ago sits on the boundary
        // (`elapsed < window` is false) → allowed.
        let mid = MemoryStage {
            cooldown_weeks: Some(2),
            repeat_tolerance: Some(0.5),
            ..Default::default()
        };
        assert!(memory_allows(&song, &mid, &hist, false), "t=0.5 boundary allowed");

        // …but a 3-day-old sync is still inside the half-width window → excluded.
        let mut hist_recent = HistorySnapshot {
            now,
            ..Default::default()
        };
        hist_recent.entries.insert(
            "t".to_string(),
            TrackHistory {
                last_synced_at: Some(now - 3 * 86_400),
                ..Default::default()
            },
        );
        assert!(!memory_allows(&song, &mid, &hist_recent, false), "t=0.5 inside window");

        // repeat_tolerance only modulates cooldown — with no cooldown it is inert.
        let no_cooldown = MemoryStage {
            cooldown_weeks: None,
            repeat_tolerance: Some(0.5),
            ..Default::default()
        };
        assert!(memory_allows(&song, &no_cooldown, &hist, false), "tolerance is inert without cooldown");
    }

    #[test]
    fn stable_core_fills_core_fraction_from_on_device_tracks() {
        let now = 1_000_000_000i64;
        let mut hist = HistorySnapshot {
            now,
            ..Default::default()
        };
        let mut library = Vec::new();
        // 4 tracks already on the device (have last_synced_at).
        for i in 0..4 {
            let id = format!("dev{i}");
            hist.entries.insert(
                id.clone(),
                TrackHistory {
                    last_synced_at: Some(now - 100 * 7 * 86_400),
                    ..Default::default()
                },
            );
            library.push(cand(song_sized(&id, false, 0, "2024-01-01", 1_000_000)));
        }
        // 4 fresh tracks never synced.
        for i in 0..4 {
            library.push(cand(song_sized(&format!("fresh{i}"), false, 0, "2024-01-01", 1_000_000)));
        }
        let input = PipelineInput {
            history: hist,
            ..Default::default()
        }
        .with_pool(SourceKind::Library, None, library);

        // 8 MB budget, p = 0.5 → ~4 MB core from on-device, ~4 MB delta from fresh.
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            memory: MemoryStage {
                stable_core_pct: Some(0.5),
                ..Default::default()
            },
            budget: BudgetStage {
                max_bytes: Some(8_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = run_pipeline(&input, &pipeline);
        let total: u64 = result.iter().map(|i| i.size_bytes).sum();
        assert!(total <= 8_000_000, "never exceeds the ceiling");
        let core_bytes: u64 = result
            .iter()
            .filter(|i| i.id.starts_with("dev"))
            .map(|i| i.size_bytes)
            .sum();
        assert!(core_bytes >= 4_000_000, "≈p of the budget is the on-device core");
        assert!(
            result.iter().any(|i| i.id.starts_with("fresh")),
            "the delta still draws fresh tracks"
        );
        // No within-run duplicates between the core and delta passes.
        let mut seen = std::collections::HashSet::new();
        assert!(result.iter().all(|i| seen.insert(i.id.clone())));
    }

    #[test]
    fn stable_core_empty_history_first_sync_is_a_normal_fill() {
        let library = (0..4)
            .map(|i| cand(song_sized(&format!("t{i}"), false, 0, "2024-01-01", 1_000_000)))
            .collect();
        let input = PipelineInput::default().with_pool(SourceKind::Library, None, library);
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            memory: MemoryStage {
                stable_core_pct: Some(0.5),
                ..Default::default()
            },
            budget: BudgetStage {
                max_bytes: Some(4_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        // No history → the core is empty → the whole budget fills normally.
        assert_eq!(run_pipeline(&input, &pipeline).len(), 4);
    }

    // ===================================================================
    // Backward-compatibility & guarantee tests (AC 3, 5, 7).
    // ===================================================================

    #[test]
    fn legacy_equivalence_favorites_playcount_datecreated_order_and_truncation() {
        // default_legacy must reproduce today's favorites → playCount → dateCreated priority over
        // the library, byte-truncated — the same priority order as
        // run_auto_fill_provider / rank_and_truncate.
        let library = vec![
            song_sized("old-new", false, 0, "2023-01-01", 1_000_000), // neither
            song_sized("fav", true, 0, "2020-01-01", 1_000_000),      // favorite (wins)
            song_sized("played-lots", false, 50, "2021-01-01", 1_000_000), // play count
            song_sized("newest", false, 0, "2024-12-31", 1_000_000), // newest of the non-fav/non-played
            song_sized("played-few", false, 2, "2022-01-01", 1_000_000), // some plays
        ]
        .into_iter()
        .map(cand)
        .collect();
        let input = PipelineInput::default().with_pool(SourceKind::Library, None, library);

        // Budget for exactly 4 of 5 tracks (each 1 MB) → last in priority order is truncated.
        let pipeline = AutoFillPipeline::default_legacy(Some(4_000_000));
        let result = run_pipeline(&input, &pipeline);

        assert_eq!(
            ids(&result),
            vec!["fav", "played-lots", "played-few", "newest"],
            "legacy order: favorite, then by play count desc, then by date desc; 'old-new' truncated"
        );
        let total: u64 = result.iter().map(|i| i.size_bytes).sum();
        assert!(total <= 4_000_000);
    }

    #[test]
    fn legacy_default_for_empty_pipeline_draws_from_library() {
        // An effectively-empty pipeline (only a budget) defaults to the Library source.
        let library = vec![cand(song_sized("a", false, 0, "2024-01-01", 1_000_000))];
        let input = PipelineInput::default().with_pool(SourceKind::Library, None, library);
        let pipeline = AutoFillPipeline {
            budget: BudgetStage {
                max_bytes: Some(10_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = run_pipeline(&input, &pipeline);
        assert_eq!(ids(&result), vec!["a"]);
    }

    #[test]
    fn budget_never_exceeded() {
        let library = (0..10)
            .map(|i| {
                cand(song_sized(
                    &format!("t{i}"),
                    false,
                    0,
                    "2024-01-01",
                    3_000_000,
                ))
            })
            .collect();
        let input = PipelineInput::default().with_pool(SourceKind::Library, None, library);
        let pipeline = AutoFillPipeline::default_legacy(Some(10_000_000)); // fits 3 × 3 MB = 9 MB
        let result = run_pipeline(&input, &pipeline);
        let total: u64 = result.iter().map(|i| i.size_bytes).sum();
        assert!(
            total <= 10_000_000,
            "cumulative bytes must never exceed the budget"
        );
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn headroom_is_subtracted_from_ceiling() {
        let library = (0..5)
            .map(|i| {
                cand(song_sized(
                    &format!("t{i}"),
                    false,
                    0,
                    "2024-01-01",
                    1_000_000,
                ))
            })
            .collect();
        let input = PipelineInput::default().with_pool(SourceKind::Library, None, library);
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            budget: BudgetStage {
                max_bytes: Some(5_000_000),
                headroom_bytes: Some(2_500_000), // effective ceiling 2.5 MB → 2 tracks
                ..Default::default()
            },
            ..Default::default()
        };
        let result = run_pipeline(&input, &pipeline);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn unknown_zero_and_negative_size_skipped() {
        // size_bytes None + no bitrate → unknown; size_bytes Some(0) → zero. Both skipped, never
        // counted as 0-byte fillers. (Negative sizes can't reach here — Song.size_bytes is u64 —
        // but the provider layer maps them to None, which we cover via the no-size case.)
        let mut no_size = song_sized("nosize", true, 0, "2024-01-01", 0);
        no_size.size_bytes = None;
        no_size.bitrate_kbps = None;
        let mut zero = song_sized("zero", true, 0, "2024-01-01", 0);
        zero.size_bytes = Some(0);
        zero.bitrate_kbps = Some(320);
        let good = song_sized("good", false, 0, "2024-01-01", 1_000_000);

        let library = vec![cand(no_size), cand(zero), cand(good)];
        let input = PipelineInput::default().with_pool(SourceKind::Library, None, library);
        let pipeline = AutoFillPipeline::default_legacy(Some(10_000_000));
        let result = run_pipeline(&input, &pipeline);
        assert_eq!(
            ids(&result),
            vec!["good"],
            "unknown/zero-size tracks must be skipped"
        );
    }

    #[test]
    fn duration_target_is_never_overshot() {
        let library = vec![
            cand(song_sized("a", false, 0, "2024-01-01", 1_000_000)),
            cand(song_sized("b", false, 0, "2024-01-01", 1_000_000)),
        ];
        let input = PipelineInput::default().with_pool(SourceKind::Library, None, library);
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            budget: BudgetStage {
                max_bytes: Some(10_000_000),
                target_duration_secs: Some(300), // each fixture track is 180s; two would overshoot
                ..Default::default()
            },
            ..Default::default()
        };

        let result = run_pipeline(&input, &pipeline);
        assert_eq!(ids(&result), vec!["a"]);
    }

    #[test]
    fn empty_album_ids_are_singletons() {
        let mut first = song_sized("a", false, 0, "2024-01-01", 1_000_000);
        first.album_id = Some(String::new());
        let mut second = song_sized("b", false, 0, "2024-01-01", 1_000_000);
        second.album_id = Some(String::new());

        let input = PipelineInput::default().with_pool(
            SourceKind::Library,
            None,
            vec![cand(first), cand(second)],
        );
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            unit: Unit::Album,
            budget: BudgetStage {
                max_bytes: Some(1_500_000),
                ..Default::default()
            },
            ..Default::default()
        };

        let result = run_pipeline(&input, &pipeline);
        assert_eq!(
            ids(&result),
            vec!["a"],
            "empty album IDs must not form one atomic group"
        );
    }

    #[test]
    fn overflowing_size_estimates_are_skipped_without_breaking_budget() {
        let mut impossible = song_bitrate("overflow", u32::MAX, u32::MAX);
        impossible.size_bytes = None;
        let good = song_sized("good", false, 0, "2024-01-01", 1_000_000);

        let input = PipelineInput::default().with_pool(
            SourceKind::Library,
            None,
            vec![cand(impossible), cand(good)],
        );
        let pipeline = AutoFillPipeline::default_legacy(Some(10_000_000));
        let result = run_pipeline(&input, &pipeline);

        assert_eq!(ids(&result), vec!["good"]);
        let total: u64 = result.iter().map(|i| i.size_bytes).sum();
        assert!(total <= 10_000_000);
    }

    #[test]
    fn manual_exclude_and_within_run_dedup_honored() {
        // "dup" appears in both Favorites and Library → emitted once. "excluded" is manually
        // selected → never emitted.
        let favorites = vec![
            cand(song_sized("dup", true, 0, "2024-01-01", 1_000_000)),
            cand(song_sized("excluded", true, 0, "2024-01-01", 1_000_000)),
        ];
        let library = vec![
            cand(song_sized("dup", true, 0, "2024-01-01", 1_000_000)),
            cand(song_sized("only-lib", false, 0, "2024-01-01", 1_000_000)),
        ];
        let input = PipelineInput {
            exclude_item_ids: vec!["excluded".to_string()],
            ..Default::default()
        }
        .with_pool(SourceKind::Favorites, None, favorites)
        .with_pool(SourceKind::Library, None, library);

        let pipeline = AutoFillPipeline {
            sources: vec![
                SourceEntry::new(SourceKind::Favorites),
                SourceEntry::new(SourceKind::Library),
            ],
            budget: BudgetStage {
                max_bytes: Some(100_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = run_pipeline(&input, &pipeline);
        let result_ids = ids(&result);
        assert_eq!(
            result_ids.iter().filter(|id| *id == "dup").count(),
            1,
            "no within-run duplicate"
        );
        assert!(
            !result_ids.contains(&"excluded".to_string()),
            "manual-exclude honored"
        );
        assert!(result_ids.contains(&"only-lib".to_string()));
    }

    #[test]
    fn empty_library_yields_empty() {
        let input = PipelineInput::default().with_pool(SourceKind::Library, None, vec![]);
        let pipeline = AutoFillPipeline::default_legacy(Some(10_000_000));
        assert!(run_pipeline(&input, &pipeline).is_empty());
    }

    #[test]
    fn zero_budget_yields_empty() {
        let library = vec![cand(song_sized("a", true, 0, "2024-01-01", 1_000_000))];
        let input = PipelineInput::default().with_pool(SourceKind::Library, None, library);
        let pipeline = AutoFillPipeline::default_legacy(Some(0));
        assert!(
            run_pipeline(&input, &pipeline).is_empty(),
            "zero budget selects nothing"
        );
    }

    #[test]
    fn fallback_reached_only_after_primary_exhaustion() {
        // Primary playlist has a single small track; fallback library fills the rest of the budget.
        let energy = vec![cand(song_sized("e1", false, 0, "2024-01-01", 1_000_000))];
        let library = vec![
            cand(song_sized("lib1", false, 0, "2024-01-01", 1_000_000)),
            cand(song_sized("lib2", false, 0, "2024-01-01", 1_000_000)),
        ];
        let input = PipelineInput::default()
            .with_pool(SourceKind::Playlist, Some("energy"), energy)
            .with_pool(SourceKind::Library, None, library);

        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry {
                kind: SourceKind::Playlist,
                ref_id: Some("energy".to_string()),
                share: None,
            }],
            fallback: vec![SourceEntry::new(SourceKind::Library)],
            budget: BudgetStage {
                max_bytes: Some(10_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = run_pipeline(&input, &pipeline);
        assert_eq!(
            ids(&result),
            vec!["e1", "lib1", "lib2"],
            "primary first, then fallback fills"
        );
        // The primary item keeps its source reason; fallback items are tagged as such.
        assert_eq!(result[0].priority_reason, "playlist:energy");
        assert!(result[1].priority_reason.starts_with("fallback:"));
    }

    #[test]
    fn share_allocates_budget_across_sources() {
        // Two equal-share sources over a 10 MB budget → ~5 MB each (5 × 1 MB tracks available
        // per source, but each capped at 5 MB → 5 tracks each = 10 total).
        let favorites = (0..8)
            .map(|i| {
                cand(song_sized(
                    &format!("f{i}"),
                    false,
                    0,
                    "2024-01-01",
                    1_000_000,
                ))
            })
            .collect();
        let library = (0..8)
            .map(|i| {
                cand(song_sized(
                    &format!("l{i}"),
                    false,
                    0,
                    "2024-01-01",
                    1_000_000,
                ))
            })
            .collect();
        let input = PipelineInput::default()
            .with_pool(SourceKind::Favorites, None, favorites)
            .with_pool(SourceKind::Library, None, library);

        let pipeline = AutoFillPipeline {
            sources: vec![
                SourceEntry {
                    kind: SourceKind::Favorites,
                    ref_id: None,
                    share: Some(0.5),
                },
                SourceEntry {
                    kind: SourceKind::Library,
                    ref_id: None,
                    share: Some(0.5),
                },
            ],
            budget: BudgetStage {
                max_bytes: Some(10_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = run_pipeline(&input, &pipeline);
        let fav_count = result.iter().filter(|i| i.id.starts_with('f')).count();
        let lib_count = result.iter().filter(|i| i.id.starts_with('l')).count();
        assert_eq!(fav_count, 5, "favorites capped at its 50% share");
        assert_eq!(lib_count, 5, "library capped at its 50% share");
    }

    #[test]
    fn unshared_sources_split_budget_equally() {
        let favorites = (0..8)
            .map(|i| {
                cand(song_sized(
                    &format!("f{i}"),
                    false,
                    0,
                    "2024-01-01",
                    1_000_000,
                ))
            })
            .collect();
        let library = (0..8)
            .map(|i| {
                cand(song_sized(
                    &format!("l{i}"),
                    false,
                    0,
                    "2024-01-01",
                    1_000_000,
                ))
            })
            .collect();
        let input = PipelineInput::default()
            .with_pool(SourceKind::Favorites, None, favorites)
            .with_pool(SourceKind::Library, None, library);

        let pipeline = AutoFillPipeline {
            sources: vec![
                SourceEntry::new(SourceKind::Favorites),
                SourceEntry::new(SourceKind::Library),
            ],
            budget: BudgetStage {
                max_bytes: Some(10_000_000),
                ..Default::default()
            },
            ..Default::default()
        };

        let result = run_pipeline(&input, &pipeline);
        let fav_count = result.iter().filter(|i| i.id.starts_with('f')).count();
        let lib_count = result.iter().filter(|i| i.id.starts_with('l')).count();
        assert_eq!(fav_count, 5);
        assert_eq!(lib_count, 5);
    }

    // ===================================================================
    // Model / serde shape (AC 1) — config persisted verbatim by Story 12.2.
    // ===================================================================

    #[test]
    fn pipeline_serde_round_trips_with_camelcase_shape() {
        let json = r#"{
            "enabled": true,
            "filter": { "includeGenres": ["kids"], "excludeTags": ["explicit"] },
            "sources": [ { "kind": "playlist", "ref": "energy", "share": 0.5 }, { "kind": "library" } ],
            "unit": "track",
            "ordering": ["favorite", "playCount", "dateCreated", "quality"],
            "memory": { "cooldownWeeks": 2, "playedExclusion": true },
            "budget": { "maxBytes": 8000000000, "headroomBytes": 50000000 },
            "fallback": [ { "kind": "library" } ]
        }"#;
        let pipeline: AutoFillPipeline = serde_json::from_str(json).expect("camelCase JSON parses");

        assert!(pipeline.enabled);
        assert_eq!(pipeline.filter.include_genres, vec!["kids"]);
        assert_eq!(pipeline.sources[0].kind, SourceKind::Playlist);
        assert_eq!(pipeline.sources[0].ref_id.as_deref(), Some("energy"));
        assert_eq!(pipeline.sources[0].share, Some(0.5));
        assert_eq!(pipeline.unit, Unit::Track);
        assert_eq!(pipeline.ordering[0], OrderingKey::Favorite);
        assert_eq!(pipeline.memory.cooldown_weeks, Some(2));
        assert_eq!(pipeline.budget.max_bytes, Some(8_000_000_000));

        // Re-serialize and re-parse: the shape is stable for verbatim manifest persistence.
        let reser = serde_json::to_string(&pipeline).unwrap();
        let back: AutoFillPipeline = serde_json::from_str(&reser).unwrap();
        assert_eq!(pipeline, back);
    }

    #[test]
    fn empty_object_deserializes_to_pass_through_default() {
        // Every field is optional; an empty object is a valid (if inert) pipeline.
        let pipeline: AutoFillPipeline = serde_json::from_str("{}").unwrap();
        assert_eq!(pipeline, AutoFillPipeline::default());
        assert!(pipeline.sources.is_empty());
        assert_eq!(pipeline.unit, Unit::Track);
    }

    #[test]
    fn default_legacy_maps_legacy_block() {
        let p = AutoFillPipeline::default_legacy(Some(1_234));
        assert!(p.enabled);
        assert_eq!(
            p.ordering,
            vec![
                OrderingKey::Favorite,
                OrderingKey::PlayCount,
                OrderingKey::DateCreated
            ]
        );
        assert_eq!(p.sources, vec![SourceEntry::new(SourceKind::Library)]);
        assert_eq!(p.budget.max_bytes, Some(1_234));
    }

    // ===================================================================
    // Story 13.2 — Quality & Version ordering.
    // ===================================================================

    /// A song with explicit title/artist/album/format, for version + best-version tests.
    fn song_meta(
        id: &str,
        title: &str,
        artist: &str,
        album: &str,
        suffix: &str,
        bitrate: u32,
    ) -> Song {
        Song {
            title: title.to_string(),
            artist_name: Some(artist.to_string()),
            album_title: Some(album.to_string()),
            suffix: Some(suffix.to_string()),
            content_type: Some(format!("audio/{suffix}")),
            bitrate_kbps: Some(bitrate),
            size_bytes: Some(1_000_000),
            ..song_sized(id, false, 0, "2024-01-01", 1_000_000)
        }
    }

    // ---- AC 1: format_quality_rank --------------------------------------

    #[test]
    fn format_quality_rank_lossless_beats_lossy_beats_unknown() {
        let mut flac = song_bitrate("a", 0, 200);
        flac.suffix = Some("FLAC".to_string()); // case-insensitive
        flac.content_type = Some("audio/flac".to_string());
        assert_eq!(format_quality_rank(&flac), 2, "flac suffix → lossless");

        let mut alac_mime = song_bitrate("b", 0, 200);
        alac_mime.suffix = None;
        alac_mime.content_type = Some("audio/x-alac".to_string()); // x- prefix stripped
        assert_eq!(format_quality_rank(&alac_mime), 2, "alac mime → lossless");

        let mp3 = song_bitrate("c", 320, 200); // suffix "mp3" from fixture
        assert_eq!(format_quality_rank(&mp3), 1, "mp3 → lossy");

        let mut unknown = song_bitrate("d", 320, 200);
        unknown.suffix = None;
        unknown.content_type = None;
        assert_eq!(format_quality_rank(&unknown), 0, "no format metadata → unknown");
    }

    #[test]
    fn quality_ordering_is_lossless_first_then_bitrate() {
        // FLAC (no bitrate) > FLAC (low bitrate)?  flac-hi has higher bitrate so it leads its tier;
        // both lossless beat the 320 MP3, which beats the format-less unknown.
        let mut flac_hi = song_meta("flac-hi", "S1", "A", "Al", "flac", 1000);
        flac_hi.bitrate_kbps = Some(1000);
        let mut flac_lo = song_meta("flac-lo", "S2", "A", "Al", "flac", 500);
        flac_lo.bitrate_kbps = Some(500);
        let mp3 = song_meta("mp3", "S3", "A", "Al", "mp3", 320);
        let mut unknown = song_meta("unknown", "S4", "A", "Al", "mp3", 320);
        unknown.suffix = None;
        unknown.content_type = None;

        let input = PipelineInput::default().with_pool(
            SourceKind::Library,
            None,
            vec![cand(unknown), cand(mp3), cand(flac_lo), cand(flac_hi)],
        );
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            ordering: vec![OrderingKey::Quality],
            budget: BudgetStage {
                max_bytes: Some(100_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            ids(&run_pipeline(&input, &pipeline)),
            vec!["flac-hi", "flac-lo", "mp3", "unknown"],
        );
    }

    #[test]
    fn quality_ordering_breaks_ties_by_bitrate_within_a_tier() {
        let hi = song_meta("hi", "S", "A", "Al", "mp3", 320);
        let lo = song_meta("lo", "S", "A", "Al", "mp3", 128);
        let input = PipelineInput::default().with_pool(
            SourceKind::Library,
            None,
            vec![cand(lo), cand(hi)],
        );
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            ordering: vec![OrderingKey::Quality],
            budget: BudgetStage {
                max_bytes: Some(100_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(ids(&run_pipeline(&input, &pipeline)), vec!["hi", "lo"]);
    }

    #[test]
    fn pipelines_without_quality_key_are_unaffected_by_format() {
        // Default-legacy ordering never lists Quality, so a FLAC and an MP3 sort purely by the
        // legacy keys (here: equal favorite/playcount, so date desc) — format is irrelevant.
        let flac = song_meta("flac", "S1", "A", "Al", "flac", 100);
        let mp3 = song_meta("mp3", "S2", "B", "Al2", "mp3", 320);
        let input = PipelineInput::default().with_pool(
            SourceKind::Library,
            None,
            vec![cand(flac), cand(mp3)],
        );
        let pipeline = AutoFillPipeline::default_legacy(Some(100_000_000));
        // Same date → stable order preserved (flac first as inserted) — NOT reordered by format.
        assert_eq!(ids(&run_pipeline(&input, &pipeline)), vec!["flac", "mp3"]);
    }

    // ---- AC 3: version-trait detection ----------------------------------

    fn traits_of(title: &str) -> Vec<VersionTrait> {
        let mut s = song_sized("x", false, 0, "2024-01-01", 1);
        s.title = title.to_string();
        s.album_title = None;
        detect_version_traits(&s)
    }

    #[test]
    fn version_trait_detection_each_trait() {
        assert_eq!(traits_of("My Song"), vec![VersionTrait::Studio]);
        assert!(traits_of("My Song (Live)").contains(&VersionTrait::Live));
        assert!(traits_of("My Song - Live at Wembley").contains(&VersionTrait::Live));
        assert!(traits_of("MTV Unplugged").contains(&VersionTrait::Live));
        assert!(traits_of("My Song (2011 Remaster)").contains(&VersionTrait::Remastered));
        assert!(traits_of("My Song - Remastered").contains(&VersionTrait::Remastered));
        assert!(traits_of("My Song (Club Remix)").contains(&VersionTrait::Remix));
        assert!(traits_of("My Song (Acoustic)").contains(&VersionTrait::Acoustic));
        assert!(traits_of("My Song (Demo)").contains(&VersionTrait::Demo));
    }

    #[test]
    fn version_trait_false_positive_guards() {
        // "Alive"/"Believe" must NOT be Live; "Demolition"/"Demon" must NOT be Demo.
        assert_eq!(traits_of("Stayin' Alive"), vec![VersionTrait::Studio]);
        assert_eq!(traits_of("Don't Believe"), vec![VersionTrait::Studio]);
        assert_eq!(traits_of("Demolition Man"), vec![VersionTrait::Studio]);
        assert_eq!(traits_of("Speak of the Demon"), vec![VersionTrait::Studio]);
        // "mix" alone is not a remix marker.
        assert_eq!(traits_of("Mixtape Intro"), vec![VersionTrait::Studio]);
    }

    #[test]
    fn version_trait_multi_match() {
        let t = traits_of("My Song (Live) [2011 Remaster]");
        assert!(t.contains(&VersionTrait::Live));
        assert!(t.contains(&VersionTrait::Remastered));
        assert!(!t.contains(&VersionTrait::Studio), "studio is only the absence of markers");
    }

    #[test]
    fn version_trait_reads_album_title_too() {
        let mut s = song_sized("x", false, 0, "2024-01-01", 1);
        s.title = "My Song".to_string();
        s.album_title = Some("Unplugged in New York".to_string());
        assert!(detect_version_traits(&s).contains(&VersionTrait::Live));
    }

    // ---- AC 4/5: version preference parse + ordering tiebreak -----------

    #[test]
    fn version_rank_empty_preference_is_no_op() {
        let s = song_meta("s", "Song (Live)", "A", "Al", "mp3", 320);
        assert_eq!(version_rank(&s, &[]), 0);
    }

    #[test]
    fn version_rank_index_of_first_match_else_last() {
        let prefs = [VersionTrait::Studio, VersionTrait::Live];
        let live = song_meta("a", "Song (Live)", "A", "Al", "mp3", 320);
        let studio = song_meta("b", "Song", "A", "Al", "mp3", 320);
        let remix = song_meta("c", "Song (Remix)", "A", "Al", "mp3", 320);
        assert_eq!(version_rank(&studio, &prefs), 0, "studio is first preference");
        assert_eq!(version_rank(&live, &prefs), 1, "live is second preference");
        assert_eq!(version_rank(&remix, &prefs), 2, "no listed trait → last (len)");
    }

    #[test]
    fn version_preference_orders_preferred_versions_first() {
        // Prefer Live over Studio; with no other distinguishing keys, the live cut leads.
        let studio = song_meta("studio", "Song", "A", "Al", "mp3", 320);
        let live = song_meta("live", "Song (Live)", "A", "Al", "mp3", 320);
        let input = PipelineInput::default().with_pool(
            SourceKind::Library,
            None,
            vec![cand(studio), cand(live)],
        );
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            quality: QualityStage {
                version_preference: vec![VersionTrait::Live],
                ..Default::default()
            },
            budget: BudgetStage {
                max_bytes: Some(100_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(ids(&run_pipeline(&input, &pipeline)), vec!["live", "studio"]);
    }

    #[test]
    fn version_preference_is_a_trailing_tiebreak_under_explicit_keys() {
        // Favorite dominates version preference: the favored studio cut wins even though Live is
        // the preferred version (AC 5 — version pref applies AFTER the explicit ordering keys).
        let studio_fav = song_meta("studio-fav", "Song", "A", "Al", "mp3", 320);
        let mut studio_fav = studio_fav;
        studio_fav.is_favorite = Some(true);
        let live = song_meta("live", "Song (Live)", "A", "Al", "mp3", 320);
        let input = PipelineInput::default().with_pool(
            SourceKind::Library,
            None,
            vec![cand(live), cand(studio_fav)],
        );
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            ordering: vec![OrderingKey::Favorite],
            quality: QualityStage {
                version_preference: vec![VersionTrait::Live],
                ..Default::default()
            },
            budget: BudgetStage {
                max_bytes: Some(100_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            ids(&run_pipeline(&input, &pipeline)),
            vec!["studio-fav", "live"],
            "explicit Favorite key dominates the version-preference tiebreak",
        );
    }

    #[test]
    fn version_preference_parse_is_malformed_tolerant_and_dedups() {
        // AC 4 / 13.1 parse_tiers precedent: an unknown trait, a non-string entry, and a duplicate
        // are all dropped on parse — the slot is never aborted. Order of the survivors is preserved.
        let q: QualityStage = serde_json::from_str(
            r#"{ "versionPreference": ["live", "bogus", 42, "remastered", "live"] }"#,
        )
        .expect("malformed entries are tolerated, never an error");
        assert_eq!(
            q.version_preference,
            vec![VersionTrait::Live, VersionTrait::Remastered],
            "unknown/non-string dropped, duplicates collapsed, order preserved",
        );

        // An all-bogus list degrades cleanly to "no preference".
        let empty: QualityStage =
            serde_json::from_str(r#"{ "versionPreference": ["nope", null] }"#).unwrap();
        assert!(empty.version_preference.is_empty());
    }

    // ---- AC 6/7: best-version resolution --------------------------------

    #[test]
    fn best_version_keeps_lossless_studio_over_lossy_live() {
        // Same song, same artist: a FLAC studio cut and a lossy live cut collapse to the FLAC.
        let flac = song_meta("flac-studio", "My Song", "The Band", "Album", "flac", 900);
        let live = song_meta("mp3-live", "My Song (Live)", "The Band", "Live Album", "mp3", 320);
        let input = PipelineInput::default().with_pool(
            SourceKind::Library,
            None,
            vec![cand(live), cand(flac)],
        );
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            quality: QualityStage {
                best_version: true,
                ..Default::default()
            },
            budget: BudgetStage {
                max_bytes: Some(100_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            ids(&run_pipeline(&input, &pipeline)),
            vec!["flac-studio"],
            "best-version collapses the duplicate, keeping the lossless cut",
        );
    }

    #[test]
    fn best_version_preference_flips_the_winner() {
        // With Live preferred, the live cut wins the collapse even though it's lossy.
        let flac = song_meta("flac-studio", "My Song", "The Band", "Album", "flac", 900);
        let live = song_meta("mp3-live", "My Song (Live)", "The Band", "Live Album", "mp3", 320);
        let input = PipelineInput::default().with_pool(
            SourceKind::Library,
            None,
            vec![cand(flac), cand(live)],
        );
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            quality: QualityStage {
                best_version: true,
                version_preference: vec![VersionTrait::Live],
            },
            budget: BudgetStage {
                max_bytes: Some(100_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(ids(&run_pipeline(&input, &pipeline)), vec!["mp3-live"]);
    }

    #[test]
    fn best_version_never_over_merges_distinct_songs_or_unknown_artist() {
        // Different titles, different artists, and a None-artist candidate all survive.
        let a = song_meta("a", "Song One", "Artist X", "Al", "mp3", 320);
        let b = song_meta("b", "Song Two", "Artist X", "Al", "mp3", 320);
        let c = song_meta("c", "Song One", "Artist Y", "Al", "mp3", 320); // same title, other artist
        let mut d = song_meta("d", "Song One", "Artist X", "Al", "flac", 900);
        d.artist_name = None; // unknown artist → never collapsed even though title matches `a`
        let input = PipelineInput::default().with_pool(
            SourceKind::Library,
            None,
            vec![cand(a), cand(b), cand(c), cand(d)],
        );
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            quality: QualityStage {
                best_version: true,
                ..Default::default()
            },
            budget: BudgetStage {
                max_bytes: Some(100_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let got = ids(&run_pipeline(&input, &pipeline));
        for id in ["a", "b", "c", "d"] {
            assert!(got.contains(&id.to_string()), "{id} must survive (no over-merge)");
        }
    }

    #[test]
    fn best_version_collapses_across_pools() {
        // Winner (FLAC studio) in the library, loser (lossy live) in a playlist → the playlist
        // loser is dropped; the library winner remains.
        let flac = song_meta("flac-studio", "My Song", "The Band", "Album", "flac", 900);
        let live = song_meta("mp3-live", "My Song (Live)", "The Band", "Live Album", "mp3", 320);
        let input = PipelineInput::default()
            .with_pool(SourceKind::Library, None, vec![cand(flac)])
            .with_pool(SourceKind::Playlist, Some("set"), vec![cand(live)]);
        let pipeline = AutoFillPipeline {
            sources: vec![
                SourceEntry {
                    kind: SourceKind::Playlist,
                    ref_id: Some("set".to_string()),
                    share: None,
                },
                SourceEntry::new(SourceKind::Library),
            ],
            quality: QualityStage {
                best_version: true,
                ..Default::default()
            },
            budget: BudgetStage {
                max_bytes: Some(100_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let got = ids(&run_pipeline(&input, &pipeline));
        assert_eq!(got, vec!["flac-studio"], "cross-pool collapse keeps the global winner only");
    }

    #[test]
    fn best_version_disabled_keeps_all_duplicates() {
        let flac = song_meta("flac-studio", "My Song", "The Band", "Album", "flac", 900);
        let live = song_meta("mp3-live", "My Song (Live)", "The Band", "Live Album", "mp3", 320);
        let input = PipelineInput::default().with_pool(
            SourceKind::Library,
            None,
            vec![cand(flac), cand(live)],
        );
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            budget: BudgetStage {
                max_bytes: Some(100_000_000),
                ..Default::default()
            },
            ..Default::default() // best_version defaults to false
        };
        assert_eq!(run_pipeline(&input, &pipeline).len(), 2, "no collapse when disabled");
    }

    #[test]
    fn best_version_never_emits_zero_byte_or_over_budget() {
        // Collapse only removes candidates; the surviving winner still respects the budget ceiling.
        let flac = song_meta("flac-studio", "My Song", "The Band", "Album", "flac", 900);
        let live = song_meta("mp3-live", "My Song (Live)", "The Band", "Live Album", "mp3", 320);
        let other = song_meta("other", "Another", "The Band", "Album", "mp3", 320);
        let input = PipelineInput::default().with_pool(
            SourceKind::Library,
            None,
            vec![cand(flac), cand(live), cand(other)],
        );
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            quality: QualityStage {
                best_version: true,
                ..Default::default()
            },
            budget: BudgetStage {
                max_bytes: Some(1_500_000), // fits exactly one 1 MB track
                ..Default::default()
            },
            ..Default::default()
        };
        let result = run_pipeline(&input, &pipeline);
        let total: u64 = result.iter().map(|i| i.size_bytes).sum();
        assert!(total <= 1_500_000, "budget respected");
        assert!(result.iter().all(|i| i.size_bytes > 0), "never a 0-byte filler");
    }

    #[test]
    fn strip_version_markers_normalizes_to_a_shared_base() {
        assert_eq!(normalize_ws(&strip_version_markers("My Song (Live)")), "my song");
        assert_eq!(normalize_ws(&strip_version_markers("My Song - 2011 Remaster")), "my song");
        assert_eq!(normalize_ws(&strip_version_markers("My Song [Acoustic]")), "my song");
        assert_eq!(normalize_ws(&strip_version_markers("My Song - Live at Wembley")), "my song");
        // A non-version parenthetical is preserved (distinct songs stay distinct).
        assert_eq!(
            normalize_ws(&strip_version_markers("My Song (feat. Guest)")),
            "my song (feat. guest)",
        );
    }

    // ---- AC 8/11: routing + serde round-trip ----------------------------

    #[test]
    fn quality_stage_serde_round_trips() {
        let json = r#"{
            "ordering": ["quality"],
            "quality": { "bestVersion": true, "versionPreference": ["live", "remastered"] }
        }"#;
        let p: AutoFillPipeline = serde_json::from_str(json).unwrap();
        assert!(p.quality.best_version);
        assert_eq!(
            p.quality.version_preference,
            vec![VersionTrait::Live, VersionTrait::Remastered]
        );
        let back: AutoFillPipeline = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn quality_stage_default_omitted_pipeline_is_default() {
        // A pipeline with no `quality` key deserializes to the default QualityStage (today's behavior).
        let p: AutoFillPipeline = serde_json::from_str("{}").unwrap();
        assert_eq!(p.quality, QualityStage::default());
        assert!(!p.quality.best_version);
        assert!(p.quality.version_preference.is_empty());
    }
}
