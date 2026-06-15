// Story 12.6: the TypeScript mirror of the daemon `AutoFillPipeline` serde contract
// (hifimule-daemon/src/auto_fill/pipeline.rs). The JSON these types produce MUST match the
// daemon shape byte-for-byte — notably the playlist id field is `ref` (not refId), `share` is
// omitted when unset, and every enum value is lowercase camelCase.

export type SourceKind = 'library' | 'favorites' | 'history' | 'playlist';
export type OrderingKey = 'favorite' | 'playCount' | 'dateCreated' | 'random' | 'quality' | 'excavation' | 'rediscovery' | 'rarity';
export type Unit = 'track' | 'album' | 'artist';
/** Recording-version traits the auto-fill engine detects from title/album text (Story 13.2 #34).
 * Mirrors the daemon `VersionTrait` enum (camelCase serde). */
export type VersionTrait = 'studio' | 'live' | 'remastered' | 'remix' | 'acoustic' | 'demo';

export interface FilterStage {
    includeTags: string[];
    excludeTags: string[];
    includeGenres: string[];
    excludeGenres: string[];
}

export interface SourceEntry {
    kind: SourceKind;
    /** Playlist id for `kind: 'playlist'`. Serializes as `ref`. Omitted when unset. */
    ref?: string;
    /** Blend weight in 0.0..=1.0. Omitted when unset (engine splits remainder equally). */
    share?: number;
}

/** A rotation-tier definition (Story 13.1, #25/#26): a playlist-backed tier or the whole library.
 * Mirrors the daemon `TierDef` serde shape (`{ kind: 'playlist', ref } | { kind: 'library' }`). */
export type TierDef = { kind: 'playlist'; ref: string } | { kind: 'library' };

export interface MemoryStage {
    cooldownWeeks?: number;
    playedExclusion?: boolean;
    /** Fraction (0.0–1.0) of the budget kept stable across syncs (Story 13.1 #24). */
    stableCorePct?: number;
    /** How tolerant of repeats the cooldown is (0.0 strict … 1.0 off) (Story 13.1 #23). */
    repeatTolerance?: number;
    /** Ordered playlist-backed rotation tiers (Story 13.1 #25/#26). */
    tiers?: TierDef[];
}

export interface BudgetStage {
    maxBytes?: number;
    targetDurationSecs?: number;
    headroomBytes?: number;
    /** Story 13.5 #20 (encoding-from-goals): derive the transcode bitrate from the size + duration
     * goals. Needs both `maxBytes` and a positive `targetDurationSecs`. Omitted when false. */
    encodingFromGoals?: boolean;
}

/** Quality & version modifiers (Story 13.2). Mirrors the daemon `QualityStage` serde shape;
 * defaults (`bestVersion: false`, empty `versionPreference`) are today's behavior. */
export interface QualityStage {
    /** Collapse same-logical-song duplicates to a single best version globally (#11). */
    bestVersion?: boolean;
    /** Ordered version-trait preference (earlier = more preferred) (#34). Empty = no preference. */
    versionPreference?: VersionTrait[];
}

/** Weighted rarity draw (Story 13.4 #29) — loot-table classes feeding the `rarity` ordering key.
 * Mirrors the daemon `RarityStage` serde shape; default (`enabled: false`) is today's behavior. */
export interface RarityStage {
    /** Off ⇒ a `rarity` ordering key degrades to a uniform shuffle. */
    enabled?: boolean;
    /** Draw weight for the legendary class (never-/0-played gems). */
    legendaryWeight?: number;
    /** Draw weight for the rare class (1..=rareMaxPlays). */
    rareWeight?: number;
    /** Draw weight for the common class (> rareMaxPlays — the hits). */
    commonWeight?: number;
    /** Play-count boundary between rare and common. */
    rareMaxPlays?: number;
}

/** Pity timer (Story 13.4 #30) — a deterministic discovery guarantee after a dry streak. Mirrors
 * the daemon `PityStage` serde shape; default (`enabled: false`) is today's behavior. The dry-streak
 * counter is machine-local daemon DB state, never part of this (manifest) config. */
