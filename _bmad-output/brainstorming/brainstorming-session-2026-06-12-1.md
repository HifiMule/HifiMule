---
stepsCompleted: [1, 2, 3, 4]
ideas_count: 36
technique_execution_complete: true
session_active: false
workflow_completed: true
inputDocuments: []
session_topic: 'Auto-fill for device: track selection strategies users would want'
session_goals: 'Broad catalog of candidate selection strategies for device auto-fill'
selected_approach: 'ai-recommended'
techniques_used: ['Role Playing', 'Cross-Pollination', 'Morphological Analysis']
ideas_generated: []
context_file: ''
---

# Brainstorming Session Results

**Facilitator:** Alexis
**Date:** 2026-06-12

## Session Overview

**Topic:** What users would like when activating auto-fill for a device in HifiMule — the strategies that decide which tracks get selected to fill the device. Users should be able to configure how tracks are selected.

**Goals:** Generate a broad catalog of candidate track-selection strategies (diverge wide; organization comes later).

### Session Setup

_Session initiated from user prompt about configurable auto-fill strategies. Focus confirmed: device auto-fill, strategy catalog as primary outcome._

## Technique Selection

**Approach:** AI-Recommended Techniques
**Analysis Context:** Device auto-fill track selection, with focus on a broad catalog of candidate strategies.

**Recommended Techniques:**

- **Role Playing:** Embody distinct listener personas to surface user-grounded strategies (commuter, audiophile, gym-goer, parent, explorer…).
- **Cross-Pollination:** Steal selection patterns from other domains (radio DJs, tasting menus, capsule wardrobes, museum rotations…) for non-obvious strategies.
- **Morphological Analysis:** Map the strategy parameter space (seed × scoring × diversity × refresh × budget) and fill gaps systematically.

**AI Rationale:** The goal needs empathy (what users want), divergence (novel strategies), and systematic coverage (complete catalog) — one technique per need, sequenced from grounded to exhaustive.

## Technique Execution Results

**Role Playing (4 personas):**

