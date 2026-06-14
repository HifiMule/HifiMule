# Sprint Change Proposal — Configurable Auto-Fill Pipeline & Multi-Server Auto-Fill

**Date:** 2026-06-14
**Author:** Alexis (with Developer agent)
**Status:** Proposed
**Scope classification:** MAJOR (new feature area, 2 new epics, PRD + Architecture + UX changes)
**Source:** Brainstorming session [brainstorming-session-2026-06-12-1.md](brainstorming-session-2026-06-12-1.md)

---

## Section 1 — Issue Summary

The 2026-06-12 brainstorming session ("Auto-fill for device: track selection strategies users would want") produced a catalog of 36 ideas that converged — without being imposed — on a single **pipeline configuration model** for auto-fill. The session's own recommended next step was to feed this catalog into a PRD. This proposal acts on that recommendation and addresses two concrete limitations in the currently shipped auto-fill feature:

1. **Fixed selection algorithm.** Auto-fill today expands a virtual slot via one hardcoded ranking — favorites → play count → creation date ([Story 3.6](../implementation-artifacts/3-6-auto-fill-sync-mode-synchronise-all.md), [Story 3.8](../implementation-artifacts/3-8-lazy-auto-fill-virtual-slot.md), [FR29](prd.md)). Users cannot configure *what* the fill draws from or *how* it picks.

2. **One-server-only limit.** There is exactly one `__auto_fill_slot__` in the basket, bound to `selectedServerId`. Enabling auto-fill on a second server **overwrites** the first server's slot. Multi-slot auto-fill (one per server) was explicitly **deferred** in the multi-server change ([sprint-change-proposal-2026-06-09-multi-server-management.md](sprint-change-proposal-2026-06-09-multi-server-management.md), line 382: *"Multi-slot auto-fill (one per server) is deferred to a future change."*).

