# Architecture — JellyfinSync Daemon

_Generated: 2026-03-08 | Scan Level: Quick | Part: jellyfinsync-daemon | Type: backend_

## Executive Summary

The JellyfinSync Daemon is a Rust background service that runs as a system tray application. It provides a local JSON-RPC 2.0 API for the companion UI application, manages media synchronization with a remote Jellyfin server, handles credential storage, and maintains a local SQLite database for device and scrobble tracking.

## Technology Stack

| Category | Technology | Version |
|----------|-----------|---------|
| Language | Rust | Edition 2021 (MSRV 1.93.0) |
| Async Runtime | Tokio | ~1.49 (multi-thread) |
| HTTP Server | Axum | ~0.8 |
| HTTP Client | Reqwest | ~0.12 (JSON + streaming) |
| Database | rusqlite (SQLite) | ~0.38 (bundled) |
| Serialization | serde + serde_json | ~1.0 |
| System Tray | tray-icon + tao | ~0.19 / ~0.31 |
| Credentials | keyring | 2.3 |
| Notifications | notify-rust | ~4.12 |
| Error Handling | anyhow + thiserror | ~1.0 / ~2.0 |
| Windows APIs | windows-sys | 0.59 |
| Testing | mockito + tempfile | 1.5 / 3 |

## Architecture Pattern

**Service-oriented daemon** with:
- Event-driven main loop (tao event loop for system tray)
- Local HTTP API server (axum, JSON-RPC 2.0)
- External API client (reqwest to Jellyfin server)
- Embedded database (SQLite)
- OS-level credential storage (keyring)

## Module Structure

```
src/
├── main.rs          # Application bootstrap, tray icon setup, event loop
├── rpc.rs           # JSON-RPC 2.0 request router + 24 handler functions
├── api.rs           # Jellyfin HTTP API client + CredentialManager
├── db.rs            # SQLite schema + CRUD operations
├── sync.rs          # Sync engine (delta calculation, file transfer)
├── scrobbler.rs     # Playback history tracking
├── paths.rs         # Platform-specific path resolution
├── tests.rs         # Integration tests
└── device/
    ├── mod.rs       # Device management (storage, folders, init)
    └── tests.rs     # Device module unit tests
```

## API Design

**Protocol:** JSON-RPC 2.0 over HTTP POST on `127.0.0.1:19140`

### Endpoints

| Route | Method | Purpose |
|-------|--------|---------|
| `POST /` | JSON-RPC dispatch | All RPC methods (24 total) |
| `GET /jellyfin/image/{id}` | Image proxy | Proxies Jellyfin artwork with auth |

### RPC Method Groups

- **Auth (4):** `test_connection`, `login`, `save_credentials`, `get_credentials`
- **Jellyfin (5):** `jellyfin_get_views`, `jellyfin_get_items`, `jellyfin_get_item_details`, `jellyfin_get_item_counts`, `jellyfin_get_item_sizes`
- **Device (4):** `device_initialize`, `device_get_storage_info`, `device_list_root_folders`, `set_device_profile`
- **Sync (5):** `sync_calculate_delta`, `sync_execute`, `sync_get_operation_status`, `sync_get_resume_state`, `sync_get_device_status_map`
- **Manifest (6):** `manifest_get_basket`, `manifest_save_basket`, `manifest_get_discrepancies`, `manifest_prune`, `manifest_relink`, `manifest_clear_dirty`
- **State (1):** `get_daemon_state`
- **Scrobbler (1):** `scrobbler_get_last_result`

## Data Architecture

### SQLite Database

**Table: `devices`**
- Device registration and profile information

**Table: `scrobble_history`**
- Media playback history tracking (scrobbling)

### Credential Storage
- OS keyring via `keyring` crate
- Stores: Jellyfin server URL, API token, user ID

## Authentication & Security

- **Jellyfin Auth:** Username/password to API token exchange
- **Token Storage:** OS keyring (not on disk)
- **Local API:** Bound to `127.0.0.1` only (not exposed to network)
- **Image Proxy:** Daemon handles auth headers for Jellyfin image requests

## Testing Strategy

- Unit tests in `tests.rs` and `device/tests.rs`
- HTTP mocking via `mockito`
- Temp file handling via `tempfile`
- Run: `cargo test -p jellyfinsync-daemon`

## System Tray Integration

- Three icon states: default, syncing, error
- Context menu: Open UI, Quit
- Hover tooltip: current daemon status
- Desktop notifications via `notify-rust`
