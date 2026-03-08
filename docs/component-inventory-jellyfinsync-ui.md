# Component Inventory — JellyfinSync UI

_Generated: 2026-03-08 | Scan Level: Quick | Part: jellyfinsync-ui_

## UI Framework

- **Approach:** Vanilla TypeScript with class-based components (no framework)
- **UI Library:** Shoelace web components (`@shoelace-style/shoelace` ^2.19.1)
- **Styling:** Global CSS (`styles.css`)

## Components

### MediaCard

| Property | Value |
|----------|-------|
| **File** | `src/components/MediaCard.ts` |
| **Type** | Display |
| **Class** | `MediaCard` |
| **Purpose** | Renders a media item card with artwork, title, and metadata |
| **RPC Calls** | `jellyfin_get_item_counts`, `jellyfin_get_item_sizes` |

### BasketSidebar

| Property | Value |
|----------|-------|
| **File** | `src/components/BasketSidebar.ts` |
| **Type** | Layout / Interactive |
| **Class** | `BasketSidebar` |
| **Purpose** | Sync basket panel — displays selected items, storage info, and executes sync operations with progress tracking |
| **RPC Calls** | `device_get_storage_info`, `device_list_root_folders`, `get_daemon_state`, `manifest_get_basket`, `sync_calculate_delta`, `sync_execute`, `sync_get_operation_status` |

### InitDeviceModal

| Property | Value |
|----------|-------|
| **File** | `src/components/InitDeviceModal.ts` |
| **Type** | Modal / Form |
| **Class** | `InitDeviceModal` |
| **Purpose** | Device initialization wizard — configure sync folder and device settings |
| **RPC Calls** | `get_credentials`, `device_initialize` |

### RepairModal

| Property | Value |
|----------|-------|
| **File** | `src/components/RepairModal.ts` |
| **Type** | Modal / Interactive |
| **Class** | `RepairModal` |
| **Purpose** | Manifest repair tool — detect discrepancies, prune orphans, relink moved files |
| **RPC Calls** | `manifest_get_discrepancies`, `manifest_prune`, `manifest_relink`, `manifest_clear_dirty` |

## State Management

### BasketStore

| Property | Value |
|----------|-------|
| **File** | `src/state/basket.ts` |
| **Type** | State Store |
| **Class** | `BasketStore` (extends `EventTarget`) |
| **Export** | `basketStore` (singleton) |
| **Purpose** | Manages sync basket items with dual persistence (localStorage + daemon manifest) |
| **RPC Calls** | `manifest_save_basket`, `jellyfin_get_item_sizes` |

## Pages

### Login Page

| Property | Value |
|----------|-------|
| **File** | `src/login.ts` |
| **Purpose** | Jellyfin server authentication form |
| **RPC Calls** | `login` |

### Library Page

| Property | Value |
|----------|-------|
| **File** | `src/library.ts` |
| **Purpose** | Browse Jellyfin library views and items |
| **RPC Calls** | `jellyfin_get_views`, `jellyfin_get_items`, `sync_get_device_status_map` |

## Utility Modules

### RPC Client

| Property | Value |
|----------|-------|
| **File** | `src/rpc.ts` |
| **Export** | `rpcCall(method, params)` |
| **Purpose** | JSON-RPC 2.0 client — sends HTTP POST to daemon at `127.0.0.1:19140` |

## HTML Entry Points

| File | Purpose |
|------|---------|
| `index.html` | Main application HTML |
| `splashscreen.html` | Startup splash screen (transparent, always-on-top) |

## Design System

- Uses **Shoelace** web components for consistent UI elements
- No custom design tokens or theme configuration detected
- Global styles in `src/styles.css`