**Evidence:** The brainstorm pipeline + prioritization tracks; the single-algorithm code path in `hifimule-daemon/src/auto_fill.rs`; the deferral note in the multi-server proposal; the per-device-profile conclusion (brainstorm idea #12, "the device IS the listener").

**Trigger type:** Strategic feature expansion (not a defect).

---

## Section 2 — Impact Analysis

### Epic Impact
- **Epic 3 (Curation Hub)** is `done`; it should **not** be reopened for a feature of this size.
- **New Epic 12** — Configurable Auto-Fill Pipeline & Multi-Server Auto-Fill (foundation + MVP strategies).
- **New Epic 13** — Advanced Auto-Fill Strategies (memory/rotation, quality, discovery, delight).
- Builds on shipped foundations: **Epic 2/8** multi-server (`ServerManager`, portable `server_id`, `get_provider_by_server_id`) and **Epic 9** provider capability contract / browse modes ([FR8](prd.md) supplies the Source vocabulary). No existing epic is invalidated or resequenced.

### Story Impact
- Stories 3.6 / 3.8 remain `done` and become **foundations to extend** (their single-slot model is generalized, not replaced). No rollback.

### Artifact Conflicts
- **PRD** — FR29 rewritten; new FR49–FR54 added; "Auto-Fill Sync Mode" feature bullet updated. MVP scope unaffected (auto-fill is a Growth feature).
- **Architecture** — "Auto-Fill Algorithm" component (arch:77) replaced by "Auto-Fill Pipeline"; new "Auto-Fill Pipeline Model" subsection; slot/RPC contract generalized to per-(device, portable serverId); new enforcement rules.
- **UX** — §5.3 rewritten from single toggle+slider to a pipeline-builder configuration panel + coexisting per-server slot cards; persona note (§1.2) and playlist-save notice (§5.2) updated.
- **Provider trait** — may need additive capability methods (genre/tag enumeration for filters; played-track data for memory strategies). Transcoding ties into the Budget "encoding-from-goals" idea (Epic 13).

### Technical Impact
- New daemon DB table `autofill_history` (cooldown windows, stable-core, pity-timer) — machine-local runtime state, **not** in the manifest.
- Manifest `autoFill` block becomes `Map<serverId, AutoFillPipeline>`; legacy `{enabled, maxBytes}` read as the default pipeline (backward compatible, migration-free).
- `sync.start` auto-fill param becomes an **array** of per-server descriptors; expansion routes per server via `get_provider_by_server_id`.

---

## Section 3 — Recommended Approach

**Selected path: Direct Adjustment via a new epic (Hybrid).**

| Option | Verdict | Effort / Risk |
|---|---|---|
| **1 — Direct Adjustment (new Epic 12/13)** | ✅ **RECOMMENDED** | High / Medium |
| 2 — Rollback | ❌ Not viable / not needed — nothing to revert; 3.6/3.8 are foundations | — |
| 3 — MVP Review | N/A — core MVP (single-device/single-server sync) untouched; purely additive Growth scope | — |

**Unifying design insight:** The two asks converge. The brainstorm makes config **per-device** (idea #12); the multi-server change made slots **server-bound**. Synthesis:

> An auto-fill definition is **one pipeline config per `(device, portable serverId)` pair**. Lifting "one slot total" → "one slot per server" *is* the same change as "store a pipeline config per slot." Today's hardcoded algorithm becomes the **default single-Ordering-stage pipeline**, so every shipped device keeps working with zero migration.

**Storage split (decided):** pipeline **configuration** lives in the device manifest (portable, per `device×server`); transient **runtime state** required by some strategies (cooldown windows, stable-core, pity-timer) lives in the **daemon DB** keyed by device+server (machine-local, not portable).

**Effort:** High (2 epics, ~13 stories). **Risk:** Medium — primary risk is over-abstraction, mitigated by gating the domain model (Story 12.1) on the 4-persona test before any UI. **Timeline:** additive; does not block any in-flight work.

---

## Section 4 — Detailed Change Proposals

### 4.1 PRD

**Rewrite FR29:**
> **FR29:** The system can reserve capacity in the sync basket via one or more virtual Auto-Fill slots — one per configured media server (see FR51). At sync time the daemon expands each slot by running that server's configured **auto-fill pipeline** (FR49) against the current library state of the slot's server, up to the device's available capacity or the slot's budget (FR52). When no pipeline is configured, the slot uses the default pipeline — a single ordering stage equivalent to the legacy algorithm (favorites → play count → creation date) — so existing devices behave unchanged.

**New requirements:**
- **FR49 (Epic 12):** Configurable pipeline with ordered stages Filter → Sources → Unit → Ordering → Memory → Budget; config stored per server in the device manifest; valid with as little as one stage; unconfigured stages use neutral defaults.
- **FR50 (Epic 12):** Source × Strategy separation — a pipeline is an ordered list of `(Source, Picker, share)` entries + global modifiers + budget; first-class Sources are Playlist pools and a Tag/Genre pre-filter; per-source shares blend sources; a terminal fallback chain guarantees the budget target is reached.
- **FR51 (Epic 12):** Auto-fill definable independently per configured media server (lifts single-slot/single-server limit); one slot per server may coexist; config in manifest, runtime state in daemon DB keyed by device+server.
- **FR52 (Epic 12):** Budget stage — size target, duration target (bytes derived), headroom reserve; no fill exceeds `capacity − reserve`; fallback chain reaches target.
- **FR53 (Epic 13):** Memory/rotation strategies (sync cooldown, played-track exclusion, stable-core+delta, rotation tiers, repeat-tolerance dial), DB-history backed.
- **FR54 (Epic 13):** Advanced ordering/quality/discovery/delight modifiers (best-version, version preference, deep-cuts, acclaimed-classics, community-rating, context-aware, Artist Spotlight, rarity draws, pity timer).

**Feature bullet (PRD line 51):** updated to describe a configurable selection pipeline defaulting to favorites → play count → creation date, definable per media server.

### 4.2 Architecture
- Replace "Auto-Fill Algorithm" component (arch:77) with **"Auto-Fill Pipeline"** (pure-function stages; legacy behaviour = default single-Ordering pipeline; routed per server via `get_provider_by_server_id`).
- New subsection **"Auto-Fill Pipeline Model"**: manifest `autoFill : Map<serverId, AutoFillPipeline>` (with backward-compat read of legacy block); daemon DB `autofill_history` for runtime state; sync-time expansion loop (one slot per server, manual items win dedup).
- Generalize slot/RPC contract: `AutoFillSlot` is one-per-server (toggling the selected server's slot never removes others'); `sync.start` carries an array of per-server auto-fill descriptors; `sync.setAutoFill` superseded by `autoFill.setPipeline { deviceId, serverId, pipeline }` (legacy params still mapped to the default pipeline); `basket.autoFill` preview gains `serverId` + optional inline pipeline; `autoSyncOnConnect` stays server-independent.
- New enforcement rules: one-slot-per-server; route every expansion via `get_provider_by_server_id`; config in manifest / history in DB, never mixed.

### 4.3 Epics

**Epic 12 — Configurable Auto-Fill Pipeline & Multi-Server Auto-Fill** (foundation + MVP):

| Story | Title |
|---|---|
| 12.1 | Pipeline domain model & pure-function engine (persona-validated, fully unit-tested, no UI) |
| 12.2 | Manifest schema (`Map<serverId, AutoFillPipeline>`, legacy read/migration) + DB `autofill_history` scaffolding |
| 12.3 | Multi-slot sync-time expansion + lift single-slot limit (array of per-server descriptors; per-provider routing) |
| 12.4 | PlaylistSource + Tag/Genre filter + per-source shares (capability-gated) |
| 12.5 | Budget stage: headroom reserve + duration target + fallback chain |
| 12.6 | Auto-Fill configuration UI (pipeline builder) + coexisting per-server slot cards (non-selected read-locked) |
| 12.7 | RPC/state contract & persistence wiring (`autoFill.setPipeline`, `basket.autoFill`+serverId, `get_daemon_state`) + i18n |

**Epic 13 — Advanced Auto-Fill Strategies** (delight/depth, depends on Epic 12):

| Story | Title (brainstorm idea #) |
|---|---|
| 13.1 | Memory & rotation: cooldown (#4), played-track exclusion (#5), stable-core+delta (#24), rotation tiers (#25/#26), repeat-tolerance (#23) |
| 13.2 | Quality & version ordering: best-version (#11), quality modifier (#13), version preference (#34) |
| 13.3 | Curation & discovery sources: deep-cuts (#14), acclaimed-classics (#16), community-rating fallback (#15), musical-memories (#31) |
| 13.4 | Delight: weighted rarity draws (#29), pity timer (#30) |
| 13.5 | Context & encoding-from-goals: time-of-day (#3), energy-curve (#17), seasonal (#32), encoding from goals (#20) |
| 13.6 | Advanced units & promotion: Artist Spotlight (#33), album/track ratio (#8), affinity-triggered album promotion (#9), coherence fill (#27) |

*Consciously cut (per brainstorm, out of scope — not deferred): listener profiles (#22), smart refill triggers, skip-based negative feedback.*

**Traceability block additions** (epics.md): FR49–FR54 mapped to Epics 12/13; FR29 line annotated as pipeline/multi-server (Epic 12).

### 4.4 UX Spec
- **§5.3 rewritten** ("Auto-Fill Configuration & Slots"): pipeline-builder configuration panel (collapsible stage sections, Advanced disclosure, Default simple state); per-server scope; one slot card per server (server icon+name badge, dashed border, non-selected read-locked); budget readout + fallback hint; ambition-tier inline cheap equivalents.
- **§1.2 (Sarah persona):** note distinct fill per server.
- **§5.2 ("Save selection as playlist" notice):** wording extended to "Auto-Fill slots (one per server)".

---

## Section 5 — Implementation Handoff

**Scope: MAJOR.** Route to **Product Manager / Architect first**, then Product Owner / Developer.

**Sequencing & dependencies:**
- Critical path: **12.1 → 12.2 → 12.3** (engine → schema/migration → multi-slot expansion). Story 12.3 alone delivers the multi-server ask if prioritized early.
- 12.4, 12.5 depend on 12.1; 12.6, 12.7 depend on 12.2/12.3.
- **Epic 13** entirely depends on Epic 12 (engine + DB-history scaffolding from 12.2).

**Risk & mitigation:** Over-abstraction → Story 12.1 gates the model on the 4-persona test (Claire/Antoine/Léo/Nadia) before any UI is built (brainstorm Priority-1 success criterion: "four personas, one model").

**Success criteria:**
1. Existing single-server devices behave identically with zero migration pain.
2. Two servers can each have an independent fill in one basket simultaneously.
3. "70% from 2 playlists, 30% library, no repeats within 3 weeks, leave 1 GB free" is configurable in under a minute.

**Deliverables on approval:**
- This Sprint Change Proposal.
- `sprint-status.yaml` updated with Epics 12 & 13 (status `backlog`).
- Edit proposals ready for `create-story` to expand into full Given/When/Then story specs.

---

## Workflow Execution Log
- 2026-06-14: Correct-course workflow run. Mode: Incremental. Scope decision: full pipeline. Storage decision: config in manifest, runtime history in daemon DB. Epic-shape decision: two epics (12 foundation, 13 advanced). All four artifact chunks (PRD, Architecture, Epics, UX) approved by Alexis.
