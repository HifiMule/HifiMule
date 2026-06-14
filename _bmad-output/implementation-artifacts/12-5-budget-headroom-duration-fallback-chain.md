---
baseline_commit: 180e59ae26b99db28f2e1b49584ab9581aa39637
---

# Story 12.5: Budget Stage — Headroom Reserve, Duration Target & Fallback Chain

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a user who trusts the tool with my whole device,
I want size/duration budgets with a headroom reserve and a guaranteed full fill,
so that no auto-fill ever exceeds my device capacity minus a reserve, I can target "≈1 hour of music" by duration, and a fallback chain still reaches my target when my chosen sources run dry.

## Context — what is already built (read before writing code)

The Budget stage **fields, pure-engine logic, and unit tests already exist** from Story 12.1. Story 12.4 deliberately **disabled** headroom and duration at the sync-time materialization seam, marking them "12.5 concerns." This story is primarily an **activation + correct-reconciliation** story, not a from-scratch build. The work is small and surgical — get the capacity-vs-config reconciliation and the path-routing right.

What exists today:
- `BudgetStage { max_bytes: Option<u64>, target_duration_secs: Option<u64>, headroom_bytes: Option<u64> }` — [hifimule-daemon/src/auto_fill/pipeline.rs:182-189]
- Pure engine `run_pipeline` already: subtracts headroom (`budget_ceiling` = `max_bytes − headroom_bytes`), accumulates real playtime (`Selector.cum_secs`) and stops at `target_duration_secs`, and applies the terminal `fallback` chain after primary exhaustion. These have passing unit tests (`headroom_is_subtracted_from_ceiling` [pipeline.rs:1052-1077], `duration_target_is_never_overshot` [pipeline.rs:1104-1122], `fallback_reached_only_after_primary_exhaustion` [pipeline.rs:1236-1269]). **Do not rewrite the pure engine's budget math — it is correct as a pure function and tested.**
- `expand_with_pipeline` (the async materialization layer) **zeroes** `target_duration_secs` and `headroom_bytes` before running the engine, and caps `max_bytes = min(config.max_bytes ?? capacity, capacity)`. [hifimule-daemon/src/auto_fill/fetch.rs:115-126]
- A guard test pins the current inert behavior: `headroom_and_duration_budget_fields_are_inert_until_12_5` [hifimule-daemon/src/auto_fill/fetch.rs:1034-1060]. **This story replaces that test with active-behavior tests.**
- The sync-time seam `expand_auto_fill_slot` + the discriminator `needs_configurable_expansion` route a slot to either the fast `run_auto_fill_provider` path (default-legacy) or `expand_with_pipeline` (non-default). The discriminator **intentionally excludes budget today** ("the budget is already honored by the fast path's `max_fill_bytes`"). [hifimule-daemon/src/auto_fill/fetch.rs:62-86; hifimule-daemon/src/rpc.rs:3526-3558]

## Acceptance Criteria

1. **Headroom reserve subtracts from device capacity (FR52 hard guarantee).** Given a configured pipeline with `budget.headroomBytes = R` and a sync-time slot capacity `C` (the `AutoFillParams.max_fill_bytes` derived in `rpc.rs`), when the slot expands, then the effective byte ceiling is `min(config.maxBytes.unwrap_or(C), C.saturating_sub(R))` and the cumulative estimated bytes of the materialized fill **never exceed `C − R`**. The reserve subtracts from device capacity, **not** from the user's optional `maxBytes` target — so `maxBytes = 8 GB`, `C = 10 GB`, `R = 1 GB` yields a `min(8, 9) = 8 GB` ceiling (not `min(8,10) − 1 = 7 GB`). [Source: prd.md#FR52; architecture.md#Auto-Fill-Pipeline-Model]

