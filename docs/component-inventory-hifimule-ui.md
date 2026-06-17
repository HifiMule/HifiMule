# Component Inventory — HifiMule UI

**Generated:** 2026-05-23 | **Last Updated:** 2026-06-17 | **Scan depth:** Deep

---

## Entry Points

### `main.ts`

The DOMContentLoaded handler and application bootstrapper.

**Responsibilities:**
- Detects which window is loaded (main vs splashscreen) by checking `window.location.pathname`
- **Splashscreen path**: polls `rpc_proxy('get_daemon_state')` every 1s until daemon responds, then shows main window and closes splash (timeout: 10s)
- **Main path**: calls `rpcCall('get_daemon_state')` → routes to first-run login, no-server-selected empty state, or library view
- `renderMainLayout()`: injects `sl-split-panel` with library-view left and basket-view right; instantiates `ServerHub` and `BasketSidebar`
- Registers a global `hifimule:server-unauthorized` handler for scoped re-auth when browse RPCs return daemon error `-8`

---

## Views

### Login View (`login.ts`)

Minimal stateless view rendering a provider-neutral media-server connection form.

**Renders:** `sl-card` with `sl-input` fields for server URL, username, and password. The URL field is debounced and calls `server.probe`; known server types render as Shoelace badges (`Jellyfin`, `Subsonic`, `OpenSubsonic`).  
**On submit:** `rpcCall('server.connect', { url, serverType: 'auto', username, password })` → calls `onLoginSuccess()` callback on success, shows error message on failure.  
**Injected into:** `.app-container` (replaces any existing content).

---

### Library View (`library.ts`)

Provider-neutral media browser with mode tabs, hierarchical navigation, pagination, quick-navigation, and favorite tree handling.

**Exported:** `initLibraryView()` (entry), `clearNavigationCache()`

**State:**
```typescript
interface AppState {
    browseMode: BrowseMode;
    availableModes: BrowseMode[];
    parentId?: string;
    breadcrumbStack: { id: string, name: string }[];
    items: BrowseDisplayItem[];
    pagination: { startIndex: number; limit: number; total: number };
    loading: boolean;
    listLoading: boolean;
    scrollCache: Map<string, number>;
    pageCache: Map<string, { items: BrowseDisplayItem[]; total: number }>;
    artistViewTotal: number;
    albumViewTotal: number;
    activeLetter: string | null;
    favoriteTree: FavoriteTree | null;
    listViewMode: 'grid' | 'list';
    selectedIds: Set<string>;
    selectionAnchorIdx: number | null;
}
```

**Navigation flow:**
1. `fetchBrowseModes()` — asks daemon which modes the active provider supports
2. `renderModeBar()` — renders only supported modes
3. `loadModeRoot()` — loads root view for artists, albums, playlists, genres, history, or favorites
4. `navigateToBrowseItem(item)` — drills into artist albums, album tracks, playlist tracks, or genre tracks
5. `navigateToCrumb(index)` — back-navigation via breadcrumb
6. Leaf items (`Audio`) do not navigate — clicking the basket toggle is the action

**Caching:**
- `pageCache`: stores fetched items by `${browseMode}:${parentId ?? 'root'}`; hit path skips the network call entirely
- `scrollCache`: stores `scrollTop` by the same mode-aware key; restored after back-navigation

**Rendering:** Grid view uses `MediaCard`; list view uses virtualized rows with multi-selection, bulk add-to-basket, and bulk add-to-playlist actions. Tracks mode delegates to `TracksBrowseView`.

**Pagination:** Default 50 items per page for normal modes; Tracks mode uses panel-specific pagination constants. "Load More" appears for grid/list modes when `items.length < total`.

**Quick-nav bar:** Renders A-Z + `#` alphabet bar for large artist and album roots. Letter filtering is forwarded to provider-neutral RPCs. `#` = non-alpha names.

