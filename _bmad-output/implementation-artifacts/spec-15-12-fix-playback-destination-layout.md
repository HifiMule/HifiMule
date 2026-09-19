---
title: 'Story 15.12 follow-up: fix Playback destination layout'
type: 'bugfix'
created: '2026-09-19'
status: 'done'
baseline_commit: '830e02d22a78ea56a4bce9548a491d25ee5d34d6'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/15-12-access-playback-as-an-always-available-destination.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** When Playback is selected, the read-only queue replaces the library body but the authored `display:flex` rule keeps the hidden browse toolbar visible. The result looks like the playlist is overlaid inside the Library view, and long queues do not reliably own the remaining scrollable height.

**Approach:** Enforce mutually exclusive visibility for destination content even when authored display rules exist, give the Playback panel an explicit flex/scroll contract, and preserve the host element's structural classes.

## Boundaries & Constraints

**Always:** Playback and library content must be visually exclusive; PlaybackControls and the destination switcher remain mounted; the basket/sidebar stays independent; the selected destination remains daemon-owned; layouts below 600px, from 600–1000px, and above 1000px must retain reachable controls and one bounded content scroller.

**Ask First:** Changing the overall split-panel proportions, moving the server selector, or redesigning the shared Library header.

**Never:** Hide PlaybackControls, duplicate the queue, use absolute positioning, create a second scroll owner over the queue, or rebuild the persistent main layout on destination changes.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Playback selected | Playback destination is active | Browse toolbar and library body are absent; Playback queue owns remaining height and scrolls internally | Empty/error queue states remain visible |
| Managed device selected | Device destination is active | Playback panel is absent; browse toolbar and library body return | Existing physical basket workflow is unchanged |
| Long queue or short window | Queue exceeds available height | Header, destination switcher, controls, and sidebar remain fixed/reachable while only queue content scrolls | No overlay or shell overflow |
| Repeated destination changes | Playback and device are alternated | Existing component hosts/classes and PlaybackControls remain stable | No duplicate queue mount |

</frozen-after-approval>

## Code Map

- `hifimule-ui/src/main.ts` -- owns the sibling library/Playback hosts and toggles their `hidden` state.
- `hifimule-ui/src/styles.css` -- currently overrides `hidden` on the flex browse toolbar and lacks a complete Playback flex contract.
- `hifimule-ui/src/components/PlaybackDestination.ts` -- mounts the queue and currently replaces all host classes.
- `scripts/tests/destination-ui.test.mjs` -- production-component harness for destination and Playback presentation behavior.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-ui/src/styles.css` -- make hidden destination siblings authoritative and size the Playback host as the single remaining-height scroller.
- [x] `hifimule-ui/src/components/PlaybackDestination.ts` -- add the component class without discarding structural host classes.
- [x] `scripts/tests/destination-ui.test.mjs` -- lock the hidden CSS and class-preservation contracts with regression coverage.

**Acceptance Criteria:**
- Given Playback is selected, when the main view renders, then neither the browse toolbar nor library body occupies layout space and the queue does not overlap Library chrome.
- Given a device is selected after Playback, when visibility is reversed, then the library browser returns and the Playback panel occupies no layout space.
- Given a long queue at narrow, medium, or wide widths, when content exceeds the viewport, then the Playback panel owns the remaining height and scrolls without obscuring controls or the basket.
- Given destination state refreshes repeatedly, when the same Playback host is reused, then structural classes and persistent controls remain intact.

## Spec Change Log

## Design Notes

The HTML `hidden` attribute must win over authored component display rules. Scope that enforcement to the three destination siblings rather than adding a global rule. Structural sizing belongs on the stable `#playback-destination-container`; component styling remains on `.playback-destination`.

## Verification

**Commands:**
- `rtk proxy node --test scripts/tests/destination-ui.test.mjs scripts/tests/playback-ui.test.mjs` -- expected: all destination/playback UI behavior tests pass.
- `rtk npm --prefix hifimule-ui run build` -- expected: TypeScript/Vite production build succeeds.
- `rtk git diff --check` -- expected: no whitespace errors.

**Manual checks:**
- At narrow, medium, and desktop widths, select Playback and a managed device; confirm exact content exclusivity and bounded scrolling.

## Suggested Review Order

**Visibility and scroll ownership**

- Hidden destination siblings leave layout despite authored display modes. [`styles.css:104`](../../hifimule-ui/src/styles.css#L104)
- Stable Playback host owns remaining height and the only scrollbar. [`styles.css:121`](../../hifimule-ui/src/styles.css#L121)
- Long metadata wraps instead of introducing horizontal overflow. [`styles.css:2253`](../../hifimule-ui/src/styles.css#L2253)

**Component host preservation**

- Queue styling is added without discarding structural host classes. [`PlaybackDestination.ts:16`](../../hifimule-ui/src/components/PlaybackDestination.ts#L16)

**Regression coverage**

- Tests lock hidden selectors, scroll ownership, wrapping, and preserved classes. [`destination-ui.test.mjs:98`](../../scripts/tests/destination-ui.test.mjs#L98)
