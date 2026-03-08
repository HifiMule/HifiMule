# API Contracts — JellyfinSync Daemon

_Generated: 2026-03-08 | Scan Level: Quick | Part: jellyfinsync-daemon_

## Protocol

- **Type:** JSON-RPC 2.0
- **Transport:** HTTP POST
- **Endpoint:** `http://127.0.0.1:19140/`
- **Content-Type:** `application/json`

### Request Format

```json
{
  "jsonrpc": "2.0",
  "method": "<method_name>",
  "params": { ... },
  "id": 1
}
```

### Response Format

```json
{
  "jsonrpc": "2.0",
  "result": { ... },
  "id": 1
}
```

### Error Response

```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": <error_code>,
    "message": "<error_message>"
  },
  "id": 1
}
```

## REST Endpoints

| Method | Path | Description |
|--------|------|-------------|
| POST | `/` | JSON-RPC dispatch (all methods below) |
| GET | `/jellyfin/image/{id}` | Proxy Jellyfin media artwork (handles auth) |

## RPC Methods

### Authentication

| Method | Params | Description |
|--------|--------|-------------|
| `test_connection` | `{ url, token }` | Validate Jellyfin server URL and API key |
| `login` | `{ url, username, password }` | Authenticate with Jellyfin server, store credentials |
| `save_credentials` | `{ url, token, user_id }` | Manually save credentials to OS keyring |
| `get_credentials` | _none_ | Retrieve stored credentials from OS keyring |

### Jellyfin Data

| Method | Params | Description |
|--------|--------|-------------|
| `jellyfin_get_views` | _none_ | Get library views/collections |
| `jellyfin_get_items` | `{ parentId, ... }` | Get items within a library view |
| `jellyfin_get_item_details` | `{ itemId }` | Get detailed info for a specific item |
| `jellyfin_get_item_counts` | `{ itemIds }` | Get child item counts for items |
| `jellyfin_get_item_sizes` | `{ itemIds }` | Get file sizes for items |

### Device Management

| Method | Params | Description |
|--------|--------|-------------|
| `device_initialize` | `{ ... }` | Initialize local device for sync |
| `device_get_storage_info` | _none_ | Get device storage capacity and usage |
| `device_list_root_folders` | _none_ | List root folders on sync device |
| `set_device_profile` | `{ ... }` | Configure device sync profile |

### Sync Operations

| Method | Params | Description |
|--------|--------|-------------|
| `sync_calculate_delta` | `{ itemIds }` | Calculate sync delta (what needs downloading/removing) |
| `sync_execute` | `{ delta }` | Execute a sync operation based on calculated delta |
| `sync_get_operation_status` | `{ operationId }` | Check progress of running sync operation |
| `sync_get_resume_state` | _none_ | Get state for resuming interrupted sync |
| `sync_get_device_status_map` | _none_ | Get sync status map for all items on device |

### Manifest Management

| Method | Params | Description |
|--------|--------|-------------|
| `manifest_get_basket` | _none_ | Get current sync basket from device manifest |
| `manifest_save_basket` | `{ basketItems }` | Save sync basket to device manifest |
| `manifest_get_discrepancies` | _none_ | Detect discrepancies between manifest and filesystem |
| `manifest_prune` | `{ itemIds }` | Remove orphaned entries from manifest |
| `manifest_relink` | `{ jellyfinId, newLocalPath }` | Re-link a manifest entry to a moved file |
| `manifest_clear_dirty` | _none_ | Clear the dirty flag on the manifest |

### State & Scrobbling

| Method | Params | Description |
|--------|--------|-------------|
| `get_daemon_state` | _none_ | Get current daemon state (connected, syncing, etc.) |
| `scrobbler_get_last_result` | _none_ | Get the last scrobble/playback tracking result |
