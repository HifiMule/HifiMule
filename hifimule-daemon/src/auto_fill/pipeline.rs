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
    /// Weighted rarity draw (Story 13.4 #29): loot-table common/rare/legendary classes feeding the
    /// [`OrderingKey::Rarity`] draw. Default ⇒ zero behavior change.
    pub rarity: RarityStage,
    /// Pity timer (Story 13.4 #30): a deterministic discovery reserve that fires after a dry streak.
    /// Default (`enabled:false`) ⇒ zero behavior change; the dry-streak counter is machine-local DB
    /// state carried via [`PipelineInput::pity_streak`], never stored in the manifest.
    pub pity: PityStage,
    /// Clock-driven Context stage (Story 13.5 #3 time-of-day / #17 energy-curve / #32 seasonal) — the
    /// brainstorm's *cheap proxies* expressed as one ordered list of context rules (time/calendar
    /// window + source-activation/weighting + scheduled tag filter). Evaluated purely against
    /// [`HistorySnapshot::local`]. Default (`enabled:false`, no rules) ⇒ zero behavior change.
    pub context: ContextStage,
    /// Advanced-unit & promotion modifiers (Story 13.6 #33/#8/#9/#27) — Artist Spotlight, album/track
    /// space ratio, affinity-triggered album promotion, and coherence ordering, as reserve pre-passes /
    /// unit-grouping refinement / output reorder over the existing `unit` axis. All-default ⇒ zero
    /// behavior change.
    pub promotion: PromotionStage,
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
    /// Story 13.4 — a seeded uniform shuffle (every song weight 1). Deterministic given
    /// [`PipelineInput::seed`]; a pipeline that never lists `Random` is byte-for-byte unaffected.
    /// (Shipped as a deterministic no-op since 12.1; activated here once the engine carries a seed.)
    Random,
    /// Higher bitrate first.
    Quality,
    /// Story 13.3 #14 — fewer plays first: surfaces owned-but-barely-played music (deep cuts).
    /// The exact inverse of [`OrderingKey::PlayCount`]; a never-played track (`None`/0) ranks first.
    Excavation,
    /// Story 13.3 #31 (cheap musical-memories) — oldest-added first: resurfaces music added long
    /// ago. Inverts [`OrderingKey::DateCreated`] for *present* dates; an absent or blank
    /// `date_added` deliberately sorts LAST (not a strict inverse — a missing add-date is the
    /// *worst* rediscovery candidate, per AC 3), so unknown-date tracks sink under both keys.
    Rediscovery,
    /// Story 13.4 #29 — a seeded *weighted* loot-table draw. Each song draws an Efraimidis–Spirakis
    /// key `u^(1/w)` from `(seed, id, rarity-class weight)`; a higher-weighted class tends to draw
    /// earlier. Reads its weights from [`AutoFillPipeline::rarity`]; degrades to a uniform shuffle
    /// when `rarity.enabled` is false. Deterministic given [`PipelineInput::seed`].
    Rarity,
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
    /// Story 13.5 #20 (encoding-from-goals): derive the transcode bitrate backwards from the
    /// size + duration goals instead of guessing. When `true` AND both `max_bytes` and a positive
    /// `target_duration_secs` are set, [`target_bitrate_kbps`] yields a clamped target bitrate that
    /// (a) makes the byte estimate bitrate-aware so the fill packs to the duration goal within the
    /// byte ceiling, and (b) overrides the device profile's `max_bitrate_kbps` for that slot's
    /// auto-fill downloads at sync. Default `false` ⇒ today's behavior.
    pub encoding_from_goals: bool,
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

/// Weighted rarity draw (Story 13.4 #29) — loot-table classes. A candidate's rarity class is derived
/// purely from `Song.play_count` (the only universal signal — there is no rating field): `None`/`0`
/// → legendary, `1..=rare_max_plays` → rare, else → common. The [`OrderingKey::Rarity`] arm draws
/// each song with an Efraimidis–Spirakis key `u^(1/w)` where `w` is its class weight.
///
/// All-default (`enabled:false`, weights `0.0`, `rare_max_plays:0`) ⇒ today's behavior, exactly like
/// [`QualityStage::default()`] — so a default `RarityStage` is omitted from the routing discriminator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct RarityStage {
    /// Off ⇒ zero behavior change. When off, an `OrderingKey::Rarity` degrades to a uniform shuffle.
    pub enabled: bool,
    /// Draw weight for the legendary class (never-/0-played — the "deepest" gems).
    pub legendary_weight: f32,
    /// Draw weight for the rare class (`1..=rare_max_plays`).
    pub rare_weight: f32,
    /// Draw weight for the common class (`> rare_max_plays` — the hits).
    pub common_weight: f32,
    /// Boundary play-count between rare and common. (UI default 5; struct default 0 keeps the no-op.)
    pub rare_max_plays: u32,
}

/// Pity timer (Story 13.4 #30) — a deterministic discovery guarantee after a dry streak. The dry
/// streak itself is machine-local DB state carried via [`PipelineInput::pity_streak`]; only this
/// *config* lives in the manifest (storage split, architecture.md:922).
///
/// All-default (`enabled:false`) ⇒ today's behavior — no reserve, no counter interaction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PityStage {
    /// Off ⇒ no reserve, no counter interaction.
    pub enabled: bool,
    /// Dry syncs before the guarantee fires. (UI default 3; struct default 0 keeps the no-op off.)
    pub threshold_syncs: u32,
    /// Fraction of the budget reserved for discovery when the guarantee fires (`0.0..=1.0`).
    pub guaranteed_ratio: f32,
    /// A "discovery" candidate has `play_count <= this`. (UI default 0 = never-played.)
    pub discovery_max_plays: u32,
}

/// Advanced-unit & promotion modifiers (Story 13.6) — the brainstorm's remaining **Unit**-stage ideas
/// expressed as one additive, default-noop stage that *augments* (never replaces) [`AutoFillPipeline::unit`]:
/// **#33 Artist Spotlight** (a reserve pre-pass featuring one deterministically-chosen artist in depth),
/// **#8 album/track space ratio** (a reserve pre-pass filling complete atomic albums first, the rest as
/// loose tracks), **#9 affinity-triggered album promotion** (when base `unit == Track`, an album with
/// `>= n` favorited candidates is grouped into a single atomic album unit), and **#27 coherence ordering**
/// (a reorder-only pass clustering the final selection by artist→album→disc→track — the selected id-set and
/// byte total are unchanged). All four are pure functions of the candidate set; Spotlight's per-sync
/// variation rides the already-threaded [`PipelineInput::seed`] (no new entropy, no clock/RNG read).
///
/// All-default (`spotlight:false`, all `None`, `coherence:false`) ⇒ today's behavior, exactly like
/// [`QualityStage::default()`] — so a default `PromotionStage` is omitted from the routing discriminator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PromotionStage {
    /// #33 Artist Spotlight: feature ONE artist in depth via a track-level reserve pre-pass.
    pub spotlight: bool,
    /// Share of the byte ceiling reserved for the featured artist (clamped `0.0..=1.0` at consumption).
    /// `None` ⇒ default `0.5`. `0` (or an unbounded ceiling / no artist-bearing candidate) ⇒ no-op.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spotlight_share: Option<f32>,
    /// #8 album/track space ratio: fraction of the ceiling filled as COMPLETE albums (atomic), the
    /// remainder as the base unit. Clamped `0.0..=1.0` at consumption. `None`/`0` ⇒ no album reserve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album_track_ratio: Option<f32>,
    /// #9 affinity promotion: when base `unit == Track`, an album with `>= n` favorited candidate tracks
    /// is promoted to a single atomic album unit. `None`/`0` ⇒ no promotion (today's track grouping).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub promote_album_min_favorites: Option<u32>,
    /// #27 coherence: reorder the final selection by artist→album→disc→track for flow. Reorder-only —
    /// the selected id-set and byte total are byte-identical to the un-clustered run.
    pub coherence: bool,
}

/// Clock-driven Context stage (Story 13.5) — one mechanism for three brainstorm ideas (#3 time-of-day,
/// #17 energy-curve, #32 seasonal), each delivered as its prescribed *cheap proxy*. The stage is an
/// ordered list of [`ContextRule`]s; a rule is *active* when its [`ContextWindow`] matches the
/// caller-supplied [`CivilTime`] ([`context_rule_active`]). Active rules (a) gate/weight which
/// already-configured sources run (the playlist proxy for #3/#17) and (b) augment the effective
/// [`FilterStage`] with scheduled include/exclude tags+genres (the seasonal proxy for #32). The stage
/// never invents candidates, so every downstream budget/dedup/memory guarantee is unchanged.
///
/// All-default (`enabled:false`, no rules) ⇒ today's behavior, exactly like [`QualityStage::default()`],
/// so a default `ContextStage` is omitted from the routing discriminator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ContextStage {
    /// Off ⇒ zero behavior change: no rule is ever consulted (AC 4).
    pub enabled: bool,
    /// Context rules, evaluated in order against [`HistorySnapshot::local`]. A malformed rule/window
    /// degrades to "no effect" via the parse-tolerant [`deserialize_context_rules`] (AC 3), never
    /// aborting the pipeline parse.
    #[serde(default, deserialize_with = "deserialize_context_rules")]
    pub rules: Vec<ContextRule>,
}

/// One context rule: a time/calendar window plus its effect (source activation/weighting +
/// scheduled tag/genre filter). See [`ContextStage`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ContextRule {
    /// When this rule is active.
    pub window: ContextWindow,
    /// `ref_id`s of pipeline sources this rule activates/boosts while active (#3/#17). A source named
    /// here is *retained* whenever any rule mentioning it is active; a source mentioned only in
    /// currently-inactive rules is suppressed; a source named in no rule always runs (AC 5).
    pub source_refs: Vec<String>,
    /// Optional share multiplier for this rule's activated sources (energy-curve phase emphasis).
    /// `None` ⇒ `1.0`. Multiple active rules touching one source compose by taking the **max** (AC 5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<f32>,
    /// Scheduled filter additions while active (#32 seasonal). Unioned with the static `FilterStage`;
    /// exclude wins over include on conflict (AC 6).
    pub include_tags: Vec<String>,
    pub exclude_tags: Vec<String>,
    pub include_genres: Vec<String>,
    pub exclude_genres: Vec<String>,
}

/// A context rule's time/calendar predicate (Story 13.5). All three kinds are evaluated purely
/// against the caller-supplied [`CivilTime`]; see [`context_rule_active`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ContextWindow {
    /// #3/#17: hour-of-day window, `start_hour <= hour < end_hour` (inclusive start, exclusive end).
    /// Wraps past midnight when `start_hour > end_hour` (e.g. `22..6` = "≥22 OR <6").
    TimeOfDay { start_hour: u8, end_hour: u8 },
    /// #32: active in any of these calendar months (`1..=12`), e.g. `[12]` = December, `[6,7,8]` = summer.
    Months { months: Vec<u8> },
    /// #32: `(month, day)` within `[start, end]` inclusive, wrapping across year-end when `start > end`
    /// (e.g. Dec 15 → Jan 5).
    DateRange { start: (u8, u8), end: (u8, u8) },
}

impl Default for ContextWindow {
    /// A default window matches nothing useful (an empty month list) — a `ContextRule::default()` is
    /// inert until configured. Used only as the serde container default.
    fn default() -> Self {
        ContextWindow::Months { months: Vec::new() }
    }
}

/// Parse-tolerant deserializer for [`ContextStage::rules`] (Story 13.5 AC 3), mirroring 13.2's
/// `deserialize_version_preference` and 13.1's `parse_tiers`: read a list of arbitrary JSON values,
/// keep the ones that deserialize into a well-formed [`ContextRule`], and silently drop the rest. A
/// malformed rule/window therefore degrades to "no effect" instead of aborting the whole pipeline parse.
fn deserialize_context_rules<'de, D>(deserializer: D) -> Result<Vec<ContextRule>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Vec::<serde_json::Value>::deserialize(deserializer)?;
    let mut out: Vec<ContextRule> = Vec::new();
    for value in raw {
        if let Ok(rule) = serde_json::from_value::<ContextRule>(value) {
            out.push(rule);
        }
    }
    Ok(out)
}

/// Story 13.5 AC 4: whether a context rule's window matches the caller-supplied civil time. Pure —
/// no clock/RNG. Mirrors the pure-predicate style of [`is_on_device`].
/// - `TimeOfDay`: `start <= hour < end`, with midnight-wrap when `start > end` (`hour >= start || hour < end`).
///   A degenerate `start == end` matches nothing.
/// - `Months`: the local month is listed.
/// - `DateRange`: `(month, day)` within `[start, end]` inclusive, with year-end wrap when `start > end`.
fn context_rule_active(rule: &ContextRule, local: &CivilTime) -> bool {
    match &rule.window {
        ContextWindow::TimeOfDay {
            start_hour,
            end_hour,
        } => {
            let h = local.hour;
            if start_hour == end_hour {
                false
            } else if start_hour < end_hour {
                h >= *start_hour && h < *end_hour
            } else {
                // midnight wrap: e.g. 22..6 ⇒ hour >= 22 OR hour < 6
                h >= *start_hour || h < *end_hour
            }
        }
        ContextWindow::Months { months } => months.contains(&local.month),
        ContextWindow::DateRange { start, end } => {
            // Encode (month, day) as a comparable ordinal `month*100 + day`.
            let key = u16::from(local.month) * 100 + u16::from(local.day);
            let s = u16::from(start.0) * 100 + u16::from(start.1);
            let e = u16::from(end.0) * 100 + u16::from(end.1);
            if s <= e {
                key >= s && key <= e
            } else {
                // year-end wrap: e.g. Dec 15 → Jan 5 ⇒ key >= Dec15 OR key <= Jan5
                key >= s || key <= e
            }
        }
    }
}

/// Story 13.5 AC 4: the context rules active for this run (window matches the civil time).
fn active_context_rules<'a>(stage: &'a ContextStage, local: &CivilTime) -> Vec<&'a ContextRule> {
    stage
        .rules
        .iter()
        .filter(|r| context_rule_active(r, local))
        .collect()
}

/// Story 13.5 AC 6: the effective filter for this run — the static [`FilterStage`] unioned with the
/// include/exclude tags+genres of every active rule. Exclude-wins-over-include is preserved by
/// [`filter_stage`] (an exclude match rejects regardless of include), so a plain union suffices. With
/// no active rules the static filter is returned unchanged.
fn effective_filter_with(base: &FilterStage, active: &[&ContextRule]) -> FilterStage {
    if active.is_empty() {
        return base.clone();
    }
    let mut f = base.clone();
    for r in active {
        f.include_tags.extend(r.include_tags.iter().cloned());
        f.exclude_tags.extend(r.exclude_tags.iter().cloned());
        f.include_genres.extend(r.include_genres.iter().cloned());
        f.exclude_genres.extend(r.exclude_genres.iter().cloned());
    }
    f
}

/// Story 13.5 AC 5: the effective source set for this run. Context gates only the sources its rules
/// *mention* (by `ref_id`): a source named in **no** rule always runs; a source named in **any active**
/// rule runs (weighted by the **max** of those rules' weights — documented choice to avoid unbounded
/// products; a non-positive weight is clamped to `0` and **suppresses** the source, same as an inactive
/// mention); a source named **only in inactive** rules is suppressed. When a non-trivial weight is in
/// play, the retained sources' shares are recomputed as `base_share × weight`, normalized to explicit
/// shares — so the energy-phase emphasis rides the existing [`source_caps`] machinery. With no weighting,
/// only suppression is applied (shares untouched, so a pure seasonal/time gate is byte-identical to the
/// configured blend minus the suppressed sources).
fn effective_sources(
    sources: &[SourceEntry],
    all_rules: &[ContextRule],
    active: &[&ContextRule],
) -> Vec<SourceEntry> {
    // ref_ids mentioned anywhere, and the max weight among *active* rules per ref_id.
    let mut mentioned_any: HashSet<&str> = HashSet::new();
    for r in all_rules {
        for rid in &r.source_refs {
            mentioned_any.insert(rid.as_str());
        }
    }
    let mut active_weight: HashMap<&str, f32> = HashMap::new();
    for r in active {
        // Clamp negatives to 0; a non-positive weight means "this active rule contributes no share for
        // this source" — i.e. suppress it (review 13.5). Without the clamp, `weight: 0.0` flowed into
        // the share normalization where an all-zero total fell back to an equal `1/n` split — inverting
        // the user's intent (0 = de-emphasize) into a full equal fill.
        let w = r.weight.unwrap_or(1.0).max(0.0);
        for rid in &r.source_refs {
            active_weight
                .entry(rid.as_str())
                .and_modify(|e| *e = e.max(w))
                .or_insert(w);
        }
    }

    // Retain: sources with no ref_id, or whose ref_id is in no rule, always run; a mentioned source
    // runs only when an active rule names it with a positive weight (weight 0 ⇒ suppressed, mirroring
    // an inactive mention — see the clamp above).
    let retained: Vec<SourceEntry> = sources
        .iter()
        .filter(|s| match s.ref_id.as_deref() {
            Some(rid) if mentioned_any.contains(rid) => {
                active_weight.get(rid).is_some_and(|w| *w > 0.0)
            }
            _ => true,
        })
        .cloned()
        .collect();

    let weight_of = |s: &SourceEntry| -> f32 {
        s.ref_id
            .as_deref()
            .and_then(|rid| active_weight.get(rid).copied())
            .unwrap_or(1.0)
    };
    let any_weight = retained.iter().any(|s| (weight_of(s) - 1.0).abs() > 1e-6);
    if !any_weight || retained.is_empty() {
        return retained;
    }

    // Recompute shares as base × weight, normalized to sum 1. `base` mirrors `source_caps`'
    // share/no-share split so a source's pre-existing share still informs the weighted blend.
    let n = retained.len() as f32;
    let any_share = retained.iter().any(|s| s.share.is_some());
    let explicit: f32 = retained.iter().filter_map(|s| s.share).sum();
    let n_unshared = retained.iter().filter(|s| s.share.is_none()).count();
    let remainder = (1.0 - explicit).max(0.0);
    let base_of = |s: &SourceEntry| -> f32 {
        if !any_share {
            1.0 / n
        } else {
            match s.share {
                Some(sh) => sh,
                None if n_unshared > 0 => remainder / n_unshared as f32,
                None => 0.0,
            }
        }
    };
    let weighted: Vec<f32> = retained.iter().map(|s| base_of(s) * weight_of(s)).collect();
    let total: f32 = weighted.iter().sum();
    retained
        .into_iter()
        .enumerate()
        .map(|(i, mut s)| {
            s.share = Some(if total > 0.0 {
                weighted[i] / total
            } else {
                1.0 / n
            });
            s
        })
        .collect()
}

