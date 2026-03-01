---
title: 'Save Basket Selection in Manifest'
slug: 'save-basket-selection-manifest'
created: '2026-03-01T20:39:42+01:00'
status: 'completed'
stepsCompleted: [1, 2, 3, 4, 5, 6]
tech_stack: ['Rust', 'TypeScript', 'Tauri']
files_to_modify: ['jellysync-daemon/src/device/mod.rs', 'jellysync-daemon/src/rpc.rs', 'jellysync-ui/src/state/basket.ts', 'jellysync-ui/src/components/BasketSidebar.ts']
code_patterns: ['Write-Temp-Rename atomic manifest updates', 'JSON-RPC for UI/Daemon communication', 'LocalStorage for UI basket state']
test_patterns: []
---

# Tech-Spec: Save Basket Selection in Manifest

**Created:** 2026-03-01T20:39:42+01:00

## Overview

### Problem Statement

When a device is connected, the selection of artists and playlists (the "basket") is not restored from previous syncs. Because the `.jellysync.json` manifest only stores the actual synchronized files and not the high-level user selection, the user loses context of what they had originally selected to synchronize.

### Solution

Store the selected artists, albums, or playlists (the basket contents) directly in the `.jellysync.json` manifest on the device. When the device is loaded, the `jellysync-ui` will read this selection from the manifest and pre-populate the basket. This ensures that if the server-side contents of a playlist or artist change, the next sync will automatically detect and synchronize the new items under those existing selections.

### Scope

**In Scope:**
- Update `.jellysync.json` schema to include a `basket` or `selections` field.
- Update `jellysync-daemon` to read and write these selections during the sync or manifest creation process.
- Update `jellysync-ui` (e.g., `basket.ts`, `rpc`) to send the user's basket items to the daemon when syncing.
- Update `jellysync-ui` to restore the basket contents from the manifest when a device is connected.

**Out of Scope:**
- Changes to the differential sync algorithm itself (it will just use the restored basket to build the target state).
- Other manifest repairs or legacy support.

## Context for Development

### Codebase Patterns

- **Manifest Updates**: Atomic writes via Write-Temp-Rename pattern using `device::write_manifest`.
- **UI State**: `basket.ts` uses an `EventTarget` based store, hydrating sizes using JSON-RPC calls.
- **Communication**: JSON-RPC over a local HTTP server handles UI-to-Daemon communication (`rpc.rs`).

### Files to Reference

| File | Purpose |
| ---- | ------- |
| `jellysync-daemon/src/device/mod.rs` | Defines `DeviceManifest`. This is where the `.jellysync.json` schema is defined. |
| `jellysync-daemon/src/rpc.rs` | Exposes RPC endpoints. We need to expose or return the basket state to the UI upon device connection or state fetch. |
| `jellysync-daemon/src/sync.rs` | Handles manifest updates during sync. We could save the basket state at the beginning or end of `sync_execute`. |
| `jellysync-ui/src/state/basket.ts` | The UI singleton for the basket. Needs to load data from the daemon side. |
| `jellysync-ui/src/components/BasketSidebar.ts` | Triggers the sync and could trigger saving the basket to the daemon. |

### Technical Decisions

- **Schema Addition**: Add `basket_items: Vec<BasketItem>` (or similar) to `DeviceManifest` in `device/mod.rs`.
- **State Load**: When extending `handle_get_daemon_state` or `handle_sync_get_device_status_map`, return the saved basket items so the UI can populate its local state.
- **State Save**: Either introduce a new RPC method `manifest_save_basket` to save the UI's basket independently, OR strictly bundle it into `sync_execute`. The spec will refine this in Step 3.

## Implementation Plan

### Tasks

