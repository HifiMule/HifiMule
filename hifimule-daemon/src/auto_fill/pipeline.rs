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
    for unit in units.iter_mut() {
        unit.sort_by(|a, b| compare_by_ordering(&a.song, &b.song, &pipeline.ordering));
    }
    units.sort_by(|a, b| match (a.first(), b.first()) {
        (Some(x), Some(y)) => compare_by_ordering(&x.song, &y.song, &pipeline.ordering),
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

/// Compare two songs by the ordered ranking keys. Returns the ordering that places the "better"
/// song first (i.e. ascending sort yields the desired ranking). Stable on full ties.
fn compare_by_ordering(a: &Song, b: &Song, keys: &[OrderingKey]) -> std::cmp::Ordering {
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
            // higher bitrate first
            OrderingKey::Quality => b
                .bitrate_kbps
                .unwrap_or(0)
                .cmp(&a.bitrate_kbps.unwrap_or(0)),
            // deterministic no-op in 12.1 — no entropy in the pure core (Epic 13 adds seeding)
            OrderingKey::Random => Ordering::Equal,
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    Ordering::Equal
}

fn fav_rank(s: &Song) -> u8 {
    u8::from(s.is_favorite == Some(true))
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
        // Antoine: 512 GB DAP, quality-first. Large budget, ordering [Quality]. Higher-bitrate
        // tracks rank first and the (relatively) large budget is filled.
        let library = vec![
            cand(song_bitrate("low", 128, 200)),
            cand(song_bitrate("hi", 1_411, 200)), // FLAC-ish
            cand(song_bitrate("mid", 320, 200)),
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
            vec!["hi", "mid", "low"],
            "tracks must rank by descending bitrate"
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
}
