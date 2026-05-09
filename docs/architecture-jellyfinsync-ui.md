# HifiMule UI — Architecture

**Part:** `hifimule-ui` | **Generated:** 2026-05-07 | **Scan depth:** Exhaustive

---

## Overview

The UI is a **Tauri 2** desktop application that provides a thin shell around the daemon. All business logic lives in the daemon; the UI's responsibility is:
1. Launching and monitoring the daemon process
2. Rendering the library browser and basket using data from the daemon
3. Proxying RPC calls and image requests to the daemon

---

## Process Structure

```
Tauri 2 Shell (Rust)
  ├─ Window: splashscreen (400×500, transparent, no decorations, always-on-top)
  │   └─ splashscreen.html → main.ts → initSplashScreen()
  └─ Window: main (1024×768, initially hidden)
      └─ index.html → main.ts → init()
           ├─ rpcCall('get_daemon_state') → serverConnected?
           │   ├─ true  → initLibraryView()
           │   └─ false → initLoginView()
           └─ renderMainLayout()
                ├─ sl-split-panel (70/30)
                │   ├─ left: library-view → library.ts
                │   └─ right: basket-view → BasketSidebar.ts
                └─ statusbar-container → StatusBar.ts (not wired in current index.html)
```

---

## Tauri Shell (`src-tauri/src/lib.rs`)

### Exposed Tauri Commands

| Command | Params | Returns | Description |
|---------|--------|---------|-------------|
| `rpc_proxy` | `method: String, params: Value` | `Value` or `String` (error) | Forwards JSON-RPC to daemon; extracts `result` or surfaces `error.message` |
| `image_proxy` | `id: String, maxHeight?, quality?` | `String` (data URL) | Fetches image from daemon → base64 data URL |
| `get_sidecar_status` | — | `String` | Returns daemon launch status for splashscreen |

### Daemon Launch Strategy

Executed in a background thread on startup:

1. **Health check** — POST `get_daemon_state` to `http://127.0.0.1:19140`; if OK, daemon is running → status `"startup"`
2. **Windows Service** (Windows only) — `sc start hifimule-daemon`; health check after 2s → status `"service"`
3. **Sidecar spawn** — `app.shell().sidecar("hifimule-daemon").spawn()` → status `"running (pid=N)"`; monitors stdout/stderr/terminated events; kills child on `RunEvent::Exit`

Status strings exposed via `get_sidecar_status`:
- `"starting"` — initial state
- `"startup"` — existing running instance detected
- `"service"` — Windows Service started successfully
- `"running (pid=N)"` — sidecar spawned
- `"spawn_failed: ..."` — sidecar spawn error
- `"command_failed: ..."` — sidecar command creation error
- `"terminated (code=N)"` — sidecar exited unexpectedly

### Logging

`ui_log(msg)` writes to `<AppData>/HifiMule/ui.log` (1 MB cap, truncated on overflow) in addition to `println!`. Windows only (uses `APPDATA` env var).

---

## RPC Layer (`src/rpc.ts`)

```typescript
export const RPC_PORT = '19140';
export const RPC_URL = `http://localhost:${RPC_PORT}`;

export async function rpcCall(method: string, params: any = {}): Promise<any> {
    return await invoke('rpc_proxy', { method, params });
}

