---
title: 'Basket-Device Synchronization Linkage'
slug: 'basket-device-sync-linkage'
created: '2026-03-01T21:55:00+01:00'
status: 'ready-for-dev'
stepsCompleted: [1, 2, 3, 4]
tech_stack: ['TypeScript', 'Rust', 'Tauri']
files_to_modify: ['hifimule-ui/src/state/basket.ts', 'hifimule-ui/src/components/BasketSidebar.ts', 'hifimule-daemon/src/rpc.rs', 'hifimule-daemon/src/sync.rs', 'hifimule-ui/src/styles.css']
code_patterns: ['EventTarget store reactivity', 'JSON-RPC', 'Stateful dirty tracking', 'LocalStorage persistence']
test_patterns: []
---

# Tech-Spec: Basket-Device Synchronization Linkage (Revised)

**Created:** 2026-03-01T21:55:00+01:00

## Overview

### Problem Statement

The synchronization process currently treats the media basket as a "batch of additions", but does not strictly enforce that the basket *is* the desired state of the device. Items removed from the basket should be removed from the device during the next sync to maintain a 1:1 linkage. Furthermore, the UI lacks proactive feedback to suggest a sync when the basket's content changes, and the "dirty" state must persist across application restarts.

### Solution

1.  **Strict Linkage**: Ensure the sync process interprets the current basket as the absolute desired state.
2.  **Persistent Sync Proposal**: Track a persistent `dirty` state in `BasketStore` (saved to `localStorage`). Display a "Sync Proposed" indicator using design tokens.
3.  **Conflict Detection**: Set `dirty` flag upon hydration if the local basket differs from the daemon's manifest.
4.  **Empty Basket Support**: Allow sync for empty baskets to clear device contents.

### Scope

**In Scope:**
- Persistent `dirty` state in `BasketStore`.
- Mismatch detection during `hydrateFromDaemon`.
- "Sync Proposed" status zone in `BasketSidebar.ts`.
- Safe `dirty` flag resetting logic (avoiding races with mid-sync changes).
- Enhanced logging in daemon for empty syncs.

**Out of Scope:**
- Background auto-sync.

## Context for Development

### Codebase Patterns

- **UI State**: `basket.ts` uses `localStorage` for item persistence.
- **Tokens**: Use `--sl-color-warning-600` and similar for UI feedback.

### Files to Reference

| File | Purpose |
| ---- | ------- |
| `hifimule-ui/src/state/basket.ts` | Basket state management. |
| `hifimule-ui/src/components/BasketSidebar.ts` | Main sidebar UI and sync trigger. |
| `hifimule-daemon/src/sync.rs` | Core sync logic. |

### Technical Decisions

- **Sticky Dirty Flag**: The `dirty` flag will be saved to `localStorage` key `hifimule_basket_dirty`.
- **Atomic Reset**: `resetDirty()` will only clear the flag if the specific items that were synced haven't changed since the sync started.
- **Status Zone**: Reserve a permanent 32px height vertical space in the sidebar for "Status Messages" to avoid layout jumping.

## Implementation Plan

### Tasks

- [ ] Task 1: Persistent Dirty Flag in `BasketStore`
  - File: `hifimule-ui/src/state/basket.ts`
  - Action: Update `BasketStore` to load/save `_dirty` from `localStorage`.
  - Action: Update `hydrateFromDaemon` to compare local items with daemon items; set `_dirty = true` if items differ.
- [ ] Task 2: UI Status Zone and Banner
  - File: `hifimule-ui/src/styles.css`
  - Action: Add `.basket-status-zone` (fixed height) and `.sync-proposed-banner`. Use Shoelace color tokens.
- [ ] Task 3: Render and Animate Banner
  - File: `hifimule-ui/src/components/BasketSidebar.ts`
  - Action: Update `render()` to use the new status zone. Show banner when `basketStore.isDirty()`.
- [ ] Task 4: Fix Sync Trigger Logic & Empty Sync Logging
  - File: `hifimule-ui/src/components/BasketSidebar.ts`
  - Action: Enable "Start Sync" if `isDirty` or `items.length > 0`.
  - File: `hifimule-daemon/src/sync.rs`
  - Action: Add `info!("Executing empty sync to clear device managed paths")` when no items are selected.
- [ ] Task 5: Race-Safe Sync Completion
  - File: `hifimule-ui/src/components/BasketSidebar.ts`
  - Action: Store a snapshot of item IDs when starting sync.
  - Action: In `handleSyncComplete`, only call `resetDirty()` if the current basket matches the snapshot and the sync was successful.

### Acceptance Criteria

- [ ] AC 1: Given a connected device, when I modify the basket and refresh the app, then the "Sync Proposed" banner persists.
- [ ] AC 2: Given a connected device, when I add an item *during* an active sync, then the "Sync Proposed" banner remains after the first sync completes.
- [ ] AC 3: Given an empty basket on a device with synced content, when I click "Start Sync", then the device managed paths are cleared.
- [ ] AC 4: Given a mismatched local/daemon state, when the app hydrates, then the "Sync Proposed" banner appears.

## Additional Context

### Notes
- Ensure `BasketStore` re-emits `update` on `localStorage` changes if necessary.
