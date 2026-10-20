---
title: 'Separate expected file size from a one-percent capacity reserve'
type: 'feature'
created: '2026-10-20'
status: 'done'
baseline_commit: '85f82dcac500ed902a4cfc9169fb281dd0dec2f5'
baseline_working_tree: '/tmp/hifimule-reserve-baseline'
context: []
---

<frozen-after-approval reason="User explicitly approved separate display and a one-percent allowance in this chat">

## Intent

**Problem:** The basket presents a capacity reservation as predicted file size. For the supplied 99-track playlist, the 659 MiB reservation contains about 25 MiB of allowances, while copied size was reported as 637 MiB. The user approved a separate expected-size/reserve display and reducing the percentage from 3% to 1%.

**Approach:** Share an output-size breakdown through planning, RPC and UI. Display expected output bytes prominently and safety reserve separately; use their full sum for capacity checks and Auto-Fill. Retain the existing 64 KiB allowance per converted track.

## Boundaries & Constraints

**Always:** Keep direct/reused/nonmusic bytes exact with zero extra reserve. Converted expected bytes are the existing audio payload prediction, including conservative PCM bounds for lossless; reserve is one percent of payload plus 64 KiB per converted track. Preserve server-qualified deduplication, profile/force handling, measured resident reuse, unknown-duration policy, saturating arithmetic, stale-response fencing and actual-transfer progress. Keep reserve included in capacity bars, warnings, confirmation and all Auto-Fill planning. Translate visible copy in English, French, Spanish and German. Preserve existing compact desktop design and accessible labels.

**Ask First:** Changes to encoding targets, server settings, destructive operations or publication.

**Never:** Remove the fixed allowance, present the entire reserve as only 1%, infer a precise breakdown by subtracting guessed percentages from older daemon totals, download/encode media to estimate size, overwrite original basket metadata with output estimates, or replace prior authorized working-tree fixes.

## I/O & Edge-Case Matrix

| Scenario | Expected behavior |
| --- | --- |
| Encoded selection | Main estimate shows payload; separate reserve shows 1% plus fixed overhead; full sum budgets capacity |
| Direct, measured reuse, nonmusic | Exact known output bytes; reserve zero |
| Mixed or overlapping selection | Aggregate breakdown uses identical authoritative dedup/reuse decisions as preparation |
| Prepared result replaces estimate | Expected and reserve travel with the prepared reserved total; no display regression after declined confirmation |
| Older daemon response | Preserve capacity total; omit unknown breakdown rather than inventing one |
| Empty/tiny/saturated totals | No negative values or invalid numbers; tiny reserves remain legible; sums saturate safely |
| Narrow sidebar or translated copy | Text wraps; controls remain reachable and the bar describes expected size plus reserve accessibly |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/sync.rs` — shared audio/selection breakdown, reservations, reuse, goal encoding and regression tests.
- `hifimule-daemon/src/rpc.rs`, `rpc/sync_plan_tests.rs` — basket aggregate/item response and prepared manual selection breakdown.
- `hifimule-daemon/src/{auto_fill,device_music_scan,tests.rs}` — callers and expectations that consume reserved bytes.
- `hifimule-ui/src/state/{syncCapacity,basketEstimate}.ts` — fallback breakdown and compatible RPC types.
- `hifimule-ui/src/components/BasketSidebar.ts`, `src/styles.css`, `../hifimule-i18n/catalog.json` — expected item/selection display, reserve label, capacity accessibility and localization.
- `hifimule-ui/tests/{basketEstimate,syncCapacity,syncPlan,deviceSettings}.test.mjs`, `tests/basket-scroll.html` — data-flow, prepared-state, budgeting and visual checks.
- `docs/api-contracts-hifimule-daemon.md` — additive RPC fields and distinction between expected and reserved bytes.

## Tasks & Acceptance

**Execution:**
- [x] Backend planner — expose expected/reserved breakdown from one decision path and lower percentage to 1%; keep existing reserved-byte helpers compatible.
- [x] Backend RPC — add aggregate/item `expectedBytes` and `reserveBytes` to basket estimates, plus `selectedExpectedBytes` and `selectedReserveBytes` to prepared summaries; preserve reserved existing fields.
- [x] UI state — use a shared 1% fallback breakdown, preserve optional legacy fields, and keep prepared/estimated summaries coherent.
- [x] Basket display — show expected bytes in totals/items, a separate positive reserve, and full reserved capacity in geometry/admission/Auto-Fill; cover all four locales.
- [x] Validation — update meaningful numeric regressions, add breakdown/reuse/mixed/prepared/legacy/saturation tests, run parallel daemon/UI suites and build, inspect production-rendered desktop/compact states, review independently.

**Acceptance Criteria:**
- Given the Alchemy sample at forced MP3 192 kbps, when estimated, then expected payload is 664368000 bytes, reserve is 13131744 bytes, and reserved capacity is 677499744 bytes (about 646 MiB rounded), with separate display values.
- Given capacity between expected and reserved bytes, when selecting or filling, then full reservation controls warnings and available Auto-Fill bytes.
- Given copied resident files, when the refreshed basket estimate is rendered, then measured bytes replace prediction and reserve becomes zero.
- Given a prepared response followed by a declined confirmation, when the idle basket rerenders, then it retains that response's expected size and reserve coherently.
- Given old or unavailable estimates, when rendering, then capacity remains safe and unknown reserve is not invented from authoritative totals.

## Spec Change Log

- 2026-10-20: User-approved policy supersedes the prior 3% allowance. Prior investigation calculations remain historical evidence. The existing dirty tree contains authorized fixes from the same chat; the incremental baseline captures them before this change.

## Design Notes

Keep `selectedBytes` and per-item `plannedBytes` as reserved totals for compatibility. New basket fields are `expectedBytes` and `reserveBytes`; prepared summaries use `selectedExpectedBytes` and `selectedReserveBytes`. A clean measured entry contributes its exact bytes to both expected and reserved totals. Aggregate the decisions once rather than deriving the expected total by globally removing a percentage, which would be wrong for mixed selections.

The primary summary uses expected bytes; muted secondary text says “Safety reserve: {size}”. A positive tiny reserve must not round to an apparently zero value. Keep the capacity bar based on reserved bytes and include the breakdown in its accessible description. Auto-Fill slot budget text remains a ceiling. No reserve is added to actual transfer/completion figures.

## Verification

- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon --offline` — normal parallel daemon and contract suite.
- `rtk node --test hifimule-ui/tests/*.test.mjs` — UI regressions and data-flow tests.
- `rtk npm run build --prefix hifimule-ui` — typecheck and production bundle.
- `rtk git diff --check` — clean patch whitespace.
- Inspect the production basket fixture at normal desktop and 300px width, including translated reserve text, measured/no-reserve and legacy responses.

