---
title: 'Restore device selection to Basket while Playing'
type: 'bugfix'
created: '2026-09-20'
status: 'done'
baseline_commit: 'NO_VCS'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/spec-15-12-fix-playback-destination-layout.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The Playing destination switch moved device selection into the Library’s upper-left area. This leaves the Basket panel unable to act as the familiar device-selection home shown in the reference design and separates selection from the basket, folder, and sync controls it governs.

**Approach:** Keep Playing available from the Library navigation, but render the physical-device choices inside the Basket panel with their recognizable device icons. Remove the now-redundant device-name/settings strip between auto-fill/auto-sync controls and Device Folders. The selected device must continue to drive the existing basket and sync workflow without changing daemon-owned destination state.

## Boundaries & Constraints

**Always:** Preserve the existing serialized destination-selection behavior, including saving the outgoing basket before opening Playing; show physical-device choices in the Basket area with the existing icon treatment; remove duplicate device naming from the space between auto-fill/auto-sync controls and Device Folders; keep Basket content, device folders, auto-fill, and Start Sync associated with the selected device; retain keyboard focus, selected state, and accessible device names; keep Playing and Library content mutually exclusive.

**Ask First:** Altering the split-panel proportions, server selection placement, device-management semantics, or the meaning of the Playing destination.

**Never:** Duplicate device selection state, add new RPCs or persistence, hide PlaybackControls, remove the Playing entry point, or use absolute positioning to relocate controls.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|----------------------------|----------------|
| Device available while browsing | One or more managed physical devices | Device choices appear in Basket with their existing icons; current selection is clear | Existing discovery issues remain visible |
| Playing active | Playback is the selected destination | Library-top device selector is absent; Basket still exposes device choices so a user can return to a device | Existing save/select failures are surfaced; no optimistic mismatch |
| Multiple devices | Device list changes or selection changes | Basket choices refresh with stable selection/accessibility and retain the current device’s basket context | Removed device uses existing fallback/error behavior |
| No device | Only Playing or no physical destinations available | Basket presents its existing disconnected/empty guidance without unusable choices | Start Sync remains correctly gated |

</frozen-after-approval>

## Code Map

- `hifimule-ui/src/main.ts` -- constructs the Library header, destination hub, and BasketSidebar hosts; coordinates surface changes.
- `hifimule-ui/src/components/DestinationHub.ts` -- owns daemon-backed serialized choice between Playing and physical devices.
- `hifimule-ui/src/components/BasketSidebar.ts` -- owns Basket-panel rendering and device-scoped basket/sync controls.
- `hifimule-ui/src/styles.css` -- scopes destination and Basket layout, icon treatment, focus, and responsive behavior.
- `scripts/tests/destination-ui.test.mjs` -- existing production-component harness for destination-selection sequencing and regression coverage.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-ui/src/main.ts` and `hifimule-ui/src/components/DestinationHub.ts` -- separate the Playing entry point from physical-device presentation and mount the latter through the Basket-side UI without changing selection ownership.
- [x] `hifimule-ui/src/components/BasketSidebar.ts` -- render/reuse the physical-device chooser in the Basket panel, including recognizable icons, selected state, accessible names, and refresh behavior alongside existing device-scoped controls; remove the redundant device-name/settings strip.
- [x] `hifimule-ui/src/styles.css` -- style the Basket selector to match the established icon-bearing device controls while preserving responsive reachability and focus visibility; remove obsolete shortcut styling.
- [x] `scripts/tests/destination-ui.test.mjs` -- extend the production-module regression harness for Basket placement, state ownership, icon/accessibility contract, and serialized Playing/device transitions.

**Acceptance Criteria:**
- Given a managed device is selected, when the Library view renders, then its icon-bearing selection control is in the Basket panel and no physical-device selector occupies the Library’s upper-left area.
- Given Playing is active, when the user views the Basket panel, then physical-device choices remain available there and selecting one returns to the existing device/library flow without losing the basket’s saved state.
- Given several devices or a refreshed device list, when the Basket chooser updates, then one current selection is accurately represented and each control remains keyboard reachable and correctly named.
- Given no selectable physical device exists, when the Basket renders, then no inactive device controls are offered and existing connection/sync gating remains intact.
- Given auto-fill/auto-sync controls and Device Folders render, when a device is selected, then no duplicate device-name/settings strip appears between them.

## Spec Change Log

## Design Notes

The Basket is the device-specific work area: device choice, folder visibility, auto-fill settings, and sync all concern the same target. Keep `DestinationHub` as the single owner of asynchronous selection so relocation is presentational, not a second state machine. Reuse its existing device icon mapping rather than introducing a second visual vocabulary.

## Verification

**Commands:**
- `rtk node --test scripts/tests/destination-ui.test.mjs` -- expected: destination sequencing and Basket-placement regressions pass.
- `rtk npm --prefix hifimule-ui run build` -- expected: TypeScript/Vite production build succeeds.
- `rtk git diff --check` -- expected: no whitespace errors.

**Manual checks:**
- Select Playing, then select each available physical device from Basket at narrow, medium, and desktop widths; confirm the icon-bearing controls, focus indication, correct selected state, basket retention, and sync/folder gating.

## Suggested Review Order

**Device-choice ownership**

- Keeps selection serialized while moving only its visible chooser into Basket.
  [`DestinationHub.ts:23`](../../hifimule-ui/src/components/DestinationHub.ts#L23)

- Wires the gear action to the existing device-settings workflow.
  [`main.ts:478`](../../hifimule-ui/src/main.ts#L478)

**Basket presentation**

- Reattaches the chooser across Basket states and removes the duplicate device row.
  [`BasketSidebar.ts:784`](../../hifimule-ui/src/components/BasketSidebar.ts#L784)

- Preserves chooser focus through unrelated Basket re-renders.
  [`BasketSidebar.ts:943`](../../hifimule-ui/src/components/BasketSidebar.ts#L943)

- Gives the relocated selector its bounded Basket spacing and icon alignment.
  [`styles.css:2374`](../../hifimule-ui/src/styles.css#L2374)

**Regression coverage**

- Locks Basket placement, icon naming, and removal of the redundant shortcut.
  [`destination-ui.test.mjs:102`](../../scripts/tests/destination-ui.test.mjs#L102)
