---
title: 'Story 15.12 follow-up: return from Playback to library browsing'
type: 'bugfix'
created: '2026-09-19'
status: 'done'
baseline_commit: '15b5896bdfc8373addad5b59d8a826862cd1f7b1'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/15-12-access-playback-as-an-always-available-destination.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-15-12-fix-playback-destination-layout.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Playback correctly replaces the library body when its queue is opened, but with no physical device there is no visible action that returns to the library. Users therefore cannot browse new tracks even though Story 15.12 requires library Play and Preview to remain usable while Playback is selected.

**Approach:** Add an explicit, localized “Back to library” action to the Playback queue. It changes only the visible UI surface: Playback remains the daemon-owned destination, and selecting Playback again reopens its authoritative queue.

## Boundaries & Constraints

**Always:** The return action is keyboard-accessible and visible in empty, populated, Preview and recoverable queue states; returning shows both the browse modes and library content; Playback remains selected; PlaybackControls and the basket/sidebar stay mounted; clicking Playback after browsing restores the queue.

**Ask First:** Introducing Library as a new daemon destination, changing destination wire types, or redesigning the full navigation hierarchy.

**Never:** Mutate the listening session, queue, destination selection, basket or physical-device state when returning to the library; create a second Playback component or poller; restore the previous overlay; hide transport controls.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|----------------------------|----------------|
| Browse from Playback | Playback queue is visible | “Back to library” shows browse modes and library content, hides the queue, and preserves Playback selection | No daemon mutation is attempted |
| Return to queue | Library is visible while Playback remains selected | Activating Playback restores the current authoritative queue | Existing queue recovery behavior remains authoritative |
| Empty or failed queue | Queue has no rows or renders a recoverable error | The library return action remains available | Queue errors cannot trap the user away from browsing |
| Responsive layout | Narrow, medium or wide viewport | The heading action remains reachable without horizontal overflow | Existing single-scroll-owner contract remains intact |

</frozen-after-approval>

## Code Map

- `hifimule-ui/src/components/PlaybackDestination.ts` -- renders queue states and will expose the library-return action.
- `hifimule-ui/src/main.ts` -- owns the mutually exclusive library and Playback surfaces.
- `hifimule-ui/src/styles.css` -- owns Playback heading layout and bounded scrolling.
- `hifimule-i18n/catalog.json` -- supplies EN/FR/ES/DE action labels.
- `scripts/tests/destination-ui.test.mjs` -- production-component harness for destination behavior and presentation contracts.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-ui/src/components/PlaybackDestination.ts` -- accept a browse callback and render a native library-return button in every queue state.
- [x] `hifimule-ui/src/main.ts` -- switch locally from the queue to existing library surfaces without changing daemon destination or persistent controls.
- [x] `hifimule-ui/src/styles.css` and `hifimule-i18n/catalog.json` -- keep the action responsive and add four-locale parity.
- [x] `scripts/tests/destination-ui.test.mjs` -- cover callback behavior, visibility contracts, queue restoration wiring and locale parity.

**Acceptance Criteria:**
- Given Playback is selected and its queue is visible, when the user activates “Back to library,” then normal library browse modes and content become usable without a destination or playback RPC.
- Given library browsing is visible while Playback remains selected, when the user activates Playback, then the queue view returns without rebuilding persistent controls.
- Given any supported queue state or viewport width, when the view renders, then the return action remains accessible and the Playback host retains one bounded scroller.

## Spec Change Log

## Design Notes

Library browsing is a presentation surface, not a synchronization destination. Keeping Playback selected preserves physical-action isolation and avoids inventing a wire-level Library destination. The selected Playback button doubles as the route back to the queue after browsing.

## Verification

**Commands:**
- `rtk proxy node --test scripts/tests/destination-ui.test.mjs scripts/tests/playback-ui.test.mjs` -- expected: destination and Playback behavior tests pass.
- `rtk npm --prefix hifimule-ui run build` -- expected: TypeScript/Vite production build succeeds.
- `rtk git diff --check` -- expected: no whitespace errors.

**Manual checks:**
- At narrow, medium and desktop widths, alternate Playback → Back to library → Playback; confirm browsing, queue restoration, focus visibility and unchanged basket/transport state.

## Suggested Review Order

**Local presentation transition**

- Entry point restores browsing, cleans the queue view, and transfers keyboard focus.
  [`main.ts:516`](../../hifimule-ui/src/main.ts#L516)

- Playback receives a presentation callback without gaining destination-state authority.
  [`PlaybackDestination.ts:17`](../../hifimule-ui/src/components/PlaybackDestination.ts#L17)

**Stable and recoverable queue UI**

- A persistent heading keeps the return action focused across queue refreshes.
  [`PlaybackDestination.ts:110`](../../hifimule-ui/src/components/PlaybackDestination.ts#L110)

- Disposal fencing prevents late queue failures from repainting an abandoned surface.
  [`PlaybackDestination.ts:62`](../../hifimule-ui/src/components/PlaybackDestination.ts#L62)

**Responsive localization**

- Flexible actions keep translated labels reachable on narrow layouts.
  [`styles.css:2249`](../../hifimule-ui/src/styles.css#L2249)

- Four localized labels make the navigation action explicit.
  [`catalog.json:12`](../../hifimule-i18n/catalog.json#L12)

**Regression coverage**

- Tests cover focus stability, late rejection, local switching, restoration, and locale parity.
  [`destination-ui.test.mjs:96`](../../scripts/tests/destination-ui.test.mjs#L96)