export interface PityStage {
    /** Off ⇒ no reserve, no counter interaction. */
    enabled?: boolean;
    /** Dry syncs before the guarantee fires. */
    thresholdSyncs?: number;
    /** Fraction of the budget reserved for discovery when it fires (0.0..=1.0). */
    guaranteedRatio?: number;
    /** A "discovery" candidate has playCount <= this (0 = never-played). */
    discoveryMaxPlays?: number;
}

/** A context-rule time/calendar window (Story 13.5 #3/#17/#32). Mirrors the externally-tagged daemon
 * `ContextWindow` enum (camelCase): `timeOfDay` (hour window), `months`, or `dateRange` ((month,day)
 * tuples serialized as `[m, d]`). */
export type ContextWindow =
    | { timeOfDay: { startHour: number; endHour: number } }
    | { months: { months: number[] } }
    | { dateRange: { start: [number, number]; end: [number, number] } };

/** One context rule: a window + its effect (source activation/weighting + scheduled tag/genre filter).
 * Mirrors the daemon `ContextRule` serde shape; `weight` is omitted when unset. */
export interface ContextRule {
    window: ContextWindow;
    /** Source `ref`s this rule activates/boosts while active. */
    sourceRefs: string[];
    /** Optional share multiplier for the activated sources (energy-phase emphasis). */
    weight?: number;
    includeTags: string[];
    excludeTags: string[];
    includeGenres: string[];
    excludeGenres: string[];
}

/** Clock-driven Context stage (Story 13.5 #3 time-of-day / #17 energy-curve / #32 seasonal). Mirrors
 * the daemon `ContextStage` serde shape; default (`enabled: false`, no rules) is today's behavior. */
export interface ContextStage {
    /** Off ⇒ no rule is ever consulted (zero behavior change). */
    enabled?: boolean;
    /** Context rules, evaluated in order against the caller-supplied local civil time. */
    rules?: ContextRule[];
}

export interface AutoFillPipeline {
    enabled: boolean;
    filter: FilterStage;
    sources: SourceEntry[];
    unit: Unit;
    ordering: OrderingKey[];
    memory: MemoryStage;
    budget: BudgetStage;
    fallback: SourceEntry[];
    quality: QualityStage;
    rarity: RarityStage;
    pity: PityStage;
    context: ContextStage;
}

/** The context-window kinds the rule editor offers. */
export type ContextWindowKind = 'timeOfDay' | 'months' | 'dateRange';

/** The user-facing ordering keys. Story 13.3 adds the discovery keys `excavation` (deep cuts) and
 * `rediscovery` (added long ago). Story 13.4 surfaces `random` (now a functional seeded shuffle —
 * previously a hidden no-op) and adds `rarity` (the weighted loot-table draw). */
export const ORDERING_KEYS: OrderingKey[] = ['favorite', 'playCount', 'dateCreated', 'quality', 'excavation', 'rediscovery', 'random', 'rarity'];

/** The selectable version traits, in the order the preference editor offers them. */
export const VERSION_TRAITS: VersionTrait[] = ['studio', 'live', 'remastered', 'remix', 'acoustic', 'demo'];

export function emptyFilter(): FilterStage {
    return { includeTags: [], excludeTags: [], includeGenres: [], excludeGenres: [] };
}

/** The default-legacy pipeline — identical fill behavior to the pre-12.6 single toggle+slider
 * (favorites → play count → creation date over the library). Mirrors
 * `AutoFillPipeline::default_legacy` so a "Default" UI state round-trips with zero behavior change. */
export function defaultLegacyPipeline(maxBytes?: number): AutoFillPipeline {
    return {
        enabled: true,
        filter: emptyFilter(),
        sources: [{ kind: 'library' }],
        unit: 'track',
        ordering: ['favorite', 'playCount', 'dateCreated'],
        memory: { playedExclusion: false },
        budget: maxBytes != null ? { maxBytes } : {},
        fallback: [],
        quality: {},
        rarity: {},
        pity: {},
        context: {},
    };
}

