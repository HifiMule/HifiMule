# Story 3.6: Auto-Fill Sync Mode (Synchronise All)

Status: ready-for-dev

## Story

As a Convenience Seeker (Sarah),
I want the basket to automatically fill with music from my entire library prioritized by my favorites, most-played, and newest additions,
So that I can fill my device without manually browsing and selecting every album.

## Acceptance Criteria

1. **Auto-Fill Trigger & Population:**
   - Given the Basket sidebar is visible
   - When I enable the "Auto-Fill" toggle
   - Then the daemon queries the Jellyfin library and ranks all music tracks using the priority algorithm: favorites first, then by play count (descending), then by creation date (descending)
   - And the basket populates with tracks up to the device's available capacity or a user-defined size limit
   - And the Storage Projection bar updates in real-time

2. **Manual Selection Priority:**
   - Given Auto-Fill is enabled and I have manually added artists/playlists to the basket
   - When the auto-fill algorithm runs
   - Then manual selections take priority and occupy space first
   - And auto-fill uses the remaining capacity for algorithmically selected tracks
   - And duplicates between manual and auto-fill selections are excluded

3. **Max Fill Size Adjustment:**
   - Given Auto-Fill is active
   - When I adjust the optional "Max Fill Size" slider
   - Then the basket recalculates to respect the new limit
   - And tracks beyond the limit are removed from the basket in reverse priority order

4. **Auto-Sync on Connect Preference:**
   - Given a device is connected and I am viewing the Basket sidebar
   - When I enable the "Auto-Sync on Connect" toggle
   - Then the preference is persisted to the device manifest
   - And future device connections trigger sync automatically without UI interaction
   - And the toggle reflects the current saved state when the device reconnects

5. **Auto-Filled Item Display:**
   - Given Auto-Fill items are displayed in the basket
   - When I view the item list
   - Then auto-filled items show a distinct "Auto" badge to differentiate them from manually added items
   - And each item shows its priority reason (e.g., "★ Favorite", "▶ 47 plays", "New")

## Tasks / Subtasks

