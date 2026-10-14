---
title: 'Show current Auto-Fill usage in the basket capacity display'
type: 'feature'
created: '2026-10-14'
status: 'done'
baseline_commit: '73c5124849b3ad4cc1734f8c58596e2da3a97816'
context: ['{project-root}/DESIGN.md']
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The basket correctly reports manual selection demand and reclaimable capacity, but hides the space already occupied by Auto-Fill. A device with 1909 selected tracks (8.6 GB) and approximately 200 GB of synced Auto-Fill appears nearly empty.

**Approach:** Add a translucent cyan segment for recorded Auto-Fill usage beside the existing reserved-selection segment. Split the remaining-space display into available space and space currently used by Auto-Fill. Present Compact and Labeled previews before implementing; use the user's chosen presentation.

## Boundaries & Constraints

**Always:** Preserve manual track/size totals, sync admission, warning thresholds, Auto-Fill budgets, descriptor estimates, and slot “will fill … at sync time” text. Read usage for the selected device across all its servers. Distinguish a current Auto-Fill measurement from a reservation or future budget. Keep the 8 px bar, existing unmanaged-content segment, and accessible text labels.

**Ask First:** Implement only after the user has reviewed the proposed design. Any change to sync calculations, daemon contracts, or Auto-Fill behavior requires renewed scope agreement.

**Never:** Add Auto-Fill bytes to manual totals or demand, infer usage from enabled settings or budget ceilings, fetch full library records solely to render the bar, or change device files/manifests.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Mixed device | Selection 8.6 GB; remaining reclaimable capacity 496 GB; recorded Auto-Fill 200 GB | Solid reserved segment; translucent Auto-Fill segment; 296 GB available and 200 GB used by Auto-Fill | Estimates retain existing size formatting |
| No synced Auto-Fill | Recorded Auto-Fill zero | Existing bar and remaining-space wording; omit additional zero-usage row | Enabling Auto-Fill alone does not imply usage |
| Multiple servers | Device totals include several servers | Show their aggregate recorded Auto-Fill once | Do not multiply by virtual slot count |
| Selection displaces fill | Positive remaining capacity smaller than existing Auto-Fill bytes | Keep full recorded usage in its label; clip segment to remaining bar width; available label clamps to zero | No invented overflow warning or change to admission |
| Warning | Existing amber or red capacity zone | Preserve original tight-fit/exceeds status and reserved-segment warning color; Auto-Fill detail stays secondary | Do not replace warnings with a healthy available message |
| Unavailable storage | No device, missing storage, or zero capacity | Preserve existing unavailable/empty behavior | Do not invent percentages |
| Legacy/MTP data | Legacy records or compact byte totals; approximate storage | Use existing provenance defaults and aggregate/record fallback; clamp geometry | No new RPC or origin inference |

</frozen-after-approval>

## Code Map