/** Normalizes a (possibly partial) pipeline read from the daemon into a fully-populated object so
 * the editor never dereferences undefined. Reserved Memory fields are carried through verbatim. */
export function normalizePipeline(raw: Partial<AutoFillPipeline> | null | undefined): AutoFillPipeline {
    const base = defaultLegacyPipeline();
    if (!raw) return base;
    return {
        enabled: raw.enabled ?? false,
        filter: { ...emptyFilter(), ...(raw.filter ?? {}) },
        sources: Array.isArray(raw.sources) && raw.sources.length > 0
            ? raw.sources.map((s) => ({ ...s }))
            : [{ kind: 'library' }],
        unit: raw.unit ?? 'track',
        ordering: Array.isArray(raw.ordering) && raw.ordering.length > 0
            ? [...raw.ordering]
            : ['favorite', 'playCount', 'dateCreated'],
        memory: { ...(raw.memory ?? {}) },
        budget: { ...(raw.budget ?? {}) },
        fallback: Array.isArray(raw.fallback) ? raw.fallback.map((s) => ({ ...s })) : [],
        quality: {
            ...(raw.quality ?? {}),
            versionPreference: Array.isArray(raw.quality?.versionPreference)
                ? [...raw.quality.versionPreference]
                : [],
        },
        rarity: { ...(raw.rarity ?? {}) },
        pity: { ...(raw.pity ?? {}) },
        context: {
            ...(raw.context ?? {}),
            rules: Array.isArray(raw.context?.rules) ? raw.context.rules.map((r) => ({ ...r })) : [],
        },
    };
}

/** Strips `undefined`/empty optionals so the produced JSON matches the daemon serde shape (which
 * omits `ref`/`share`/budget keys when unset). Reserved Memory fields survive when present. */