/// Story 13.4: the size in bytes of the pity discovery reserve for this run — `0` when the reserve
/// does **not** fire. The reserve fires iff the feature is enabled, the dry streak has reached the
/// threshold, the budget is bounded (`ceiling != u64::MAX`), and the reserved fraction rounds to a
/// positive byte count. This is the **single source of truth** for the fire condition: [`run_pipeline`]
/// calls it to size/run the reserve, and the impure sync-completion path calls it (with the same
/// per-run ceiling) to decide whether the dry-streak was *genuinely consumed* this run — so the
/// fire-gate and the streak-reset gate can never drift. A streak that crosses the threshold while the
/// budget is unbounded or the ratio rounds to zero therefore reserves nothing **and** is not reset,
/// keeping the guarantee armed until a run where it actually fires. (Note: "fires" means the reserve
/// pass ran with a positive budget; whether the library still holds an undiscovered gem to fill it is
/// a content question the deterministic engine cannot answer here.)
pub fn pity_reserve_bytes(pity: &PityStage, pity_streak: i64, ceiling: u64) -> u64 {
    if !pity.enabled || pity_streak < i64::from(pity.threshold_syncs) || ceiling == u64::MAX {
        return 0;
    }
    let ratio = f64::from(pity.guaranteed_ratio).clamp(0.0, 1.0);
    ((ceiling as f64) * ratio).round() as u64
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
                encoding_from_goals: false,
            },
            fallback: Vec::new(),
            quality: QualityStage::default(),
            rarity: RarityStage::default(),
            pity: PityStage::default(),
            context: ContextStage::default(),
            promotion: PromotionStage::default(),
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

/// Caller-supplied **local civil time** (Story 13.5) — the clock-as-value carrier for the Context
/// stage (#3 time-of-day, #17 energy-curve, #32 seasonal). Exactly like [`HistorySnapshot::now`] and
/// [`PipelineInput::seed`], civil time enters the deliberately clock-free engine as a *value*, never
/// a read: the pure core derives every time-of-day / calendar decision from these fields and there is
/// **no** `Local::now`/`Utc::now`/`SystemTime`/`chrono` call anywhere in `pipeline.rs` or the
/// `fetch.rs` selection path. The single mint site is `rpc.rs`'s `now_civil()`, beside `now_unix_secs()`.
///
/// `#[derive(Default)]` ⇒ all-zero (`hour 0`, `month 0`, `day 0`, `weekday 0`). An all-zero value
/// matches no `Months`/`DateRange` rule and would match a `TimeOfDay` only at hour 0 — but the Context
/// stage is gated on **both** `ContextStage::enabled` (AC 4) **and** [`CivilTime::is_set`], so an
/// unminted default is never consulted even if `enabled` is true (review 13.5 hardening).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CivilTime {
    /// Hour of day, `0..=23` (local).
    pub hour: u8,
    /// Month, `1..=12` (local). `0` ⇒ "unset" (matches no calendar rule).
    pub month: u8,
    /// Day of month, `1..=31` (local). `0` ⇒ "unset".
    pub day: u8,
    /// Day of week, `0 = Monday ..= 6 = Sunday` (local). Reserved for future weekday windows.
    pub weekday: u8,
}

impl CivilTime {
    /// True when this value was minted from a real clock (not the all-zero [`Default`]). A real local
    /// civil time always has `month` in `1..=12`; only the unset default has `month == 0`. The Context
    /// stage consults civil time **only** when this is true (see [`run_pipeline`]), so an unminted
    /// default keeps the stage inert even if a caller sets `context.enabled = true` without minting
    /// `now_civil()` — hardening against the `TimeOfDay { start_hour: 0 }` footgun the all-zero default
    /// would otherwise trip (review 13.5). Production always mints civil time at the engine fill sites,
    /// so this is a defensive guard, not a behavior change for any current path.
    pub fn is_set(&self) -> bool {
        self.month != 0
    }
}

/// A snapshot of runtime history plus the caller's notion of "now" (Unix seconds). The engine
/// derives every time-based decision from this snapshot — never from the system clock.
#[derive(Debug, Clone, Default)]
pub struct HistorySnapshot {
    pub now: i64,
    pub entries: HashMap<String, TrackHistory>,
    /// Story 13.5: caller-supplied local civil time (hour/month/day/weekday). Drives the Context
    /// stage's window predicates. Defaults to all-zero (inert unless `context.enabled`).
    pub local: CivilTime,
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
    /// Story 13.4: caller-supplied entropy seed. Every random decision (`Random`/`Rarity` draws) is
    /// derived from this — the pure core never reads a clock or RNG. Same `(input, seed, pipeline)`
    /// ⇒ byte-identical output. `#[derive(Default)]` ⇒ `0` (no effect when no random key is used).
    /// Mirrors how [`HistorySnapshot::now`] carries "now" into the otherwise clock-free engine.
    pub seed: u64,
    /// Story 13.4: caller-supplied pity dry-streak counter (machine-local DB state, like the rotation
    /// cursor on `AutoFillParams`). The discovery reserve fires when `pity.enabled && pity_streak >=
    /// threshold`. Defaults to `0` (no effect when the pity stage is off).
    pub pity_streak: i64,
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

    // Best-version collapse (#11) is a pre-pass over the materialized pools: it only *removes*
    // losing-version candidates, so every downstream budget/dedup guarantee is untouched. Done
    // before any unit grouping/selection so a loser never occupies budget. The `ceiling` lets the
    // winner-selection prefer a version that can actually fit (Decision 2). Default (`false`) keeps
    // the borrowed input as-is — zero clone, zero behavior change.
    let collapsed;
    let input: &PipelineInput = if pipeline.quality.best_version {
        collapsed = collapse_best_version(input, pipeline, ceiling);
        &collapsed
    } else {
        input
    };

    // A one-stage pipeline with no `sources` still draws from the Library source so that the
    // legacy mapping (ordering + budget only) works.
    let default_sources;
    let sources: &[SourceEntry] = if pipeline.sources.is_empty() {
        default_sources = [SourceEntry::new(SourceKind::Library)];
        &default_sources
    } else {
        &pipeline.sources
    };

    // Context stage (Story 13.5 #3/#17/#32): when enabled, the active rules (matched purely against
    // the caller-supplied civil time) adjust *only* (a) the effective source set + weights and (b) the
    // effective filter — every budget/dedup/memory guarantee below is untouched. Disabled ⇒ byte-for-
    // byte today's behavior: the static `filter` and the configured sources/fallback verbatim (AC 4).
    let ctx_sources;
    let ctx_fallback;
    let ctx_filter;
    let (sources, fallback_sources, effective_filter): (
        &[SourceEntry],
        &[SourceEntry],
        &FilterStage,
    ) = if pipeline.context.enabled && input.history.local.is_set() {
        let active = active_context_rules(&pipeline.context, &input.history.local);
        ctx_filter = effective_filter_with(&pipeline.filter, &active);
        ctx_sources = effective_sources(sources, &pipeline.context.rules, &active);
        ctx_fallback = effective_sources(&pipeline.fallback, &pipeline.context.rules, &active);
        (&ctx_sources, &ctx_fallback, &ctx_filter)
    } else {
        (sources, pipeline.fallback.as_slice(), &pipeline.filter)
    };

    let exclude: HashSet<String> = input.exclude_item_ids.iter().cloned().collect();
    // Story 13.5 #20: derive the encoding-from-goals target bitrate once; `None` ⇒ today's estimate.
    let target_kbps = target_bitrate_kbps(&pipeline.budget);
    let mut selector = Selector::new(
        ceiling,
        pipeline.budget.target_duration_secs,
        target_kbps,
        exclude,
        pipeline.promotion.coherence,
    );

    // Story 13.6 #9: affinity promotion only refines the *base-unit* (Track) grouping; it's a no-op when
    // the base unit is already atomic (Album/Artist — every album is its own unit there). Resolve once and
    // thread into every base-unit pass (stable-core / pity / primary / fallback). The dedicated reserve
    // pre-passes (spotlight Track-depth, album-ratio Album) pass `None` — they force their own grouping.
    let promote_min_favorites = if pipeline.unit == Unit::Track {
        pipeline
            .promotion
            .promote_album_min_favorites
            .filter(|n| *n > 0)
    } else {
        None
    };

    // Stable-core (#24, AC 6): when `stable_core_pct = p > 0` and the budget is bounded, fill up to
    // `round(ceiling × p)` bytes FIRST from candidates already on the device (have a `last_synced_at`
    // row), exempt from cooldown — the *stable core*. The remaining budget then fills as the *delta*
    // from all candidates honoring full memory rules. Same Filter/Ordering/Unit/dedup as the delta;
    // dedup against the core is automatic via the shared selector. `p = 0`/unbounded ceiling = no-op.
    let core_pct = pipeline
        .memory
        .stable_core_pct
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    if core_pct > 0.0 && ceiling != u64::MAX {
        let core_cap = ((ceiling as f64) * f64::from(core_pct)).round() as u64;
        if core_cap > 0 {
            selector.ceiling = core_cap;
            // Split the core budget across sources by their share so one source can't monopolize the
            // whole core allocation (otherwise every source got the full `core_cap` cap).
            let core_caps = source_caps(sources, core_cap);
            for (source, cap) in sources.iter().zip(core_caps) {
                let units = build_source_units(
                    input,
                    pipeline,
                    source,
                    effective_filter,
                    pipeline.unit,
                    promote_min_favorites,
                );
                selector.fill(
                    units,
                    source,
                    cap,
                    &pipeline.memory,
                    &input.history,
                    FillMode::Core,
                );
            }
            selector.ceiling = ceiling; // restore the full ceiling for the delta pass
        }
    }

    // Story 13.6 #33 (Artist Spotlight): a reserve pre-pass that fills ONE deterministically-chosen
    // featured artist in depth FIRST, mirroring the stable-core/pity pre-passes exactly (temporary
    // ceiling, restricted candidate set, shared `Selector` ⇒ automatic dedup into the later passes).
    // The featured artist is the one owning the single best-ranked candidate under the configured
    // ordering (so it composes with the user's ordering and, when `Random`/`Rarity` is in the ordering,
    // varies per `seed` — the #33 delight, with zero new entropy). An under-filled reserve spills to the
    // primary pass for free. `spotlight:false`, `spotlight_share == 0`, an unbounded ceiling, or no
    // artist-bearing candidate ⇒ no-op. Track-level depth (no affinity promotion in the reserve).
    if pipeline.promotion.spotlight && ceiling != u64::MAX {
        let share = pipeline
            .promotion
            .spotlight_share
            .unwrap_or(0.5)
            .clamp(0.0, 1.0);
        if share > 0.0
            && let Some(featured) =
                choose_featured_artist(input, pipeline, sources, effective_filter)
        {
            let reserve = ((ceiling as f64) * f64::from(share)).round() as u64;
            if reserve > 0 {
                selector.ceiling = selector.cum_bytes.saturating_add(reserve).min(ceiling);
                let spot_caps = source_caps(sources, reserve);
                for (source, cap) in sources.iter().zip(spot_caps) {
                    let mut units = build_source_units(
                        input,
                        pipeline,
                        source,
                        effective_filter,
                        Unit::Track,
                        None,
                    );
                    // Track units are singletons — keep only the featured artist's candidates.
                    units.retain(|u| {
                        u.iter()
                            .any(|c| c.song.artist_id.as_deref() == Some(featured.as_str()))
                    });
                    selector.fill(
                        units,
                        source,
                        cap,
                        &pipeline.memory,
                        &input.history,
                        FillMode::Primary,
                    );
                }
                selector.ceiling = ceiling; // restore the full ceiling for the later passes
            }
        }
    }

    // Story 13.6 #8 (album/track space ratio): a reserve pre-pass that fills COMPLETE albums (atomic,
    // `Unit::Album` grouping — a whole album fits or the source stops, exactly as the `Selector` already
    // enforces) FIRST, then the remaining budget fills with the base `unit` as today. Mirrors the
    // stable-core pre-pass (temporary ceiling, `source_caps` split, shared `Selector` ⇒ automatic dedup).
    // `album_track_ratio` is `None`/`0` or the ceiling is unbounded ⇒ no-op (base unit governs everything).
    let album_ratio = pipeline
        .promotion
        .album_track_ratio
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    if album_ratio > 0.0 && ceiling != u64::MAX {
        let reserve = ((ceiling as f64) * f64::from(album_ratio)).round() as u64;
        if reserve > 0 {
            selector.ceiling = selector.cum_bytes.saturating_add(reserve).min(ceiling);
            let album_caps = source_caps(sources, reserve);
            for (source, cap) in sources.iter().zip(album_caps) {
                let units = build_source_units(
                    input,
                    pipeline,
                    source,
                    effective_filter,
                    Unit::Album,
                    None,
                );
                selector.fill(
                    units,
                    source,
                    cap,
                    &pipeline.memory,
                    &input.history,
                    FillMode::Primary,
                );
            }
            selector.ceiling = ceiling; // restore the full ceiling for the base-unit primary pass
        }
    }

    // Pity discovery reserve (#30, AC 7): when the dry streak has reached the threshold and the
    // budget is bounded, reserve `round(ceiling × guaranteed_ratio)` bytes and fill them FIRST from
    // discovery-class candidates only (`play_count <= discovery_max_plays && !is_on_device`) so the
    // guarantee surfaces genuinely *new* gems, not residents. Mirrors the stable-core pre-pass
    // exactly (temporary ceiling, restricted candidate set, shared `Selector` ⇒ automatic dedup into
    // the primary pass). Order: stable-core (keep) → pity reserve (force-new) → primary → fallback.
    // The reserve adds on top of whatever the core already spent, capped by the full ceiling.
    // `guaranteed_ratio = 0`, an unbounded ceiling, or `pity_streak < threshold` ⇒ no-op.
    let pity = &pipeline.pity;
    let reserve_bytes = pity_reserve_bytes(pity, input.pity_streak, ceiling);
    if reserve_bytes > 0 {
        selector.ceiling = selector
            .cum_bytes
            .saturating_add(reserve_bytes)
            .min(ceiling);
        let reserve_caps = source_caps(sources, reserve_bytes);
        for (source, cap) in sources.iter().zip(reserve_caps) {
            let units = build_source_units(
                input,
                pipeline,
                source,
                effective_filter,
                pipeline.unit,
                promote_min_favorites,
            );
            selector.fill(
                units,
                source,
                cap,
                &pipeline.memory,
                &input.history,
                FillMode::Discovery {
                    max_plays: pity.discovery_max_plays,
                },
            );
        }
        selector.ceiling = ceiling; // restore the full ceiling for the primary pass
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
        let units = build_source_units(
            input,
            pipeline,
            source,
            effective_filter,
            pipeline.unit,
            promote_min_favorites,
        );
        selector.fill(
            units,
            source,
            cap,
            &pipeline.memory,
            &input.history,
            FillMode::Primary,
        );
    }

    // Terminal fallback chain — only reached once primary sources can't fill the budget. Context
    // suppression applies here too (a fallback source named only in inactive rules is dropped); the
    // share/weight is irrelevant since fallback fills against the full ceiling.
    for source in fallback_sources {
        let units = build_source_units(
            input,
            pipeline,
            source,
            effective_filter,
            pipeline.unit,
            promote_min_favorites,
        );
        selector.fill(
            units,
            source,
            ceiling,
            &pipeline.memory,
            &input.history,
            FillMode::Fallback,
        );
    }

    selector.into_items()
}

/// Which fill pass is running. `Core` (stable-core, AC 6) restricts to on-device candidates and
/// exempts them from cooldown; `Primary` and `Fallback` apply the full Memory rules and differ only
/// in how the source reason is tagged (`Fallback` items are prefixed `fallback:`). `Discovery`
/// (pity reserve, Story 13.4 AC 7) restricts to discovery-class candidates that are **not** on the
/// device (`play_count <= max_plays && !is_on_device`) so the guarantee surfaces genuinely new gems.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FillMode {
    Core,
    Primary,
    Fallback,
    Discovery { max_plays: u32 },
}

/// A selection unit: one or more candidates that are added to the budget atomically (a single
/// track for [`Unit::Track`]; a whole album/artist otherwise).
type UnitGroup = Vec<Candidate>;

/// Story 13.6 #33: choose the featured artist for the Spotlight reserve — purely, from the candidate
/// set, never reading a clock/RNG. Across the primary sources' filtered candidates, pick the single
/// `artist_id` owning the **best-ranked candidate** under the configured ordering
/// ([`compare_by_ordering`]), so the choice composes with the user's ordering and rides the existing
/// `seed` when `Random`/`Rarity` is in the ordering (a different spotlight each sync — the #33 delight,
/// with zero new entropy). Ties broken by `artist_id` string for determinism; candidates with no
/// `artist_id` are never featured. "In depth" is bounded by what the configured sources materialized —
/// the engine does **not** fetch the artist's full discography (deferred, see Dev Notes).
fn choose_featured_artist(
    input: &PipelineInput,
    pipeline: &AutoFillPipeline,
    sources: &[SourceEntry],
    filter: &FilterStage,
) -> Option<String> {
    let version_pref = &pipeline.quality.version_preference;
    let rarity = &pipeline.rarity;
    let seed = input.seed;
    let mut best: Option<Song> = None;
    for source in sources {
        // Track units are sorted by the pipeline ordering, so the first artist-bearing candidate (in
        // rank order) is this source's best featured-eligible candidate.
        let units = build_source_units(input, pipeline, source, filter, Unit::Track, None);
        let cand = units.iter().flatten().find(|c| {
            c.song
                .artist_id
                .as_deref()
                .is_some_and(|a| !a.trim().is_empty())
        });
        if let Some(c) = cand {
            let take = match &best {
                None => true,
                Some(b) => match compare_by_ordering(
                    &c.song,
                    b,
                    &pipeline.ordering,
                    version_pref,
                    seed,
                    rarity,
                ) {
                    std::cmp::Ordering::Less => true,
                    std::cmp::Ordering::Greater => false,
                    // Equal rank ⇒ deterministic tie-break by the smaller artist_id string.
                    std::cmp::Ordering::Equal => c.song.artist_id < b.artist_id,
                },
            };
            if take {
                best = Some(c.song.clone());
            }
        }
    }
    best.and_then(|s| s.artist_id)
}

