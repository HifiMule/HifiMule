---
title: 'Fix: Empty basket blocks sync and leaves stale files on device'
type: 'bugfix'
created: '2026-05-29'
status: 'done'
baseline_commit: 'b2e4945'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** When all items are removed from the basket, the sync button becomes permanently disabled (after the 1-second debounced basket save clears `isDirty`), preventing the user from triggering a cleanup sync. As a result, previously synced files remain on the device indefinitely.

**Approach:** Enable the sync button when the basket is empty but the device has synced items. Also remove the two equivalent early exits in the auto-sync daemon path, relying on the existing destructive-threshold guard (line 739) for safety.

## Boundaries & Constraints

**Always:**
- The existing destructive-cleanup confirmation dialog must remain: any manual sync that deletes more than `DESTRUCTIVE_CLEANUP_THRESHOLD` files must still prompt the user before executing.
- The existing auto-sync destructive threshold skip (line 739) must remain unchanged — auto-sync silently skips when cleanup count exceeds 25.
- `calculate_delta` called with empty `desired_items` already produces the correct delete list. Do not change sync logic.

**Ask First:**
- None anticipated.

**Never:**
- Do not change the destructive threshold value or the confirmation dialog behavior.
- Do not touch `calculate_delta`, `sync_execute`, or anything outside the three affected locations.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Empty basket, device has synced items | `basket_items: []`, `synced_items: [N]`, device connected | Sync button enabled; click → delta shows N deletes; if N > threshold, confirmation dialog appears | N/A |
| Empty basket, device has no synced items | `basket_items: []`, `synced_items: []`, device connected | Sync button disabled (nothing to do) | N/A |
| Empty basket, no device connected | `basket_items: []`, any `synced_items`, no device | Sync button disabled (`selectedDevicePath` is null) | N/A |
| Auto-sync on connect, empty basket, ≤25 synced items | `auto_sync_on_connect: true`, `basket_items: []`, `synced_items: [N≤25]` | Auto-sync triggers, deletes N files, device cleaned | N/A |
| Auto-sync on connect, empty basket, >25 synced items | `auto_sync_on_connect: true`, `basket_items: []`, `synced_items: [N>25]` | Auto-sync skips (threshold guard at line 739), logs the skip | User must use manual sync |

</frozen-after-approval>

## Code Map