2. **Duration target enforced by real accumulated playtime.** Given `budget.targetDurationSecs = T`, when the slot expands, then units are accumulated until adding the next unit would push cumulative `duration_seconds` over `T`; the fill is simultaneously bounded by the byte ceiling from AC1, and whichever bound is reached first stops the fill. Enforcement uses the engine's real-seconds accumulation (`Selector.cum_secs`), which is strictly more accurate than a bitrate-derived byte estimate — see Dev Notes "Duration semantics" for why the FR's "bytes derived" wording is superseded. [Source: prd.md#FR52; hifimule-daemon/src/auto_fill/pipeline.rs (Selector)]

3. **Terminal fallback chain reaches the target.** Given primary sources that cannot fill the budget and a configured `fallback` chain, when the slot expands, then fallback pools are materialized in the async layer and drawn in configured order until the byte ceiling **or** duration target is reached (or all sources, primary and fallback, are exhausted) — guaranteeing a full fill whenever material exists. Fallback source pools must be fetched, not just configured (already chained at [fetch.rs:136]). [Source: prd.md#FR50, #FR52; epics.md#Story-12.5]

4. **A non-trivial budget forces the configurable path.** Given an otherwise-default pipeline that sets `headroomBytes` and/or `targetDurationSecs`, when sync expansion runs, then `needs_configurable_expansion` returns `true` and the slot routes through `expand_with_pipeline` (the Jellyfin-client fast path cannot honor headroom/duration). A pipeline whose budget has only `maxBytes` (or an all-`None` budget) still takes the fast `run_auto_fill_provider` path — **zero regression to default-legacy behavior**. [Source: hifimule-daemon/src/auto_fill/fetch.rs:62-86]

5. **Inert overrides removed and reconciliation moved to the seam.** Given `expand_with_pipeline`, when it normalizes the budget, then it no longer sets `target_duration_secs`/`headroom_bytes` to `None`; instead it (a) applies the headroom reserve against `params.max_fill_bytes` per AC1, bakes the resulting ceiling into `normalized.budget.max_bytes`, and sets `normalized.budget.headroom_bytes = None` so the pure engine does not double-subtract; and (b) leaves `target_duration_secs` live for the engine to enforce. The 12.4 guard test `headroom_and_duration_budget_fields_are_inert_until_12_5` is removed/rewritten as active-behavior tests. [Source: hifimule-daemon/src/auto_fill/fetch.rs:115-126]

6. **Skip + overflow guarantees preserved.** Given the activated budget, when the engine selects, then unknown/zero/negative-size tracks are still skipped (never counted as 0-byte fillers), within-run dedup and manual-exclude ids are still honored, and all size/budget arithmetic remains overflow-safe (`saturating_*`/`checked_*`). No regression to the 12.1 guarantee tests. [Source: hifimule-daemon/src/auto_fill/pipeline.rs (estimated_size, Selector)]

7. **Scope: daemon-only; no RPC/UI/i18n/manifest-schema/new-dep changes.** Given this is Story 12.5, when implementation is complete, then it activates budget semantics only in the existing sync-time expansion path. It does **not** add or change RPC methods (`autoFill.setPipeline`, `basket.autoFill` — Story 12.7), any UI (Story 12.6), i18n keys (12.7), manifest serialization (the `BudgetStage` fields already serialize per 12.1/12.2), or `Song`/provider models; and it introduces no new crate dependency. [Source: epics.md#Story-12.5, #Story-12.6, #Story-12.7]

8. **Build & tests green.** Given the workspace builds, when `rtk cargo test -p hifimule-daemon` runs, then all existing tests still pass (baseline: 505 daemon tests from Story 12.4) and the new tests for headroom-vs-capacity reconciliation, live duration target, fallback-reaches-target through the async path, and discriminator routing pass. `rtk cargo clippy -p hifimule-daemon --all-targets` adds no new warnings.

## Tasks / Subtasks

- [x] **Activate headroom + duration in the materialization seam** (AC: 1, 2, 5, 6)
  - [x] In `expand_with_pipeline` ([fetch.rs:115-126]) replace the budget-cap block. Compute `let capacity = params.max_fill_bytes;`, `let headroom = normalized.budget.headroom_bytes.unwrap_or(0);`, `let cap_after_reserve = capacity.saturating_sub(headroom);`, then `let ceiling = normalized.budget.max_bytes.map(|m| m.min(cap_after_reserve)).unwrap_or(cap_after_reserve);`. Set `normalized.budget.max_bytes = Some(ceiling)` and `normalized.budget.headroom_bytes = None` (reserve already consumed against capacity — prevents double-subtract in the pure engine's `budget_ceiling`).
  - [x] Delete the two `normalized.budget.target_duration_secs = None;` / `normalized.budget.headroom_bytes = None;` inert lines from the old block (the new `headroom_bytes = None` above is intentional and different in meaning; keep `target_duration_secs` live).
  - [x] Do **not** modify the pure engine (`pipeline.rs`) budget math — `budget_ceiling`, `Selector`, `estimated_size`, and the fallback loop are already correct and tested. Verify by reading them before editing anything.

- [x] **Route non-trivial budgets through the configurable path** (AC: 4)
  - [x] Extend `needs_configurable_expansion` ([fetch.rs:62-86]) so it returns `true` when `p.budget.headroom_bytes.is_some_and(|h| h > 0)` OR `p.budget.target_duration_secs.is_some_and(|t| t > 0)`. Keep `max_bytes`-only budgets on the fast path (the fast path honors `max_fill_bytes`, so a bare `maxBytes` needs no materialization).
  - [x] Update the doc-comment that currently says "`enabled` and `budget` are intentionally NOT part of the discriminator" to reflect that headroom/duration now force the configurable path (max_bytes alone still does not).
  - [x] Confirm `auto_fill_needs_configurable_routing` ([rpc.rs:3548-3558]) — which delegates to `needs_configurable_expansion` — automatically picks up the new behavior so multi-slot routing forces per-provider expansion when any slot has a headroom/duration budget. No separate edit expected; add a routing test to prove it.

- [x] **Replace the inert guard test with active-behavior tests** (AC: 1, 2, 3, 5, 8)
  - [x] Remove/rewrite `headroom_and_duration_budget_fields_are_inert_until_12_5` ([fetch.rs:1034-1060]).
  - [x] Add `headroom_reserve_subtracts_from_device_capacity`: capacity `C` via `params(C)`, pipeline with `headroom_bytes = R` and no `max_bytes` → fill total ≤ `C − R`; assert the count/ids match a `C − R` ceiling.
  - [x] Add `config_max_bytes_and_headroom_reconcile`: `max_bytes = 8 units`, `C = 10 units`, `R = 1 unit` → ceiling is `8` (not `7`); `max_bytes = 8`, `C = 5`, `R = 1` → ceiling is `4`.
  - [x] Add `duration_target_live_through_async_path`: pipeline with `target_duration_secs = T` over a materialized pool → fill stops at real accumulated `duration_seconds ≤ T`, mirroring `pipeline.rs::duration_target_is_never_overshot` but through `expand_with_pipeline`.
  - [x] Add `fallback_reaches_target_through_async_path`: a `Playlist` primary source too small to fill the budget + a `Library` fallback → fallback pool is materialized (provider stub returns library songs) and fill reaches the byte ceiling. Mirror `pipeline.rs::fallback_reached_only_after_primary_exhaustion` but verify the async layer actually fetches the fallback pool.
  - [x] Use the existing `fetch.rs` test fixtures/stubs (mock `MediaProvider`, `params(bytes)`, `ids(&result)`) — match the established style; do not introduce a new mocking approach.

- [x] **Discriminator routing tests** (AC: 4)
  - [x] Add a `needs_configurable_expansion` unit test table: `{headroom_bytes: Some(N)}` → true; `{target_duration_secs: Some(N)}` → true; `{max_bytes: Some(N)}` only → false; all-`None` budget on an otherwise-default pipeline → false.
  - [x] Confirm a default pipeline (no budget refinements) still resolves to the fast path via `expand_auto_fill_slot` (existing `discriminator_default_pipelines_take_fast_path` should remain green).

- [x] **Regression + green build** (AC: 6, 7, 8)
  - [x] Run `rtk cargo test -p hifimule-daemon`; confirm the 505-test baseline plus the new tests pass with no regressions (especially the 12.1 `auto_fill::pipeline::tests` guarantee tests and 12.4 `fetch` tests).
  - [x] Run `rtk cargo clippy -p hifimule-daemon --all-targets`; no new warnings.
  - [x] Confirm no new crate added to `Cargo.toml` and no RPC/UI/manifest files touched.

## Dev Notes

### What this story is (and is not)

This is an **activation/reconciliation** story. The Budget stage's data model and pure-function behavior shipped in Story 12.1; Story 12.4 fenced headroom and duration off ("inert until 12.5"). 12.5 turns them on at the async materialization seam and fixes the one genuinely subtle bug: **headroom must subtract from device capacity, not from the user's configured `maxBytes`.** Resist the urge to rewrite the pure engine — its budget math is correct and pinned by tests. The only production edits are in `fetch.rs` (`expand_with_pipeline` + `needs_configurable_expansion`). [Source: 12-1 story; 12-4 review findings]

### The headroom reconciliation (the one thing to get exactly right)

There are **two distinct byte numbers** in play, and conflating them is the trap:
- **`params.max_fill_bytes`** = the device's available capacity for this slot, derived in `rpc.rs` ([rpc.rs:2590-2625]) as either the UI's `maxBytes` (manual items already subtracted) or the server-side fallback `free + synced − basket`.
- **`pipeline.budget.max_bytes`** = the user's optional *configured cap* ("don't fill more than 8 GB even if there's room").

FR52 requires "no fill exceeds `capacity − reserve`." The reserve (`headroom_bytes`) is about **leaving room on the device**, so it subtracts from capacity. The correct formula is:

```
effective_ceiling = min( config.max_bytes.unwrap_or(capacity), capacity − headroom )
```

The pure engine's `budget_ceiling` computes `max_bytes − headroom` against a single number (it has no concept of a separate capacity — for the engine, `max_bytes` *is* the ceiling input). That is self-consistent **inside** the pure engine and its `headroom_is_subtracted_from_ceiling` test stays valid. The reconciliation between the two numbers belongs in `expand_with_pipeline`: bake the formula above into `normalized.budget.max_bytes` and zero `headroom_bytes` for the engine so it does not subtract twice. [Source: hifimule-daemon/src/auto_fill/pipeline.rs (budget_ceiling); hifimule-daemon/src/auto_fill/fetch.rs:115-126]

### Duration semantics — real seconds, not bytes-derived

FR52 and the epic line say "duration target (bytes derived)." The engine implemented in 12.1 instead accumulates **real `duration_seconds`** (`Selector.cum_secs`) and stops when the next unit would overshoot `target_duration_secs` — pinned by `duration_target_is_never_overshot`. Real accumulation is strictly more accurate than a bitrate→bytes estimate (it targets actual playtime, which is exactly what "≈1 hour of music" means) and it is already tested. **Implement/keep real-seconds enforcement; do not convert duration to a byte ceiling.** The byte ceiling (AC1) and the duration target are enforced together — whichever binds first stops the fill. See the open question at the end of this file flagging the FR-wording discrepancy. [Source: prd.md#FR52; hifimule-daemon/src/auto_fill/pipeline.rs (Selector::fill, duration_target_is_never_overshot)]

### Why headroom/duration must force the configurable path

The fast path (`run_auto_fill_provider`) only knows `max_fill_bytes`; it cannot reserve headroom or count playtime. Today `needs_configurable_expansion` ignores the whole budget block because, in 12.4, only `maxBytes` mattered and the fast path honored it. Once headroom/duration are live, an otherwise-default pipeline that sets either one would silently ignore them on the fast path. The fix is to make the discriminator return `true` for a non-trivial budget. Trade-off: such a pipeline materializes the Library pool generically instead of the smart-incremental fast path; the **selection is equivalent** (the default `[Favorite, PlayCount, DateCreated]` ordering is reproduced by the engine), only the fetch strategy differs. This is acceptable — headroom/duration are opt-in and rare. A bare `maxBytes` budget keeps the fast path. [Source: hifimule-daemon/src/auto_fill/fetch.rs:47-86; hifimule-daemon/src/auto_fill/mod.rs (run_auto_fill_provider)]

### Current code being extended (read before writing)

- **`hifimule-daemon/src/auto_fill/fetch.rs`** — the only production file to edit.
  - `expand_with_pipeline` ([fetch.rs:94-168]): the normalize-then-run flow. The budget block is [fetch.rs:115-126]; fallback pools are already materialized via `normalized.sources.iter().chain(normalized.fallback.iter())` [fetch.rs:136]. `history` is an empty `HistorySnapshot` (memory inert until Epic 13) — leave it untouched.
  - `needs_configurable_expansion` ([fetch.rs:62-86]): the fast-vs-configurable discriminator.
  - Test fixtures + stub provider live at the bottom of `fetch.rs` (~line 360+): `params(bytes)`, `ids(&result)`, mock `MediaProvider`. The inert guard test to replace is at [fetch.rs:1034-1060].
- **`hifimule-daemon/src/auto_fill/pipeline.rs`** — read-only for this story.
  - `BudgetStage` [pipeline.rs:182-189], `budget_ceiling` (`max_bytes.saturating_sub(headroom_bytes)`), `Selector { ceiling, duration_target, cum_bytes, cum_secs, ... }` and `Selector::fill` (atomic per-unit, stop-on-first-oversized, duration early-exit), `estimated_size` (prefer `size_bytes`; else `bitrate_kbps*1000/8*duration_seconds`; skip zero/unknown), and the fallback loop. Reference tests: `headroom_is_subtracted_from_ceiling`, `duration_target_is_never_overshot`, `fallback_reached_only_after_primary_exhaustion`, `budget_never_exceeded`.
- **`hifimule-daemon/src/domain/models.rs:24-50`** — `Song` already carries `duration_seconds: u32`, `bitrate_kbps: Option<u32>`, `size_bytes: Option<u64>`. No model change needed.
- **`hifimule-daemon/src/rpc.rs`** — `expand_auto_fill_slot` seam [rpc.rs:3526-3542] and `auto_fill_needs_configurable_routing` [rpc.rs:3548-3558] delegate to the discriminator; the budget derivation is [rpc.rs:2586-2640]. **No edit expected in `rpc.rs`** — the discriminator change propagates automatically. If a test reveals a routing gap, prefer fixing the discriminator over special-casing `rpc.rs`.

### Architecture compliance (non-negotiable)

- The pipeline is `Filter → Sources → Unit → Ordering → Memory → Budget` with a terminal fallback chain; Budget is `{ maxBytes?, targetDurationSecs?, headroomBytes? }` and the JSON field names are camelCase. Do not rename fields. [Source: architecture.md#Auto-Fill-Pipeline-Model (lines 788-826)]
- Pipeline **config** lives in the manifest (portable, `server_id`-keyed); runtime **history** lives in the daemon DB. 12.5 touches neither persistence layer — it consumes the already-loaded `AutoFillPipeline` at sync time. Do not read/write the manifest or DB here. [Source: architecture.md#Enforcement (lines 913-923)]
- Every expansion is routed per server via `get_provider_by_server_id` at the caller; the engine stays provider-agnostic. Do not bake server identity into the budget logic. [Source: architecture.md (lines 814-824)]
- Keep fetching (async/impure) and selection (sync/pure) split: all your edits are in the async `fetch.rs` layer; the pure `pipeline.rs` engine stays untouched. [Source: 12-1 story Dev Notes]

### Previous story intelligence (12.1 → 12.4)

- **12.1** built the engine and, in code review, already hardened: explicit zero-size skipping, no-share source caps, fixed stage order, duration-target enforcement, empty album/artist-id grouping, and overflow-safe arithmetic. Those fixes are in `pipeline.rs` — your activation rides on top of correct math.
- **12.4** review explicitly flagged "Headroom and duration budget fields still affect 12.4 materialized fills" and the fix was to *zero them* — the very lines you now reverse. It also fixed "source-less configurable pipelines materialize no Library pool" ([fetch.rs] adds an implicit Library key when `sources` is empty — see [fetch.rs:131-135]); keep that behavior. Genre membership is capped at `MAX_PER_LIST = 2000` and capability-gated — unrelated to budget, leave as-is.
- **12.2/12.3** wired the manifest `Map<serverId, AutoFillPipeline>` and multi-slot sync-time expansion. The budget you receive at sync time is per-slot; multiple slots each get their own `params.max_fill_bytes`. Your headroom math is per-slot — correct by construction.
- Pattern: each story in this epic kept the pure engine pure and confined I/O to `fetch.rs`/`rpc.rs`; reviews reward minimal, well-tested diffs. The 12.4 review applied 5 patches — expect a review pass, so write the active-behavior tests defensively.

### Testing standards

- Pure-engine guarantees are tested in `pipeline.rs::tests` (hand-built `Song` fixtures, no async). Async materialization is tested in `fetch.rs` with a stub `MediaProvider` and `#[tokio::test]`. Put **headroom-vs-capacity reconciliation and live duration/fallback** tests in `fetch.rs` (they exercise the seam where the bug lived); put any pure-ceiling assertions in `pipeline.rs` if needed (the existing ones likely suffice). Mirror existing fixture helpers — do not add mockito or a new provider-mock pattern. [Source: hifimule-daemon/src/auto_fill/fetch.rs tests; hifimule-daemon/src/auto_fill/pipeline.rs:718+]
- Run via `rtk cargo test -p hifimule-daemon`. Note from 12.1: the sandbox cannot run the *full* suite if mockito/local networking is blocked (`Operation not permitted`); in that case run the targeted modules `rtk cargo test -p hifimule-daemon auto_fill` and report the environment limitation rather than claiming a clean full run.

### Latest technical context

- No new crate is needed or permitted (AC #7): `serde`/`serde_json` (workspace `~1.0`), `async-trait`, `anyhow`, `tokio` (test), and std collections cover everything. Rust edition 2024-era (let-chains in use). Adding a dependency is scope creep and a review rejection. [Source: 12-1 story; Cargo.toml]

### Project Structure Notes

- All production edits are in `hifimule-daemon/src/auto_fill/fetch.rs`. The pure engine `hifimule-daemon/src/auto_fill/pipeline.rs` is read-only. `rpc.rs` is not edited (routing change propagates through the discriminator). This is a binary crate (`main.rs`, no `lib.rs`); tests are `#[cfg(test)] mod tests` inside each module.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Epic-12 (Story 12.5, lines 3050-3057)]
- [Source: _bmad-output/planning-artifacts/prd.md (FR50, FR52)]
- [Source: _bmad-output/planning-artifacts/architecture.md#Auto-Fill-Pipeline-Model (lines 788-826); #Enforcement (lines 913-923)]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-06-14-configurable-auto-fill.md (Sections 4.1 FR52, 4.3, 5)]
- [Source: hifimule-daemon/src/auto_fill/fetch.rs:62-86 (discriminator), 94-168 (expand_with_pipeline), 115-126 (budget block), 1034-1060 (inert guard test)]
- [Source: hifimule-daemon/src/auto_fill/pipeline.rs:182-189 (BudgetStage), budget_ceiling/Selector/estimated_size, 1052-1077/1104-1122/1236-1269 (budget tests)]
- [Source: hifimule-daemon/src/rpc.rs:2586-2640 (budget derivation), 3526-3558 (expand_auto_fill_slot seam + routing)]
- [Source: hifimule-daemon/src/domain/models.rs:24-50 (Song)]
- [Source: _bmad-output/implementation-artifacts/12-1-autofill-pipeline-domain-model-and-engine.md; 12-4-playlist-source-tag-filter-and-shares.md (review findings)]

## Open Questions / Clarifications

1. **Duration "bytes derived" vs real seconds.** FR52 and the epic say "duration target (bytes derived)," but the engine (12.1, tested) enforces a real-`duration_seconds` accumulation, which is more accurate for a playtime target. This story specifies keeping the real-seconds enforcement. Confirm this is the intended behavior (recommended) rather than reverting to a bitrate-derived byte estimate.
2. **Headroom on the fast path vs forcing the configurable path.** This story makes a headroom/duration budget force the configurable materialization path (losing the smart-incremental fetch but keeping equivalent selection). The alternative is to subtract headroom in `rpc.rs` at budget derivation so the fast path is retained for default+headroom pipelines. Confirm the chosen approach (force configurable path) is acceptable, or prefer the `rpc.rs` capacity-subtraction approach.

## Dev Agent Record

### Agent Model Used

claude-opus-4-8[1m] (Opus 4.8, 1M context)

### Debug Log References

- `rtk cargo test -p hifimule-daemon` → 510 passed (505 baseline − 1 removed inert test + 6 new tests).
- `rtk cargo test -p hifimule-daemon auto_fill` → 53 passed.
- `rtk cargo clippy -p hifimule-daemon --all-targets` → no new warnings in touched files (`fetch.rs` clean; the pre-existing `field_reassign_with_default` at `rpc.rs:7685-7686` is the 12.4 test, not introduced here).

### Completion Notes List

- **Activation, not rewrite.** Only `expand_with_pipeline` and `needs_configurable_expansion` in `fetch.rs` were changed in production. The pure engine (`pipeline.rs`) was read but left untouched — its `budget_ceiling`/`Selector`/fallback math is already correct and tested.
- **Headroom reconciliation (the subtle bit).** The reserve subtracts from *device capacity* (`params.max_fill_bytes`), not from the user's configured `max_bytes`. Implemented as `ceiling = min(config.max_bytes.unwrap_or(capacity), capacity − headroom)`, baked into `normalized.budget.max_bytes`, with `headroom_bytes` then zeroed so the engine's `budget_ceiling` does not double-subtract. `target_duration_secs` is left live for the engine to enforce via real accumulated playtime (`Selector.cum_secs`).
- **Routing.** A headroom reserve or duration target now forces the configurable materialization path; a bare `maxBytes` (or all-`None`) budget stays on the fast `run_auto_fill_provider` path (zero regression to default-legacy). Zero-valued headroom/duration are treated as inert. The change propagates automatically through `auto_fill_needs_configurable_routing` in `rpc.rs` (verified by a new test; no production edit in `rpc.rs`).
- **Tests.** Removed the inert guard test; added `headroom_reserve_subtracts_from_device_capacity`, `config_max_bytes_and_headroom_reconcile`, `duration_target_live_through_async_path`, `fallback_reaches_target_through_async_path` (fetch.rs) plus `discriminator_budget_headroom_and_duration_force_configurable` (fetch.rs) and `test_auto_fill_budget_headroom_forces_routing` (rpc.rs). Existing fixtures (`song`, `params`, `ids`, `arc`, `MockProvider`) reused — no new mocking approach.
- **Scope honored (AC7).** No new crate, no RPC method add/change, no UI/i18n/manifest-schema edits. The only `rpc.rs` change is a unit test.
- **Open Questions resolution.** Q1 (duration real-seconds vs bytes-derived) and Q2 (force configurable path vs `rpc.rs` capacity subtraction) were both resolved per the story's recommended approach: real-seconds duration enforcement, and forcing the configurable path for headroom/duration budgets.

### File List

- `hifimule-daemon/src/auto_fill/fetch.rs` (modified — production: `expand_with_pipeline` budget reconciliation, `needs_configurable_expansion` budget discriminator + doc-comment; tests: replaced inert guard with 4 active-behavior tests + 1 discriminator test)
- `hifimule-daemon/src/rpc.rs` (modified — test only: `test_auto_fill_budget_headroom_forces_routing` routing-propagation test)

## Change Log

| Date       | Change                                                                                              |
|------------|-----------------------------------------------------------------------------------------------------|
| 2026-06-14 | Activated headroom reserve + duration target at the sync-time materialization seam; routed headroom/duration budgets through the configurable path; replaced the 12.4 inert guard test with active-behavior + routing tests (510 daemon tests pass, no new clippy warnings). |