/// filter → unit → ordering for a single source's materialized pool.
/// Memory/dedup is intentionally applied later by the selector so the full pipeline order stays
/// `filter → source-blend → unit → ordering → dedupe-vs-memory → budget`.
fn build_source_units(
    input: &PipelineInput,
    pipeline: &AutoFillPipeline,
    source: &SourceEntry,
    filter: &FilterStage,
    unit: Unit,
    promote_min_favorites: Option<u32>,
) -> Vec<UnitGroup> {
    let pool = input.pools.get(&source.key()).cloned().unwrap_or_default();

    // filter (genres/tags) — the *effective* filter (static ∪ active context rules, Story 13.5 AC 6).
    let filtered = filter_stage(pool, filter);
    // unit grouping — `unit` is an explicit override (Story 13.6: the album-ratio pre-pass forces
    // `Unit::Album`, the spotlight pre-pass forces `Unit::Track`; the normal passes pass `pipeline.unit`).
    // Story 13.6 #9: when the override is the base `Track` grouping and affinity promotion is armed,
    // high-favorite albums become atomic units (the rest stay track singletons).
    let mut units = match promote_min_favorites {
        Some(n) if unit == Unit::Track && n > 0 => unit_stage_promoted(filtered, n),
        _ => unit_stage(filtered, unit),
    };
    // ordering — sort within each unit (so its first track is its best), then sort units by their
    // best track. For Unit::Track this reduces to a single stable global sort.
    let version_pref = &pipeline.quality.version_preference;
    let seed = input.seed;
    let rarity = &pipeline.rarity;
    for unit in units.iter_mut() {
        unit.sort_by(|a, b| {
            compare_by_ordering(
                &a.song,
                &b.song,
                &pipeline.ordering,
                version_pref,
                seed,
                rarity,
            )
        });
    }
    units.sort_by(|a, b| match (a.first(), b.first()) {
        (Some(x), Some(y)) => compare_by_ordering(
            &x.song,
            &y.song,
            &pipeline.ordering,
            version_pref,
            seed,
            rarity,
        ),
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
fn memory_allows(
    song: &Song,
    mem: &MemoryStage,
    hist: &HistorySnapshot,
    skip_cooldown: bool,
) -> bool {
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

/// Story 13.6 #9 (affinity-triggered album promotion): Track-level grouping where an album whose
/// materialized candidate set contains `>= min_favorites` favorited tracks (`is_favorite == Some(true)`)
/// is promoted to a single atomic album unit (synced whole-or-not, like [`Unit::Album`]); every other
/// candidate stays a track singleton. First-seen order is preserved by reusing [`group_by`]: a candidate's
/// group key is its `album_id` **only** when that album cleared the threshold, else `None` (⇒ singleton).
/// The only signal read is `Song.is_favorite` (no rating field exists on `Song` — ratings deferred). The
/// affinity is **per-pool / per-run**: counted over the favorited tracks present in the candidate set, not
/// the album's full track list (the engine doesn't have it). Caller gates this on base `unit == Track`.
fn unit_stage_promoted(cands: Vec<Candidate>, min_favorites: u32) -> Vec<UnitGroup> {
    // Count favorited candidates per album_id (blank/whitespace album ids never promote).
    let mut fav_counts: HashMap<String, u32> = HashMap::new();
    for c in &cands {
        if c.song.is_favorite == Some(true)
            && let Some(album) = c.song.album_id.clone().filter(|a| !a.trim().is_empty())
        {
            *fav_counts.entry(album).or_insert(0) += 1;
        }
    }
    group_by(cands, |c| {
        c.song
            .album_id
            .clone()
            .filter(|a| !a.trim().is_empty())
            .filter(|a| fav_counts.get(a).copied().unwrap_or(0) >= min_favorites)
    })
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
    seed: u64,
    rarity: &RarityStage,
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
            // Story 13.3 #14: fewer plays first — owned-but-barely-played (inverse of PlayCount).
            // `None` is treated as 0, so a never-played track is the deepest cut and ranks first.
            OrderingKey::Excavation => a.play_count.unwrap_or(0).cmp(&b.play_count.unwrap_or(0)),
            // Story 13.3 #31 (cheap musical-memories): oldest `date_added` first (inverse of
            // DateCreated). A missing OR blank add-date sorts LAST — an unknown add-date is the
            // *worst* rediscovery candidate, NOT the best. A naive `unwrap_or("")` ascending would
            // wrongly float empty strings to the front, so the absent-last branch is explicit
            // (AC 3 guard; `nonblank_date` also folds whitespace-only `Some("")` into "absent").
            OrderingKey::Rediscovery => match (nonblank_date(a), nonblank_date(b)) {
                (Some(x), Some(y)) => x.cmp(y),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            },
            // Story 13.4 #29: seeded loot-table draw — each song gets an Efraimidis–Spirakis key
            // `u^(1/w)` from `(seed, id, rarity-class weight)`; higher key sorts first (descending),
            // so a higher-weighted class tends to draw earlier. When `rarity.enabled` is false this
            // degrades to a uniform weight-1 shuffle (never a panic). Float keys compared via
            // `total_cmp` (never `partial_cmp().unwrap()`).
            OrderingKey::Rarity => {
                let (wa, wb) = if rarity.enabled {
                    (
                        rarity_class_weight(a, rarity),
                        rarity_class_weight(b, rarity),
                    )
                } else {
                    (1.0, 1.0)
                };
                // Story 13.4 review: break a draw-key tie on `song.id` so the order is canonical even
                // when keys collide (all-zero rarity weights sink every key to 0.0, or a rare
                // `draw_unit01` collision) — never the pool's materialization order. Distinct ids with
                // non-zero weights never tie, so seeded fixtures are unaffected. Scoped to the
                // randomized keys only; other keys keep their fall-through-to-next-key behavior.
                es_draw_key(seed, &b.id, wb)
                    .total_cmp(&es_draw_key(seed, &a.id, wa))
                    .then_with(|| a.id.cmp(&b.id))
            }
            // Story 13.4: seeded uniform shuffle — every song draws with weight 1.0 (the special case
            // of the rarity draw), higher key first. A deterministic permutation given the seed.
            OrderingKey::Random => es_draw_key(seed, &b.id, 1.0)
                .total_cmp(&es_draw_key(seed, &a.id, 1.0))
                .then_with(|| a.id.cmp(&b.id)),
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

/// Story 13.4: a deterministic per-song uniform draw in `[0,1)` derived from `(seed, song id)`.
///
/// Uses an **explicit** mix — a stable FNV-1a hash of the id folded into a splitmix64 finalizer with
/// the seed — so the comparison value is reproducible and unit-testable. It deliberately does **not**
/// rely on `DefaultHasher`'s unspecified internals for the value, and reads no global entropy: all
/// randomness comes from the caller-supplied `seed`. The top 53 bits are mapped to a uniform double.
fn draw_unit01(seed: u64, id: &str) -> f64 {
    // Stable FNV-1a 64-bit hash of the id (explicit constants — not DefaultHasher).
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in id.as_bytes() {
        h ^= u64::from(*byte);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    // splitmix64 finalizer mixing the seed with the id hash.
    let mut x = seed ^ h;
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^= x >> 31;
    // Top 53 bits → [0,1): the standard uniform-double construction.
    ((x >> 11) as f64) / ((1u64 << 53) as f64)
}

/// Story 13.4 #29: the Efraimidis–Spirakis weighted-draw key `key = u^(1/w)`, where `u =
/// draw_unit01(seed, id)` and `w` is the song's rarity-class weight. Higher key sorts first
/// (descending), so a higher weight tends to draw earlier. `w <= 0` is handled **explicitly** as
/// key `0.0` (the class sinks to the bottom) — never `1.0/0.0` (`inf`) or a NaN.
fn es_draw_key(seed: u64, id: &str, weight: f32) -> f64 {
    if weight <= 0.0 {
        return 0.0;
    }
    draw_unit01(seed, id).powf(1.0 / f64::from(weight))
}

/// Story 13.4 #29: the draw weight for a song's rarity class. Class boundaries from `play_count`
/// (the only universal signal — no rating field): `None`/`0` → legendary, `1..=rare_max_plays` →
/// rare, else → common.
fn rarity_class_weight(song: &Song, r: &RarityStage) -> f32 {
    let plays = song.play_count.unwrap_or(0);
    if plays == 0 {
        r.legendary_weight
    } else if plays <= r.rare_max_plays {
        r.rare_weight
    } else {
        r.common_weight
    }
}

/// The song's `date_added` as a present, non-blank ISO string — or `None` if absent or
/// whitespace-only. Used by [`OrderingKey::Rediscovery`] so an unknown add-date sorts LAST rather
/// than masquerading as the oldest (Story 13.3 #31, AC 3).
fn nonblank_date(s: &Song) -> Option<&str> {
    s.date_added
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
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
        return if LOSSLESS.contains(&suffix.as_str()) {
            2
        } else {
            1
        };
    }

    // Fall back to the mime subtype (`audio/flac` → `flac`, `audio/x-flac` → `flac`).
    let mime_sub = song
        .content_type
        .as_deref()
        .and_then(|c| c.rsplit('/').next())
        .map(|s| s.trim().trim_start_matches("x-").to_ascii_lowercase())
        .unwrap_or_default();
    if !mime_sub.is_empty() {
        return if LOSSLESS.contains(&mime_sub.as_str()) {
            2
        } else {
            1
        };
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

/// Whether a (lowercased) text segment carries any recognized version marker. Drives the
/// merge-key path ([`strip_bracketed_markers`]/[`strip_version_markers`]), so **every** marker is
/// word-anchored here (Story 13.2 review — Decision 1): stripping must be conservative — `acoustic`
/// inside an unrelated word (or `remix`/`re-mix` embedded in `pre-mixed`, `remixology`, …) must
/// **not** strip a parenthetical and over-merge two distinct songs. `detect_version_traits` keeps
/// its own (looser) substring matching for tagging, where a false positive is harmless.
fn segment_has_marker(low: &str) -> bool {
    has_word(low, "live")
        || has_word(low, "unplugged")
        || has_word(low, "remaster")
        || has_word(low, "remastered")
        || has_word(low, "remix")
        || has_word(low, "rmx")
        || has_word(low, "re-mix")
        || has_word(low, "acoustic")
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
    // Strip *stacked* trailing ` - <marker>` suffixes, not just the last one, so a multi-suffix
    // title (`Song - Live - 2011 Remaster`) collapses to the same base as a clean `Song` (Story
    // 13.2 review — Patch). Stops at the first non-marker tail, leaving distinct songs distinct.
    while let Some(idx) = s.rfind(" - ")
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

/// Whether a candidate could fit under the global byte ceiling at all — its estimated size is known
/// and ≤ `ceiling`. An unbounded ceiling (`u64::MAX`) fits everything (preserving pre-13.2 behavior
/// when there is no byte budget); an unknown/zero size never fits a bounded budget (the selector
/// skips zero/unknown-size tracks anyway). Lets [`best_version_cmp`] avoid electing a winner that
/// can never be selected.
fn fits_ceiling(song: &Song, ceiling: u64, target_kbps: Option<u32>) -> bool {
    ceiling == u64::MAX || estimated_size(song, target_kbps).is_some_and(|sz| sz <= ceiling)
}

/// Deterministic best-version comparator (Story 13.2 #11, AC 6): budget fit → version-preference
/// rank → quality rank → the full `ordering` → `song.id` lexicographic (the ultimate tiebreak that
/// makes the winner independent of pool iteration order). The budget-fit tier (Story 13.2 review —
/// Decision 2) prefers a version that can fit the global byte ceiling over one that never can, so
/// best-version degrades to a smaller copy instead of dropping the song; it is a no-op for an
/// unbounded ceiling. Returns `Less` when `a` is the better version.
///
/// Story 13.4: `seed` is threaded so the `ordering` tiebreak (3) can include a randomized key
/// (`Random`/`Rarity`). When the pipeline lists such a key, the best-*version* choice becomes
/// seed-dependent — still fully deterministic *given the seed*, but called out here so the behavior
/// is intentional, not surprising. A pipeline with no randomized key is unaffected (the `song.id`
/// tiebreak (4) still makes the winner stable).
fn best_version_cmp(
    a: &Song,
    b: &Song,
    pipeline: &AutoFillPipeline,
    ceiling: u64,
    seed: u64,
) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    // Story 13.5: budget-fit uses the same bitrate-aware estimate as selection, so a version that
    // fits *after* transcode is preferred consistently with how it will actually be packed.
    let target_kbps = target_bitrate_kbps(&pipeline.budget);
    // (0) budget fit: a version that can fit the ceiling beats one that never can (Decision 2).
    match (
        fits_ceiling(a, ceiling, target_kbps),
        fits_ceiling(b, ceiling, target_kbps),
    ) {
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        _ => {}
    }
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
    let ord = compare_by_ordering(a, b, &pipeline.ordering, &[], seed, &pipeline.rarity);
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
fn collapse_best_version(
    input: &PipelineInput,
    pipeline: &AutoFillPipeline,
    ceiling: u64,
) -> PipelineInput {
    // Pick the winning Song per logical key. Iteration order over pools/candidates is irrelevant:
    // `best_version_cmp` is a total order (ties broken by id), so the minimum is deterministic.
    let seed = input.seed;
    let mut winners: HashMap<(String, String), Song> = HashMap::new();
    for pool in input.pools.values() {
        for cand in pool {
            let Some(key) = logical_key(&cand.song) else {
                continue;
            };
            match winners.get(&key) {
                Some(current)
                    if best_version_cmp(&cand.song, current, pipeline, ceiling, seed)
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
///
/// Story 13.5 #20 (encoding-from-goals): when `target_kbps` is `Some(b)` the size becomes
/// **bitrate-aware** — the post-transcode size is `min(source_estimate, b×1000/8 × duration)`.
/// Transcoding only ever *shrinks*: a source already below the target bitrate is unchanged, and a
/// zero/unknown target size (e.g. unknown duration) is ignored so we never produce a 0-byte item.
/// `None` ⇒ byte-for-byte the source estimate (today's behavior).
fn estimated_size(song: &Song, target_kbps: Option<u32>) -> Option<u64> {
    let base = if let Some(sz) = song.size_bytes {
        if sz > 0 { sz } else { return None }
    } else {
        let kbps = song.bitrate_kbps?;
        u64::from(kbps)
            .checked_mul(1_000)?
            .checked_div(8)?
            .checked_mul(u64::from(song.duration_seconds))?
    };
    let est = match target_kbps {
        Some(b) if b > 0 => {
            let transcoded = u64::from(b)
                .checked_mul(1_000)
                .and_then(|x| x.checked_div(8))
                .and_then(|x| x.checked_mul(u64::from(song.duration_seconds)));
            match transcoded {
                Some(t) if t > 0 => base.min(t), // transcoding down only shrinks
                _ => base,
            }
        }
        _ => base,
    };
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

/// Story 13.5 #20 (encoding-from-goals): derive the target transcode bitrate (kbps) backwards from
/// the size + duration goals. Returns `Some(clamped)` only when `encoding_from_goals` is set AND both
/// a byte ceiling (`max_bytes`) and a positive `target_duration_secs` are present:
/// `effective_bytes × 8 / (target_duration_secs × 1000)` where `effective_bytes = budget_ceiling`
/// (i.e. `max_bytes − headroom`), clamped to a sane audio range so a tiny/huge goal can't produce a
/// nonsense encode. Returns `None` (no derivation — today's behavior) when the flag is off or either
/// goal is missing. Pure, unit-testable, no I/O.
pub fn target_bitrate_kbps(budget: &BudgetStage) -> Option<u32> {
    /// Clamp bounds for a derived audio bitrate (kbps).
    const MIN_KBPS: u64 = 32;
    const MAX_KBPS: u64 = 320;
    if !budget.encoding_from_goals {
        return None;
    }
    let secs = budget.target_duration_secs.filter(|s| *s > 0)?;
    budget.max_bytes?; // a byte ceiling must be configured for the derivation to be meaningful
    let effective_bytes = budget_ceiling(budget);
    if effective_bytes == 0 || effective_bytes == u64::MAX {
        return None;
    }
    let raw = effective_bytes
        .checked_mul(8)
        .and_then(|x| x.checked_div(secs.checked_mul(1_000)?))?;
    Some(raw.clamp(MIN_KBPS, MAX_KBPS) as u32)
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

/// Story 13.6 #27: the per-item sort key for coherence clustering. Captured from `Song` at selection
/// time because `AutoFillItem` carries album/artist names but not `disc_number`/`track_number`.
#[derive(Clone)]
struct CoherenceKey {
    artist_id: String,
    album_id: String,
    disc: u32,
    track: u32,
    id: String,
}

/// Build a [`CoherenceKey`] from a song. Missing artist/album ids fold to `""` (their own cluster);
/// missing disc/track to `0` (sorted first within an album).
fn coherence_key(song: &Song) -> CoherenceKey {
    CoherenceKey {
        artist_id: song.artist_id.clone().unwrap_or_default(),
        album_id: song.album_id.clone().unwrap_or_default(),
        disc: song.disc_number.unwrap_or(0),
        track: song.track_number.unwrap_or(0),
        id: song.id.clone(),
    }
}

/// Story 13.6 #27 (coherence ordering): reorder a selected set into coherent clusters — artist
/// (first-appearance order) → album (first-appearance order) → disc → track → id. **Reorder-only**:
/// the result is a permutation of `items`, so the selected id-set and byte total are byte-identical to
/// the input (the whole safety guarantee). `keys[i]` describes `items[i]`. First-appearance ranks keep
/// the output deterministic and independent of any id sort. Pure, signal-free.
fn coherence_reorder(items: Vec<AutoFillItem>, keys: Vec<CoherenceKey>) -> Vec<AutoFillItem> {
    debug_assert_eq!(items.len(), keys.len());
    let mut artist_rank: HashMap<String, usize> = HashMap::new();
    let mut album_rank: HashMap<String, usize> = HashMap::new();
    for k in &keys {
        let n = artist_rank.len();
        artist_rank.entry(k.artist_id.clone()).or_insert(n);
        let m = album_rank.len();
        album_rank.entry(k.album_id.clone()).or_insert(m);
    }
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by(|&i, &j| {
        let (a, b) = (&keys[i], &keys[j]);
        artist_rank[&a.artist_id]
            .cmp(&artist_rank[&b.artist_id])
            .then(album_rank[&a.album_id].cmp(&album_rank[&b.album_id]))
            .then(a.disc.cmp(&b.disc))
            .then(a.track.cmp(&b.track))
            .then_with(|| a.id.cmp(&b.id))
    });
    // Apply the permutation without cloning items.
    let mut slots: Vec<Option<AutoFillItem>> = items.into_iter().map(Some).collect();
    order
        .into_iter()
        .map(|i| slots[i].take().expect("each index visited once"))
        .collect()
}

/// Accumulates the selection across sources: enforces the global ceiling, per-source caps, the
/// optional duration target, manual-exclude ids, and within-run dedup. Mirrors the
/// stop-on-first-oversized semantics of the legacy `ProviderFillState`/`rank_and_truncate`.
struct Selector {
    ceiling: u64,
    duration_target: Option<u64>,
    /// Story 13.5 #20: when encoding-from-goals derived a target bitrate, every size estimate is
    /// bitrate-aware so the byte math packs to the duration goal within the byte ceiling. `None` ⇒
    /// today's source-based estimate.
    target_kbps: Option<u32>,
    exclude: HashSet<String>,
    seen: HashSet<String>,
    items: Vec<AutoFillItem>,
    cum_bytes: u64,
    cum_secs: u64,
    /// Story 13.6 #27: when set, [`Selector::into_items`] reorders the selection into coherent
    /// artist→album→disc→track clusters. Reorder-only — the id-set and byte total are unchanged.
    coherence: bool,
    /// Story 13.6 #27: per-selected-item sort keys, pushed in lockstep with `items` (only when
    /// `coherence`). `AutoFillItem` carries album/artist but not disc/track, so the engine captures the
    /// `Song`-level sort fields here while it still holds the candidate.
    coherence_keys: Vec<CoherenceKey>,
}

impl Selector {
    fn new(
        ceiling: u64,
        duration_target: Option<u64>,
        target_kbps: Option<u32>,
        exclude: HashSet<String>,
        coherence: bool,
    ) -> Self {
        Self {
            ceiling,
            duration_target,
            target_kbps,
            exclude,
            seen: HashSet::new(),
            items: Vec::new(),
            cum_bytes: 0,
            cum_secs: 0,
            coherence,
            coherence_keys: Vec::new(),
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
        // Discovery (pity) reserve: only never-/barely-played tracks not already on the device.
        let discovery_max = match mode {
            FillMode::Discovery { max_plays } => Some(max_plays),
            _ => None,
        };
        let mut source_bytes: u64 = 0;
        for unit in units {
            if let Some(target) = self.duration_target
                && self.cum_secs >= target
            {
                break;
            }
            // Stage the syncable, non-excluded, not-yet-seen tracks of this unit. The optional third
            // element is the Story 13.6 #27 coherence sort key, captured here while the `Song` is in hand.
            let mut staged: Vec<(AutoFillItem, u64, Option<CoherenceKey>)> = Vec::new();
            let mut local_seen: HashSet<String> = HashSet::new();
            let mut unit_bytes: u64 = 0;
            let mut unit_secs: u64 = 0;
            for cand in &unit {
                let song = &cand.song;
                // The discovery (pity) reserve only draws genuinely new gems: not on the device and
                // at/under the discovery play-count cap. (Full Memory rules still apply below.)
                if let Some(max_plays) = discovery_max
                    && (is_on_device(song, history) || song.play_count.unwrap_or(0) > max_plays)
                {
                    continue;
                }
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
                let Some(size) = estimated_size(song, self.target_kbps) else {
                    continue; // unknown/zero size — never a 0-byte filler
                };
                unit_bytes = unit_bytes.saturating_add(size);
                unit_secs = unit_secs.saturating_add(u64::from(song.duration_seconds));
                let reason = reason_for(song, source, is_fallback);
                let key = self.coherence.then(|| coherence_key(song));
                staged.push((make_item(song, size, reason), size, key));
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
            for (item, size, key) in staged {
                self.seen.insert(item.id.clone());
                self.cum_bytes = self.cum_bytes.saturating_add(size);
                source_bytes = source_bytes.saturating_add(size);
                self.items.push(item);
                if let Some(k) = key {
                    self.coherence_keys.push(k);
                }
            }
            self.cum_secs = self.cum_secs.saturating_add(unit_secs);
        }
    }

    fn into_items(self) -> Vec<AutoFillItem> {
        if self.coherence {
            coherence_reorder(self.items, self.coherence_keys)
        } else {
            self.items
        }
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

        // Story 13.6 — Antoine also cares about ALBUM INTEGRITY: he wants each album's tracks to play
        // contiguously and in track order, not scattered by the ranking. Coherence (#27) delivers that
        // PURELY in config (no `if persona` branch): with it off the selection comes out in quality
        // order; with it on the same tracks cluster artist→album→disc→track. Two albums, deliberately
        // interleaved by quality so the un-clustered order splits them apart.
        let mut flac_a2 = song_album(
            "acclaim-a2",
            "AntoineFav",
            "Acclaimed",
            1,
            2,
            false,
            0,
            30_000_000,
        );
        flac_a2.suffix = Some("flac".to_string());
        flac_a2.content_type = Some("audio/flac".to_string());
        flac_a2.bitrate_kbps = Some(1000);
        let album_pool = vec![
            // mp3 from album "Live" (track 1), then two FLACs from "Acclaimed" (tracks 2 then 1).
            cand(song_album(
                "live-1",
                "AntoineFav",
                "Live",
                1,
                1,
                false,
                0,
                10_000_000,
            )),
            cand(flac_a2),
            cand({
                let mut s = song_album(
                    "acclaim-a1",
                    "AntoineFav",
                    "Acclaimed",
                    1,
                    1,
                    false,
                    0,
                    30_000_000,
                );
                s.suffix = Some("flac".to_string());
                s.content_type = Some("audio/flac".to_string());
                s.bitrate_kbps = Some(1000);
                s
            }),
        ];
        let album_input = PipelineInput::default().with_pool(SourceKind::Library, None, album_pool);
        let integrity_off = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            ordering: vec![OrderingKey::Quality],
            budget: BudgetStage {
                max_bytes: Some(512u64 * 1_000 * 1_000 * 1_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let integrity_on = AutoFillPipeline {
            promotion: PromotionStage {
                coherence: true,
                ..Default::default()
            },
            ..integrity_off.clone()
        };
        // Quality order splits the "Acclaimed" album's tracks around nothing here but emits them in
        // quality order (FLACs first, then the lossy "Live"): acclaim-a2, acclaim-a1, live-1.
        assert_eq!(
            ids(&run_pipeline(&album_input, &integrity_off)),
            vec!["acclaim-a2", "acclaim-a1", "live-1"],
            "default: quality ordering governs (album tracks not in track order)"
        );
        // Coherence clusters by album (Acclaimed first-seen) and orders within it by track number.
        assert_eq!(
            ids(&run_pipeline(&album_input, &integrity_on)),
            vec!["acclaim-a1", "acclaim-a2", "live-1"],
            "coherence keeps each album contiguous and in track order — album integrity, from config"
        );
    }

    #[test]
    fn persona_leo_gym_energy_playlist_tiny_device() {
        // Léo: the explorer. Tiny device, energy-driven, tired of the same hits. A single Playlist
        // source ("energy") with an Excavation ordering (Story 13.3 #14) and a tiny budget. Only the
        // playlist pool's tracks are picked, the barely-played deep cuts surface ahead of the hit,
        // and the result is truncated to budget. The library pool is present but never referenced —
        // so it must not leak in. Excavation is expressed purely in config: no `if persona` branch.
        let energy = vec![
            // `hit` is heavily played and must be excavated to the BACK despite being listed first.
            cand(song_sized("e-hit", false, 90, "2024-01-01", 3_000_000)),
            cand(song_sized("e-deep1", false, 1, "2024-01-01", 3_000_000)),
            cand(song_sized("e-deep2", false, 0, "2024-01-01", 3_000_000)),
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
            ordering: vec![OrderingKey::Excavation],
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
            vec!["e-deep2", "e-deep1"],
            "excavation surfaces the barely-played deep cuts; the 90-play hit is dropped past budget"
        );
        assert!(
            !result_ids.iter().any(|id| id == "e-hit"),
            "the heavily-played hit must yield to the deep cuts under Excavation"
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

        // Story 13.4 — Léo's discovery guarantee, expressed purely in config (no `if persona`).
        // He now orders by PlayCount (hits first, which would bury the deep cut at this tiny budget)
        // but arms the pity timer; after a dry streak the reserve GUARANTEES a never-played gem that
        // the ordering alone would drop. With pity OFF the gem stays buried — behavior emerges only
        // from the config.
        let pity_pipeline = AutoFillPipeline {
            sources: vec![SourceEntry {
                kind: SourceKind::Playlist,
                ref_id: Some("energy".to_string()),
                share: None,
            }],
            ordering: vec![OrderingKey::PlayCount],
            pity: PityStage {
                enabled: true,
                threshold_syncs: 3,
                guaranteed_ratio: 0.5,
                discovery_max_plays: 0,
            },
            budget: BudgetStage {
                max_bytes: Some(7_000_000), // fits 2 of the 3 MB tracks
                ..Default::default()
            },
            ..Default::default()
        };
        let dry_input = PipelineInput {
            pity_streak: 3, // reached the threshold ⇒ the guarantee fires
            ..Default::default()
        }
        .with_pool(
            SourceKind::Playlist,
            Some("energy"),
            vec![
                cand(song_sized("e-hit", false, 90, "2024-01-01", 3_000_000)),
                cand(song_sized("e-deep1", false, 1, "2024-01-01", 3_000_000)),
                cand(song_sized("e-deep2", false, 0, "2024-01-01", 3_000_000)),
            ],
        );
        let dry_ids = ids(&run_pipeline(&dry_input, &pity_pipeline));
        assert!(
            dry_ids.contains(&"e-deep2".to_string()),
            "pity reserve guarantees the never-played gem even though PlayCount would bury it"
        );

        // Same config, streak below threshold ⇒ no reserve ⇒ PlayCount buries the never-played gem.
        let fresh_input = PipelineInput {
            pity_streak: 0,
            ..Default::default()
        }
        .with_pool(
            SourceKind::Playlist,
            Some("energy"),
            vec![
                cand(song_sized("e-hit", false, 90, "2024-01-01", 3_000_000)),
                cand(song_sized("e-deep1", false, 1, "2024-01-01", 3_000_000)),
                cand(song_sized("e-deep2", false, 0, "2024-01-01", 3_000_000)),
            ],
        );
        let fresh_ids = ids(&run_pipeline(&fresh_input, &pity_pipeline));
        assert!(
            !fresh_ids.contains(&"e-deep2".to_string()),
            "no dry streak ⇒ no guarantee ⇒ the never-played gem stays buried under PlayCount"
        );

        // Story 13.5 — Léo's gym energy follows the CLOCK (config-driven, no `if persona`). His "energy"
        // playlist is gated to morning gym hours (6–11). At 07:00 it drives the fill; at 20:00 it is
        // suppressed (the only configured source ⇒ empty fill). With context OFF the gate is inert.
        let energy_pool = vec![
            cand(song_sized("e-deep2", false, 0, "2024-01-01", 3_000_000)),
            cand(song_sized("e-deep1", false, 1, "2024-01-01", 3_000_000)),
        ];
        let context_pipeline = AutoFillPipeline {
            sources: vec![SourceEntry {
                kind: SourceKind::Playlist,
                ref_id: Some("energy".to_string()),
                share: None,
            }],
            ordering: vec![OrderingKey::Excavation],
            budget: BudgetStage {
                max_bytes: Some(7_000_000),
                ..Default::default()
            },
            context: ContextStage {
                enabled: true,
                rules: vec![ContextRule {
                    window: ContextWindow::TimeOfDay {
                        start_hour: 6,
                        end_hour: 11,
                    },
                    source_refs: vec!["energy".to_string()],
                    ..Default::default()
                }],
            },
            ..Default::default()
        };
        let make_input = |hour: u8| {
            let mut i = PipelineInput::default().with_pool(
                SourceKind::Playlist,
                Some("energy"),
                energy_pool.clone(),
            );
            i.history.local = CivilTime {
                hour,
                month: 1,
                day: 1,
                weekday: 0,
            };
            i
        };
        assert_eq!(
            ids(&run_pipeline(&make_input(7), &context_pipeline)),
            vec!["e-deep2", "e-deep1"],
            "07:00 ⇒ the morning-gated energy playlist drives the fill"
        );
        assert!(
            run_pipeline(&make_input(20), &context_pipeline).is_empty(),
            "20:00 ⇒ the energy playlist is suppressed; no un-gated source remains"
        );
        // Context OFF ⇒ the gate is inert: the energy playlist fills regardless of the hour.
        let context_off = AutoFillPipeline {
            context: ContextStage {
                enabled: false,
                ..context_pipeline.context.clone()
            },
            ..context_pipeline.clone()
        };
        assert_eq!(
            ids(&run_pipeline(&make_input(20), &context_off)),
            vec!["e-deep2", "e-deep1"],
            "context disabled ⇒ byte-identical to no-context (the 20:00 fill is unchanged)"
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
        assert!(
            memory_allows(&song, &mid, &hist, false),
            "t=0.5 boundary allowed"
        );

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
        assert!(
            !memory_allows(&song, &mid, &hist_recent, false),
            "t=0.5 inside window"
        );

        // repeat_tolerance only modulates cooldown — with no cooldown it is inert.
        let no_cooldown = MemoryStage {
            cooldown_weeks: None,
            repeat_tolerance: Some(0.5),
            ..Default::default()
        };
        assert!(
            memory_allows(&song, &no_cooldown, &hist, false),
            "tolerance is inert without cooldown"
        );
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
            library.push(cand(song_sized(
                &format!("fresh{i}"),
                false,
                0,
                "2024-01-01",
                1_000_000,
            )));
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
        assert!(
            core_bytes >= 4_000_000,
            "≈p of the budget is the on-device core"
        );
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
        assert_eq!(
            format_quality_rank(&unknown),
            0,
            "no format metadata → unknown"
        );
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
        let input =
            PipelineInput::default().with_pool(SourceKind::Library, None, vec![cand(lo), cand(hi)]);
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
        assert!(
            !t.contains(&VersionTrait::Studio),
            "studio is only the absence of markers"
        );
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
        assert_eq!(
            version_rank(&studio, &prefs),
            0,
            "studio is first preference"
        );
        assert_eq!(version_rank(&live, &prefs), 1, "live is second preference");
        assert_eq!(
            version_rank(&remix, &prefs),
            2,
            "no listed trait → last (len)"
        );
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
        assert_eq!(
            ids(&run_pipeline(&input, &pipeline)),
            vec!["live", "studio"]
        );
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
        let live = song_meta(
            "mp3-live",
            "My Song (Live)",
            "The Band",
            "Live Album",
            "mp3",
            320,
        );
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
        let live = song_meta(
            "mp3-live",
            "My Song (Live)",
            "The Band",
            "Live Album",
            "mp3",
            320,
        );
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
            assert!(
                got.contains(&id.to_string()),
                "{id} must survive (no over-merge)"
            );
        }
    }

    #[test]
    fn best_version_collapses_across_pools() {
        // Winner (FLAC studio) in the library, loser (lossy live) in a playlist → the playlist
        // loser is dropped; the library winner remains.
        let flac = song_meta("flac-studio", "My Song", "The Band", "Album", "flac", 900);
        let live = song_meta(
            "mp3-live",
            "My Song (Live)",
            "The Band",
            "Live Album",
            "mp3",
            320,
        );
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
        assert_eq!(
            got,
            vec!["flac-studio"],
            "cross-pool collapse keeps the global winner only"
        );
    }

    #[test]
    fn best_version_disabled_keeps_all_duplicates() {
        let flac = song_meta("flac-studio", "My Song", "The Band", "Album", "flac", 900);
        let live = song_meta(
            "mp3-live",
            "My Song (Live)",
            "The Band",
            "Live Album",
            "mp3",
            320,
        );
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
        assert_eq!(
            run_pipeline(&input, &pipeline).len(),
            2,
            "no collapse when disabled"
        );
    }

    #[test]
    fn best_version_never_emits_zero_byte_or_over_budget() {
        // Collapse only removes candidates; the surviving winner still respects the budget ceiling.
        let flac = song_meta("flac-studio", "My Song", "The Band", "Album", "flac", 900);
        let live = song_meta(
            "mp3-live",
            "My Song (Live)",
            "The Band",
            "Live Album",
            "mp3",
            320,
        );
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
        assert!(
            result.iter().all(|i| i.size_bytes > 0),
            "never a 0-byte filler"
        );
    }

    #[test]
    fn strip_version_markers_normalizes_to_a_shared_base() {
        assert_eq!(
            normalize_ws(&strip_version_markers("My Song (Live)")),
            "my song"
        );
        assert_eq!(
            normalize_ws(&strip_version_markers("My Song - 2011 Remaster")),
            "my song"
        );
        assert_eq!(
            normalize_ws(&strip_version_markers("My Song [Acoustic]")),
            "my song"
        );
        assert_eq!(
            normalize_ws(&strip_version_markers("My Song - Live at Wembley")),
            "my song"
        );
        // A non-version parenthetical is preserved (distinct songs stay distinct).
        assert_eq!(
            normalize_ws(&strip_version_markers("My Song (feat. Guest)")),
            "my song (feat. guest)",
        );
    }

    #[test]
    fn strip_version_markers_word_anchors_remix_and_acoustic() {
        // A marker substring embedded in a larger word must NOT strip the parenthetical, so two
        // genuinely distinct songs don't over-merge (Story 13.2 review — Decision 1).
        assert_eq!(
            normalize_ws(&strip_version_markers("My Song (Played Acoustically)")),
            "my song (played acoustically)",
        );
        assert_eq!(
            normalize_ws(&strip_version_markers("My Song (Premixed Tape)")),
            "my song (premixed tape)",
        );
        // A real standalone marker word still strips.
        assert_eq!(
            normalize_ws(&strip_version_markers("My Song (Remix)")),
            "my song"
        );
        assert_eq!(
            normalize_ws(&strip_version_markers("My Song (Acoustic)")),
            "my song"
        );
    }

    #[test]
    fn strip_version_markers_strips_stacked_dash_suffixes() {
        // Multiple trailing ` - <marker>` suffixes all strip, collapsing to the clean base (Story
        // 13.2 review — Patch).
        assert_eq!(
            normalize_ws(&strip_version_markers("My Song - Live - 2011 Remaster")),
            "my song",
        );
        // Stops at the first non-marker tail (distinct songs stay distinct).
        assert_eq!(
            normalize_ws(&strip_version_markers("My Song - Part Two - Live")),
            "my song - part two",
        );
    }

    #[test]
    fn best_version_falls_back_to_a_fitting_version_over_budget() {
        // The quality winner (a huge FLAC) can't fit the ceiling; best-version keeps the smaller
        // lossy cut so the song still lands rather than vanishing (Story 13.2 review — Decision 2).
        let mut flac = song_meta("flac-studio", "My Song", "The Band", "Album", "flac", 900);
        flac.size_bytes = Some(50_000_000); // 50 MB — exceeds the ceiling, can never be selected
        let mut live = song_meta(
            "mp3-live",
            "My Song (Live)",
            "The Band",
            "Live Album",
            "mp3",
            320,
        );
        live.size_bytes = Some(1_000_000); // 1 MB — fits
        let input = PipelineInput::default().with_pool(
            SourceKind::Library,
            None,
            vec![cand(flac), cand(live)],
        );
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            quality: QualityStage {
                best_version: true,
                ..Default::default()
            },
            budget: BudgetStage {
                max_bytes: Some(2_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            ids(&run_pipeline(&input, &pipeline)),
            vec!["mp3-live"],
            "a winner that can't fit the ceiling yields to a fitting lesser version",
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
        let back: AutoFillPipeline =
            serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
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

    // ===================================================================
    // Story 13.3 — Excavation (#14) & Rediscovery (#31) ordering keys.
    // Both are pure field comparisons over existing `Song` fields; no clock, no RNG, no new data.
    // ===================================================================

    /// Build a `Song` with an explicit `Option` play_count (the fixture helper only takes `u32`).
    fn song_plays(id: &str, play_count: Option<u32>) -> Song {
        Song {
            play_count,
            ..song_sized(id, false, 0, "2024-01-01", 1_000_000)
        }
    }

    /// Build a `Song` with an explicit `Option` date_added.
    fn song_dated(id: &str, date_added: Option<&str>) -> Song {
        Song {
            date_added: date_added.map(str::to_string),
            ..song_sized(id, false, 0, "2024-01-01", 1_000_000)
        }
    }

    fn lib_pipeline(ordering: Vec<OrderingKey>) -> AutoFillPipeline {
        AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            ordering,
            budget: BudgetStage {
                max_bytes: Some(100_000_000),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    // ---- AC 1/2: Excavation -------------------------------------------------

    #[test]
    fn excavation_ranks_fewer_played_first() {
        // never-played (None) and 0-play are the deepest cuts; a 50-play hit ranks last.
        let never = song_plays("never", None);
        let zero = song_plays("zero", Some(0));
        let some = song_plays("some", Some(5));
        let hit = song_plays("hit", Some(50));
        let input = PipelineInput::default().with_pool(
            SourceKind::Library,
            None,
            // Insert out of order; the sort must reorder by ascending play_count.
            vec![cand(hit), cand(some), cand(never), cand(zero)],
        );
        let result = ids(&run_pipeline(
            &input,
            &lib_pipeline(vec![OrderingKey::Excavation]),
        ));
        // never (None→0) and zero (0) tie at 0 → stable insertion order keeps `never` before `zero`.
        assert_eq!(result, vec!["never", "zero", "some", "hit"]);
    }

    #[test]
    fn excavation_is_the_exact_inverse_of_play_count() {
        let a = song_plays("a", Some(1));
        let b = song_plays("b", Some(10));
        let c = song_plays("c", Some(100));
        let pool = vec![cand(b.clone()), cand(c.clone()), cand(a.clone())];

        let by_excavation = ids(&run_pipeline(
            &PipelineInput::default().with_pool(SourceKind::Library, None, pool.clone()),
            &lib_pipeline(vec![OrderingKey::Excavation]),
        ));
        let mut by_play_count = ids(&run_pipeline(
            &PipelineInput::default().with_pool(SourceKind::Library, None, pool),
            &lib_pipeline(vec![OrderingKey::PlayCount]),
        ));
        by_play_count.reverse();
        assert_eq!(
            by_excavation, by_play_count,
            "excavation reverses PlayCount on distinct counts"
        );
        assert_eq!(by_excavation, vec!["a", "b", "c"]);
    }

    #[test]
    fn excavation_ties_are_stable() {
        // All same play_count → input order preserved (deterministic, no RNG).
        let pool = vec![
            cand(song_plays("p1", Some(3))),
            cand(song_plays("p2", Some(3))),
            cand(song_plays("p3", Some(3))),
        ];
        let input = PipelineInput::default().with_pool(SourceKind::Library, None, pool);
        assert_eq!(
            ids(&run_pipeline(
                &input,
                &lib_pipeline(vec![OrderingKey::Excavation])
            )),
            vec!["p1", "p2", "p3"],
        );
    }

    // ---- AC 3/4: Rediscovery ------------------------------------------------

    #[test]
    fn rediscovery_ranks_oldest_added_first() {
        let old = song_dated("old", Some("2018-01-01"));
        let mid = song_dated("mid", Some("2021-06-15"));
        let new = song_dated("new", Some("2024-12-31"));
        let input = PipelineInput::default().with_pool(
            SourceKind::Library,
            None,
            vec![cand(new), cand(old), cand(mid)],
        );
        assert_eq!(
            ids(&run_pipeline(
                &input,
                &lib_pipeline(vec![OrderingKey::Rediscovery])
            )),
            vec!["old", "mid", "new"],
        );
    }

    #[test]
    fn rediscovery_sorts_missing_or_blank_date_last() {
        // The AC 3 guard: an unknown/blank add-date is the WORST rediscovery candidate, not the best.
        let oldest = song_dated("oldest", Some("2015-01-01"));
        let none = song_dated("none", None);
        let blank = song_dated("blank", Some("   ")); // whitespace-only → folded into "absent"
        let newer = song_dated("newer", Some("2023-01-01"));
        let input = PipelineInput::default().with_pool(
            SourceKind::Library,
            None,
            vec![cand(none), cand(newer), cand(blank), cand(oldest)],
        );
        let result = ids(&run_pipeline(
            &input,
            &lib_pipeline(vec![OrderingKey::Rediscovery]),
        ));
        // Real dates first (oldest→newer); the two absent-date tracks sink to the bottom, stable.
        assert_eq!(result, vec!["oldest", "newer", "none", "blank"]);
        assert!(
            result.iter().position(|id| id == "oldest").unwrap()
                < result.iter().position(|id| id == "none").unwrap(),
            "a missing date must never jump ahead of a real one",
        );
    }

    #[test]
    fn rediscovery_is_the_inverse_of_date_created() {
        let pool = vec![
            cand(song_dated("a", Some("2019-01-01"))),
            cand(song_dated("b", Some("2020-01-01"))),
            cand(song_dated("c", Some("2021-01-01"))),
        ];
        let by_rediscovery = ids(&run_pipeline(
            &PipelineInput::default().with_pool(SourceKind::Library, None, pool.clone()),
            &lib_pipeline(vec![OrderingKey::Rediscovery]),
        ));
        let mut by_date_created = ids(&run_pipeline(
            &PipelineInput::default().with_pool(SourceKind::Library, None, pool),
            &lib_pipeline(vec![OrderingKey::DateCreated]),
        ));
        by_date_created.reverse();
        assert_eq!(
            by_rediscovery, by_date_created,
            "rediscovery reverses DateCreated on distinct dates"
        );
        assert_eq!(by_rediscovery, vec!["a", "b", "c"]);
    }

    // ---- AC 2/10: composition & backward-compat -----------------------------

    #[test]
    fn new_keys_compose_with_precedence_preserved() {
        // [Excavation, Favorite]: deep cuts first, favorites break ties at equal play_count.
        let fav_low = {
            let mut s = song_plays("fav-low", Some(2));
            s.is_favorite = Some(true);
            s
        };
        let plain_low = song_plays("plain-low", Some(2));
        let hit = song_plays("hit", Some(80));
        let input = PipelineInput::default().with_pool(
            SourceKind::Library,
            None,
            vec![cand(hit), cand(plain_low), cand(fav_low)],
        );
        let result = ids(&run_pipeline(
            &input,
            &lib_pipeline(vec![OrderingKey::Excavation, OrderingKey::Favorite]),
        ));
        // play_count dominates (2 < 80); within the 2-play tie, favorite wins.
        assert_eq!(result, vec!["fav-low", "plain-low", "hit"]);
    }

    #[test]
    fn pipelines_without_new_keys_are_unchanged() {
        // A legacy-default pipeline never lists the new keys → identical selection to today.
        let a = song_plays("a", Some(99)); // many plays, would sink under Excavation
        let b = song_dated("b", Some("2010-01-01")); // ancient, would lead under Rediscovery
        let input =
            PipelineInput::default().with_pool(SourceKind::Library, None, vec![cand(a), cand(b)]);
        let legacy = AutoFillPipeline::default_legacy(Some(100_000_000));
        // Neither key participates; legacy keys (fav/playCount/dateCreated) decide. `a` has 99 plays
        // vs `b`'s 0 → `a` leads on PlayCount. Confirms the new arms are fully opt-in.
        assert_eq!(ids(&run_pipeline(&input, &legacy)), vec!["a", "b"]);
    }

    // ---- AC 11: serde round-trip -------------------------------------------

    #[test]
    fn new_ordering_keys_serde_round_trip() {
        let json = r#"{ "ordering": ["excavation", "rediscovery"] }"#;
        let p: AutoFillPipeline = serde_json::from_str(json).unwrap();
        assert_eq!(
            p.ordering,
            vec![OrderingKey::Excavation, OrderingKey::Rediscovery]
        );
        // camelCase wire form round-trips byte-stable.
        assert!(
            serde_json::to_string(&p)
                .unwrap()
                .contains("\"excavation\"")
        );
        let back: AutoFillPipeline =
            serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn rarity_pity_and_new_keys_serde_round_trip() {
        // AC 14: a pipeline carrying rarity/pity stages and Random/Rarity ordering keys round-trips
        // byte-stable through camelCase serde.
        let json = r#"{
            "ordering": ["random", "rarity"],
            "rarity": { "enabled": true, "legendaryWeight": 8.0, "rareWeight": 3.0, "commonWeight": 1.0, "rareMaxPlays": 5 },
            "pity": { "enabled": true, "thresholdSyncs": 3, "guaranteedRatio": 0.25, "discoveryMaxPlays": 0 }
        }"#;
        let p: AutoFillPipeline = serde_json::from_str(json).unwrap();
        assert_eq!(p.ordering, vec![OrderingKey::Random, OrderingKey::Rarity]);
        assert!(p.rarity.enabled && (p.rarity.legendary_weight - 8.0).abs() < f32::EPSILON);
        assert!(p.pity.enabled && p.pity.threshold_syncs == 3);
        let wire = serde_json::to_string(&p).unwrap();
        assert!(wire.contains("\"rarity\"") && wire.contains("\"legendaryWeight\""));
        let back: AutoFillPipeline = serde_json::from_str(&wire).unwrap();
        assert_eq!(p, back);

        // A default pipeline omits nothing surprising and round-trips identical (fast-path safe).
        let def = AutoFillPipeline::default();
        let back: AutoFillPipeline =
            serde_json::from_str(&serde_json::to_string(&def).unwrap()).unwrap();
        assert_eq!(def, back);
    }

    // ===================================================================
    // Story 13.4 — seeded entropy, weighted rarity draws (#29), pity timer (#30).
    // ===================================================================

    /// A pool of `n` tracks, all not-on-device, big enough to all fit a large budget.
    fn shuffle_pool(n: usize) -> Vec<Candidate> {
        (0..n)
            .map(|i| {
                cand(song_sized(
                    &format!("t{i:02}"),
                    false,
                    0,
                    "2024-01-01",
                    1_000_000,
                ))
            })
            .collect()
    }

    fn run_seeded(
        pool: Vec<Candidate>,
        ordering: Vec<OrderingKey>,
        rarity: RarityStage,
        seed: u64,
    ) -> Vec<String> {
        let input = PipelineInput {
            seed,
            ..Default::default()
        }
        .with_pool(SourceKind::Library, None, pool);
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            ordering,
            rarity,
            budget: BudgetStage {
                max_bytes: Some(1_000_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        ids(&run_pipeline(&input, &pipeline))
    }

    #[test]
    fn random_shuffle_is_deterministic_given_seed() {
        // AC 1/2: same (input, seed, pipeline) ⇒ byte-identical order; re-running is identical.
        let a = run_seeded(
            shuffle_pool(8),
            vec![OrderingKey::Random],
            RarityStage::default(),
            42,
        );
        let b = run_seeded(
            shuffle_pool(8),
            vec![OrderingKey::Random],
            RarityStage::default(),
            42,
        );
        assert_eq!(a, b, "same seed ⇒ identical order");
        // No track is lost or duplicated by the shuffle.
        let mut sorted = a.clone();
        sorted.sort();
        assert_eq!(
            sorted,
            (0..8).map(|i| format!("t{i:02}")).collect::<Vec<_>>()
        );
        // A seeded shuffle does not leave the pool in its input order (would mean entropy never fired).
        let input_order: Vec<String> = (0..8).map(|i| format!("t{i:02}")).collect();
        assert_ne!(a, input_order, "seeded shuffle must actually reorder");
    }

    #[test]
    fn random_shuffle_differs_across_seeds() {
        // AC 2: a different seed (very likely) yields a different order.
        let a = run_seeded(
            shuffle_pool(8),
            vec![OrderingKey::Random],
            RarityStage::default(),
            1,
        );
        let b = run_seeded(
            shuffle_pool(8),
            vec![OrderingKey::Random],
            RarityStage::default(),
            999,
        );
        assert_ne!(
            a, b,
            "different seeds should (overwhelmingly likely) differ for 8 items"
        );
    }

    #[test]
    fn pipeline_without_random_is_seed_independent() {
        // AC 2: a pipeline that never lists Random/Rarity is byte-for-byte unaffected by the seed.
        let a = run_seeded(
            shuffle_pool(6),
            vec![OrderingKey::PlayCount],
            RarityStage::default(),
            1,
        );
        let b = run_seeded(
            shuffle_pool(6),
            vec![OrderingKey::PlayCount],
            RarityStage::default(),
            12345,
        );
        assert_eq!(a, b, "no random key ⇒ seed has no effect");
    }

    #[test]
    fn rarity_weighting_favors_legendary_over_common() {
        // AC 4/14: with legendary_weight ≫ common_weight, a legendary (never-played) candidate is
        // reliably drawn before a common (heavily-played) one. Extreme weights make `u^(1/w)` ≈ 1 for
        // legendary and ≈ 0 for common for essentially any seed.
        let pool = vec![
            cand(song_sized("com", false, 50, "2024-01-01", 1_000_000)), // common (50 plays)
            cand(song_sized("leg", false, 0, "2024-01-01", 1_000_000)),  // legendary (never played)
        ];
        let rarity = RarityStage {
            enabled: true,
            legendary_weight: 1_000_000.0,
            rare_weight: 1.0,
            common_weight: 0.000_001,
            rare_max_plays: 5,
        };
        let order = run_seeded(pool, vec![OrderingKey::Rarity], rarity, 7);
        assert_eq!(
            order,
            vec!["leg", "com"],
            "the legendary gem draws ahead of the common hit"
        );
    }

    #[test]
    fn rarity_weight_zero_sinks_a_class_without_panic() {
        // AC 4/14: a 0.0 class weight forces that class's key to the bottom (no divide-by-zero/NaN).
        let pool = vec![
            cand(song_sized("com", false, 50, "2024-01-01", 1_000_000)), // common, weight 0 ⇒ sinks
            cand(song_sized("leg", false, 0, "2024-01-01", 1_000_000)),  // legendary, weight 1
        ];
        let rarity = RarityStage {
            enabled: true,
            legendary_weight: 1.0,
            rare_weight: 1.0,
            common_weight: 0.0,
            rare_max_plays: 5,
        };
        let order = run_seeded(pool, vec![OrderingKey::Rarity], rarity, 3);
        assert_eq!(
            order,
            vec!["leg", "com"],
            "the 0-weight common class sinks below the legendary"
        );
    }

    #[test]
    fn rarity_disabled_degrades_to_uniform_shuffle() {
        // AC 5: an `OrderingKey::Rarity` with `rarity.enabled=false` is a plain seeded shuffle (weight
        // 1 for all) — identical to `OrderingKey::Random` at the same seed, and never panics.
        let disabled = RarityStage::default(); // enabled:false
        let as_rarity = run_seeded(shuffle_pool(8), vec![OrderingKey::Rarity], disabled, 77);
        let as_random = run_seeded(
            shuffle_pool(8),
            vec![OrderingKey::Random],
            RarityStage::default(),
            77,
        );
        assert_eq!(
            as_rarity, as_random,
            "disabled Rarity == uniform Random at the same seed"
        );
    }

    #[test]
    fn rarity_composes_behind_favorite() {
        // AC 4/14: `[Favorite, Rarity]` keeps favorites ahead of non-favorites; the rarity draw only
        // orders within each favorite tier (placement precedence preserved).
        let pool = vec![
            // Two non-favorite legendaries (huge weight) — must still rank BELOW the favorites.
            cand(song_sized("nf1", false, 0, "2024-01-01", 1_000_000)),
            cand(song_sized("nf2", false, 0, "2024-01-01", 1_000_000)),
            // Two favorite commons (heavily played, tiny weight) — favorites win regardless.
            cand(song_sized("fav1", true, 80, "2024-01-01", 1_000_000)),
            cand(song_sized("fav2", true, 90, "2024-01-01", 1_000_000)),
        ];
        let rarity = RarityStage {
            enabled: true,
            legendary_weight: 1_000_000.0,
            rare_weight: 1.0,
            common_weight: 0.000_001,
            rare_max_plays: 5,
        };
        let order = run_seeded(
            pool,
            vec![OrderingKey::Favorite, OrderingKey::Rarity],
            rarity,
            5,
        );
        assert_eq!(
            &order[0..2]
                .iter()
                .filter(|id| id.starts_with("fav"))
                .count(),
            &2,
            "both favorites must occupy the top two slots despite non-favorites being legendary"
        );
    }

    /// Build a 3-track Library pipeline + input for pity tests: two hits (high play_count) and one
    /// never-played gem, all not on the device, each 3 MB. Ordering [PlayCount] buries the gem.
    fn pity_setup(pity: PityStage, pity_streak: i64, max_bytes: Option<u64>) -> Vec<String> {
        let pool = vec![
            cand(song_sized("hit1", false, 100, "2024-01-01", 3_000_000)),
            cand(song_sized("hit2", false, 90, "2024-01-01", 3_000_000)),
            cand(song_sized("gem", false, 0, "2024-01-01", 3_000_000)),
        ];
        let input = PipelineInput {
            pity_streak,
            ..Default::default()
        }
        .with_pool(SourceKind::Library, None, pool);
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            ordering: vec![OrderingKey::PlayCount],
            pity,
            budget: BudgetStage {
                max_bytes,
                ..Default::default()
            },
            ..Default::default()
        };
        ids(&run_pipeline(&input, &pipeline))
    }

    fn pity_on() -> PityStage {
        PityStage {
            enabled: true,
            threshold_syncs: 3,
            guaranteed_ratio: 0.5,
            discovery_max_plays: 0,
        }
    }

    #[test]
    fn pity_reserve_fires_and_surfaces_a_new_gem() {
        // AC 7: streak >= threshold reserves the discovery quota and surfaces a never-played,
        // not-on-device track the PlayCount ordering would otherwise drop past budget.
        // 6 MB budget fits two 3 MB tracks; reserve = round(6M × 0.5) = 3 MB ⇒ one discovery (gem).
        let with_pity = pity_setup(pity_on(), 3, Some(6_000_000));
        assert!(
            with_pity.contains(&"gem".to_string()),
            "pity guarantees the never-played gem"
        );
        assert_eq!(
            with_pity.first().map(String::as_str),
            Some("gem"),
            "the reserve fills first"
        );
        // Without pity the gem is dropped past budget (PlayCount keeps the two hits).
        let no_pity = pity_setup(PityStage::default(), 3, Some(6_000_000));
        assert!(
            !no_pity.contains(&"gem".to_string()),
            "no pity ⇒ gem stays buried"
        );
        assert_eq!(no_pity, vec!["hit1", "hit2"]);
    }

    #[test]
    fn pity_below_threshold_is_a_noop() {
        // AC 7: pity_streak < threshold ⇒ no reserve.
        let order = pity_setup(pity_on(), 2, Some(6_000_000));
        assert!(
            !order.contains(&"gem".to_string()),
            "streak below threshold ⇒ no guarantee"
        );
        assert_eq!(order, vec!["hit1", "hit2"]);
    }

    #[test]
    fn pity_zero_ratio_is_a_noop() {
        // AC 7: guaranteed_ratio = 0 ⇒ reserve bytes round to 0 ⇒ no-op.
        let pity = PityStage {
            guaranteed_ratio: 0.0,
            ..pity_on()
        };
        let order = pity_setup(pity, 3, Some(6_000_000));
        assert!(
            !order.contains(&"gem".to_string()),
            "a 0 ratio reserves nothing"
        );
    }

    #[test]
    fn pity_unbounded_ceiling_is_a_noop() {
        // AC 7: an unbounded ceiling ⇒ no reserve. Everything fits, so the gem appears via the normal
        // PlayCount fill (ordered last), NOT pulled to the front by a reserve.
        let order = pity_setup(pity_on(), 5, None);
        assert_eq!(
            order,
            vec!["hit1", "hit2", "gem"],
            "unbounded ceiling: pure PlayCount, no reserve"
        );
    }

    #[test]
    fn pity_composes_with_stable_core() {
        // AC 7: stable-core (keep on-device residents) → pity reserve (force new gem) → primary.
        // resident is on-device (stable-core keeps it); gem is a never-played discovery; hits fill the
        // rest. With a 9 MB budget (three 3 MB tracks), core keeps `resident`, pity reserves `gem`.
        let resident = song_sized("resident", false, 200, "2024-01-01", 3_000_000);
        let pool = vec![
            cand(resident.clone()),
            cand(song_sized("hit", false, 100, "2024-01-01", 3_000_000)),
            cand(song_sized("gem", false, 0, "2024-01-01", 3_000_000)),
        ];
        let mut history = HistorySnapshot {
            now: 1_000_000_000,
            ..Default::default()
        };
        history.entries.insert(
            "resident".to_string(),
            TrackHistory {
                last_synced_at: Some(1),
                ..Default::default()
            },
        );
        let input = PipelineInput {
            history,
            pity_streak: 3,
            ..Default::default()
        }
        .with_pool(SourceKind::Library, None, pool);
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            ordering: vec![OrderingKey::PlayCount],
            memory: MemoryStage {
                stable_core_pct: Some(0.34),
                ..Default::default()
            },
            pity: PityStage {
                guaranteed_ratio: 0.34,
                ..pity_on()
            },
            budget: BudgetStage {
                max_bytes: Some(9_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let order = ids(&run_pipeline(&input, &pipeline));
        assert!(
            order.contains(&"resident".to_string()),
            "stable-core keeps the on-device resident"
        );
        assert!(
            order.contains(&"gem".to_string()),
            "pity reserve still surfaces the new gem"
        );
    }

    #[test]
    fn pity_reserve_bytes_is_the_shared_fire_gate() {
        // Story 13.4 review: `pity_reserve_bytes` is the single source of truth shared by the engine
        // (run the reserve) and the RPC sync-completion path (reset the dry-streak only on a real
        // fire). Returns 0 ⇒ the reserve does NOT fire AND the streak is not consumed.
        let pity = pity_on(); // enabled, threshold 3, ratio 0.5
        // Fires: enabled, streak >= threshold, bounded budget, positive reserve.
        assert_eq!(
            pity_reserve_bytes(&pity, 3, 6_000_000),
            3_000_000,
            "round(6M × 0.5)"
        );
        assert!(
            pity_reserve_bytes(&pity, 4, 6_000_000) > 0,
            "streak above threshold also fires"
        );
        // Does NOT fire — these are exactly the cases the recorder must NOT reset on:
        assert_eq!(
            pity_reserve_bytes(&pity, 2, 6_000_000),
            0,
            "below threshold"
        );
        assert_eq!(
            pity_reserve_bytes(&pity, 3, u64::MAX),
            0,
            "unbounded ceiling never fires"
        );
        assert_eq!(
            pity_reserve_bytes(
                &PityStage {
                    guaranteed_ratio: 0.0,
                    ..pity_on()
                },
                3,
                6_000_000
            ),
            0,
            "zero ratio reserves nothing"
        );
        assert_eq!(
            pity_reserve_bytes(
                &PityStage {
                    guaranteed_ratio: 0.000_001,
                    ..pity_on()
                },
                3,
                100,
            ),
            0,
            "a ratio that rounds to <1 byte does not fire"
        );
        assert_eq!(
            pity_reserve_bytes(&PityStage::default(), 99, 6_000_000),
            0,
            "disabled never fires"
        );
        // Ratio is clamped to [0,1]: an out-of-range manifest value cannot reserve beyond the ceiling.
        assert_eq!(
            pity_reserve_bytes(
                &PityStage {
                    guaranteed_ratio: 9.0,
                    ..pity_on()
                },
                3,
                6_000_000
            ),
            6_000_000,
            "ratio clamps to 1.0"
        );
    }

    // ===================================================================
    // Story 13.5 — Context stage (#3/#17/#32) + encoding-from-goals (#20).
    // ===================================================================

    fn civil(hour: u8, month: u8, day: u8) -> CivilTime {
        CivilTime {
            hour,
            month,
            day,
            weekday: 0,
        }
    }

    fn playlist_src(ref_id: &str) -> SourceEntry {
        SourceEntry {
            kind: SourceKind::Playlist,
            ref_id: Some(ref_id.to_string()),
            share: None,
        }
    }

    #[test]
    fn context_window_time_of_day_normal_and_midnight_wrap() {
        // Normal window 6..11 (inclusive start, exclusive end).
        let morning = ContextRule {
            window: ContextWindow::TimeOfDay {
                start_hour: 6,
                end_hour: 11,
            },
            ..Default::default()
        };
        assert!(
            context_rule_active(&morning, &civil(6, 1, 1)),
            "06:00 is in [6,11)"
        );
        assert!(
            context_rule_active(&morning, &civil(10, 1, 1)),
            "10:00 is in [6,11)"
        );
        assert!(
            !context_rule_active(&morning, &civil(11, 1, 1)),
            "11:00 is exclusive end"
        );
        assert!(
            !context_rule_active(&morning, &civil(5, 1, 1)),
            "05:00 is before start"
        );

        // Midnight wrap 22..6 ⇒ active if hour >= 22 OR hour < 6.
        let night = ContextRule {
            window: ContextWindow::TimeOfDay {
                start_hour: 22,
                end_hour: 6,
            },
            ..Default::default()
        };
        assert!(context_rule_active(&night, &civil(23, 1, 1)), "23:00 wraps");
        assert!(context_rule_active(&night, &civil(2, 1, 1)), "02:00 wraps");
        assert!(
            !context_rule_active(&night, &civil(6, 1, 1)),
            "06:00 is the exclusive end"
        );
        assert!(
            !context_rule_active(&night, &civil(12, 1, 1)),
            "12:00 is outside the wrap"
        );

        // Degenerate start==end matches nothing.
        let degenerate = ContextRule {
            window: ContextWindow::TimeOfDay {
                start_hour: 9,
                end_hour: 9,
            },
            ..Default::default()
        };
        assert!(!context_rule_active(&degenerate, &civil(9, 1, 1)));
    }

    #[test]
    fn context_window_months_and_date_range_with_year_end_wrap() {
        let december = ContextRule {
            window: ContextWindow::Months { months: vec![12] },
            ..Default::default()
        };
        assert!(context_rule_active(&december, &civil(0, 12, 25)));
        assert!(!context_rule_active(&december, &civil(0, 11, 30)));
        // A default (all-zero) civil time matches no Months rule (month 0 is unlisted).
        assert!(!context_rule_active(&december, &CivilTime::default()));

        let summer = ContextRule {
            window: ContextWindow::Months {
                months: vec![6, 7, 8],
            },
            ..Default::default()
        };
        assert!(context_rule_active(&summer, &civil(0, 7, 15)));
        assert!(!context_rule_active(&summer, &civil(0, 9, 1)));

        // DateRange normal: Mar 1 .. May 31.
        let spring = ContextRule {
            window: ContextWindow::DateRange {
                start: (3, 1),
                end: (5, 31),
            },
            ..Default::default()
        };
        assert!(context_rule_active(&spring, &civil(0, 4, 10)));
        assert!(!context_rule_active(&spring, &civil(0, 6, 1)));

        // DateRange year-end wrap: Dec 15 .. Jan 5.
        let holidays = ContextRule {
            window: ContextWindow::DateRange {
                start: (12, 15),
                end: (1, 5),
            },
            ..Default::default()
        };
        assert!(
            context_rule_active(&holidays, &civil(0, 12, 25)),
            "Dec 25 wraps"
        );
        assert!(
            context_rule_active(&holidays, &civil(0, 1, 3)),
            "Jan 3 wraps"
        );
        assert!(
            !context_rule_active(&holidays, &civil(0, 1, 6)),
            "Jan 6 is past the end"
        );
        assert!(
            !context_rule_active(&holidays, &civil(0, 6, 1)),
            "June is well outside"
        );
    }

    #[test]
    fn context_disabled_is_byte_identical_and_civil_time_determines_result() {
        // Two playlist phase sources; a morning rule activates only "morning".
        let pools = || {
            PipelineInput::default()
                .with_pool(
                    SourceKind::Playlist,
                    Some("morning"),
                    vec![cand(song_sized("m1", false, 0, "2024-01-01", 1_000_000))],
                )
                .with_pool(
                    SourceKind::Playlist,
                    Some("evening"),
                    vec![cand(song_sized("e1", false, 0, "2024-01-01", 1_000_000))],
                )
        };
        let context = ContextStage {
            enabled: true,
            rules: vec![
                ContextRule {
                    window: ContextWindow::TimeOfDay {
                        start_hour: 6,
                        end_hour: 11,
                    },
                    source_refs: vec!["morning".to_string()],
                    ..Default::default()
                },
                ContextRule {
                    window: ContextWindow::TimeOfDay {
                        start_hour: 18,
                        end_hour: 23,
                    },
                    source_refs: vec!["evening".to_string()],
                    ..Default::default()
                },
            ],
        };
        let base = AutoFillPipeline {
            sources: vec![playlist_src("morning"), playlist_src("evening")],
            budget: BudgetStage {
                max_bytes: Some(10_000_000),
                ..Default::default()
            },
            ..Default::default()
        };

        // context.enabled = false ⇒ both phase sources run (today's behavior).
        let disabled = AutoFillPipeline {
            context: ContextStage {
                enabled: false,
                ..context.clone()
            },
            ..base.clone()
        };
        let mut disabled_ids = {
            let mut input = pools();
            input.history.local = civil(8, 1, 1); // ignored because disabled
            ids(&run_pipeline(&input, &disabled))
        };
        disabled_ids.sort();
        assert_eq!(
            disabled_ids,
            vec!["e1", "m1"],
            "disabled ⇒ no gating, both phases fill"
        );

        // Enabled at 08:00 ⇒ only the morning phase; the evening source is suppressed.
        let enabled = AutoFillPipeline {
            context: context.clone(),
            ..base.clone()
        };
        let mut morning_input = pools();
        morning_input.history.local = civil(8, 1, 1);
        assert_eq!(ids(&run_pipeline(&morning_input, &enabled)), vec!["m1"]);

        // Enabled at 20:00 ⇒ only the evening phase. Same config, different civil time ⇒ different result.
        let mut evening_input = pools();
        evening_input.history.local = civil(20, 1, 1);
        assert_eq!(ids(&run_pipeline(&evening_input, &enabled)), vec!["e1"]);

        // Determinism: same civil time ⇒ byte-identical repeat.
        let mut again = pools();
        again.history.local = civil(8, 1, 1);
        assert_eq!(ids(&run_pipeline(&again, &enabled)), vec!["m1"]);
    }

    #[test]
    fn context_unmentioned_source_always_runs_and_only_inactive_is_suppressed() {
        let input = PipelineInput::default()
            .with_pool(
                SourceKind::Playlist,
                Some("morning"),
                vec![cand(song_sized("m1", false, 0, "2024-01-01", 1_000_000))],
            )
            .with_pool(
                SourceKind::Playlist,
                Some("evening"),
                vec![cand(song_sized("e1", false, 0, "2024-01-01", 1_000_000))],
            )
            .with_pool(
                SourceKind::Library,
                None,
                vec![cand(song_sized("lib", false, 0, "2024-01-01", 1_000_000))],
            );
        let mut input = input;
        input.history.local = civil(8, 1, 1);

        let pipeline = AutoFillPipeline {
            sources: vec![
                playlist_src("morning"),
                playlist_src("evening"),
                SourceEntry::new(SourceKind::Library),
            ],
            budget: BudgetStage {
                max_bytes: Some(10_000_000),
                ..Default::default()
            },
            context: ContextStage {
                enabled: true,
                rules: vec![
                    ContextRule {
                        window: ContextWindow::TimeOfDay {
                            start_hour: 6,
                            end_hour: 11,
                        },
                        source_refs: vec!["morning".to_string()],
                        ..Default::default()
                    },
                    ContextRule {
                        window: ContextWindow::TimeOfDay {
                            start_hour: 18,
                            end_hour: 23,
                        },
                        source_refs: vec!["evening".to_string()],
                        ..Default::default()
                    },
                ],
            },
            ..Default::default()
        };
        let mut got = ids(&run_pipeline(&input, &pipeline));
        got.sort();
        assert_eq!(
            got,
            vec!["lib", "m1"],
            "active morning + unmentioned library run; the inactive evening source is suppressed"
        );
    }

    #[test]
    fn context_weight_boosts_share_and_max_composes() {
        // Two playlist sources, 10×1MB tracks each, 10MB ceiling. A weight on "a" biases its share.
        let pools = || {
            let mk = |prefix: &str| {
                (0..10)
                    .map(|i| {
                        cand(song_sized(
                            &format!("{prefix}{i}"),
                            false,
                            0,
                            "2024-01-01",
                            1_000_000,
                        ))
                    })
                    .collect::<Vec<_>>()
            };
            PipelineInput::default()
                .with_pool(SourceKind::Playlist, Some("a"), mk("a"))
                .with_pool(SourceKind::Playlist, Some("b"), mk("b"))
        };
        let base = AutoFillPipeline {
            sources: vec![playlist_src("a"), playlist_src("b")],
            budget: BudgetStage {
                max_bytes: Some(10_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let count = |items: &[AutoFillItem], prefix: &str| {
            items.iter().filter(|i| i.id.starts_with(prefix)).count()
        };

        // Single active rule: weight 3.0 on "a" ⇒ a share 0.75 (cap 7.5MB → 7), b share 0.25 (2.5MB → 2).
        let weighted = AutoFillPipeline {
            context: ContextStage {
                enabled: true,
                rules: vec![ContextRule {
                    window: ContextWindow::Months { months: vec![7] },
                    source_refs: vec!["a".to_string()],
                    weight: Some(3.0),
                    ..Default::default()
                }],
            },
            ..base.clone()
        };
        let mut wi = pools();
        wi.history.local = civil(0, 7, 1); // July ⇒ rule active
        let w = run_pipeline(&wi, &weighted);
        assert_eq!(
            count(&w, "a"),
            7,
            "weight 3 ⇒ a gets the 0.75 share (7 of 1MB)"
        );
        assert_eq!(
            count(&w, "b"),
            2,
            "b gets the residual 0.25 share (2 of 1MB)"
        );

        // Max-compose: TWO active rules each weight 2.0 on "a" ⇒ effective weight 2.0 (NOT 4.0 product).
        // weight 2 ⇒ a share = 0.5·2/(0.5·2+0.5·1) = 0.667 (cap 6.67MB → 6), b → 0.333 (3.33MB → 3).
        let composed = AutoFillPipeline {
            context: ContextStage {
                enabled: true,
                rules: vec![
                    ContextRule {
                        window: ContextWindow::Months { months: vec![7] },
                        source_refs: vec!["a".to_string()],
                        weight: Some(2.0),
                        ..Default::default()
                    },
                    ContextRule {
                        window: ContextWindow::Months { months: vec![7] },
                        source_refs: vec!["a".to_string()],
                        weight: Some(2.0),
                        ..Default::default()
                    },
                ],
            },
            ..base.clone()
        };
        let mut ci = pools();
        ci.history.local = civil(0, 7, 1);
        let c = run_pipeline(&ci, &composed);
        assert_eq!(
            count(&c, "a"),
            6,
            "max-compose keeps weight 2 (6 tracks), not the 4.0 product (8)"
        );
        assert_eq!(count(&c, "b"), 3);
    }

    #[test]
    fn context_weight_zero_suppresses_not_equal_splits() {
        // Review 13.5 regression: weight 0 must SUPPRESS the source, not fall back to an equal split.
        let pools = || {
            let mk = |prefix: &str| {
                (0..10)
                    .map(|i| {
                        cand(song_sized(
                            &format!("{prefix}{i}"),
                            false,
                            0,
                            "2024-01-01",
                            1_000_000,
                        ))
                    })
                    .collect::<Vec<_>>()
            };
            PipelineInput::default()
                .with_pool(SourceKind::Playlist, Some("a"), mk("a"))
                .with_pool(SourceKind::Playlist, Some("b"), mk("b"))
        };
        let count = |items: &[AutoFillItem], prefix: &str| {
            items.iter().filter(|i| i.id.starts_with(prefix)).count()
        };

        // weight 0 on the ONLY mentioned source: it is suppressed, "b" (unmentioned) takes the budget.
        let zero_only = AutoFillPipeline {
            sources: vec![playlist_src("a"), playlist_src("b")],
            budget: BudgetStage {
                max_bytes: Some(10_000_000),
                ..Default::default()
            },
            context: ContextStage {
                enabled: true,
                rules: vec![ContextRule {
                    window: ContextWindow::Months { months: vec![7] },
                    source_refs: vec!["a".to_string()],
                    weight: Some(0.0),
                    ..Default::default()
                }],
            },
            ..Default::default()
        };
        let mut zi = pools();
        zi.history.local = civil(0, 7, 1);
        let z = run_pipeline(&zi, &zero_only);
        assert_eq!(
            count(&z, "a"),
            0,
            "weight 0 suppresses 'a' (not an equal 1/n split)"
        );
        assert_eq!(
            count(&z, "b"),
            10,
            "'b' (unmentioned) fills the whole budget"
        );

        // weight 0 on "a" in one active rule but 2.0 in another active rule ⇒ max-compose keeps it.
        let zero_then_boost = AutoFillPipeline {
            sources: vec![playlist_src("a"), playlist_src("b")],
            budget: BudgetStage {
                max_bytes: Some(10_000_000),
                ..Default::default()
            },
            context: ContextStage {
                enabled: true,
                rules: vec![
                    ContextRule {
                        window: ContextWindow::Months { months: vec![7] },
                        source_refs: vec!["a".to_string()],
                        weight: Some(0.0),
                        ..Default::default()
                    },
                    ContextRule {
                        window: ContextWindow::Months { months: vec![7] },
                        source_refs: vec!["a".to_string()],
                        weight: Some(2.0),
                        ..Default::default()
                    },
                ],
            },
            ..Default::default()
        };
        let mut bi = pools();
        bi.history.local = civil(0, 7, 1);
        let b = run_pipeline(&bi, &zero_then_boost);
        assert_eq!(
            count(&b, "a"),
            6,
            "max(0, 2) = 2 ⇒ 'a' retained at weight 2 (6 tracks)"
        );
        assert_eq!(count(&b, "b"), 3);
    }

    #[test]
    fn context_inert_when_civil_time_unset_even_if_enabled() {
        // Review 13.5 hardening: an unminted CivilTime::default() (month 0) keeps the Context stage
        // inert even with context.enabled = true — so a TimeOfDay { start_hour: 0 } rule can't fire.
        let pools = || {
            let mk = |prefix: &str| {
                (0..6)
                    .map(|i| {
                        cand(song_sized(
                            &format!("{prefix}{i}"),
                            false,
                            0,
                            "2024-01-01",
                            1_000_000,
                        ))
                    })
                    .collect::<Vec<_>>()
            };
            PipelineInput::default()
                .with_pool(SourceKind::Playlist, Some("morning"), mk("morning"))
                .with_pool(SourceKind::Playlist, Some("evening"), mk("evening"))
        };
        // Both sources are mentioned in time-windowed rules. With an all-zero default (hour 0), the
        // morning rule (0..6) would fire and the evening rule (18..23) would not — suppressing "evening".
        // The is_set() guard must prevent any rule from being consulted while civil time is unset.
        let pipeline = AutoFillPipeline {
            sources: vec![playlist_src("morning"), playlist_src("evening")],
            budget: BudgetStage {
                max_bytes: Some(12_000_000),
                ..Default::default()
            },
            context: ContextStage {
                enabled: true,
                rules: vec![
                    ContextRule {
                        window: ContextWindow::TimeOfDay {
                            start_hour: 0,
                            end_hour: 6,
                        },
                        source_refs: vec!["morning".to_string()],
                        ..Default::default()
                    },
                    ContextRule {
                        window: ContextWindow::TimeOfDay {
                            start_hour: 18,
                            end_hour: 23,
                        },
                        source_refs: vec!["evening".to_string()],
                        ..Default::default()
                    },
                ],
            },
            ..Default::default()
        };

        // Unset civil time (default ⇒ month 0): context inert ⇒ both sources run (6 each).
        let inert = run_pipeline(&pools(), &pipeline);
        let count =
            |items: &[AutoFillItem], p: &str| items.iter().filter(|i| i.id.starts_with(p)).count();
        assert_eq!(
            count(&inert, "morning"),
            6,
            "unset civil time ⇒ context inert ⇒ morning runs"
        );
        assert_eq!(
            count(&inert, "evening"),
            6,
            "unset civil time ⇒ evening NOT suppressed"
        );

        // Minted civil time at 00:00 Jan 1 (month set): the rule now fires ⇒ evening suppressed.
        let mut active = pools();
        active.history.local = civil(0, 1, 1);
        let gated = run_pipeline(&active, &pipeline);
        assert_eq!(
            count(&gated, "evening"),
            0,
            "minted hour-0 civil time ⇒ evening suppressed by the rule"
        );
        assert_eq!(count(&gated, "morning"), 6);
    }

    #[test]
    fn context_scheduled_filter_unions_and_exclude_beats_include() {
        // Seasonal proxy: in December, include genre "Christmas"; exclude tag "explicit" always.
        let input = PipelineInput::default().with_pool(
            SourceKind::Library,
            None,
            vec![
                cand_meta(
                    song_sized("xmas", false, 0, "2024-01-01", 1_000_000),
                    &["Christmas"],
                    &[],
                ),
                cand_meta(
                    song_sized("pop", false, 0, "2024-01-01", 1_000_000),
                    &["Pop"],
                    &[],
                ),
                cand_meta(
                    song_sized("xmas-explicit", false, 0, "2024-01-01", 1_000_000),
                    &["Christmas"],
                    &["explicit"],
                ),
            ],
        );
        let pipeline = |month: u8| {
            let mut i = input.clone();
            i.history.local = civil(0, month, 1);
            (
                i,
                AutoFillPipeline {
                    sources: vec![SourceEntry::new(SourceKind::Library)],
                    budget: BudgetStage {
                        max_bytes: Some(10_000_000),
                        ..Default::default()
                    },
                    context: ContextStage {
                        enabled: true,
                        rules: vec![ContextRule {
                            window: ContextWindow::Months { months: vec![12] },
                            include_genres: vec!["Christmas".to_string()],
                            exclude_tags: vec!["explicit".to_string()],
                            ..Default::default()
                        }],
                    },
                    ..Default::default()
                },
            )
        };

        // December ⇒ rule active: only Christmas genre, and the explicit one is excluded.
        let (dec_in, dec_pipe) = pipeline(12);
        assert_eq!(
            ids(&run_pipeline(&dec_in, &dec_pipe)),
            vec!["xmas"],
            "active rule restricts to Christmas (include) and drops the explicit one (exclude wins)"
        );

        // July ⇒ rule inactive: the static (empty) filter applies, all three pass.
        let (jul_in, jul_pipe) = pipeline(7);
        let mut jul = ids(&run_pipeline(&jul_in, &jul_pipe));
        jul.sort();
        assert_eq!(
            jul,
            vec!["pop", "xmas", "xmas-explicit"],
            "inactive rule contributes nothing"
        );
    }

    #[test]
    fn target_bitrate_kbps_off_missing_and_clamped() {
        // Off ⇒ None even with both goals.
        assert_eq!(
            target_bitrate_kbps(&BudgetStage {
                max_bytes: Some(10_000_000),
                target_duration_secs: Some(600),
                ..Default::default()
            }),
            None,
            "flag off ⇒ no derivation"
        );
        let on = |max_bytes, secs, headroom| BudgetStage {
            max_bytes,
            target_duration_secs: secs,
            headroom_bytes: headroom,
            encoding_from_goals: true,
        };
        // Missing either goal ⇒ None.
        assert_eq!(target_bitrate_kbps(&on(None, Some(600), None)), None);
        assert_eq!(target_bitrate_kbps(&on(Some(10_000_000), None, None)), None);
        assert_eq!(
            target_bitrate_kbps(&on(Some(10_000_000), Some(0), None)),
            None,
            "zero duration ⇒ None"
        );
        // Nominal: 10MB over 600s ⇒ 10_000_000*8/600_000 = 133 kbps.
        assert_eq!(
            target_bitrate_kbps(&on(Some(10_000_000), Some(600), None)),
            Some(133)
        );
        // Headroom reduces the effective bytes: (10MB-2MB)*8/600_000 = 106 kbps.
        assert_eq!(
            target_bitrate_kbps(&on(Some(10_000_000), Some(600), Some(2_000_000))),
            Some(106)
        );
        // Tiny goal clamps up to 32; huge goal clamps down to 320.
        assert_eq!(
            target_bitrate_kbps(&on(Some(1_000_000), Some(3600), None)),
            Some(32),
            "clamp floor"
        );
        assert_eq!(
            target_bitrate_kbps(&on(Some(1_000_000_000), Some(60), None)),
            Some(320),
            "clamp ceiling"
        );
    }

    #[test]
    fn bitrate_aware_estimate_shrinks_but_never_enlarges() {
        // Oversize source (10MB, 180s) at 128 kbps ⇒ transcoded 128*1000/8*180 = 2_880_000.
        let big = song_sized("big", false, 0, "2024-01-01", 10_000_000);
        assert_eq!(
            estimated_size(&big, None),
            Some(10_000_000),
            "None ⇒ source estimate"
        );
        assert_eq!(
            estimated_size(&big, Some(128)),
            Some(2_880_000),
            "transcode shrinks the oversize source"
        );

        // A source already smaller than the target is unchanged (transcoding only shrinks).
        let small = Song {
            size_bytes: Some(1_000_000),
            ..song_sized("small", false, 0, "2024-01-01", 0)
        };
        assert_eq!(
            estimated_size(&small, Some(128)),
            Some(1_000_000),
            "never enlarge a smaller source"
        );

        // Zero/unknown duration can't produce a transcoded size ⇒ keep the source estimate (no 0-byte item).
        let no_dur = Song {
            duration_seconds: 0,
            size_bytes: Some(2_000_000),
            ..song_sized("nd", false, 0, "2024-01-01", 0)
        };
        assert_eq!(estimated_size(&no_dur, Some(128)), Some(2_000_000));
    }

    #[test]
    fn encoding_from_goals_lets_duration_goal_fit_the_byte_ceiling() {
        // Three 3MB / 300s songs; 4MB ceiling, 600s target. Without encoding only 1 fits (3MB).
        // With encoding: target = 4MB*8/600_000 = 53 kbps ⇒ each transcodes to ~1.99MB, so 2 fit (≈4MB)
        // and the 600s duration target is met exactly.
        let pool = || {
            (0..3)
                .map(|i| {
                    cand(song_sized(
                        &format!("s{i}"),
                        false,
                        0,
                        "2024-01-01",
                        3_000_000,
                    ))
                })
                .map(|mut c| {
                    c.song.duration_seconds = 300;
                    c
                })
                .collect::<Vec<_>>()
        };
        let base_budget = BudgetStage {
            max_bytes: Some(4_000_000),
            target_duration_secs: Some(600),
            ..Default::default()
        };

        let plain = AutoFillPipeline {
            budget: base_budget.clone(),
            ..Default::default()
        };
        let plain_input = PipelineInput::default().with_pool(SourceKind::Library, None, pool());
        assert_eq!(
            run_pipeline(&plain_input, &plain).len(),
            1,
            "no encoding ⇒ only one 3MB song fits 4MB"
        );

        let encoded = AutoFillPipeline {
            budget: BudgetStage {
                encoding_from_goals: true,
                ..base_budget
            },
            ..Default::default()
        };
        let encoded_input = PipelineInput::default().with_pool(SourceKind::Library, None, pool());
        let result = run_pipeline(&encoded_input, &encoded);
        assert_eq!(
            result.len(),
            2,
            "encoding-from-goals shrinks each song so two fit the byte ceiling"
        );
        assert!(
            result.iter().map(|i| i.size_bytes).sum::<u64>() <= 4_000_000,
            "the bitrate-aware fill still respects the byte ceiling"
        );
    }

    #[test]
    fn context_and_encoding_serde_round_trip() {
        let pipeline = AutoFillPipeline {
            sources: vec![playlist_src("morning")],
            budget: BudgetStage {
                max_bytes: Some(8_000_000),
                target_duration_secs: Some(3600),
                encoding_from_goals: true,
                ..Default::default()
            },
            context: ContextStage {
                enabled: true,
                rules: vec![
                    ContextRule {
                        window: ContextWindow::TimeOfDay {
                            start_hour: 6,
                            end_hour: 11,
                        },
                        source_refs: vec!["morning".to_string()],
                        weight: Some(2.0),
                        ..Default::default()
                    },
                    ContextRule {
                        window: ContextWindow::Months { months: vec![12] },
                        include_genres: vec!["Christmas".to_string()],
                        exclude_tags: vec!["explicit".to_string()],
                        ..Default::default()
                    },
                    ContextRule {
                        window: ContextWindow::DateRange {
                            start: (12, 15),
                            end: (1, 5),
                        },
                        source_refs: vec!["holiday".to_string()],
                        ..Default::default()
                    },
                ],
            },
            ..Default::default()
        };
        let json = serde_json::to_string(&pipeline).unwrap();
        let back: AutoFillPipeline = serde_json::from_str(&json).unwrap();
        assert_eq!(
            pipeline, back,
            "context (all 3 window kinds) + encoding_from_goals round-trip"
        );

        // A malformed rule degrades to "no effect" (parse-tolerant) without aborting the parse.
        let with_bad_rule = r#"{"context":{"enabled":true,"rules":[{"window":{"bogusWindow":{}}},{"window":{"months":{"months":[12]}}}]}}"#;
        let parsed: AutoFillPipeline = serde_json::from_str(with_bad_rule).unwrap();
        assert_eq!(
            parsed.context.rules.len(),
            1,
            "the malformed rule is dropped; the valid one survives"
        );

        // A default ContextStage / encoding flag round-trips and keeps the engine's default behavior.
        let default_back: AutoFillPipeline =
            serde_json::from_str(&serde_json::to_string(&AutoFillPipeline::default()).unwrap())
                .unwrap();
        assert_eq!(default_back.context, ContextStage::default());
        assert!(!default_back.budget.encoding_from_goals);
    }

    // ===================================================================
    // Story 13.6 — Advanced units & promotion (#33 Spotlight, #8 album/track ratio,
    // #9 affinity promotion, #27 coherence). All four are additive, default-noop modifiers
    // over the existing Unit axis; behavior emerges purely from `PromotionStage` config.
    // ===================================================================

    /// A `Song` with explicit album/artist/disc/track + size, for unit/promotion/coherence tests.
    fn song_album(
        id: &str,
        artist: &str,
        album: &str,
        disc: u32,
        track: u32,
        fav: bool,
        play_count: u32,
        size_bytes: u64,
    ) -> Song {
        Song {
            artist_id: Some(artist.to_string()),
            artist_name: Some(format!("Artist {artist}")),
            album_id: Some(album.to_string()),
            album_title: Some(format!("Album {album}")),
            disc_number: Some(disc),
            track_number: Some(track),
            ..song_sized(id, fav, play_count, "2024-01-01", size_bytes)
        }
    }

    // ---- #9 Affinity-triggered album promotion -----------------------------------------

    #[test]
    fn promotion_affinity_promotes_high_favorite_album_atomically() {
        // Album "deep" = 3 tracks ×3MB = 9MB, 2 favorited. A loose 3MB single follows. Base unit Track,
        // threshold 2 ⇒ "deep" becomes ONE atomic 9MB unit. Ceiling 10MB: the atomic album fits, the
        // single can't (9+3 > 10) ⇒ result is the whole album, nothing partial.
        let pool = vec![
            cand(song_album("d1", "A", "deep", 1, 1, true, 0, 3_000_000)),
            cand(song_album("d2", "A", "deep", 1, 2, true, 0, 3_000_000)),
            cand(song_album("d3", "A", "deep", 1, 3, false, 0, 3_000_000)),
            cand(song_album("s1", "B", "single", 1, 1, true, 0, 3_000_000)),
        ];
        let input = PipelineInput::default().with_pool(SourceKind::Library, None, pool);
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            ordering: vec![OrderingKey::Favorite],
            budget: BudgetStage {
                max_bytes: Some(10_000_000),
                ..Default::default()
            },
            promotion: PromotionStage {
                promote_album_min_favorites: Some(2),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = ids(&run_pipeline(&input, &pipeline));
        assert_eq!(
            result.len(),
            3,
            "the whole 3-track album is atomic; the single can't also fit"
        );
        for id in ["d1", "d2", "d3"] {
            assert!(
                result.contains(&id.to_string()),
                "promoted album syncs whole: {id}"
            );
        }
        assert!(
            !result.contains(&"s1".to_string()),
            "the loose single is squeezed out by the atomic album"
        );
    }

    #[test]
    fn promotion_affinity_below_threshold_stays_track_level() {
        // Same album, but only 1 favorited track and threshold 2 ⇒ NOT promoted ⇒ track singletons.
        // Ceiling 10MB fits 3 of the 4 tracks individually (favorites first), proving track-level fill.
        let pool = vec![
            cand(song_album("d1", "A", "deep", 1, 1, true, 0, 3_000_000)),
            cand(song_album("d2", "A", "deep", 1, 2, false, 0, 3_000_000)),
            cand(song_album("d3", "A", "deep", 1, 3, false, 0, 3_000_000)),
            cand(song_album("s1", "B", "single", 1, 1, true, 0, 3_000_000)),
        ];
        let input = PipelineInput::default().with_pool(SourceKind::Library, None, pool);
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            ordering: vec![OrderingKey::Favorite],
            budget: BudgetStage {
                max_bytes: Some(10_000_000),
                ..Default::default()
            },
            promotion: PromotionStage {
                promote_album_min_favorites: Some(2),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = ids(&run_pipeline(&input, &pipeline));
        assert_eq!(
            result.len(),
            3,
            "track-level fill packs 3 individual tracks (not whole-or-nothing)"
        );
        // Favorites (d1, s1) lead; one more track fills the rest — the album was NOT taken atomically.
        assert!(result.contains(&"d1".to_string()) && result.contains(&"s1".to_string()));
    }

    #[test]
    fn promotion_affinity_inert_for_non_track_base_unit_and_for_none() {
        // Promotion is gated on base unit == Track. With unit == Album it must be a pure no-op: identical
        // to the same pipeline with no promotion. And None/0 ⇒ today's track grouping.
        let pool = vec![
            cand(song_album("d1", "A", "deep", 1, 1, true, 0, 3_000_000)),
            cand(song_album("d2", "A", "deep", 1, 2, true, 0, 3_000_000)),
            cand(song_album("s1", "B", "single", 1, 1, false, 0, 3_000_000)),
        ];
        let input = PipelineInput::default().with_pool(SourceKind::Library, None, pool);
        let base = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            unit: Unit::Album,
            ordering: vec![OrderingKey::Favorite],
            budget: BudgetStage {
                max_bytes: Some(50_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let with_promo = AutoFillPipeline {
            promotion: PromotionStage {
                promote_album_min_favorites: Some(2),
                ..Default::default()
            },
            ..base.clone()
        };
        assert_eq!(
            ids(&run_pipeline(&input, &base)),
            ids(&run_pipeline(&input, &with_promo)),
            "promotion is inert when the base unit is already atomic (Album)"
        );

        // None ⇒ track grouping equals a default pipeline.
        let track_base = AutoFillPipeline {
            unit: Unit::Track,
            ..base
        };
        let track_none = AutoFillPipeline {
            promotion: PromotionStage::default(),
            ..track_base.clone()
        };
        assert_eq!(
            ids(&run_pipeline(&input, &track_base)),
            ids(&run_pipeline(&input, &track_none))
        );
    }

    // ---- #8 Album/track space ratio -----------------------------------------------------

    #[test]
    fn promotion_album_ratio_fills_complete_albums_first_then_tracks() {
        // Reserve half the ceiling for COMPLETE albums (atomic). "fits" (2 tracks ×2MB = 4MB) precedes
        // the oversized "big" (3 tracks ×2MB = 6MB). Reserve = 4MB ⇒ the album pass takes "fits" whole;
        // "big" (6MB) can't fit the reserve and must NOT partially leak. The base Track pass then fills
        // the remaining 4MB with loose singletons.
        let pool = vec![
            cand(song_album("f1", "A", "fits", 1, 1, true, 0, 2_000_000)),
            cand(song_album("f2", "A", "fits", 1, 2, true, 0, 2_000_000)),
            cand(song_album("b1", "B", "big", 1, 1, true, 0, 2_000_000)),
            cand(song_album("b2", "B", "big", 1, 2, true, 0, 2_000_000)),
            cand(song_album("b3", "B", "big", 1, 3, true, 0, 2_000_000)),
        ];
        let input = PipelineInput::default().with_pool(SourceKind::Library, None, pool);
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            ordering: vec![OrderingKey::Favorite], // all favorited ⇒ pool order preserved
            budget: BudgetStage {
                max_bytes: Some(8_000_000),
                ..Default::default()
            },
            promotion: PromotionStage {
                album_track_ratio: Some(0.5),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut result = ids(&run_pipeline(&input, &pipeline));
        result.sort();
        // Assert the EXACT selected set + total bytes, not a loose upper bound. The 4MB album reserve is
        // filled to the byte by the complete "fits" album (atomic) — leaving no room for "big", which is
        // a 6MB atomic unit the reserve cannot admit. The base Track pass then spends the remaining 4MB
        // on the first two loose "big" singletons (b1, b2); b3 overflows the 8MB ceiling. (Reserve-pass
        // atomicity itself — a unit that doesn't fit takes nothing — is the Selector's guarantee, covered
        // by its own atomic-unit tests; here we pin the end-to-end album-ratio composition exactly.)
        assert_eq!(
            result,
            vec!["b1", "b2", "f1", "f2"],
            "complete album fills the reserve first, then loose tracks fill the remainder to the ceiling"
        );
        let total: u64 = run_pipeline(&input, &pipeline)
            .iter()
            .map(|i| i.size_bytes)
            .sum();
        assert_eq!(
            total, 8_000_000,
            "fills the ceiling exactly (4MB album reserve + 4MB loose tracks)"
        );
    }

    #[test]
    fn promotion_album_ratio_zero_is_unchanged() {
        let pool = vec![
            cand(song_album("a1", "A", "alb", 1, 1, true, 0, 2_000_000)),
            cand(song_album("a2", "A", "alb", 1, 2, false, 0, 2_000_000)),
            cand(song_album("s1", "B", "sng", 1, 1, true, 0, 2_000_000)),
        ];
        let input = PipelineInput::default().with_pool(SourceKind::Library, None, pool);
        let base = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            ordering: vec![OrderingKey::Favorite],
            budget: BudgetStage {
                max_bytes: Some(20_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let ratio_zero = AutoFillPipeline {
            promotion: PromotionStage {
                album_track_ratio: Some(0.0),
                ..Default::default()
            },
            ..base.clone()
        };
        assert_eq!(
            ids(&run_pipeline(&input, &base)),
            ids(&run_pipeline(&input, &ratio_zero)),
            "ratio 0 ⇒ no album reserve, byte-identical to today"
        );
    }

    // ---- #33 Artist Spotlight -----------------------------------------------------------

    fn spotlight_pool() -> Vec<Candidate> {
        // Artist X: one big hit + four barely-played tracks. Artist Y: four medium-play tracks.
        let mut v = vec![cand(song_album(
            "x_hit", "X", "x_alb", 1, 1, false, 100, 2_000_000,
        ))];
        for i in 1..=4 {
            v.push(cand(song_album(
                &format!("x{i}"),
                "X",
                "x_alb",
                1,
                1 + i,
                false,
                1,
                2_000_000,
            )));
        }
        for i in 1..=4 {
            v.push(cand(song_album(
                &format!("y{i}"),
                "Y",
                "y_alb",
                1,
                i,
                false,
                50,
                2_000_000,
            )));
        }
        v
    }

    #[test]
    fn promotion_spotlight_fills_featured_artist_in_depth() {
        // Ordering [PlayCount]: without spotlight, X gets ONLY its one hit (the rest of X is buried
        // behind Y's mediums). With spotlight (featured = X, owner of the best-ranked candidate x_hit),
        // a 0.6 reserve fills X in depth first ⇒ X gets ≥ 3 tracks. Behavior emerges purely from config.
        let input = PipelineInput::default().with_pool(SourceKind::Library, None, spotlight_pool());
        let no_spot = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            ordering: vec![OrderingKey::PlayCount],
            budget: BudgetStage {
                max_bytes: Some(10_000_000),
                ..Default::default()
            }, // 5 tracks
            ..Default::default()
        };
        let with_spot = AutoFillPipeline {
            promotion: PromotionStage {
                spotlight: true,
                spotlight_share: Some(0.6),
                ..Default::default()
            },
            ..no_spot.clone()
        };
        let base = ids(&run_pipeline(&input, &no_spot));
        let spot = ids(&run_pipeline(&input, &with_spot));
        let x_base = base.iter().filter(|id| id.starts_with('x')).count();
        let x_spot = spot.iter().filter(|id| id.starts_with('x')).count();
        assert_eq!(
            x_base, 1,
            "without spotlight, X is represented only by its hit"
        );
        assert!(
            x_spot >= 3,
            "spotlight fills the featured artist X in depth (got {x_spot})"
        );
        assert!(
            spot.contains(&"x_hit".to_string()),
            "the hit is still picked"
        );
    }

    #[test]
    fn promotion_spotlight_seed_varies_featured_artist() {
        // Ordering [Random]: the featured artist rides the existing seed. Two single-track artists, a
        // share of 1.0, and a ceiling fitting one track ⇒ the lone result IS the featured artist's track.
        // Across seeds, BOTH artists must win for some seed — proving seed-driven variation, no clock/RNG.
        let pool = vec![
            cand(song_album("p", "P", "p_alb", 1, 1, false, 0, 2_000_000)),
            cand(song_album("q", "Q", "q_alb", 1, 1, false, 0, 2_000_000)),
        ];
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            ordering: vec![OrderingKey::Random],
            budget: BudgetStage {
                max_bytes: Some(2_000_000),
                ..Default::default()
            }, // exactly 1 track
            promotion: PromotionStage {
                spotlight: true,
                spotlight_share: Some(1.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut winners = std::collections::HashSet::new();
        for seed in 0..64u64 {
            let input = PipelineInput {
                seed,
                ..Default::default()
            }
            .with_pool(SourceKind::Library, None, pool.clone());
            let r = ids(&run_pipeline(&input, &pipeline));
            assert_eq!(r.len(), 1);
            winners.insert(r[0].clone());
        }
        assert_eq!(
            winners.len(),
            2,
            "the featured artist (hence the picked track) varies by seed"
        );
    }

    #[test]
    fn promotion_spotlight_underfilled_reserve_spills_over() {
        // Featured artist X has only one 2MB track — far less than the 4MB reserve. The unused reserve
        // must spill to the primary pass (no wasted budget): the full 8MB ceiling is used and Y fills in.
        let mut pool = vec![cand(song_album(
            "x1", "X", "x_alb", 1, 1, true, 0, 2_000_000,
        ))];
        for i in 1..=3 {
            pool.push(cand(song_album(
                &format!("y{i}"),
                "Y",
                "y_alb",
                1,
                i,
                false,
                0,
                2_000_000,
            )));
        }
        let input = PipelineInput::default().with_pool(SourceKind::Library, None, pool);
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            ordering: vec![OrderingKey::Favorite], // X's fav track ranks first ⇒ X is featured
            budget: BudgetStage {
                max_bytes: Some(8_000_000),
                ..Default::default()
            },
            promotion: PromotionStage {
                spotlight: true,
                spotlight_share: Some(0.5),
                ..Default::default()
            },
            ..Default::default()
        };
        let items = run_pipeline(&input, &pipeline);
        let total: u64 = items.iter().map(|i| i.size_bytes).sum();
        assert_eq!(
            total, 8_000_000,
            "the under-filled reserve spills to the primary pass; no wasted budget"
        );
        assert_eq!(items.len(), 4, "x1 + all three y tracks");
    }

    #[test]
    fn promotion_spotlight_inert_and_unbounded_are_byte_identical() {
        let input = PipelineInput::default().with_pool(SourceKind::Library, None, spotlight_pool());
        let no_spot = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            ordering: vec![OrderingKey::PlayCount],
            budget: BudgetStage {
                max_bytes: Some(10_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        // spotlight:false ⇒ byte-identical to no-spotlight.
        let off = AutoFillPipeline {
            promotion: PromotionStage {
                spotlight: false,
                spotlight_share: Some(0.6),
                ..Default::default()
            },
            ..no_spot.clone()
        };
        assert_eq!(
            ids(&run_pipeline(&input, &no_spot)),
            ids(&run_pipeline(&input, &off))
        );

        // Unbounded ceiling (no max_bytes) ⇒ the spotlight reserve is a no-op (can't reserve a fraction
        // of infinity) ⇒ byte-identical to the same unbounded pipeline without spotlight.
        let unbounded = AutoFillPipeline {
            budget: BudgetStage::default(),
            ..no_spot.clone()
        };
        let unbounded_spot = AutoFillPipeline {
            promotion: PromotionStage {
                spotlight: true,
                spotlight_share: Some(0.6),
                ..Default::default()
            },
            ..unbounded.clone()
        };
        assert_eq!(
            ids(&run_pipeline(&input, &unbounded)),
            ids(&run_pipeline(&input, &unbounded_spot)),
            "an unbounded ceiling makes the spotlight reserve inert"
        );
    }

    // ---- #27 Coherence ordering ---------------------------------------------------------

    #[test]
    fn promotion_coherence_reorders_without_changing_selection() {
        // Interleaved selection order; coherence clusters artist→album→disc→track but selects the SAME
        // ids and the SAME total bytes. Empty ordering ⇒ pool order is the selection order.
        let pool = vec![
            cand(song_album("a_b", "A", "A1", 1, 2, false, 0, 2_000_000)),
            cand(song_album("b_1", "B", "B1", 1, 1, false, 0, 2_000_000)),
            cand(song_album("a_a", "A", "A1", 1, 1, false, 0, 2_000_000)),
            cand(song_album("a2_1", "A", "A2", 1, 1, false, 0, 2_000_000)),
        ];
        let input = PipelineInput::default().with_pool(SourceKind::Library, None, pool);
        let base = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            ordering: vec![],
            budget: BudgetStage {
                max_bytes: Some(100_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let coherent = AutoFillPipeline {
            promotion: PromotionStage {
                coherence: true,
                ..Default::default()
            },
            ..base.clone()
        };
        let plain = run_pipeline(&input, &base);
        let clustered = run_pipeline(&input, &coherent);

        // Same selection: identical id-set and identical total bytes (the whole safety guarantee).
        let mut plain_ids = ids(&plain);
        let mut clustered_ids = ids(&clustered);
        plain_ids.sort();
        clustered_ids.sort();
        assert_eq!(
            plain_ids, clustered_ids,
            "coherence never changes WHICH tracks are selected"
        );
        let plain_bytes: u64 = plain.iter().map(|i| i.size_bytes).sum();
        let clustered_bytes: u64 = clustered.iter().map(|i| i.size_bytes).sum();
        assert_eq!(plain_bytes, clustered_bytes, "total bytes unchanged");

        // Order clusters by artist (A first-seen) → album (A1 before A2) → disc → track.
        assert_eq!(ids(&clustered), vec!["a_a", "a_b", "a2_1", "b_1"]);
        // Plain order is the raw selection order (un-clustered) — proving the reorder did something.
        assert_eq!(ids(&plain), vec!["a_b", "b_1", "a_a", "a2_1"]);

        // Deterministic: a second run is identical.
        assert_eq!(ids(&run_pipeline(&input, &coherent)), ids(&clustered));
    }

    // ---- Combined reserves & serde ------------------------------------------------------

    #[test]
    fn promotion_combined_reserves_respect_ceiling_no_double_count() {
        // Spotlight + album-ratio reserves together must never exceed the global ceiling and must never
        // emit a duplicate id (the shared Selector dedups across all passes).
        let pool = vec![
            cand(song_album("x1", "X", "x_alb", 1, 1, true, 5, 2_000_000)),
            cand(song_album("x2", "X", "x_alb", 1, 2, true, 5, 2_000_000)),
            cand(song_album("y1", "Y", "y_alb", 1, 1, true, 3, 2_000_000)),
            cand(song_album("y2", "Y", "y_alb", 1, 2, true, 3, 2_000_000)),
            cand(song_album("z1", "Z", "z_alb", 1, 1, false, 0, 2_000_000)),
        ];
        let input = PipelineInput::default().with_pool(SourceKind::Library, None, pool);
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            ordering: vec![OrderingKey::PlayCount],
            budget: BudgetStage {
                max_bytes: Some(8_000_000),
                ..Default::default()
            },
            promotion: PromotionStage {
                spotlight: true,
                spotlight_share: Some(0.4),
                album_track_ratio: Some(0.4),
                ..Default::default()
            },
            ..Default::default()
        };
        let items = run_pipeline(&input, &pipeline);
        let total: u64 = items.iter().map(|i| i.size_bytes).sum();
        assert!(
            total <= 8_000_000,
            "combined reserves never exceed the ceiling (got {total})"
        );
        let unique: std::collections::HashSet<_> = items.iter().map(|i| &i.id).collect();
        assert_eq!(
            unique.len(),
            items.len(),
            "no id is emitted twice across the reserve passes"
        );
    }

    #[test]
    fn promotion_serde_round_trips_and_default_omits() {
        // All four fields round-trip through camelCase serde.
        let mut p = AutoFillPipeline::default_legacy(Some(1000));
        p.promotion = PromotionStage {
            spotlight: true,
            spotlight_share: Some(0.4),
            album_track_ratio: Some(0.3),
            promote_album_min_favorites: Some(2),
            coherence: true,
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"spotlightShare\""));
        assert!(json.contains("\"albumTrackRatio\""));
        assert!(json.contains("\"promoteAlbumMinFavorites\""));
        let back: AutoFillPipeline = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back, "promotion stage round-trips byte-identically");

        // A default PromotionStage round-trips and the Option fields are omitted.
        let d = PromotionStage::default();
        let dj = serde_json::to_string(&d).unwrap();
        assert!(
            !dj.contains("spotlightShare"),
            "None options are omitted: {dj}"
        );
        assert_eq!(d, serde_json::from_str::<PromotionStage>(&dj).unwrap());

        // A missing promotion block degrades to default (parse tolerance).
        let none: AutoFillPipeline = serde_json::from_str("{}").unwrap();
        assert_eq!(none.promotion, PromotionStage::default());

        // Out-of-range floats are tolerated at parse (clamped only at consumption); a malformed-but-typed
        // block still deserializes rather than aborting.
        let wide: AutoFillPipeline =
            serde_json::from_str(r#"{"promotion":{"spotlight":true,"spotlightShare":2.0}}"#)
                .unwrap();
        assert_eq!(wide.promotion.spotlight_share, Some(2.0));
    }
}
