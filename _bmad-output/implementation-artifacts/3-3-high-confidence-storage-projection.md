# Story 3.3: High-Confidence Storage Projection

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a **Ritualist (Arthur)**,
I want to **know *exactly* how many megabytes my selection will take on my device**,
so that **I don't trigger a "Disk Full" error mid-sync.**

## Acceptance Criteria

1. **Live Size Calculation**: When items are in the Sync Basket and the list changes, the sidebar MUST calculate and display the literal byte-size of the selection (factoring in actual file sizes from Jellyfin `MediaSources`). (AC: #1)
2. **Projected Capacity Bar (3-Zone Emotional Design)**: The sidebar MUST display a visual "Projected Capacity" bar showing used space, projected sync size, and remaining free space with three distinct states: **Green** = Safe (comfortable fit), **Amber** (`#EBB334`) = Tight (selection fits but <10% free remaining), **Red** = Over Limit (selection exceeds available space). Each state MUST include contextual messaging (e.g., Amber: "Tight fit — 42 MB remaining", Red: "Selection exceeds available space by 128 MB"). (AC: #2)
3. **Real-Time Device Free Space**: The daemon MUST report the current free disk space on the connected device, updated when the basket changes or a device is detected. (AC: #3)
4. **Accurate Byte-Level Totals**: Size projections MUST use actual file sizes from Jellyfin's `MediaSources[].Size` field (bytes), NOT duration-based estimates from `cumulativeRunTimeTicks`. (AC: #4)
5. **Intentional No-Device State**: If no device is connected, the sidebar MUST prominently display the total selection size (e.g., "Your selection: 1.2 GB") so users can curate confidently from memory of their device capacity. Show a styled "No device connected" indicator that feels intentional (not broken), with the capacity bar placeholder greyed out or replaced by the selection total. (AC: #5)

## Tasks / Subtasks

- [ ] **T1: Daemon - Jellyfin File Size API** (AC: #1, #4)
    - [ ] T1.1: Add a new RPC method `jellyfin_get_item_sizes(itemIds: Vec<String>)` that fetches each item's `MediaSources` and returns `{id, totalSizeBytes}` for each item.
    - [ ] T1.2: For container items (Albums, Playlists), recursively fetch child items' `MediaSources` and sum their `Size` fields to get the total byte count.
    - [ ] T1.3: Add caching for size lookups to avoid repeated API calls for the same items (in-memory HashMap keyed by item ID, cleared on server reconnect).
- [ ] **T2: Daemon - Device Free Space RPC** (AC: #3)
    - [ ] T2.1: Add a new RPC method `device_get_storage_info()` that returns `{totalBytes, freeBytes, usedBytes, devicePath}` for the currently connected device.
    - [ ] T2.2: Use `std::fs::metadata` / platform-specific calls or the `fs2` crate's `available_space()` / `total_space()` to query disk space on the device mount path.
    - [ ] T2.3: Return an error/null if no device is currently connected.
- [ ] **T3: UI - BasketItem Size Enhancement** (AC: #1, #4)
    - [ ] T3.1: Add `sizeBytes: number` field to the `BasketItem` interface in `basket.ts`.
    - [ ] T3.2: When adding an item to the basket, call `jellyfin_get_item_sizes` (instead of or alongside `jellyfin_get_item_counts`) and populate `sizeBytes`.
    - [ ] T3.3: Add a `getTotalSizeBytes(): number` method to `BasketStore`.
- [ ] **T4: UI - Capacity Bar Component** (AC: #2, #3, #5)
    - [ ] T4.1: Create a `CapacityBar` rendering function (or small component) in `BasketSidebar.ts` that draws a segmented bar: [Used | Projected | Free].
    - [ ] T4.2: Fetch device storage info via `device_get_storage_info` RPC when basket updates and a device is connected.
    - [ ] T4.3: Implement 3-zone color logic: **Green** (comfortable fit, >10% free remaining), **Amber** `#EBB334` (tight fit, <10% free remaining — show "Tight fit — X MB remaining"), **Red** (over limit — show "Selection exceeds available space by X MB" and disable Sync button). Add a subtle checkmark icon in Green state.
    - [ ] T4.4: Display human-readable sizes with scale-appropriate precision: "342 MB", "1.2 GB", "4.7 GB" (NOT raw bytes or excessive decimals). Use `GB` above 1024 MB, `MB` otherwise.
    - [ ] T4.5: No-device state: Show total selection size prominently (e.g., "Your selection: 1.2 GB") with a styled greyed-out capacity bar placeholder and "No device connected" indicator. Must feel intentional, not broken.
- [ ] **T5: UI - BasketSidebar Integration** (AC: #1, #2)
    - [ ] T5.1: Update `BasketSidebar.ts` footer to show total size (e.g., "24 tracks | 342 MB") instead of just track count.
    - [ ] T5.2: Integrate CapacityBar above the "Start Sync" button.
    - [ ] T5.3: Disable "Start Sync" button with clear red messaging when projected size exceeds available space. Show exactly how much to remove (e.g., "Remove 128 MB to fit").

## Dev Notes

### Architecture Patterns & Constraints

- **IPC:** JSON-RPC 2.0 over localhost HTTP. All new RPC methods follow the existing pattern in `rpc.rs` (match on method name string, delegate to handler function).
- **Naming:** Rust uses `snake_case`, TypeScript uses `camelCase`. JSON-RPC payloads use `camelCase` per `#[serde(rename_all = "camelCase")]` convention.
- **Error Handling:** Rust uses `thiserror` for typed errors, `anyhow` at binary level. RPC errors return `JsonRpcError` with code and message.
- **State Management:** BasketStore uses EventTarget pattern for reactive updates. Components subscribe to `'update'` events.

### Technical Specifics

- **Jellyfin API - Getting File Sizes:**
  - The `BaseItemDto` contains a `MediaSources` array. Each `MediaSource` has a `Size` field (bytes, `i64`).
  - For individual audio tracks: call `/Users/{userId}/Items/{itemId}` with `Fields=MediaSources` and read `MediaSources[0].Size`.
  - For containers (Albums/Playlists): use `/Users/{userId}/Items?ParentId={containerId}&IncludeItemTypes=Audio&Fields=MediaSources&Recursive=true` to get all child audio items with their sizes, then sum.
  - **IMPORTANT:** `cumulativeRunTimeTicks` is duration, NOT file size. Do NOT use it for storage projection.

- **Device Storage (Cross-Platform):**
  - Use the `fs2` crate (`fs2::available_space`, `fs2::total_space`) which works cross-platform (Windows, macOS, Linux).
  - Alternatively, use `std::fs::metadata` on the device path and platform-specific statvfs/GetDiskFreeSpaceEx.
  - The device mount path is already tracked in `DeviceManager` via `DeviceEvent::Detected { path, manifest }`. You'll need to store the `path` alongside the manifest in `DeviceManager` so it can be queried later.

- **Caching Strategy:**
  - Size lookups are expensive (one API call per item or batch). Cache results in a `HashMap<String, u64>` in AppState.
  - Clear cache when server connection changes or on explicit refresh.
  - Consider batching: when multiple items are added at once, batch the size requests.

### Learnings from Previous Stories (3.1, 3.2, 3.5)

- **Story 3.2 (Basket):** The `BasketStore` in `jellysync-ui/src/state/basket.ts` already has `sizeTicks` field - this stores `cumulativeRunTimeTicks` which is DURATION not file size. Add a new `sizeBytes` field rather than repurposing `sizeTicks`.
- **Story 3.2 (RPC):** `jellyfin_get_item_counts` in `rpc.rs` already fetches `recursiveItemCount` and `cumulativeRunTimeTicks`. You can extend this or create a parallel method for sizes. Extending is preferred to minimize RPC chatter.
- **Story 3.1 (Image Proxy):** The image proxy pattern at `http://localhost:19140/jellyfin/image` works well. Follow the same localhost RPC pattern.
- **Story 3.5 (Filtering):** Music-only filtering (`MUSIC_ITEM_TYPES`) is already in place. Size calculations should only apply to music items.
- **MediaCard.ts (line ~86-98):** Currently calls `jellyfin_get_item_counts` when adding to basket. This is where to also fetch/populate `sizeBytes`.

### Project Structure Notes

- **Files to CREATE:**
  - None expected (all changes are modifications to existing files)
- **Files to MODIFY:**
  - `jellysync-daemon/src/api.rs`: Add `get_item_sizes()` method that fetches `MediaSources` with `Size` field.
  - `jellysync-daemon/src/rpc.rs`: Add `jellyfin_get_item_sizes` and `device_get_storage_info` RPC handlers.
  - `jellysync-daemon/src/device/mod.rs`: Store device mount path alongside manifest in `DeviceManager`. Add `get_device_storage()` method.
  - `jellysync-ui/src/state/basket.ts`: Add `sizeBytes` to `BasketItem`, add `getTotalSizeBytes()`.
  - `jellysync-ui/src/components/BasketSidebar.ts`: Add CapacityBar rendering, update footer with size display.
  - `jellysync-ui/src/components/MediaCard.ts`: Fetch and populate `sizeBytes` when adding to basket.
- **Files for REFERENCE (do not modify):**
  - `jellysync-ui/src/library.ts`: Understand navigation and item structure.
  - `jellysync-ui/src/rpc.ts`: RPC client wrapper pattern.

### References

- [Story 3.2 (Basket)](file:///c:/Workspaces/JellyfinSync/_bmad-output/implementation-artifacts/3-2-the-live-selection-basket.md) - Basket state management and sidebar
- [Story 3.5 (Music Filtering)](file:///c:/Workspaces/JellyfinSync/_bmad-output/implementation-artifacts/3-5-music-only-library-filtering.md) - API filtering patterns
- [Architecture](file:///c:/Workspaces/JellyfinSync/_bmad-output/planning-artifacts/architecture.md) - IPC and naming conventions
- [UX Design - Basket Layout](file:///c:/Workspaces/JellyfinSync/_bmad-output/planning-artifacts/ux-design-specification.md) - 70/30 split, "Vibrant Hub" theme
- [Jellyfin API - Items](https://api.jellyfin.org/#tag/Items/operation/GetItems) - MediaSources and Size field
- [fs2 crate](https://docs.rs/fs2/latest/fs2/) - Cross-platform disk space queries

## Dev Agent Record

### Agent Model Used

{{agent_model_name_version}}

### Debug Log References

### Completion Notes List

### File List