**Favorites mode:** `browse.listFavoriteItems` builds a cached `FavoriteTree`. Direct favorite artists/albums keep their original basket IDs. Scoped favorites use synthetic IDs (`favorites:artist:<id>`, `favorites:album:<id>`) so the daemon can expand only the favorite subset during sync.

---

## RPC Layer (`rpc.ts`)

**Exports:**
- `RPC_PORT`, `RPC_URL`, `IMAGE_PROXY_URL` — configuration constants
- `rpcCall(method, params)` — proxies via `invoke('rpc_proxy')`; normalizes errors via `getErrorMessage()`
- `getImageUrl(id, maxHeight?, quality?)` — proxies via `invoke('image_proxy')`; returns data URL
- Provider-neutral browse types (`BrowseMode`, `BrowseArtist`, `BrowseAlbum`, `BrowsePlaylist`, `BrowseTrack`, `BrowseGenre`)
- Provider-neutral browse wrappers (`fetchBrowseModes`, `fetchBrowseArtists`, `fetchBrowseAlbum`, `fetchBrowseFavoriteItems`, etc.)
- Multi-server wrappers (`serverList`, `serverSelect`, `serverUpdate`, `serverRemove`)
- Auto-fill preview wrapper (`previewAutoFill`) for provider-routed pipeline previews

**Error normalization:** `getErrorMessage()` handles plain string errors (from Tauri), `Error` objects, and object errors with `message`/`error`/`details` fields; falls back to JSON serialization. Browse RPCs returning `ERR_UNAUTHORIZED = -8` dispatch `hifimule:server-unauthorized` so `main.ts` can show a re-auth dialog for the selected server.

---

## State Management (`state/basket.ts`)

### `BasketStore`

Singleton class extending `EventTarget`.

**Storage layers (in priority order):**
1. `localStorage` key `"hifimule-basket"` — session persistence
2. Daemon `manifest_save_basket` — authoritative per-device store (debounced 1s write)
3. Daemon `manifest_get_basket` — hydration source when a device connects

**Key methods:**

| Method | Description |
|--------|-------------|
| `add(item)` | Adds item, marks dirty, debounces daemon save |
| `remove(id)` | Removes item, marks dirty, debounces daemon save |
| `toggle(item)` | Add if absent, remove if present |
| `clear()` | Removes all items |
| `has(id)` | Membership check |
| `getItems()` | Returns all items as array |
| `getManualItemIds()` | Returns IDs excluding the auto-fill slot |
| `getManualSizeBytes()` | Total size excluding the auto-fill slot |
| `getTotalSizeBytes()` | Total size including all items |
| `setActiveServerId(serverId)` | Sets the portable active-server id used to tag basket items |
| `reconcileServerIds(servers)` | Rewrites legacy basket tags to portable server IDs |
| `removeItemsForServer(serverId)` | Removes basket entries for a deleted server |
| `hydrateFromDaemon(items)` | Replaces local state from daemon (strips auto-fill slot) |
| `clearForDevice()` | Clears basket (called on device disconnect) |
| `flushPendingSave()` | Immediately writes debounced save (before device switch) |
| `isDirty()` | True after any add/remove since last sync |
| `resetDirty()` | Called after sync completes successfully |

**Auto-fill slot:** Virtual item with `id = "__auto_fill_slot__"`. Carries `sizeBytes` = capacity budget. Never persisted to daemon; stripped on hydration and localStorage load.

**Events:** Emits `CustomEvent('update', { detail: items[] })` on every mutation.

---

## Components

### `BasketSidebar`

The main sidebar panel. Instantiated once per session in `main.ts:renderMainLayout()`.

**Constructor:** `new BasketSidebar(container: HTMLElement)`  
**Lifecycle:** `destroy()` stops all intervals and removes event listeners.

**Polling:**
- `daemonStateInterval`: every **2s** — calls `get_daemon_state`; detects device changes, dirty manifest, active operations, multi-device changes; triggers hydration on new device connect
- `pollingInterval`: every **500ms** during sync — calls `sync_get_operation_status`

