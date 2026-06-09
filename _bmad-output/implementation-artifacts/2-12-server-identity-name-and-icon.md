# Story 2.12: Server Identity Name and Icon

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a System Admin (Alexis) and multi-server user,
I want each configured media server to have a custom display name and icon,
so that I can quickly distinguish servers in the hub, switcher, basket, and playlist flows without relying on provider type.

## Acceptance Criteria

1. **Default identity on connect:** Given I add a new server, when the connection succeeds, then the server is persisted with a stable default display name derived from the detected provider type and a default icon based on detected provider type when available.
2. **Edit identity without reconnect:** Given I open the Server Hub / Settings server controls for an existing server, when I edit its display name or icon, then `server.update({ id, name, icon })` persists the metadata and the Server Hub, compact switcher, basket server-group labels, and playlist notices update without reconnecting credentials or replacing provider cache entries.
3. **Icon picker coverage:** Given I choose a server icon, then the picker offers provider icons plus generic music/audio icons such as music note, headphones, library, album, radio, audiobook/book, and generic server. Unsupported or missing provider logos fall back to a generic server or music icon.
4. **Configured identity in multi-server UI:** Given the basket contains items from multiple servers, when basket items render, then each server group label uses the configured server icon and display name. Provider type is secondary metadata or tooltip text only when helpful.
5. **URL is immutable in identity editing:** Given I edit an existing server's identity, when the edit UI or RPC payload is submitted, then URL is not editable and `server.update` does not accept or persist URL changes, because changing URL could break existing server-linked basket items, playlists, sync history, and provider-cache identity.
6. **Pre-migration defaults:** Given a pre-migration or existing server record has no name or icon, when the migration/upgrade runs, then it receives a default icon based on provider type and a default name equal to the provider type label (for example `Jellyfin`, `OpenSubsonic`, or `Subsonic`). Startup is not blocked.
7. **RPC contract:** `server_config` stores nullable `name` and `icon`; `ServerRecord` / `ServerConfig` expose them; `server.connect` accepts optional `name` and `icon`; `server.list` and `get_daemon_state.servers` return `{ id, url, serverType, username, name, icon, selected }`; and `server.update({ id, name?: string, icon?: string | null }) -> { ok: true }` updates only identity metadata. `server.update` must reject or ignore any `url` field.

## Tasks / Subtasks