- `hifimule-ui/src/components/BasketSidebar.ts:866-899` -- Empty-basket render branch; line 883 has the disabled condition for the sync button
- `hifimule-ui/src/components/BasketSidebar.ts:180` -- `private currentDevice: any` — holds the full device manifest including `synced_items`
- `hifimule-daemon/src/main.rs:219-272` -- Auto-sync-on-connect gate: captures `has_basket`, `auto_fill_enabled` and guards the `run_auto_sync` spawn; must also capture `has_synced_items`
- `hifimule-daemon/src/main.rs:550-608` -- `run_auto_sync()` early-return when basket empty + auto-fill disabled
- `hifimule-daemon/src/main.rs:726-729` -- `run_auto_sync()` second early-return when `desired_items.is_empty()`
- `hifimule-daemon/src/main.rs:739-747` -- Existing destructive threshold guard (do not touch)

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-ui/src/components/BasketSidebar.ts` -- At line 883, extend the button disabled condition to also check if the device has synced items: add `&& !(this.currentDevice?.synced_items?.length > 0)` to the `(!basketStore.isDirty() && !this.autoFillEnabled)` sub-expression, so the button remains enabled when there is cleanup work to do -- primary fix for the user-facing bug
- [x] `hifimule-daemon/src/main.rs` -- At the auto-sync gate (line ~222-272), add `let has_synced_items = !manifest.synced_items.is_empty();` and extend the spawn condition to `auto_sync_enabled && (has_basket || auto_fill_enabled || has_synced_items)` — without this the cleanup branch inside `run_auto_sync` is unreachable dead code when basket is empty
- [x] `hifimule-daemon/src/main.rs` -- In `run_auto_sync()` at lines 604-607, replace the unconditional early return with a conditional: only return early if `manifest.synced_items.is_empty()`, otherwise fall through with empty `desired_items` so `calculate_delta` can generate the delete list -- allows auto-sync to clean up a device whose basket was emptied
- [x] `hifimule-daemon/src/main.rs` -- At lines 726-729, replace the `desired_items.is_empty() && manifest.synced_items.is_empty()` guard with `desired_items.is_empty() && !manifest.basket_items.is_empty()`: skip when basket had items but none resolved (safe); fall through when basket was empty (cleanup path already guaranteed to have synced items by the earlier guard at 604-607) -- prevents mass deletion when basket item resolution silently fails

**Acceptance Criteria:**
- Given a connected device with synced items and an empty basket, when the user views the basket sidebar, then the "Start sync" button is enabled.
- Given a connected device with synced items and an empty basket, when the user clicks "Start sync", then the sync delta is calculated and shows the synced items as deletions.
- Given a connected device with >25 synced items and an empty basket, when the user clicks "Start sync", then a confirmation dialog appears before any files are deleted.
- Given a connected device with no synced items and an empty basket, when the user views the basket sidebar, then the sync button remains disabled (nothing to do).
- Given `auto_sync_on_connect: true`, an empty basket, and ≤25 synced items, when the device connects, then auto-sync runs and deletes the stale files.
- Given `auto_sync_on_connect: true`, an empty basket, and >25 synced items, when the device connects, then auto-sync skips and logs the threshold reason (manual sync required).

## Spec Change Log

**Loop 1 (2026-05-29):**
- *Triggering findings:* (1) Edge Case Hunter: auto-sync gate at `main.rs:272` was not listed as an affected location — the condition `(has_basket || auto_fill_enabled)` always prevents `run_auto_sync` from being spawned when basket is empty, making the cleanup branch dead code. (2) Blind Hunter: guard at `main.rs:726` using `synced_items.is_empty()` as discriminator incorrectly allows empty `desired_items` through when basket had items but all failed to resolve, causing mass deletion.
- *What was amended:* Code Map gained `main.rs:219-272` entry. Tasks: added gate fix (`has_synced_items` + extend spawn condition); corrected line-726 guard from `synced_items.is_empty()` to `!basket_items.is_empty()`.
- *Known-bad state avoided:* Auto-sync cleanup never executing (gate blocks it); mass deletion of synced files when basket item API resolution fails silently.
- *KEEP:* `BasketSidebar.ts:883` fix is correct — keep unchanged. `main.rs:604-607` cleanup-else branch is correct — keep unchanged.

## Verification

**Commands:**
- `rtk cargo check` -- expected: no errors in hifimule-daemon
- `rtk tsc` -- expected: no TypeScript errors in hifimule-ui

**Manual checks (if no CLI):**
- With a device that has synced items and an empty basket: verify sync button is enabled in the sidebar empty-basket view
- With a device that has synced items and an empty basket: click sync, verify confirmation dialog for large cleanup, confirm, verify files are deleted from device and `synced_items` is cleared in manifest

## Suggested Review Order

**UI entry point — what the user sees**

- Button enabled condition: `synced_items` presence added as third enablement path
  [`BasketSidebar.ts:883`](../../hifimule-ui/src/components/BasketSidebar.ts#L883)

**Auto-sync on-connect gate — outer guard that spawns the sync task**

- New `has_synced_items` flag captures cleanup-needed state from manifest snapshot
  [`main.rs:222`](../../hifimule-daemon/src/main.rs#L222)

- Gate extended: `|| has_synced_items` allows `run_auto_sync` to be spawned for cleanup
  [`main.rs:273`](../../hifimule-daemon/src/main.rs#L273)

**`run_auto_sync` inner guards — path from basket resolution to delta calculation**

- First inner guard: allows fall-through when basket empty but synced items exist
  [`main.rs:604`](../../hifimule-daemon/src/main.rs#L604)

- Second inner guard: discriminates cleanup path (empty basket) from resolution failure (basket had items, none resolved) — uses `basket_items` not `synced_items` to avoid mass deletion on API failure
  [`main.rs:730`](../../hifimule-daemon/src/main.rs#L730)
