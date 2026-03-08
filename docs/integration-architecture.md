# JellyfinSync — Integration Architecture

_Generated: 2026-03-08 | Scan Level: Quick_

## Overview

JellyfinSync consists of two parts that communicate over a local HTTP API:

```
┌──────────────────────────────────┐     ┌─────────────────────────────────┐
│         jellyfinsync-ui          │     │      jellyfinsync-daemon        │
│    (Tauri 2 Desktop App)         │     │     (Rust Background Service)   │
│                                  │     │                                 │
│  ┌────────────┐  ┌────────────┐  │     │  ┌──────────┐   ┌────────────┐  │
│  │ TypeScript │  │  Tauri     │  │     │  │  Axum    │   │  Jellyfin  │  │
│  │ Frontend   │──│  Rust      │  │     │  │  Server  │   │  API       │  │
│  │ (Shoelace) │  │  Backend   │  │     │  │ (RPC)    │   │  Client    │  │
│  └─────┬──────┘  └────────────┘  │     │  └────┬─────┘   └─────┬──────┘  │
│        │                         │     │       │               │         │
│        │    JSON-RPC 2.0         │     │       │               │         │
│        └─────────────────────────┼─────┼───────┘               │         │
│                                  │     │                       │         │
│                                  │     │  ┌──────────┐         │         │
│                                  │     │  │ SQLite   │         │         │
│                                  │     │  │ Database │         │         │
│                                  │     │  └──────────┘         │         │
│                                  │     │                       │         │
│                                  │     │  ┌──────────┐         │         │
│                                  │     │  │ Keyring  │         │         │
│                                  │     │  │ (creds)  │         │         │
│                                  │     │  └──────────┘         │         │
└──────────────────────────────────┘     └──────────────────┬────┴─────────┘
                                                            │
                                                            ▼
                                                  ┌─────────────────┐
                                                  │  Jellyfin       │
                                                  │  Media Server   │
                                                  │  (Remote)       │
                                                  └─────────────────┘
```

## Integration Points

### 1. UI → Daemon (JSON-RPC 2.0 over HTTP)

| Property | Value |
|----------|-------|
| **Protocol** | JSON-RPC 2.0 over HTTP POST |
| **Endpoint** | `http://127.0.0.1:19140/` |
| **Direction** | UI → Daemon (request/response) |
| **Source** | `jellyfinsync-ui/src/rpc.ts` |
| **Target** | `jellyfinsync-daemon/src/rpc.rs` |

**Method Categories:**

| Category | Methods | Purpose |
|----------|---------|---------|
| Authentication | `login`, `test_connection`, `save_credentials`, `get_credentials` | Jellyfin server auth |
| Jellyfin Data | `jellyfin_get_views`, `jellyfin_get_items`, `jellyfin_get_item_details`, `jellyfin_get_item_counts`, `jellyfin_get_item_sizes` | Browse Jellyfin library |
| Device | `device_initialize`, `device_get_storage_info`, `device_list_root_folders`, `set_device_profile` | Local device management |
| Sync | `sync_calculate_delta`, `sync_execute`, `sync_get_operation_status`, `sync_get_resume_state`, `sync_get_device_status_map` | Media sync operations |
| Manifest | `manifest_get_basket`, `manifest_save_basket`, `manifest_get_discrepancies`, `manifest_prune`, `manifest_relink`, `manifest_clear_dirty` | Sync manifest/basket |
| State | `get_daemon_state` | Daemon state query |
| Scrobbler | `scrobbler_get_last_result` | Playback tracking |

### 2. UI → Daemon (Image Proxy)

| Property | Value |
|----------|-------|
| **Protocol** | HTTP GET |
| **Endpoint** | `http://127.0.0.1:19140/jellyfin/image/{id}` |
| **Direction** | UI → Daemon → Jellyfin Server |
| **Purpose** | Proxy Jellyfin media artwork through daemon (handles auth) |

### 3. Daemon → Jellyfin Server (External API)

| Property | Value |
|----------|-------|
| **Protocol** | HTTP/HTTPS (reqwest client) |
| **Direction** | Daemon → Remote Jellyfin Server |
| **Authentication** | API token (stored in OS keyring) |
| **Purpose** | Library browsing, media download/sync, playback scrobbling |

## Data Flow

### Login Flow
1. User enters Jellyfin URL + username + password in UI
2. UI calls `login` RPC → Daemon authenticates via Jellyfin API
3. Daemon stores API token + user ID in OS keyring via `CredentialManager`
4. Subsequent API calls use stored credentials automatically

### Sync Flow
1. User browses Jellyfin library via UI (`jellyfin_get_views` → `jellyfin_get_items`)
2. User adds items to sync basket (managed by `BasketStore` in UI, persisted via `manifest_save_basket`)
3. User triggers sync: UI calls `sync_calculate_delta` → daemon compares local vs remote
4. UI calls `sync_execute` with delta → daemon downloads media files
5. UI polls `sync_get_operation_status` for progress updates

### State Persistence
- **UI State**: `BasketStore` persists to `localStorage` + syncs to daemon manifest
- **Daemon State**: SQLite database (`devices`, `scrobble_history` tables)
- **Credentials**: OS keyring (via `keyring` crate)

## Shared Dependencies

Both parts share workspace-level Rust dependencies defined in the root `Cargo.toml`:
- `serde` / `serde_json` — Serialization (shared data formats)
- Common version constraints managed at workspace level
