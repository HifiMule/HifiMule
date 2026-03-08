# Data Models — JellyfinSync Daemon

_Generated: 2026-03-08 | Scan Level: Quick | Part: jellyfinsync-daemon_

## Database

- **Engine:** SQLite (via `rusqlite` with bundled SQLite)
- **Location:** Platform-specific app data directory (managed by `paths.rs`)

## Tables

### `devices`

Device registration and sync profile configuration.

```sql
CREATE TABLE IF NOT EXISTS devices (
    -- Schema discovered via quick scan (pattern-based)
    -- Full column details require deep scan
)
```

_Note: Exact column definitions require a deep scan to read `db.rs` in full._

### `scrobble_history`

Media playback history tracking for scrobbling.

```sql
CREATE TABLE IF NOT EXISTS scrobble_history (
    -- Schema discovered via quick scan (pattern-based)
    -- Full column details require deep scan
)
```

_Note: Exact column definitions require a deep scan to read `db.rs` in full._

## Credential Storage

Credentials are stored outside the database using the OS keyring:

| Field | Storage | Description |
|-------|---------|-------------|
| Jellyfin URL | OS Keyring | Server URL |
| API Token | OS Keyring | Authentication token |
| User ID | OS Keyring | Jellyfin user identifier |

Managed by `CredentialManager` in `api.rs` via the `keyring` crate.

## Client-Side State (UI)

### BasketStore (localStorage)

The UI maintains a sync basket in `localStorage`:

| Key | Type | Description |
|-----|------|-------------|
| `jellyfinsync-basket` | JSON string | Serialized basket items array |
| `jellyfinsync-basket-dirty` | string ("true"/"false") | Whether basket has unsaved changes |

### BasketItem Interface

```typescript
interface BasketItem {
    // Item metadata synced between UI localStorage and daemon manifest
    // Full interface details require deep scan
}
```

## Data Flow

```
Jellyfin Server → (reqwest HTTP) → Daemon → (SQLite) → Local DB
                                         → (keyring) → OS Credential Store
                                         → (JSON-RPC) → UI → (localStorage) → Browser Storage
```