- **Interactive Focus:** Claire (commuter, 8GB, hates repeats), Antoine (audiophile, 512GB DAP, quality-first), Léo (gym-goer, tiny device, energy-driven), Nadia (parent filling a kid's player).
- **Key Breakthroughs:** Source × Strategy separation; per-device profiles absorb "context" and "listener" concepts; playlists as the universal cheap proxy; repeat-tolerance as a bidirectional axis.
- **User Creative Strengths:** Systems thinking — repeatedly turned individual wishes into composable architecture; pragmatic feasibility instincts (scrobbling availability, encoding constraints).

**Cross-Pollination (4 raids):**

- **Building on Previous:** Radio DJ rotation tiers grounded into playlist-backed tiers; capsule wardrobe coherence resolved into tag pre-filtering; video game loot tables added rarity draws and the pity timer; photo apps/seasons added time-based strategies (flagged high-effort, with cheap tag-based versions).
- **New Insights:** Nearly every "smart" strategy has a playlist/tag-powered version at ~10% of the cost → catalog should present ambition tiers per strategy.

**Morphological Analysis:**

- **Parameter grid:** Filter → Source → Unit → Ordering → Memory → Budget → Context (pipeline emerged organically from the ideas).
- **Gap hunting yielded:** Artist Spotlight (unit=artist), Version Preference modifier, Headroom Reserve, Fallback Chain. Triggers deliberately kept simple (manual/on-connect); negative feedback (skip demotion) consciously cut.

### Complete Idea Inventory (36 ideas)

**Mix Strategies**
1. **Familiar/Discovery Ratio** — slider between known favorites and never-heard tracks.
2. **Multi-Style Ratio Blend** — several genres/styles, each with a share of the fill.
27. **Coherence-Optimized Fill** — optimize the set for flow, not tracks individually (resolved into #28 scoping).
29. **Weighted Rarity Draws** — common/rare/legendary classes give each sync texture (loot-table draw).

**Context Strategies**
3. **Time-of-Day Aware Fill** — energetic morning slots, calm evening slots; zones on the device.
17. **Energy-Curve Fill** — session-shaped selection (warm-up → plateau → cool-down) via BPM/energy or playlist proxy.
32. **Seasonal Drift** — calendar-following fills (high effort; cheap version = scheduled tag filter).

**Rotation / Memory Strategies**
4. **Sync Cooldown** — synced tracks ineligible for N weeks; works without play data.
5. **Played-Track Exclusion** — scrobble-aware exclusion of actually-played tracks.
6. **Capability-Adaptive Freshness** — user sets intent ("keep it fresh"); mechanism adapts to device capability.
23. **Repeat-Tolerance Axis** — one dial from "never repeat" to "pin beloved tracks forever".
24. **Stable-Core + Delta Fill** — keep X% of previous fill, refresh the rest; the device evolves.
25. **Rotation Tiers** — heavy/medium/gold/spice strata cycling at different speeds.
26. **Playlist-Backed Tiers** — tiers as roles assigned to existing playlists (share + refresh frequency).

**Granularity / Unit Strategies**
7. **Whole-Album Integrity Fill** — album as the selection unit.
8. **Album/Track Space Ratio** — budget split between complete albums and loose tracks.
9. **Affinity-Triggered Album Promotion** — albums qualify for full sync only with enough favorited/rated tracks.
33. **Artist Spotlight** — one featured artist per fill, in depth.

**Quality Strategies**
11. **Best-Version Resolution** — duplicates resolved to one logical track with a quality ladder.
13. **Quality-Ordering Modifier** — global combinable axis: prefer-high / prefer-low / ignore.
34. **Version Preference Modifier** — studio/live/remix/original editorial preference.

**Curation / Discovery Strategies**
14. **Deep-Cuts Excavator** — fill with owned-but-barely-played music.
15. **Community-Rating Fallback** — external scores (ListenBrainz/Last.fm/Discogs…) for unrated items.
16. **Acclaimed-Classics Fill** — owned albums acclaimed by the community but never played.
30. **Pity Timer** — discovery ratio self-adjusts from behavior (guaranteed finds after dry spells).
31. **Musical Memories Fill** — "you loved this in June 2019" (high effort; cheap version = date-added by past season).

**Source Strategies**
18. **Playlist-Seeded Pools** — any playlist as a fill source; user curation as the intent model.
28. **Genre/Tag Pre-Filter** — include/exclude scope applied before any strategy.

**Budget Strategies**
21. **Duration-Targeted Fill** — target in listening hours, not bytes.
35. **Headroom Reserve** — always leave X GB free.

**Meta / Architecture**
10. **Composable Strategy Blocks** — configuration is a stack of combinable rules, not a mode dropdown.
12. **Per-Device Strategy Profiles** — configuration lives on the device; the device is the context AND the listener.
19. **Source × Strategy Separation** — "drawing from what?" × "picking how?" as independent axes.
20. **Device Profile Editor with Encoding Policy** — encoding computed backwards from size/duration goals.
36. **Fallback Chain** — ordered strategies; last link guarantees a full device.

**Retired / Consciously Cut**
22. **Listener Profiles** — cut: the device IS the listener (one physical owner); absorbed by #12.
- **Smart refill triggers** — kept simple: manual + on-connect only.
- **Negative feedback (skip demotion)** — cut: not a user need here.

### Creative Facilitation Narrative

_The session's defining dynamic: Alexis consistently played the architect to the facilitator's explorer. Each persona produced wishes; Alexis turned wishes into composable mechanisms — and twice killed ideas (listener profiles, smart triggers) that complexity didn't justify. The breakthrough moment was the Source × Strategy separation emerging from Léo's "cheap proxy" insight, after which nearly every subsequent idea snapped into the grid. The session converged on a pipeline configuration model without ever aiming for one._

### Session Highlights

**User Creative Strengths:** Architecture-first thinking; feasibility radar; comfort killing ideas.
**Breakthrough Moments:** Source × Strategy grid; playlists as universal cheap proxy; ambition tiers (every smart strategy has a 10%-cost playlist version).
**Energy Flow:** Steady and pragmatic, with peaks on loot-table mechanics ("love this") and composability insights.

## Idea Organization and Prioritization

**Thematic Organization:** 36 ideas across 9 themes — Meta/Architecture (5), Rotation/Memory (7), Mix (4), Curation/Discovery (5), Granularity (4), Quality (3), Sources (2), Context (3), Budget (2) — plus 3 consciously-cut concepts.

**Emergent configuration model (the pipeline):**

> **Filter** (tag/genre include-exclude) → **Sources** (playlists/pools + shares) → **Unit** (track/album/artist) → **Ordering** (quality, ratings, excavation, rarity) → **Memory** (cooldown ↔ pinning, tiers, stable-core) → **Budget** (GB/hours, headroom, encoding) — configured **per device**, with a **fallback chain** guaranteeing every fill completes.

**Prioritization Results:**

- **Top Priority Ideas:**
  1. **Source × Strategy Separation (#19)** — also chosen as THE breakthrough bet. The skeleton: a fill configuration = ordered `(Source, Picker, share)` entries + global modifiers + budget.
  2. **Playlist-Backed Everything (#18/#26/#28)** — playlists as the universal intent layer; the natural first Source type.
  3. **Budget System (#20/#21/#35)** — headroom, duration targets, encoding-from-goals; the trust layer.
- **Quick Win Opportunities:** #4 Sync Cooldown, #18 Playlist Pools, #28 Tag Filter, #35 Headroom Reserve — all four are pipeline components, so quick wins double as foundation work.
- **Breakthrough Concepts:** Source × Strategy grid; playlists as universal cheap proxy (every smart strategy has a ~10%-cost playlist version → present strategies in ambition tiers); loot-table mechanics (#29 rarity draws, #30 pity timer) as the memorable-delight layer for later.

**Action Planning:**

**Priority 1 — Source × Strategy Separation:**
1. Design the domain model; validate it expresses all four personas (Claire/Antoine/Léo/Nadia) with no special cases.
2. Spike the pipeline as pure functions over the library (filter → pool → order → dedupe vs. memory → fit budget) — testable without UI.
3. Defer configuration UI until the model survives the persona test.
- *Risk:* over-abstraction — keep the algebra small. *Success:* four personas, one model.

**Priority 2 — Playlist-Backed Everything:**
1. Ship `PlaylistSource` + `TagFilter` as first pipeline components.
2. Add per-source shares (covers multi-style ratios).
3. Later: refresh-frequency attribute on sources (playlist-backed tiers).
- *Success:* "70% from 2 playlists, 30% random non-Christmas, no repeats within 3 weeks" configurable in under a minute.

**Priority 3 — Budget System:**
1. Headroom reserve first (trivial, high trust).
2. Duration-as-budget with bytes derived.
3. Encoding-from-goals in device profile editor (depends on transcode-on-sync).
- *Success:* no fill exceeds `capacity − reserve`; every fill reaches target via fallback chain.

## Session Summary and Insights

**Key Achievements:**

- 36 collaboratively developed ideas through 3 techniques (Role Playing × 4 personas, Cross-Pollination × 4 domains, Morphological Analysis with gap hunting).
- A complete configuration architecture (the pipeline) that emerged from the ideas rather than being imposed.
- 3 prioritized tracks with concrete action plans; quick wins identified that double as foundation work.
- 3 ideas consciously cut (listener profiles, smart triggers, skip-based demotion) — scope clarity as a deliverable.

**Session Reflections:**

- The most valuable pattern: every "smart" strategy revealed a playlist/tag-powered cheap version — design the catalog as ambition tiers.
- Device = listener (physical ownership) was a simplifying insight that removed an entire architectural layer.
- Recommended next step: feed this catalog into a product brief / PRD for the auto-fill feature (bmad-product-brief or bmad-prd).