## Validation Results

- Normal parallel daemon suite: 1,590 passed, 12 existing ignored; provider contracts: 5 passed.
- UI suite: 187 passed. Typecheck and production bundle passed. Patch whitespace passed.
- Production-rendered 400px and 300px baskets verified in English, French, Spanish and German. Encoded, measured, legacy and tiny reserve states verified; translated Start Sync labels wrap within compact controls.
- Three independent reviews (blind adversarial, edge-case and acceptance) found no actionable introduced defects. Root hardened tiny malformed legacy splits before review.
- No media encoding/download, server configuration change, commit, push or publication performed.

## Suggested Review Order

**Shared size policy**

- Keep expected payload and full reservation on one decision path.
  [sync.rs:1694](../../hifimule-daemon/src/sync.rs#L1694)

- Expose authoritative basket and row breakdowns without changing existing reserved totals.
  [rpc.rs:4717](../../hifimule-daemon/src/rpc.rs#L4717)

**Prepared state**

- Apply identical manual-selection filters to prepared expected and reserved totals.
  [rpc.rs:6046](../../hifimule-daemon/src/rpc.rs#L6046)

- Prefer coherent prepared pairs while retaining the complete capacity reservation.
  [BasketSidebar.ts:200](../../hifimule-ui/src/components/BasketSidebar.ts#L200)

**Basket display**

- Show expected size and reserve separately while keeping the capacity bar reserved.
  [BasketSidebar.ts:1317](../../hifimule-ui/src/components/BasketSidebar.ts#L1317)

**Supporting coverage**

- Keep optional legacy breakdowns unknown when inconsistent or absent.
  [syncCapacity.ts:41](../../hifimule-ui/src/state/syncCapacity.ts#L41)

- Pin payload, one percent and per-track allowances with deterministic regressions.
  [sync.rs:11599](../../hifimule-daemon/src/sync.rs#L11599)

- Exercise measured, mixed, legacy and tiny reserve displays.
  [syncCapacity.test.mjs:187](../../hifimule-ui/tests/syncCapacity.test.mjs#L187)
