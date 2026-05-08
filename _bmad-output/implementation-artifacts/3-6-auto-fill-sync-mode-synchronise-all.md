# Story 3.6: Auto-Fill Sync Mode (Synchronise All)

Status: done

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

- [x] Task 1: Implement priority ranking algorithm in daemon (AC: #1)
  - [x] 1.1 Create `auto_fill` module in daemon (`hifimule-daemon/src/auto_fill.rs`)
  - [x] 1.2 Implement Jellyfin API query to fetch all music tracks with `IsFavorite`, `PlayCount`, `DateCreated` fields
  - [x] 1.3 Implement priority sorting: favorites first → play count desc → creation date desc
  - [x] 1.4 Implement capacity-aware truncation using cumulative `sizeBytes` against device free space or `maxFillBytes`
  - [x] 1.5 Add unit tests for priority algorithm and capacity truncation

- [x] Task 2: Add `basket.autoFill` RPC method (AC: #1, #2)
  - [x] 2.1 Register `basket.autoFill` in `rpc.rs` RPC dispatch
  - [x] 2.2 Params: `{ deviceId: string, maxBytes?: number, excludeItemIds: string[] }`
  - [x] 2.3 Call auto_fill module, passing `excludeItemIds` (manual selections) for dedup
  - [x] 2.4 Return ranked item list with priority reason metadata (`favorite`, `playCount`, `new`)
  - [x] 2.5 Subtract manual selection sizes from available capacity before running algorithm

- [x] Task 3: Persist auto-fill and auto-sync preferences per device (AC: #1, #3, #4)
  - [x] 3.1 Add `auto_fill_enabled`, `max_fill_bytes`, and `auto_sync_on_connect` fields to device profile in manifest `.hifimule.json`
  - [x] 3.2 Add `sync.setAutoFill` RPC method: `{ deviceId, autoFillEnabled, maxFillBytes?, autoSyncOnConnect }`
  - [x] 3.3 Use Write-Temp-Rename pattern for manifest updates (existing pattern)

- [x] Task 4: Build Auto-Fill UI toggle, Max Fill Size slider, and Auto-Sync toggle (AC: #1, #3, #4)
  - [x] 4.1 Add `<sl-switch>` Auto-Fill toggle in `BasketSidebar.ts` header area
  - [x] 4.2 Add `<sl-range>` Max Fill Size slider (visible only when Auto-Fill is active)
  - [x] 4.3 Add `<sl-switch>` "Auto-Sync on Connect" toggle below Auto-Fill controls with helper text: "Automatically start syncing when this device is connected. Works with or without the UI open."
  - [x] 4.4 Wire Auto-Fill toggle to call `basket.autoFill` RPC via existing `rpc_proxy` Tauri command
  - [x] 4.5 Wire Auto-Sync toggle to call `sync.setAutoFill` RPC to persist `autoSyncOnConnect` preference
  - [x] 4.6 Wire slider changes to re-trigger auto-fill with updated `maxBytes`
  - [x] 4.7 Debounce slider changes (500ms) before re-querying
  - [x] 4.8 On device connect, read saved preferences from manifest and set toggle states accordingly

- [x] Task 5: Integrate auto-fill items into basket state (AC: #1, #2, #4)
  - [x] 5.1 Extend `BasketItem` interface in `basket.ts` with `autoFilled: boolean` and `priorityReason: string`
  - [x] 5.2 When auto-fill response arrives, merge with existing manual items (manual items first)
  - [x] 5.3 On manual add/remove while auto-fill is active, re-trigger auto-fill with updated `excludeItemIds`
  - [x] 5.4 Track which items are manual vs auto-filled so `clear()` can optionally clear only auto-filled items

- [x] Task 6: Render Auto badges and priority reason tags (AC: #4)
  - [x] 6.1 In `BasketSidebar.ts` item rendering, add "Auto" badge with muted accent color for `autoFilled === true` items
  - [x] 6.2 Add priority reason inline label: "★ Favorite", "▶ {n} plays", "New"
  - [x] 6.3 Ensure visual distinction is clear between manual and auto-filled items

- [x] Task 7: Storage projection integration (AC: #1, #3)
  - [x] 7.1 Verify existing `getCapacityZone()` and `renderCapacityBar()` update correctly with auto-fill items
  - [x] 7.2 Ensure capacity bar reflects combined manual + auto-fill size in real-time

## Dev Notes

### Architecture Compliance

- **IPC Pattern**: JSON-RPC 2.0 over localhost HTTP. Use existing `rpc_proxy` Tauri command for all RPC calls from UI (required in release builds due to mixed-content blocking)
- **Serialization**: All JSON-RPC payloads use `camelCase`. Rust structs must use `#[serde(rename_all = "camelCase")]`
- **Manifest Updates**: Use Write-Temp-Rename pattern for all `.hifimule.json` writes (see existing pattern in `device/mod.rs`)
- **Process Model**: Auto-fill algorithm runs daemon-side. UI only sends RPC requests and renders results

### Existing Code to Reuse (DO NOT Reinvent)

| What | Where | How to Reuse |
|------|-------|-------------|
| Basket state management | `hifimule-ui/src/state/basket.ts` | Extend `BasketItem` interface, use existing `add()`, `remove()`, `getTotalSizeBytes()` |
| Basket sidebar UI | `hifimule-ui/src/components/BasketSidebar.ts` | Add toggle/slider to header, modify item rendering for badges |
| Storage projection | `BasketSidebar.ts` lines 59-127 | `getCapacityZone()` and `renderCapacityBar()` already work — just ensure they see auto-fill items |
| RPC dispatch | `hifimule-daemon/src/rpc.rs` | Register new methods in existing match dispatch |
| Jellyfin API queries | `hifimule-daemon/src/api.rs` | Use existing `MUSIC_ITEM_TYPES` constant and `fetch_items` patterns from Story 3.5 |
| Device manifest I/O | `hifimule-daemon/src/device/mod.rs` | Use existing `BasketItem` struct and manifest read/write |
| Daemon-to-UI proxy | `hifimule-ui/src/rpc.ts` | Use existing `rpcCall()` function for all daemon communication |
| Item size fetching | RPC `jellyfin_get_item_sizes` | Already fetches `sizeBytes` for basket items |

### Jellyfin API Details

- **Endpoint**: `GET /Items?userId={userId}`
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

Add to `.hifimule.json` optional `autoFill` block:
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

- New daemon module: `hifimule-daemon/src/auto_fill.rs` — add `mod auto_fill;` to `main.rs`
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

claude-sonnet-4-6

### Debug Log References

- Fixed: `DeviceManifest` struct literal exhaustiveness — added `auto_fill: AutoFillPrefs::default()` across all test files (`device/tests.rs`, `sync.rs`, `tests.rs`, `rpc.rs`) using Python batch script after initial `#[cfg(test)]` replace_all missed `auto_sync_on_connect: true` variants and `JellyfinItem` struct literals in sync.rs tests.

### Completion Notes List

- **Task 1**: Created `hifimule-daemon/src/auto_fill.rs` with `rank_and_truncate()` pure function (unit-testable without network) and `run_auto_fill()` async wrapper. Extended `JellyfinItem` in `api.rs` with optional `user_data: Option<JellyfinUserData>` and `date_created: Option<String>`. Added `get_audio_tracks_for_autofill()` to `JellyfinClient` that paginates in 500-item batches with `IncludeItemTypes=Audio&Recursive=true&Fields=MediaSources,UserData,DateCreated`. Added 6 unit tests covering all sort keys, capacity truncation, exclude list, empty library, and zero-capacity edge cases.
- **Task 2**: Registered `basket.autoFill` in `rpc.rs` dispatch. Handler resolves `maxFillBytes` from params or falls back to device free space, calls `auto_fill::run_auto_fill`, returns ranked `AutoFillItem` list with `priorityReason`.
- **Task 3**: Added `AutoFillPrefs { enabled: bool, max_bytes: Option<u64> }` struct with `#[serde(rename_all = "camelCase")]` and `Default` to `device/mod.rs`. Added `auto_fill: AutoFillPrefs` field to `DeviceManifest`. Added `save_auto_fill_prefs()` to `DeviceManager`. Added `sync.setAutoFill` RPC handler that persists prefs to manifest AND updates `auto_sync_on_connect` in both manifest and DB. Updated `get_daemon_state` to expose `autoFill` object to UI.
- **Task 4**: Added `autoFillEnabled`, `autoFillMaxBytes`, `autoSyncOnConnect`, `autoFillDebounceTimer` state fields to `BasketSidebar`. Added `renderAutoFillControls()` rendering Auto-Fill toggle, conditional GB slider (visible only when enabled), and Auto-Sync toggle with helper text. Added `bindAutoFillEvents()` for `sl-change` events with 500ms debounce on slider. On device connect, reads saved prefs from daemon state and re-triggers auto-fill if enabled.
- **Task 5**: Extended `BasketItem` interface with optional `autoFilled?: boolean` and `priorityReason?: string`. Added `replaceAutoFilled()`, `getManualItemIds()`, and `getManualSizeBytes()` to `BasketStore`. `replaceAutoFilled` preserves manual items and replaces only auto-fill items atomically.
- **Task 6**: Updated `renderItem()` to show "Auto" badge (`.basket-item-auto-badge`) and priority reason label (`.basket-item-priority-reason`) for auto-filled items. `renderPriorityLabel()` maps `"favorite"→"★ Favorite"`, `"playCount:N"→"▶ N plays"`, `"new"→"New"`. Auto-filled cards get `.basket-item-auto` CSS class for visual distinction.
- **Task 7**: Verified `getTotalSizeBytes()` already sums all items including auto-filled, `renderCapacityBar()` uses that total — no changes needed. Existing storage projection correctly reflects combined manual + auto-fill size in real-time.
- **Build**: 141 tests pass, 0 errors, 1 pre-existing warning (unrelated scrobbler fields).

### File List

- `hifimule-daemon/src/auto_fill.rs` — NEW: auto-fill priority ranking module with unit tests
- `hifimule-daemon/src/api.rs` — MODIFIED: extended `JellyfinItem` with `user_data`/`date_created`; added `JellyfinUserData` struct; added `get_audio_tracks_for_autofill()` method
- `hifimule-daemon/src/device/mod.rs` — MODIFIED: added `AutoFillPrefs` struct; added `auto_fill` field to `DeviceManifest`; added `save_auto_fill_prefs()` to `DeviceManager`
- `hifimule-daemon/src/rpc.rs` — MODIFIED: registered `basket.autoFill` and `sync.setAutoFill` in dispatch; added `handle_basket_auto_fill()` and `handle_sync_set_auto_fill()` handlers; updated `get_daemon_state` to expose `autoFill` prefs; fixed all `DeviceManifest` test literals
- `hifimule-daemon/src/main.rs` — MODIFIED: added `mod auto_fill;`; updated TODO comment
- `hifimule-daemon/src/device/tests.rs` — MODIFIED: added `auto_fill` field to all `DeviceManifest` test literals
- `hifimule-daemon/src/sync.rs` — MODIFIED: added `auto_fill` field to `DeviceManifest` test literal; added `user_data`/`date_created` to `JellyfinItem` test literals
- `hifimule-daemon/src/tests.rs` — MODIFIED: added `auto_fill` field to `DeviceManifest` test literals
- `hifimule-ui/src/state/basket.ts` — MODIFIED: extended `BasketItem` with `autoFilled`/`priorityReason`; added `replaceAutoFilled()`, `getManualItemIds()`, `getManualSizeBytes()`
- `hifimule-ui/src/components/BasketSidebar.ts` — MODIFIED: added auto-fill state fields; added `triggerAutoFill()`, `scheduleAutoFill()`, `persistAutoFillPrefs()`, `renderAutoFillControls()`, `bindAutoFillEvents()`, `renderPriorityLabel()`; updated `render()` to include controls; updated `renderItem()` with badges; loads prefs on device connect

### Change Log

- Added Story 3.6 Auto-Fill Sync Mode: daemon priority ranking algorithm, basket.autoFill and sync.setAutoFill RPC methods, AutoFillPrefs manifest persistence, Auto-Fill UI controls with debounced slider, Auto badge rendering (Date: 2026-03-19)
- Code review fixes (Date: 2026-03-29): P1 race condition (await basket hydration before triggerAutoFill on reconnect); P2 in-flight guard on triggerAutoFill with pending-retrigger; P3 expand_exclude_ids "Me" userId removed, empty-vec on error; P4 auto_fill pagination total==0 guard (serde default regression); P5/P6 update_manifest returns Err when no device; P7 replaceAutoFilled two-pass Map deletion; P8 two-level container expansion; P9 chunked get_items_by_ids (50 IDs/request); P10 slider NaN guard; P11 device-full notice in Auto-Fill controls; P12 sizeBytes ?? 0; P13 priorityReason fallback for hydrated items; P14 ERR_INTERNAL_ERROR for serialization errors