- [x] Task 1: Update `DeviceManifest` schema
  - File: `jellysync-daemon/src/device/mod.rs`
  - Action: Add `#[serde(default)] pub basket_items: Vec<crate::device::BasketItem>` to `DeviceManifest`. Define `BasketItem` struct matching the UI's `BasketItem` (id, name, type, artist, childCount, sizeTicks, sizeBytes).
  - Notes: Ensure backwards compatibility by using `#[serde(default)]` so existing manifests load cleanly with an empty basket.

- [x] Task 2: Expose basket state to UI
  - File: `jellysync-daemon/src/rpc.rs`
  - Action: In `handle_get_daemon_state`, include the current device's `basket_items` in the returned JSON, under a key like `deviceBasket`.
  - Notes: The UI needs this to hydrate its LocalStorage on connection.

- [x] Task 3: Support saving basket state during sync
  - File: `jellysync-daemon/src/rpc.rs`
  - Action: In `handle_sync_execute` (or a similar handler triggering sync), accept a new parameter `basketItems`. When initializing the sync operation or updating the manifest, store these `basketItems` into the `DeviceManifest`.
  - Notes: If `sync_execute` takes a delta, it might not take the full basket. Alternatively, add a new RPC `manifest_save_basket` that the UI calls whenever the basket changes, or right before sync. Let's add `manifest_save_basket` to keep it clean and separated from the delta calculation.

- [x] Task 4: Add `manifest_save_basket` RPC method
  - File: `jellysync-daemon/src/rpc.rs`, `jellysync-daemon/src/device/mod.rs`
  - Action: Implement `handle_manifest_save_basket` taking `basketItems: Vec<BasketItem>`. Call a new `device_manager.save_basket()` method that uses `update_manifest` to overwrite `manifest.basket_items`.
  - Notes: Route the new method in the axum router.

- [x] Task 5: Update UI Basket Store to sync with Daemon
  - File: `jellysync-ui/src/state/basket.ts`
  - Action: Listen for device connection events (or poll daemon state). When `deviceBasket` is present in daemon state, merge or overwrite the local `basketStore`. Also, whenever the basket changes locally, call the new `manifest_save_basket` RPC.
  - Notes: Avoid infinite loops: flag when UI is updating from daemon so it doesn't immediately send the state back.

- [x] Task 6: Ensure `BasketSidebar` handles sync trigger correctly
  - File: `jellysync-ui/src/components/BasketSidebar.ts`
  - Action: Ensure that triggering sync relies on the current `basketStore` state. Since Task 5 ensures the daemon is updated on every basket change, no extra payload is strictly needed here, though double-checking the flow is prudent.

### Acceptance Criteria

- [x] AC 1: Given a device with a populated basket in its `.jellysync.json`, when the device is connected, then the UI basket is populated with those items.
- [x] AC 2: Given a connected device, when the user adds or removes items from the UI basket, then the `.jellysync.json` manifest is updated with the new basket contents.
- [x] AC 3: Given an existing `.jellysync.json` without a `basket_items` field, when the daemon reads it, then it parses successfully with an empty basket.

## Additional Context

### Dependencies

- Requires existing JSON-RPC infrastructure.
- Requires Tauri + Axum backend to be running.
- Tying basket updates to LocalStorage and Daemon synchronization requires careful state management to avoid race conditions.

### Testing Strategy

- **Daemon Unit Tests**: Add test in `jellysync-daemon/src/device/tests.rs` to verify that `DeviceManifest` serialization/deserialization works with and without `basket_items`.
- **Manual Verification**:
  1. Connect a test USB device.
  2. Add an artist and a playlist to the basket.
  3. Disconnect and reconnect the USB device.
  4. Verify the UI basket automatically restores the artist and playlist.
  5. Check `.jellysync.json` on the device manually to verify it contains the basket payload.

### Notes

- Storing the basket on the device is an excellent UX improvement, especially since playlists mutate serverside.
- If the jellyfin IDs for the playlist change, that edge case might need handling, but for now trusting the ID is sufficient.

## Review Notes
- Adversarial review completed
- Findings: 10 total, 8 fixed, 2 skipped
- Resolution approach: auto-fix
