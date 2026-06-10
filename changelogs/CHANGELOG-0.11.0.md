# HifiMule 0.11.0

Release date: 2026-06-10

## Highlights

- **Multi-server support**: HifiMule is no longer limited to a single media server. A new **Server Hub** lets you configure multiple servers (any mix of Jellyfin, Subsonic, and Navidrome), switch the active one, and add or remove servers without reconfiguring the app or losing your basket. The basket can hold items from several servers at once, and syncs route each item to the server it came from.
- **Server identity (name & icon)**: Each server can be given a custom display name and icon, so you can tell them apart at a glance in the hub, the switcher, the basket, and playlist flows — instead of relying on the provider type.
- **Portable server identity**: Servers now have a stable, machine-independent identity written into device manifests. A device synced on one machine is recognized on another, and removing then re-adding the same server no longer triggers a needless full resync. This change is invisible in the UI.
- **Jellyfin playlist rename fix**: Renaming a Jellyfin playlist no longer fails on newer servers.

---

## Added

### Multi-Server Hub (Story 2.11)

Complete, end-to-end multi-server support replacing the previous single-server model:

- **Server Hub UI** (`components/ServerHub.ts`): a persistent hub (in Settings → Servers and a compact header selector) that lists every configured server with its display name, detected type badge (Jellyfin / Subsonic / OpenSubsonic), and username. The selected server is highlighted.
- **Switch servers**: clicking a server calls `server.select`; the daemon persists the selection and the library browser reloads with that server's content.
- **Add servers**: "Add Server" presents the connection form inline, appending the new server without disrupting the currently selected one. Connecting upserts by normalized URL (re-adding an existing URL updates its credentials).
- **Remove servers**: removing a server deletes its stored credentials, evicts it from the provider cache, and clears its items from the basket (with a notification). Removing the active server reselects the first remaining server, or drops to the first-run login if none remain.
- **Per-server re-authentication**: an expired or invalid token surfaces a re-auth prompt scoped to that specific server's URL, replacing only that server's credential.
- **No-server / first-run states**: when servers exist but none is selected, the browser prompts "Select a server to browse your library" and add/sync actions are disabled; when no server is configured at all, the full-screen first-run login is shown.

### Mixed-server basket & sync routing (folds in Stories 3.2, 3.8)

- The basket can contain items from multiple servers simultaneously. Items are grouped by server, and the sync pipeline routes each group to the correct provider.
- Items that don't belong to the currently selected server render as **read-only / locked** (rather than being deleted) when you switch servers, and become editable again when their server is active.
- Auto-fill is bound per server.

### Cross-server playlist scope (folds in Stories 11.4, 11.5)

- `playlist.create` validates that a playlist's tracks all come from a single server, and the Save-as-Playlist UI pre-filters the selection and shows a cross-server notice when needed.

### Server identity — name & icon (Story 2.12)

- Each server is persisted with a custom **display name** and **icon**. New servers get a sensible default name and icon derived from the detected provider type.
- A compact identity editor (with an icon picker covering provider logos plus generic music/audio icons — music note, headphones, library, album, radio, audiobook, generic server) lets you rename or re-icon a server via `server.update`, **without reconnecting** credentials or evicting the provider cache.
- The configured name and icon are used consistently across the Server Hub, the compact switcher, basket server-group labels, and playlist notices, via a shared `formatServerIdentity` helper.
- The server URL is immutable when editing identity — `server.update` rejects URL changes to avoid breaking server-linked basket items, playlists, and sync history.

### Portable server identity (Story 2.13)

- A second, **deterministic** identity (`server_config.server_id`) is now derived on connect from `sha256("v1|" + serverType + "|url:" + canonicalBaseUrl + "|" + username)`, preferring a server-reported id (Jellyfin `System/Info.Id`) when available. The existing random `id` is retained as the machine-local key for credentials and the provider cache.
- This portable id is what gets written into device manifests, basket items, and sync routing, so:
  - the **same server resolves to the same id across machines**, and
  - **removing then re-adding** a server re-derives the same id — existing manifest items still match, avoiding a spurious full resync.
- **Idempotent reconciliation** rewrites items tagged with an old random id (Story 2.11) or the pre-2.11 composite `type|url|user` to the new deterministic id on load; it never blocks startup.

---

## Changed

- **Daemon — ServerManager**: `AppState.provider` / `server_type` / `server_version` were replaced by a `ServerManager` (`server_manager.rs`) holding the server list, the selected server, and a lazy provider cache keyed by machine-local id. `require_provider()` returns the selected server's provider; all existing `browse.*`, `sync.*`, scrobble, and playlist call sites keep working unchanged.
- **RPC contract** (additive): `server.connect` returns both the portable `serverId` and the machine-local `localId`; `server.list` and `get_daemon_state.servers[]` return `{ id, serverId, url, serverType, username, name, icon, selected }`; `get_daemon_state` adds `selectedServerId` (local) and `selectedServerPortableId`. New RPCs: `server.list`, `server.select`, `server.remove`, `server.update`. Sync RPCs (`sync_calculate_delta`, `sync_detect_changes`, `sync_execute`) thread a per-item `serverId`.
- **UI coherence**: active-server tracking, basket item tagging, and the read-only/locked comparison all operate on the portable `server_id` end-to-end, so a single-server user's own items never appear locked or foreign.

---

## Fixed

- **Jellyfin playlist rename**: renaming a Jellyfin playlist failed with HTTP 400 on newer servers because the posted item DTO included read-only `UserData`. The fetched item is now sanitized to drop user-scoped metadata before the update POST.

---

## Internal

- New `server_manager.rs` module; substantial growth in `rpc.rs` and `db.rs` to host server CRUD, the portable-id derivation/migration/backfill, and reconciliation.
- Schema: `server_config` gains nullable `name`, `icon`, `server_id`, and `server_reported_id` columns, each added via idempotent inline migrations that handle fresh installs and existing UUID tables without blocking startup.
- `sha2 = "0.10"` added to the daemon for the SHA-256 portable-id basis (blake3 remains reserved for the hardware-uid vault derivation).
- New UI helpers: `serverIdentity.ts` (`formatServerIdentity`), `serverUpdate` RPC wrapper in `rpc.ts`.
- Tests cover derivation determinism, cross-machine equality, remove/re-add no-resync, manifest + basket reconciliation idempotency, schema migration/backfill, and the Jellyfin rename regression.
- i18n catalog extended with all new multi-server, identity-editing, and notice strings across English, French, Spanish, and German.
