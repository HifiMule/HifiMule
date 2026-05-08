# Component Inventory — HifiMule UI

**Generated:** 2026-05-07 | **Scan depth:** Exhaustive

---

## Entry Points

### `main.ts`

The DOMContentLoaded handler and application bootstrapper.

**Responsibilities:**
- Detects which window is loaded (main vs splashscreen) by checking `window.location.pathname`
- **Splashscreen path**: polls `rpc_proxy('get_daemon_state')` every 1s until daemon responds, then shows main window and closes splash (timeout: 10s)
- **Main path**: calls `rpcCall('get_daemon_state')` → routes to login or library view
- `renderMainLayout()`: injects `sl-split-panel` (70/30) with library-view left and basket-view right; instantiates `BasketSidebar`

---

## Views

### Login View (`login.ts`)

Minimal stateless view rendering a Jellyfin connection form.

**Renders:** `sl-card` with `sl-input` fields for URL, username, and password.  
**On submit:** `rpcCall('login', { url, username, password })` → calls `onLoginSuccess()` callback on success, shows error message on failure.  
**Injected into:** `.app-container` (replaces any existing content).

---

### Library View (`library.ts`)

Hierarchical media browser with pagination and quick-navigation.

**Exported:** `initLibraryView()` (entry), `fetchViews()`, `fetchItems()`, `fetchDeviceStatusMap()`, `clearNavigationCache()`

**State:**
```typescript
interface AppState {
    view: 'libraries' | 'items';
    libraryId?: string;
    parentId?: string;
    breadcrumbStack: { id: string, name: string }[];
    items: JellyfinItem[];
    pagination: { startIndex: number; limit: number; total: number };
    loading: boolean;
    scrollCache: Map<string, number>;
    pageCache: Map<string, { items: JellyfinItem[]; total: number }>;
    artistViewTotal: number;
    activeLetter: string | null;
}
```

**Navigation flow:**
1. `renderLibrarySelection()` — root: shows filtered Jellyfin views (music + playlists collections only)
2. `navigateToLibrary(view)` — enters a library root
3. `navigateToItem(item)` — navigates into container items (Album, Artist, Playlist, Folder, etc.)
4. `navigateToCrumb(index)` — back-navigation via breadcrumb
5. Leaf items (Audio, MusicVideo) do not navigate — clicking the basket toggle is the action

**Caching:**
- `pageCache`: stores fetched items by `parentId`; hit path skips the network call entirely
- `scrollCache`: stores `scrollTop` by `parentId`; restored after back-navigation

**Pagination:** Default 50 items per page; "Load More" button appears when `items.length < total`. Hidden during letter-filtered views.

**Quick-nav bar:** Renders A-Z + `#` alphabet bar when `artistViewTotal >= 20`. Letter filter uses `nameStartsWith` / `nameLessThan` params. `#` = non-alpha names (`nameLessThan: 'A'`). Clicking active letter resets filter.

---

## RPC Layer (`rpc.ts`)

**Exports:**
- `RPC_PORT`, `RPC_URL`, `IMAGE_PROXY_URL` — configuration constants
- `rpcCall(method, params)` — proxies via `invoke('rpc_proxy')`; normalizes errors via `getErrorMessage()`
- `getImageUrl(id, maxHeight?, quality?)` — proxies via `invoke('image_proxy')`; returns data URL

**Error normalization:** `getErrorMessage()` handles plain string errors (from Tauri), `Error` objects, and object errors with `message`/`error`/`details` fields; falls back to JSON serialization.

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

### `MediaCard`

**Factory:** `MediaCard.create(item, mode, isSynced, onNavigate): HTMLElement`

Returns an `sl-card` custom element with:
- Cover art loaded async via `getImageUrl(id, 300, 90)` set as CSS `background-image`; `sl-skeleton` shown until loaded
- **`is-selected`** CSS class when item is in basket
- **`synced`** CSS class + check badge when item is in `syncedItemIds`
- **Selection overlay** (mode === 'items'): `basket-toggle-btn` icon button (plus/minus)
- **Navigation click**: distinguished from basket toggle via `composedPath()`; adds `is-navigating` CSS class during async navigation

**Adding to basket:** Concurrently fetches `jellyfin_get_item_counts` + `jellyfin_get_item_sizes` then calls `basketStore.add()`.

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
- Sync folder path (`sl-input`, optional — blank = device root)
- Transcoding profile (`sl-select`, populated from `device_profiles.list`)
- Linked Jellyfin user display (read-only, from `get_credentials`)

**Submits:** `device_initialize(folderPath, profileId, transcodingProfileId?, name, icon?)`

**States:** loading → form → submitting → success (close) / error (retry + dismiss)

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
| `sl-dialog` | InitDeviceModal, RepairModal |
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