- `hifimule-ui/src/components/BasketSidebar.ts` — `renderCapacityBar` renders bar/status; `selectedCapacity` also serves admission and Auto-Fill sizing, which stay unchanged.
- `hifimule-ui/src/state/syncCapacity.ts` — existing `syncedAutofillBytes`, fractions, and `remainingBytes` provide display inputs without changing accounting.
- `hifimule-ui/src/styles.css` — capacity segment and status styles.
- `hifimule-i18n/catalog.json` — English, French, Spanish, and German strings.
- `hifimule-ui/tests/syncCapacity.test.mjs` — behavioral capacity and renderer coverage, including compact totals and reclamation.
- `DESIGN.md` — Signal Cyan and 8 px capacity bar conventions.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-ui/src/components/BasketSidebar.ts` — Derive presentation-only available bytes as `max(remainingBytes - syncedAutofillBytes, 0)`; render the chosen layout, secondary segment, and accessible descriptions. Keep full recorded Auto-Fill usage visible even if its segment is clipped. Preserve no-usage and warning branches.
- [x] `hifimule-ui/src/styles.css` — Add translucent accent styling and wrapping status/legend layout that fits narrow sidebars and translated text.
- [x] `hifimule-i18n/catalog.json` — Add localized available/used-by-Auto-Fill wording and any chosen legend labels; retain the existing remaining key for the no-usage fallback.
- [x] `hifimule-ui/tests/syncCapacity.test.mjs` — Cover mixed usage, clipped geometry, no-usage fallback, warnings, records/totals parity, and unchanged admission/budget results. Adjust the existing renderer expectation to the new split while retaining its original accounting assertion.
- [x] `DESIGN.md` — Document the additional measured-usage segment and its labels.

**Acceptance Criteria:**
- Given recorded Auto-Fill usage, when the selected device basket renders, then the bar and labels distinguish reserved selection, current Auto-Fill usage, and available space.
- Given the same basket/device inputs, when this display change is applied, then manual totals, sync availability, warnings, and every slot estimate retain their previous values.
- Given a device/server switch or storage refresh after sync, when the basket renders, then measurements belong to the selected device without an extra full-library fetch.
- Given a narrow sidebar or any supported locale, when the capacity renders, then labels wrap without overlap and essential meaning does not depend on color or hover.

## Spec Change Log

## Design Notes

2026-10-14: User approved Compact. Implement that presentation; the Labeled alternative remains preview-only.

Recommended Compact layout keeps the current total above the bar, adding two short rows: “296.0 GB available” and “200.0 GB used by Auto-Fill.” Labeled alternative uses aligned Reserved selection / Used by Auto-Fill / Available rows. The example treats 200 GB as illustrative and assumes the existing 496 GB remaining capacity includes those reclaimable bytes. Display availability is space left after the desired manual reservation and currently recorded Auto-Fill, rather than a new physical-free-space RPC measurement.

## Verification

**Commands:**
- `rtk node --test hifimule-ui/tests/syncCapacity.test.mjs` — display and accounting regression cases pass.
- `rtk node hifimule-ui/node_modules/typescript/bin/tsc -p hifimule-ui/tsconfig.json` — no type errors.
- From `hifimule-ui`: `rtk node node_modules/vite/bin/vite.js build` — successful build.
- `rtk git -c safe.directory=C:/Workspaces/HifiMule diff --check` — no whitespace errors.

**Manual checks:** Compare the chosen layout with the design preview, switch devices, refresh after sync, and inspect normal/amber/red/no-device states at narrow sidebar widths.

**Implementation verification (2026-10-14):**
- TDD red: five expected missing-display failures before production edits; existing capacity/fallback checks passed.
- Focused capacity suite: 13 passed.
- TypeScript check and Vite production build succeeded. Vite retained warnings about mixed static/dynamic imports and chunks over 500 kB.
- Full UI suite: 65 passed, two existing Windows-path failures (`radioDefaults.test.mjs` and `serverSwitchBrowse.test.mjs`) where esbuild cannot resolve `/C:/...` source URLs. No unrelated test files were changed.
- `git diff --check` passed after preserving existing mixed line endings and using LF for newly added lines.
- Independent verification repeated the full UI suite (65 passed, the same two Windows path failures), TypeScript check, production build, and whitespace check. The pre-existing path failures are already recorded in `deferred-work.md`.
- Browser inspection used the actual renderer, production stylesheet, and catalog for English, French, Spanish, and German at 320 px: no panel overflow and 8 px bars. Mixed usage, selection displacement, amber/red warnings, and no-Auto-Fill fallback matched the expected layout.
- Three independent reviews found no actionable defects. Existing renderer parity and budget tests cover preservation; selected-device totals and the existing device/after-sync refresh routes remain unchanged. Physical device switching and an actual sync were not exercised.

## Suggested Review Order

**Capacity display**

- Derive display-only availability and clipped usage without changing shared sync accounting.
  [BasketSidebar.ts:120](../../hifimule-ui/src/components/BasketSidebar.ts#L120)
- Render the added segment and Compact usage row with an accessible bar description.
  [BasketSidebar.ts:149](../../hifimule-ui/src/components/BasketSidebar.ts#L149)
- Use translucent Signal Cyan and wrapping status labels.
  [styles.css:510](../../hifimule-ui/src/styles.css#L510)

**Supporting changes**

- Translate available space and recorded usage in all four supported languages.
  [catalog.json:445](../../hifimule-i18n/catalog.json#L445)
- Verify mixed usage, displacement, warnings, fallback, and unchanged budget accounting.
  [syncCapacity.test.mjs:37](../../hifimule-ui/tests/syncCapacity.test.mjs#L37)
- Document the capacity segments and Compact layout.
  [DESIGN.md:197](../../DESIGN.md#L197)
