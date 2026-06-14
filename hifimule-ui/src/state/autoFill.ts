// Story 12.6: the TypeScript mirror of the daemon `AutoFillPipeline` serde contract
// (hifimule-daemon/src/auto_fill/pipeline.rs). The JSON these types produce MUST match the
// daemon shape byte-for-byte — notably the playlist id field is `ref` (not refId), `share` is
// omitted when unset, and every enum value is lowercase camelCase.

export type SourceKind = 'library' | 'favorites' | 'history' | 'playlist';
export type OrderingKey = 'favorite' | 'playCount' | 'dateCreated' | 'random' | 'quality';
export type Unit = 'track' | 'album' | 'artist';

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

export interface MemoryStage {
    cooldownWeeks?: number;
    playedExclusion?: boolean;
    // Reserved for Epic 13 — never surfaced as functional controls, persisted verbatim.
    stableCorePct?: number;
    repeatTolerance?: number;
    tiers?: unknown;
}

export interface BudgetStage {
    maxBytes?: number;
    targetDurationSecs?: number;
    headroomBytes?: number;
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
}

/** The user-facing ordering keys (the reserved `random` no-op is not surfaced). */
export const ORDERING_KEYS: OrderingKey[] = ['favorite', 'playCount', 'dateCreated', 'quality'];

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
    };
}

/** Strips `undefined`/empty optionals so the produced JSON matches the daemon serde shape (which
 * omits `ref`/`share`/budget keys when unset). Reserved Memory fields survive when present. */
export function serializePipeline(p: AutoFillPipeline): AutoFillPipeline {
    const cleanSources = (list: SourceEntry[]): SourceEntry[] =>
        list.map((s) => {
            const out: SourceEntry = { kind: s.kind };
            if (s.kind === 'playlist' && s.ref) out.ref = s.ref;
            if (typeof s.share === 'number') out.share = s.share;
            return out;
        });
    const memory: MemoryStage = {};
    if (typeof p.memory.cooldownWeeks === 'number') memory.cooldownWeeks = p.memory.cooldownWeeks;
    if (p.memory.playedExclusion) memory.playedExclusion = true;
    // Reserved Epic 13 fields — persist verbatim if a loaded pipeline carried them.
    if (p.memory.stableCorePct !== undefined) memory.stableCorePct = p.memory.stableCorePct;
    if (p.memory.repeatTolerance !== undefined) memory.repeatTolerance = p.memory.repeatTolerance;
    if (p.memory.tiers !== undefined) memory.tiers = p.memory.tiers;
    const budget: BudgetStage = {};
    if (typeof p.budget.maxBytes === 'number') budget.maxBytes = p.budget.maxBytes;
    if (typeof p.budget.targetDurationSecs === 'number' && p.budget.targetDurationSecs > 0) {
        budget.targetDurationSecs = p.budget.targetDurationSecs;
    }
    if (typeof p.budget.headroomBytes === 'number' && p.budget.headroomBytes > 0) {
        budget.headroomBytes = p.budget.headroomBytes;
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
    };
}