**Render states:**
1. **Locked** (`selectedDevicePath === null`) — device hub + initialize button, no basket interaction
2. **Empty basket** — placeholder + auto-fill controls + device folders
3. **With items** — item list + capacity bar + auto-fill controls + sync button
4. **Sync starting** — spinner
5. **Sync in progress** — progress bar + file counter + ETA
6. **Sync complete** — success panel + Done button
7. **Sync failed** — error list + Dismiss button

**Capacity bar zones:**
- `green` — projected size fits with > 10% remaining
- `amber` — fits but < 10% remaining after sync
- `red` — projected size exceeds free bytes (sync button disabled/replaced with "Remove X to fit")

**ETA algorithm:** `(totalBytes - bytesTransferred) / (bytesTransferred / elapsedSeconds)`. Shows after first byte transferred; shows "Almost done" if < 10s.

**Auto-fill controls:**
- `sl-switch` to enable/disable auto-fill
- `sl-range` slider to set max fill size in GB (visible only when enabled and device not full)
- `sl-switch` for auto-sync-on-connect
- Persisted via `sync.setAutoFill`

**Device Hub:** Grid of `device-hub-card` tiles — one per connected device. Clicking switches active device via `device.select` (after flushing basket save). Active device highlighted.

**Dirty manifest banner:** Shows when `dirtyManifest: true` from daemon state. Click opens `RepairModal`.

**Initialize banner:** Shows when `hasManifest: false` from `device_list_root_folders`. Click opens `InitDeviceModal`.

---

### `ServerHub`

Compact header component mounted in `main.ts:renderMainLayout()`.

**Responsibilities:**
- Lists configured servers via `server.list`
- Switches the selected server via `server.select`, then updates `basketStore` with the selected portable `serverId`
- Adds a server by opening `login.ts` in inline add mode
- Edits server display name/icon via `server.update`
- Removes servers via `server.remove` and prunes basket items tagged with that server's portable or legacy local ID
- Logs out via `server.logout`, clears local-only basket state, and re-routes the app

---

### `PlaylistCurationView`

Playlist editing view opened from playlist cards/context actions when the provider advertises playlist-write support.

**Capabilities:** rename/delete playlist, remove tracks by track/album/artist, optimistic reorder, search and add tracks, and display duration/size statistics. Mutating operations call `playlist.rename`, `playlist.delete`, `playlist.removeTracks`, `playlist.reorder`, and `playlist.addTracks`.

---

### `TracksBrowseView`

Tracks-first browser used for the `tracks` browse mode.

**Layout:** artist panel, album panel, track panel; artist/album A-Z strips; infinite-scroll pagination. Track rows support checkbox, Ctrl/Cmd toggle, Shift range selection, bulk add-to-basket, and bulk add-to-playlist.

---

### `AutoFillPanel`

Configurable auto-fill builder UI for per-server pipelines. It edits source blend, filters, ordering, memory/budget, quality, rarity, pity, context, and promotion options, and uses `previewAutoFill()` for live provider-routed previews before persistence.

---

### `MediaCard`

**Factory:** `MediaCard.create(item, mode, isSynced, onNavigate): HTMLElement`

Returns an `sl-card` custom element with:
- Cover art loaded async via `getImageUrl(id, 300, 90)` set as CSS `background-image`; `sl-skeleton` shown until loaded
- **`is-selected`** CSS class when item is in basket
- **`synced`** CSS class + check badge when item is in `syncedItemIds`
- **Selection overlay** (mode === 'items'): `basket-toggle-btn` icon button (plus/minus)
- **Navigation click**: distinguished from basket toggle via `composedPath()`; adds `is-navigating` CSS class during async navigation

**Adding to basket:** Concurrently fetches `jellyfin_get_item_counts` + `jellyfin_get_item_sizes` then calls `basketStore.add()`. These legacy method names are provider-aware in the daemon when a non-Jellyfin provider is active.