export function serializePipeline(p: AutoFillPipeline): AutoFillPipeline {
    const cleanSources = (list: SourceEntry[]): SourceEntry[] =>
        list.map((s) => {
            const out: SourceEntry = { kind: s.kind };
            if (s.kind === 'playlist' && s.ref) out.ref = s.ref;
            if (list.length > 1 && typeof s.share === 'number') {
                out.share = Math.max(0, Math.min(1, s.share));
            }
            return out;
        });
    const memory: MemoryStage = {};
    if (typeof p.memory.cooldownWeeks === 'number') memory.cooldownWeeks = p.memory.cooldownWeeks;
    if (p.memory.playedExclusion) memory.playedExclusion = true;
    // Story 13.1 Memory fields — emit only when meaningful so a default pipeline round-trips clean.
    if (typeof p.memory.stableCorePct === 'number' && p.memory.stableCorePct > 0) {
        memory.stableCorePct = Math.max(0, Math.min(1, p.memory.stableCorePct));
    }
    if (typeof p.memory.repeatTolerance === 'number' && p.memory.repeatTolerance > 0) {
        memory.repeatTolerance = Math.max(0, Math.min(1, p.memory.repeatTolerance));
    }
    if (Array.isArray(p.memory.tiers)) {
        const tiers = p.memory.tiers.filter((tier) => tier.kind !== 'playlist' || !!tier.ref);
        if (tiers.length > 0) memory.tiers = tiers;
    }
    const budget: BudgetStage = {};
    if (typeof p.budget.maxBytes === 'number') budget.maxBytes = p.budget.maxBytes;
    if (typeof p.budget.targetDurationSecs === 'number' && p.budget.targetDurationSecs > 0) {
        budget.targetDurationSecs = p.budget.targetDurationSecs;
    }
    if (typeof p.budget.headroomBytes === 'number' && p.budget.headroomBytes > 0) {
        budget.headroomBytes = p.budget.headroomBytes;
    }
    // Story 13.5 #20 — encoding-from-goals: emit only when enabled (omit-when-default).
    if (p.budget.encodingFromGoals) budget.encodingFromGoals = true;
    // Story 13.2 Quality stage — emit only meaningful fields (mirrors the Memory-fields pattern) so a
    // default pipeline round-trips clean and stays backward-compatible.
    const quality: QualityStage = {};
    if (p.quality?.bestVersion) quality.bestVersion = true;
    if (Array.isArray(p.quality?.versionPreference)) {
        // De-duplicate, preserving order (first occurrence wins) — mirrors the engine's parse.
        const prefs = p.quality.versionPreference.filter(
            (trait, i, arr) => arr.indexOf(trait) === i,
        );
        if (prefs.length > 0) quality.versionPreference = prefs;
    }
    // Story 13.4 Rarity/Pity stages — omit-when-default (same pattern as quality): a disabled stage
    // emits `{}`, which the daemon deserializes to the default stage so routing keeps the fast path.
    const rarity: RarityStage = {};
    if (p.rarity?.enabled) {
        rarity.enabled = true;
        if (typeof p.rarity.legendaryWeight === 'number') rarity.legendaryWeight = Math.max(0, p.rarity.legendaryWeight);
        if (typeof p.rarity.rareWeight === 'number') rarity.rareWeight = Math.max(0, p.rarity.rareWeight);
        if (typeof p.rarity.commonWeight === 'number') rarity.commonWeight = Math.max(0, p.rarity.commonWeight);
        if (typeof p.rarity.rareMaxPlays === 'number') rarity.rareMaxPlays = Math.max(0, Math.floor(p.rarity.rareMaxPlays));
    }
    const pity: PityStage = {};
    if (p.pity?.enabled) {
        pity.enabled = true;
        if (typeof p.pity.thresholdSyncs === 'number') pity.thresholdSyncs = Math.max(0, Math.floor(p.pity.thresholdSyncs));
        if (typeof p.pity.guaranteedRatio === 'number') pity.guaranteedRatio = Math.max(0, Math.min(1, p.pity.guaranteedRatio));
        if (typeof p.pity.discoveryMaxPlays === 'number') pity.discoveryMaxPlays = Math.max(0, Math.floor(p.pity.discoveryMaxPlays));
    }
    // Story 13.5 Context stage — omit-when-default (same pattern). A disabled stage emits `{}`, which the
    // daemon reads as the default stage so routing keeps the fast path. Rules' empty arrays are emitted
    // verbatim to match the daemon serde (which has no skip on the Vec fields); `weight` is omitted when
    // unset. The window is passed through (the editor only ever builds well-formed windows).
    const context: ContextStage = {};
    if (p.context?.enabled) {
        context.enabled = true;
        context.rules = (Array.isArray(p.context.rules) ? p.context.rules : []).map((r) => {
            const out: ContextRule = {
                window: r.window,
                sourceRefs: Array.isArray(r.sourceRefs) ? r.sourceRefs.filter((s) => !!s) : [],
                includeTags: Array.isArray(r.includeTags) ? r.includeTags.filter((t) => !!t) : [],
                excludeTags: Array.isArray(r.excludeTags) ? r.excludeTags.filter((t) => !!t) : [],
                includeGenres: Array.isArray(r.includeGenres) ? r.includeGenres.filter((g) => !!g) : [],
                excludeGenres: Array.isArray(r.excludeGenres) ? r.excludeGenres.filter((g) => !!g) : [],
            };
            if (typeof r.weight === 'number') out.weight = Math.max(0, r.weight);
            return out;
        });
    }
    return {
        enabled: p.enabled,
        filter: {
            includeTags: p.filter.includeTags ?? [],
            excludeTags: p.filter.excludeTags ?? [],
            includeGenres: p.filter.includeGenres ?? [],
            excludeGenres: p.filter.excludeGenres ?? [],
        },
        sources: cleanSources(p.sources),
        unit: p.unit,
        ordering: [...p.ordering],
        memory,
        budget,
        fallback: cleanSources(p.fallback),
        quality,
        rarity,
        pity,
        context,
    };
}
