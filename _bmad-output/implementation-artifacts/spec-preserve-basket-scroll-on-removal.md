---
title: 'Preserve basket scroll when removing an item'
type: 'bugfix'
created: '2026-10-16'
status: 'done'
baseline_commit: 'db601e365b46234cca86d7db47662e3e6047b4a0'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Clicking an item's cross in a scrolled basket recreates the list and returns the viewport to the top. This interrupts consecutive removals and makes users find their place again.

**Approach:** Preserve the current scroll offsets through same-device basket rendering. Restore them synchronously before entry animations measure the new rows. Retain the existing store-driven rendering so counts, capacity, server groups, and auto-fill controls continue to update together.

## Boundaries & Constraints

**Always:** Build on the current uncommitted UI work, as explicitly authorized. Preserve scroll for the list and its outer sidebar when the same device is still selected. Allow normal browser clamping when content becomes too short to retain the previous offset. Respect reduced-motion preferences; scroll restoration must work independently of animation support or page visibility. Keep existing removal guards and auto-fill persistence behavior.

**Ask First:** Changes to basket store semantics or device selection behavior beyond preserving the viewport.

**Never:** Add smooth scrolling, delayed restoration that overrides later user input, dependencies, or backend changes. Transfer one device's scroll position into another device's newly rendered basket.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Middle removal | Long list scrolled partway down | Remove selected item and preserve offsets | Existing mutation guard remains authoritative |
| Bottom removal | Old offset exceeds shortened list's maximum | Clamp to new maximum without jumping to top | Browser clamps naturally |
| Empty basket | Last item removed | Existing empty placeholder appears | No missing-list dereference |
| Compact window | Outer sidebar and inner list can scroll | Preserve both offsets where still valid | Clamp each independently |
| Repeated update | Same-device refresh follows removal | Keep current offset on every render | Do not cache stale offsets |
| Device switch | Selected device changes | Existing new-device rendering behavior | Do not restore old-device offsets |
| Reduced motion | Reduced motion enabled or animation unavailable | Preserve scroll exactly as in normal mode | No dependency on animation execution |

</frozen-after-approval>

## Code Map

- `hifimule-ui/src/components/BasketSidebar.ts` — `render()` wraps `renderContent()` and invokes motion capture and playback for same-device updates. `renderContent()` replaces the sidebar HTML. The remove handler updates the basket store, which drives rendering.
- `hifimule-ui/src/components/basketMotion.ts` — current motion helpers capture row IDs and capacity transforms; animation execution can return early when hidden or unsupported.
- `hifimule-ui/src/styles.css` — `.basket-items-list` owns vertical scrolling; compact layouts also allow the outer basket to scroll.
- `hifimule-ui/tests/basket-motion.html` — existing browser fixture for same-device DOM replacement and motion behavior.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-ui/src/components/BasketSidebar.ts` — capture list and panel offsets before same-device content replacement, then restore them synchronously before animations; tolerate absent lists and preserve device-change gating.
- [x] `hifimule-ui/tests/basket-motion.html` — extend the browser regression coverage, or add a focused basket scroll fixture beside it, exercising real overflow and the matrix cases. Prefer testing the actual render boundary rather than a duplicate of production restoration logic.

**Acceptance Criteria:**
- Given a selected device with a long basket, when the user clicks the cross on a visible item while scrolled down, then the item disappears, counts and capacity update, and the viewport remains at its previous offset unless the new content requires clamping.
- Given existing basket entry and capacity animations, when the basket changes, then those animations retain their behavior and measure rows after scroll restoration.
- Given uncommitted UI changes in the workspace, when this fix is applied, then those changes remain intact.

## Spec Change Log

## Design Notes

Prefer a small render-boundary fix over incremental DOM reconciliation: the current renderer also updates grouped headers, totals, capacity and controls. Scroll restoration belongs outside animation early returns. Capture offsets immediately before each render so asynchronous refreshes cannot reinstate a stale position. The existing same-device check provides the context boundary.

## Verification

**Commands:**
- `rtk npm run build` from `hifimule-ui` — TypeScript and Vite build succeed.
- `rtk git diff --check` — no newly introduced whitespace errors.

**Browser checks:**
- Exercise middle removal, repeated removals, bottom clamping, empty state, and compact sidebar overflow with enough rows for real scrolling.
- Repeat with reduced motion enabled and verify a device change does not inherit the previous device's list offset.
- Confirm existing entry/capacity motion checks still pass.

### Implementation evidence

- Captured same-device list/panel offsets immediately before content replacement; restored synchronously before motion playback. Existing device-change gate remains intact.
- Added [basket scroll browser fixture](../../hifimule-ui/tests/basket-scroll.html), exercising the actual sidebar render/content/cross-handler path with isolated selection and device-service stubs.
- `rtk npm run build` — passed (TypeScript and Vite); Vite reports existing mixed static/dynamic import warnings.
- `rtk git diff --check` — passed.
- Browser fixture execution is pending coordinator verification.

## Final Verification

- Browser: all 15 scroll assertions pass in standard and forced reduced-motion modes, including a repeated run.
- Existing basket-motion fixture: all six assertions pass.
- Three independent review passes: no actionable findings in this fix.
- Final whitespace check passes.
- Kept uncommitted with the authorized ongoing UI work: the edited render wrapper itself is an existing uncommitted change.

## Suggested Review Order

- Preserve same-device scroll before animations measure replacement rows.
  [BasketSidebar.ts:1036](../../hifimule-ui/src/components/BasketSidebar.ts#L1036)

- Exercise actual removal handlers with overflow, clamping, device changes, and reduced motion.
  [basket-scroll.html:60](../../hifimule-ui/tests/basket-scroll.html#L60)