- [ ] Task 1: Implement priority ranking algorithm in daemon (AC: #1)
  - [ ] 1.1 Create `auto_fill` module in daemon (`jellyfinsync-daemon/src/auto_fill.rs`)
  - [ ] 1.2 Implement Jellyfin API query to fetch all music tracks with `IsFavorite`, `PlayCount`, `DateCreated` fields
  - [ ] 1.3 Implement priority sorting: favorites first → play count desc → creation date desc
  - [ ] 1.4 Implement capacity-aware truncation using cumulative `sizeBytes` against device free space or `maxFillBytes`
  - [ ] 1.5 Add unit tests for priority algorithm and capacity truncation

- [ ] Task 2: Add `basket.autoFill` RPC method (AC: #1, #2)
  - [ ] 2.1 Register `basket.autoFill` in `rpc.rs` RPC dispatch
  - [ ] 2.2 Params: `{ deviceId: string, maxBytes?: number, excludeItemIds: string[] }`
  - [ ] 2.3 Call auto_fill module, passing `excludeItemIds` (manual selections) for dedup
  - [ ] 2.4 Return ranked item list with priority reason metadata (`favorite`, `playCount`, `new`)
  - [ ] 2.5 Subtract manual selection sizes from available capacity before running algorithm

- [ ] Task 3: Persist auto-fill and auto-sync preferences per device (AC: #1, #3, #4)
  - [ ] 3.1 Add `auto_fill_enabled`, `max_fill_bytes`, and `auto_sync_on_connect` fields to device profile in manifest `.jellyfinsync.json`
  - [ ] 3.2 Add `sync.setAutoFill` RPC method: `{ deviceId, autoFillEnabled, maxFillBytes?, autoSyncOnConnect }`
  - [ ] 3.3 Use Write-Temp-Rename pattern for manifest updates (existing pattern)

- [ ] Task 4: Build Auto-Fill UI toggle, Max Fill Size slider, and Auto-Sync toggle (AC: #1, #3, #4)
  - [ ] 4.1 Add `<sl-switch>` Auto-Fill toggle in `BasketSidebar.ts` header area
  - [ ] 4.2 Add `<sl-range>` Max Fill Size slider (visible only when Auto-Fill is active)
  - [ ] 4.3 Add `<sl-switch>` "Auto-Sync on Connect" toggle below Auto-Fill controls with helper text: "Automatically start syncing when this device is connected. Works with or without the UI open."
  - [ ] 4.4 Wire Auto-Fill toggle to call `basket.autoFill` RPC via existing `rpc_proxy` Tauri command
  - [ ] 4.5 Wire Auto-Sync toggle to call `sync.setAutoFill` RPC to persist `autoSyncOnConnect` preference
  - [ ] 4.6 Wire slider changes to re-trigger auto-fill with updated `maxBytes`
  - [ ] 4.7 Debounce slider changes (500ms) before re-querying
  - [ ] 4.8 On device connect, read saved preferences from manifest and set toggle states accordingly

- [ ] Task 5: Integrate auto-fill items into basket state (AC: #1, #2, #4)
  - [ ] 5.1 Extend `BasketItem` interface in `basket.ts` with `autoFilled: boolean` and `priorityReason: string`
  - [ ] 5.2 When auto-fill response arrives, merge with existing manual items (manual items first)
  - [ ] 5.3 On manual add/remove while auto-fill is active, re-trigger auto-fill with updated `excludeItemIds`
  - [ ] 5.4 Track which items are manual vs auto-filled so `clear()` can optionally clear only auto-filled items

- [ ] Task 6: Render Auto badges and priority reason tags (AC: #4)
  - [ ] 6.1 In `BasketSidebar.ts` item rendering, add "Auto" badge with muted accent color for `autoFilled === true` items
  - [ ] 6.2 Add priority reason inline label: "★ Favorite", "▶ {n} plays", "New"
  - [ ] 6.3 Ensure visual distinction is clear between manual and auto-filled items

- [ ] Task 7: Storage projection integration (AC: #1, #3)
  - [ ] 7.1 Verify existing `getCapacityZone()` and `renderCapacityBar()` update correctly with auto-fill items
  - [ ] 7.2 Ensure capacity bar reflects combined manual + auto-fill size in real-time

## Dev Notes

### Architecture Compliance

- **IPC Pattern**: JSON-RPC 2.0 over localhost HTTP. Use existing `rpc_proxy` Tauri command for all RPC calls from UI (required in release builds due to mixed-content blocking)
- **Serialization**: All JSON-RPC payloads use `camelCase`. Rust structs must use `#[serde(rename_all = "camelCase")]`
- **Manifest Updates**: Use Write-Temp-Rename pattern for all `.jellyfinsync.json` writes (see existing pattern in `device/mod.rs`)
- **Process Model**: Auto-fill algorithm runs daemon-side. UI only sends RPC requests and renders results

### Existing Code to Reuse (DO NOT Reinvent)

| What | Where | How to Reuse |
|------|-------|-------------|
| Basket state management | `jellyfinsync-ui/src/state/basket.ts` | Extend `BasketItem` interface, use existing `add()`, `remove()`, `getTotalSizeBytes()` |
| Basket sidebar UI | `jellyfinsync-ui/src/components/BasketSidebar.ts` | Add toggle/slider to header, modify item rendering for badges |
| Storage projection | `BasketSidebar.ts` lines 59-127 | `getCapacityZone()` and `renderCapacityBar()` already work — just ensure they see auto-fill items |
| RPC dispatch | `jellyfinsync-daemon/src/rpc.rs` | Register new methods in existing match dispatch |
| Jellyfin API queries | `jellyfinsync-daemon/src/api.rs` | Use existing `MUSIC_ITEM_TYPES` constant and `fetch_items` patterns from Story 3.5 |
| Device manifest I/O | `jellyfinsync-daemon/src/device/mod.rs` | Use existing `BasketItem` struct and manifest read/write |
| Daemon-to-UI proxy | `jellyfinsync-ui/src/rpc.ts` | Use existing `rpcCall()` function for all daemon communication |
| Item size fetching | RPC `jellyfin_get_item_sizes` | Already fetches `sizeBytes` for basket items |

### Jellyfin API Details

- **Endpoint**: `GET /Users/{userId}/Items`
- **Required Fields**: `IsFavorite`, `PlayCount`, `DateCreated`, `MediaSources` (for size)
- **Filter**: `IncludeItemTypes=Audio` (individual tracks for accurate sizing)
- **Sort**: Server-side sorting not sufficient (need composite sort); fetch all and sort daemon-side
- **Pagination**: Use `StartIndex` + `Limit` for large libraries. Fetch in pages of 500

### Priority Algorithm Specification

```
Input: All Audio tracks from Jellyfin library
Output: Ranked list truncated to capacity

1. Fetch all Audio items with fields: Id, Name, IsFavorite, PlayCount, DateCreated, MediaSources, AlbumId, AlbumArtist
2. Sort by composite key:
   - Primary: IsFavorite DESC (true=1, false=0)
   - Secondary: PlayCount DESC
   - Tertiary: DateCreated DESC
3. Accumulate sizeBytes from MediaSources[0].Size
4. Truncate when cumulative size exceeds (availableCapacity - manualSelectionSize) or maxFillBytes
5. Tag each item with priorityReason:
   - IsFavorite=true → "favorite"
   - PlayCount > 0 → "playCount" (include count)
   - else → "new"
```

### Device Profile Manifest Extension

Add to `.jellyfinsync.json` optional `autoFill` block:
```json
{
  "autoFill": {
    "enabled": false,
    "maxBytes": null,
    "autoSyncOnConnect": false
  }
}
```

- `enabled`: boolean, default `false`
- `maxBytes`: integer or null (null = fill to device capacity)
- `autoSyncOnConnect`: boolean, default `false` — when true, daemon triggers sync automatically on device detection without UI interaction

### Project Structure Notes

- New daemon module: `jellyfinsync-daemon/src/auto_fill.rs` — add `mod auto_fill;` to `main.rs`
- No new UI files needed — extend existing `BasketSidebar.ts` and `basket.ts`
- Rust tests: co-located `#[cfg(test)] mod tests` in `auto_fill.rs`
- TypeScript: no separate test file needed unless complex logic is added to UI

### Critical Constraints

- **Memory**: Daemon must stay under 10MB RAM. Stream/paginate large libraries — do NOT load entire library into memory at once. Process in batches
- **Deduplication**: When manual items include playlists/albums, expand them to track IDs before passing as `excludeItemIds` to auto-fill. Use existing `sync_calculate_delta` expansion logic as reference
- **Dirty Flag**: Auto-fill changes should trigger `isDirty()` on basket so the "Sync Proposed" banner appears
- **Manifest Safety**: Only persist `autoFill` preferences via Write-Temp-Rename. Never write mid-sync

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 3.6]
- [Source: _bmad-output/planning-artifacts/architecture.md#Auto-Fill Algorithm]
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#Section 5.3 Auto-Fill Components]
- [Source: _bmad-output/planning-artifacts/prd.md#FR29]
- [Source: 3-5-music-only-library-filtering.md — MUSIC_ITEM_TYPES constant, api.rs patterns]

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List