**Listening for store updates:** Each card subscribes to `basketStore.addEventListener('update')` to toggle `is-selected` and update the toggle button icon.

---

### `StatusBar`

**Constructor:** `new StatusBar(container: HTMLElement)`

Bottom status bar showing daemon health. Polls `GET http://localhost:19140` (direct fetch, not invoke) every **3 seconds**.

**Shows:**
- Connection dot (green/red) + "Connected" / "Disconnected"
- Daemon state string (Idle, Syncing, Not logged in, etc.)
- Device name (from `deviceMapping.name` or `currentDevice.name`)
- Last RPC method + age (via `rpc:call` window event)
- Last error message (auto-cleared after 10s, via `rpc:error` window event)
- RPC URL

**Note:** Uses direct `fetch()` rather than `invoke('rpc_proxy')`. This works in development but may fail in production Tauri builds due to mixed-content restrictions.

---

### `InitDeviceModal`

**Constructor:** `new InitDeviceModal(container, onComplete?)`  
**Open:** `modal.open()` — fetches credentials + profiles, renders `sl-dialog`

**Form fields:**
- Device name (`sl-input`, max 40 chars, required)
- Icon picker (6 tile options: usb-drive, phone-fill, watch, sd-card, headphones, music-note-list)
- Music folder path (`sl-input`, optional - blank = device root)
- Playlist folder path (`sl-input`, optional - blank = music folder)
- Transcoding profile (`sl-select`, populated from `device_profiles.list`)
- Linked media-server user display (read-only, from `get_credentials`)

**Submits:** `device_initialize(folderPath, playlistFolderPath?, profileId, transcodingProfileId?, name, icon?)`

**States:** loading → form → submitting → success (close) / error (retry + dismiss)

---

### `Device Settings`

Dialog opened from the selected device hub card.

**Fields:**
- Device name (`sl-input`, max 40 chars, required)
- Icon picker using the same six tile options as initialization
- Music folder path (`sl-input`)
- Playlist folder path (`sl-input`, blank = music folder)
- Transcoding profile (`sl-select`, populated from `device_profiles.list`)

**Submits:** `device.update_manifest(deviceId, name, icon, transcodingProfileId, musicFolderPath, playlistFolderPath)`

**States:** form -> saving -> success (close + refresh) / inline error

---

### `RepairModal`

**Constructor:** `new RepairModal(container, onComplete?)`  
**Open:** `modal.open()` — calls `manifest_get_discrepancies`, renders `sl-dialog`

**Layout:** Two-column:
- **Missing** (in manifest, not on device): each item has a **Prune** button → `manifest_prune`
- **Orphaned** (on device, not in manifest): each item has a **Re-link** button → `manifest_relink` (prompts user to select which missing entry to link to if multiple)

**Actions:**
- **Prune All Missing** — bulk prune
- **Finish & Clear Dirty** — calls `manifest_clear_dirty` then closes

**Edge case:** If no discrepancies, shows "No Discrepancies Found" with "Clear Dirty Flag" button.

---

## Shoelace Components Used

| Component | Usage |
|-----------|-------|
| `sl-split-panel` | Main layout (library/basket split) |
| `sl-card` | Media cards, login card |
| `sl-dialog` | InitDeviceModal, RepairModal, ServerHub identity/remove dialogs, playlist curation dialogs |
| `sl-button` | All action buttons |
| `sl-icon-button` | Basket toggle, remove items |
| `sl-badge` | Item count badges |
| `sl-input` | Login form fields, device name input |
| `sl-select` / `sl-option` | Transcoding profile picker |
| `sl-spinner` | Loading states |
| `sl-progress-bar` | Sync progress |
| `sl-range` | Auto-fill max size slider |
| `sl-switch` | Auto-fill / auto-sync toggles |
| `sl-icon` | All icons (Bootstrap Icons CDN via Shoelace) |
| `sl-skeleton` | Image loading placeholder |
| `sl-alert` | Error banners in modals |