- [ ] **Daemon persistence and migration** (AC: #1, #6, #7)
  - [ ] Add `name: Option<String>` and `icon: Option<String>` to `ServerConfig` in `hifimule-daemon/src/db.rs` and `ServerRecord` in `hifimule-daemon/src/server_manager.rs`.
  - [ ] Extend fresh `server_config` DDL with `name TEXT NULL` and `icon TEXT NULL`.
  - [ ] Add idempotent nullable-column migration for existing TEXT UUID tables, and preserve `name`/`icon` when migrating legacy INTEGER single-server tables if present.
  - [ ] Backfill existing/pre-migration server rows that have no identity: `name` equals provider type label and `icon` equals provider-type default.
  - [ ] Update `row_to_server_config`, `list_servers`, `get_server`, and `get_server_config` SELECT lists in lockstep.
  - [ ] Update `upsert_server` so new rows receive default name/icon values, while URL upserts do not erase existing user-edited identity unless explicit values are supplied.

- [ ] **Daemon RPC contract** (AC: #2, #5, #6, #7)
  - [ ] Dispatch a new `"server.update"` method in `hifimule-daemon/src/rpc.rs`.
  - [ ] Implement `handle_server_update`: require `id`, validate server existence, trim and validate `name` if provided, allow `icon: null` to clear icon, reject or ignore `url` if present, whitelist accepted icon identifiers, persist via a DB helper, reload `ServerManager` server records, and return `{ "ok": true }`.
  - [ ] Extend `handle_server_connect` to accept optional `name` and `icon`; preserve existing authentication, vault, provider cache, and URL-upsert behavior.
  - [ ] Extend `server_row_to_json` and `handle_get_daemon_state` `servers[]` serialization with `name` and `icon`.
  - [ ] Do not reconnect providers, rewrite credentials, change selected server, or evict provider cache for identity-only updates.

- [ ] **Shared UI identity helper** (AC: #2, #4, #5, #6)
  - [ ] Extend `ServerSummary` in `hifimule-ui/src/rpc.ts` with `name: string | null` and `icon: string | null`; add a `serverUpdate` wrapper.
  - [ ] Create or colocate one shared `formatServerIdentity(server)` helper used by Server Hub and basket/playlist labels. It must return display label, icon, provider label, host, and tooltip/secondary text.
  - [ ] Replace hardcoded `username @ url`, `serverTypeLabel`, and `BasketSidebar.serverDisplayLabel` identity decisions with the shared helper. Keep provider type visible as metadata, not the primary label.
  - [ ] Preserve Story 2.11 basket behavior: server identity appears on group labels, not as redundant per-item badges.

- [ ] **Server Hub identity editing UI** (AC: #2, #3, #4, #5)
  - [ ] Update `hifimule-ui/src/components/ServerHub.ts` rows and trigger chip to render icon + display name as the primary identity, with provider badge/username/host as secondary information.
  - [ ] Add an edit affordance for each server row (use an icon button, e.g. `pencil`, with tooltip/label).
  - [ ] Add a compact identity editor dialog with required display-name input, icon picker, Save/Cancel controls, loading/error states, and validation before calling `server.update`.
  - [ ] Do not include an editable URL field in the identity editor. URL may be displayed as read-only secondary context or tooltip text only.
  - [ ] Use Shoelace `<sl-icon name="...">` icon identifiers, following the existing device identity pattern rather than custom SVGs.
  - [ ] After save, refresh the Server Hub and trigger the host refresh so basket labels and playlist notices pick up the updated identity.

- [ ] **Connect/add-server defaults** (AC: #1, #3, #5, #6)
  - [ ] Decide whether the add-server login dialog exposes identity fields immediately or relies on defaults plus later edit. Either path must satisfy AC1 and not disrupt first-run/add/reauth modes.
  - [ ] Ensure re-auth mode does not overwrite `name` or `icon`; it should only replace credentials for the existing server URL.
  - [ ] Derive default names deterministically from provider type label, not URL, for pre-migration rows and omitted-name connect payloads. Derive provider icons for Jellyfin/OpenSubsonic/Subsonic where available, else generic server/music.

- [ ] **i18n and visual polish** (AC: #2, #3, #4)
  - [ ] Add all new user-facing strings to `hifimule-i18n/catalog.json` for en/fr/es/de.
  - [ ] Add any needed CSS in the existing style locations; keep the Server Hub compact and avoid nested cards.
  - [ ] Ensure the compact trigger, menu rows, identity editor, and basket group labels fit at narrow window widths and do not overlap controls.

- [ ] **Tests and verification** (AC: all)
  - [ ] Add Rust tests for nullable-column migration, pre-migration default name/icon backfill, default identity on insert, preserving edited identity on credential upsert, `server.update`, URL immutability, icon clearing, invalid icon rejection, and `server.list` / `get_daemon_state` payload fields.
  - [ ] Add UI-level unit coverage if available for `formatServerIdentity` fallback order.
  - [ ] Run `rtk cargo test -p hifimule-daemon`.
  - [ ] Run `rtk tsc` for `hifimule-ui`.
  - [ ] Run `rtk lint` if the UI lint script exists.

## Dev Notes

### Current State and Guardrails

- Story 2.11 is complete and must not be regressed. Multi-server routing, provider cache lifecycle, basket retention, read-only foreign-server items, auto-fill server ownership, and playlist server filtering already exist. Story 2.12 is additive identity metadata only. [Source: `_bmad-output/implementation-artifacts/2-11-multi-server-hub.md`]
- `server_config` currently has `id`, `url`, `server_type`, `username`, `server_version`, `updated_at`, and `selected`; there is no name/icon storage yet. The existing migration style is inline/idempotent in `Database::init`, with no versioned migration framework. [Source: `hifimule-daemon/src/db.rs`]
- `ServerManager` owns `servers`, `selected_server_id`, and a lazy provider cache keyed by server UUID. Identity-only updates must refresh records but not reconnect or evict providers. [Source: `hifimule-daemon/src/server_manager.rs`]
- RPC dispatch currently includes `server.connect`, `server.probe`, `server.list`, `server.select`, and `server.remove`, but not `server.update`. [Source: `hifimule-daemon/src/rpc.rs`]
- `server.connect` currently accepts URL/type/username/password, upserts by normalized URL, stores credentials by UUID, updates manager state, and returns `{ ok, serverId, serverType, serverVersion }`. Keep those semantics. [Source: `hifimule-daemon/src/rpc.rs`]
- `server_row_to_json` and `get_daemon_state.servers` are the daemon serialization points the UI consumes. Add `name`/`icon` there rather than inventing a parallel identity fetch. [Source: `hifimule-daemon/src/rpc.rs`]

### UI Patterns to Reuse

- `ServerHub.ts` is the primary UI surface for server list/switch/add/remove. It currently renders `username @ url` and provider badges directly. Replace those local formatting decisions with one shared identity helper. [Source: `hifimule-ui/src/components/ServerHub.ts`]
- `BasketSidebar.ts` keeps `serversById` and currently labels server groups through `serverDisplayLabel`, which returns provider type labels. Extend the stored metadata and use the shared helper so playlist notices and basket group labels do not drift. [Source: `hifimule-ui/src/components/BasketSidebar.ts`]
- Story 2.11 code review refined basket identity: group labels are the sole server indicator for foreign/mixed items; do not re-add per-item server badges. [Source: `_bmad-output/implementation-artifacts/2-11-multi-server-hub.md`]
- `login.ts` handles first-run, add-server, and re-auth modes. Re-auth pre-fills and locks the URL. If identity fields are added to the add flow, re-auth must avoid overwriting identity metadata. [Source: `hifimule-ui/src/login.ts`]
- Device identity already established the icon-picker convention: use Shoelace Bootstrap icon names, not custom SVGs; validate on both client and daemon where practical. [Source: `_bmad-output/implementation-artifacts/2-9-device-identity-name-and-icon.md`]

### Recommended Implementation Details

- Suggested server icon identifiers: `hdd-network`, `server`, `music-note-list`, `music-note-beamed`, `headphones`, `collection-play`, `disc`, `broadcast-pin`, `book`, plus provider-specific identifiers if confirmed available in the Shoelace/Bootstrap icon set. Validate against the same list in UI and daemon.
- Suggested default name mapping: `jellyfin -> Jellyfin`, `openSubsonic -> OpenSubsonic`, `subsonic -> Subsonic`, unknown -> provider type label or generic server label.
- Suggested default icon mapping: `jellyfin -> collection-play` if no Jellyfin logo/icon is already present; `openSubsonic` / `subsonic -> music-note-list`; unknown -> `hdd-network` or `server`.
- Suggested name validation: trim whitespace; require non-empty for explicit updates; cap to 40 characters to match device identity unless product copy says otherwise; store `None` only when the server is allowed to use fallback labels.
- Fallback label helper should parse URL host with `new URL(server.url).host`, guarded for malformed URLs. Avoid showing raw passwords or tokens; only URL host, username, provider type, and configured metadata belong in UI labels.
- `list_servers` currently orders by `updated_at ASC, id ASC`; identity edits must not unexpectedly reshuffle the Server Hub. Prefer preserving the existing row order for `server.update`, or change ordering deliberately and cover it with tests.

### Regression Risks

- **Provider cache regression:** Do not call provider connect, credential save, or cache eviction from `server.update`. Identity changes must be instant metadata updates.
- **URL mutation regression:** Do not make URL editable in the identity editor and do not persist URL through `server.update`; changing URL can break existing server-linked basket items, playlists, sync history, and provider-cache identity.
- **Credential upsert overwrite:** Re-auth and connect-by-existing-URL paths must not erase edited name/icon when the payload omits them.
- **Fallback drift:** Without a shared helper, Server Hub, basket labels, removal toasts, and playlist notices will show different names for the same server.
- **Basket UI regression:** Server labels belong to group dividers (`.basket-server-group-label`) under current 2.11 behavior; per-item badges were intentionally removed.
- **i18n regression:** Existing UI has en/fr/es/de catalogs. Add every new string to all four languages.
- **SQLite migration regression:** Existing users have `server_config` tables already migrated to UUID by Story 2.11. This story must handle both fresh installs and existing UUID tables, plus older single-server legacy tables if encountered.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` Story 2.12]
- [Source: `_bmad-output/planning-artifacts/prd.md` FR45]
- [Source: `_bmad-output/planning-artifacts/architecture.md` server identity amendment]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md` Server Identity Settings]
- [Source: `_bmad-output/planning-artifacts/sprint-change-proposal-2026-06-09-server-identity.md`]
- [Source: `_bmad-output/implementation-artifacts/2-11-multi-server-hub.md` previous story intelligence]
- [Source: `_bmad-output/implementation-artifacts/2-9-device-identity-name-and-icon.md` icon-picker precedent]
- [Source: `hifimule-daemon/src/db.rs`]
- [Source: `hifimule-daemon/src/server_manager.rs`]
- [Source: `hifimule-daemon/src/rpc.rs`]
- [Source: `hifimule-ui/src/rpc.ts`]
- [Source: `hifimule-ui/src/components/ServerHub.ts`]
- [Source: `hifimule-ui/src/components/BasketSidebar.ts`]
- [Source: `hifimule-ui/src/login.ts`]
- [Source: `hifimule-i18n/catalog.json`]

## Dev Agent Record

### Agent Model Used

(to be filled by dev agent)

### Debug Log References

### Completion Notes List

Ultimate context engine analysis completed - comprehensive developer guide created.

### File List

## Change Log

- 2026-06-09: Story created from approved server identity sprint-change proposal. Status set to ready-for-dev.