export async function getImageUrl(id: string, maxHeight?: number, quality?: number): Promise<string> {
    return await invoke('image_proxy', { id, maxHeight, quality });
}
```

`rpcCall` passes through `invoke`'s error as an `Error` with `getErrorMessage()` normalization (handles plain strings, Error objects, and JSON serialized objects).

---

## State Management

### BasketStore (`state/basket.ts`)

Singleton `BasketStore extends EventTarget`. Holds items selected for the next sync.

```typescript
class BasketStore {
    private items: Map<string, BasketItem>;
    private _dirty: boolean;          // true after any add/remove since last sync
    private _syncingFromDaemon: bool; // prevents re-entrancy during hydration
}
```

**Persistence strategy:**
- `localStorage` for session persistence between page reloads
- Daemon `manifest_save_basket` as the authoritative store (debounced 1s write)
- On device connect: `manifest_get_basket` hydrates the local Map

**Auto-fill slot:**  
A virtual item with `id = "__auto_fill_slot__"` is inserted into the basket when auto-fill is enabled. It carries `sizeBytes` = the available capacity budget. This slot is never persisted to the daemon manifest; it is stripped on load.

**Events:** emits `CustomEvent('update', { detail: items[] })` on every mutation.

### Component-Level State

The `BasketSidebar` component holds most UI state as instance fields:

| Field | Description |
|-------|-------------|
| `storageInfo` | Latest device storage from `device_get_storage_info` |
| `folderInfo` | Latest folders from `device_list_root_folders` |
| `isDirtyManifest` | From `get_daemon_state.dirtyManifest` |
| `connectedDevices` | Multi-device list from `get_daemon_state.connectedDevices` |
| `selectedDevicePath` | From `get_daemon_state.selectedDevicePath` |
| `autoFillEnabled` / `autoFillMaxBytes` | Auto-fill settings (synced to daemon manifest) |
| `autoSyncOnConnect` | Per-device setting (synced to daemon via `device_set_auto_sync_on_connect`) |
| `isSyncing` / `currentOperationId` / `currentOperation` | Active sync tracking |
| `lastHydratedDeviceId` | Tracks which device's basket is currently loaded |

---

## Component Architecture

### `BasketSidebar` (orchestrator)

The main sidebar component owns the UI's sync lifecycle. It runs two polling loops:

- **`daemonStateInterval`** — every 2s: polls `get_daemon_state` to detect device connect/disconnect, dirty manifest, new active operation, and multi-device changes
- **`pollingInterval`** — every 500ms during sync: polls `sync_get_operation_status` to update progress bar

**Render states (mutually exclusive):**
1. Locked (no device selected) → placeholder
2. Empty basket → auto-fill controls + device folder info
3. Basket items → item list + capacity bar + sync button
4. Syncing (starting) → spinner
5. Syncing (in progress) → progress bar + ETA
6. Sync complete → success panel
7. Sync error → error panel

**ETA calculation:** `(totalBytes - bytesTransferred) / (bytesTransferred / elapsedSeconds)` — shown after first byte transferred.

### `MediaCard`

`sl-card`-based grid item. Loaded via `document.createElement('sl-card')`. Features:
- Cover art loaded asynchronously via `getImageUrl(id, 300, 90)` as CSS `background-image`
- `is-selected` CSS class when item is in basket
- `synced` CSS class when item is in `syncedItemIds` from `sync_get_device_status_map`
- Navigation click (on card body) vs. basket toggle click (on `basket-toggle-btn`) are distinguished via `composedPath()`
- When adding to basket: fetches metadata via `jellyfin_get_item_counts` + `jellyfin_get_item_sizes` concurrently

### `StatusBar`

Shows daemon health at the bottom of the window. Polls `get_daemon_state` every 3s via direct `fetch()` (Note: this uses fetch rather than invoke — works in dev mode but may be unreliable in release builds due to mixed content). Listens for `rpc:call`, `rpc:success`, `rpc:error`, `rpc:disconnect` custom window events.

### `InitDeviceModal`

`sl-dialog`-based wizard for initializing a new unrecognized device:
- Loads profiles from `device_profiles.list` and credentials from `get_credentials`
- Fields: device name (max 40 chars), icon picker (6 icons), sync folder path (optional), transcoding profile
- Calls `device_initialize(folderPath, profileId, transcodingProfileId?, name, icon?)` on confirm

### `RepairModal`

`sl-dialog`-based manifest repair tool:
- Loads `manifest_get_discrepancies` → shows two columns: missing (in manifest, not on device) and orphaned (on device, not in manifest)
- Per-item: **Prune** removes from manifest (`manifest_prune`), **Re-link** associates orphan with missing item (`manifest_relink`)
- Bulk: **Prune All Missing** removes all missing items at once
- **Finish & Clear Dirty** calls `manifest_clear_dirty` and closes dialog

---

## Navigation Flow (Library)

```
initLibraryView()
  └─ renderLibrarySelection()
       └─ fetchViews() → filter music + playlists collections
            └─ renderGrid(views, 'libraries')
                 └─ MediaCard.create() per view
                      └─ click → navigateToLibrary(view)
                           └─ loadItems(true) → fetchItems(parentId, MUSIC_ITEM_TYPES, 0, 50)
                                └─ renderGrid(items, 'items', deviceStatus)
                                     ├─ breadcrumbs
                                     ├─ quick-nav bar (A-Z+# for MusicArtist views with ≥20 total)
                                     └─ MediaCard.create() per item
                                          ├─ container click → navigateToItem(item) → push to breadcrumbStack
                                          └─ basket-toggle click → basketStore.add/remove
```

**Page/scroll cache:** `pageCache: Map<parentId, {items, total}>` enables instant back-navigation. `scrollCache: Map<parentId, scrollTop>` restores scroll position after cache hit or fresh load.

**Quick-nav bar:** visible for `MusicArtist` views with `artistViewTotal >= 20`. Letter buttons call `loadItemsByLetter(letter)` which uses `nameStartsWith`/`nameLessThan` parameters to filter. `#` maps to `nameLessThan: 'A'`.

---

## Tauri Configuration (`tauri.conf.json`)

```json
{
  "productName": "HifiMule",
  "version": "0.2.0",
  "identifier": "hifimule.github.io",
  "bundle": {
    "externalBin": ["sidecars/hifimule-daemon"],
    "windows": {
      "wix": { "fragmentPaths": ["wix/startup-fragment.wxs"] },
      "nsis": { "installerHooks": "nsis/hooks.nsh" }
    }
  }
}
```

- **`externalBin`**: the compiled daemon is bundled as a sidecar in `src-tauri/sidecars/` (copied by `scripts/prepare-sidecar.mjs` during build)
- **WiX startup fragment**: registers the daemon (or UI) to run at Windows startup via a registry key
- **NSIS hooks**: custom installer behavior on Windows
- **`security.csp: null`**: CSP disabled (acceptable given daemon runs locally; no remote content)

---

## Build Process

```bash
# Development
cd hifimule-ui
npm run dev          # Starts Vite dev server on :1420 + Tauri in dev mode

# Production
npm run build        # tsc + vite build → dist/
node ../scripts/prepare-sidecar.mjs   # copies daemon binary to src-tauri/sidecars/
npm run tauri build  # Tauri bundles: .dmg / .deb / .exe
```

`prepare-sidecar.mjs` finds the compiled daemon binary for the current platform and copies it to `hifimule-ui/src-tauri/sidecars/hifimule-daemon-<triple>` (Tauri sidecar naming convention).
