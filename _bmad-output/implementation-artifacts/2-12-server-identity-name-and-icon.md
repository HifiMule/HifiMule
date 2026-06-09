# Story 2.12: Server Identity Name and Icon

Status: done

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

- [x] **Daemon persistence and migration** (AC: #1, #6, #7)
  - [x] Add `name: Option<String>` and `icon: Option<String>` to `ServerConfig` in `hifimule-daemon/src/db.rs` and `ServerRecord` in `hifimule-daemon/src/server_manager.rs`.
  - [x] Extend fresh `server_config` DDL with `name TEXT NULL` and `icon TEXT NULL`.
  - [x] Add idempotent nullable-column migration for existing TEXT UUID tables, and preserve `name`/`icon` when migrating legacy INTEGER single-server tables if present.
  - [x] Backfill existing/pre-migration server rows that have no identity: `name` equals provider type label and `icon` equals provider-type default.
  - [x] Update `row_to_server_config`, `list_servers`, `get_server`, and `get_server_config` SELECT lists in lockstep.
  - [x] Update `upsert_server` so new rows receive default name/icon values, while URL upserts do not erase existing user-edited identity unless explicit values are supplied.

- [x] **Daemon RPC contract** (AC: #2, #5, #6, #7)
  - [x] Dispatch a new `"server.update"` method in `hifimule-daemon/src/rpc.rs`.
  - [x] Implement `handle_server_update`: require `id`, validate server existence, trim and validate `name` if provided, allow `icon: null` to clear icon, reject or ignore `url` if present, whitelist accepted icon identifiers, persist via a DB helper, reload `ServerManager` server records, and return `{ "ok": true }`.
  - [x] Extend `handle_server_connect` to accept optional `name` and `icon`; preserve existing authentication, vault, provider cache, and URL-upsert behavior.
  - [x] Extend `server_row_to_json` and `handle_get_daemon_state` `servers[]` serialization with `name` and `icon`.
  - [x] Do not reconnect providers, rewrite credentials, change selected server, or evict provider cache for identity-only updates.

- [x] **Shared UI identity helper** (AC: #2, #4, #5, #6)
  - [x] Extend `ServerSummary` in `hifimule-ui/src/rpc.ts` with `name: string | null` and `icon: string | null`; add a `serverUpdate` wrapper.
  - [x] Create or colocate one shared `formatServerIdentity(server)` helper used by Server Hub and basket/playlist labels. It must return display label, icon, provider label, host, and tooltip/secondary text.
  - [x] Replace hardcoded `username @ url`, `serverTypeLabel`, and `BasketSidebar.serverDisplayLabel` identity decisions with the shared helper. Keep provider type visible as metadata, not the primary label.
  - [x] Preserve Story 2.11 basket behavior: server identity appears on group labels, not as redundant per-item badges.

- [x] **Server Hub identity editing UI** (AC: #2, #3, #4, #5)
  - [x] Update `hifimule-ui/src/components/ServerHub.ts` rows and trigger chip to render icon + display name as the primary identity, with provider badge/username/host as secondary information.
  - [x] Add an edit affordance for each server row (use an icon button, e.g. `pencil`, with tooltip/label).
  - [x] Add a compact identity editor dialog with required display-name input, icon picker, Save/Cancel controls, loading/error states, and validation before calling `server.update`.
  - [x] Do not include an editable URL field in the identity editor. URL may be displayed as read-only secondary context or tooltip text only.
  - [x] Use Shoelace `<sl-icon name="...">` icon identifiers, following the existing device identity pattern rather than custom SVGs.
  - [x] After save, refresh the Server Hub and trigger the host refresh so basket labels and playlist notices pick up the updated identity.

- [x] **Connect/add-server defaults** (AC: #1, #3, #5, #6)
  - [x] Decide whether the add-server login dialog exposes identity fields immediately or relies on defaults plus later edit. Either path must satisfy AC1 and not disrupt first-run/add/reauth modes.
  - [x] Ensure re-auth mode does not overwrite `name` or `icon`; it should only replace credentials for the existing server URL.
  - [x] Derive default names deterministically from provider type label, not URL, for pre-migration rows and omitted-name connect payloads. Derive provider icons for Jellyfin/OpenSubsonic/Subsonic where available, else generic server/music.

- [x] **i18n and visual polish** (AC: #2, #3, #4)
  - [x] Add all new user-facing strings to `hifimule-i18n/catalog.json` for en/fr/es/de.
  - [x] Add any needed CSS in the existing style locations; keep the Server Hub compact and avoid nested cards.
  - [x] Ensure the compact trigger, menu rows, identity editor, and basket group labels fit at narrow window widths and do not overlap controls.

- [x] **Tests and verification** (AC: all)
  - [x] Add Rust tests for nullable-column migration, pre-migration default name/icon backfill, default identity on insert, preserving edited identity on credential upsert, `server.update`, URL immutability, icon clearing, invalid icon rejection, and `server.list` / `get_daemon_state` payload fields.
  - [x] Add UI-level unit coverage if available for `formatServerIdentity` fallback order.
  - [x] Run `rtk cargo test -p hifimule-daemon`.
  - [x] Run `rtk tsc` for `hifimule-ui`.
  - [x] Run `rtk lint` if the UI lint script exists.

### Review Findings

- [x] [Review][Patch] Add-server defaults can override provider-derived identity before probe completes [hifimule-ui/src/login.ts:50]

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

GPT-5 Codex

### Debug Log References

- 2026-06-09: Started dev-story workflow for Story 2.12; loaded sprint status, project context, and story guidance.
- 2026-06-09: Implemented daemon persistence, migration/backfill, RPC contract, and targeted Rust tests; `rtk cargo test -p hifimule-daemon` passed with 441 tests.
- 2026-06-09: Implemented shared UI identity formatting, Server Hub identity editor, basket group identity labels, i18n strings, and responsive CSS.
- 2026-06-09: Final validation passed: `rtk cargo test` passed with 447 tests; direct TypeScript compiler invocation passed for `hifimule-ui`; no UI lint script exists.
- 2026-06-09: Added server identity fields to first-run login and add-server flows; defaults are prefilled from probed server type while re-auth omits identity metadata.

### Completion Notes List

Ultimate context engine analysis completed - comprehensive developer guide created.
- Daemon server identity metadata is persisted, backfilled, serialized, and editable through `server.update` without reconnecting providers or mutating URLs.
- Server Hub and basket labels now use one shared server identity formatter with configured display names/icons and provider/host secondary metadata.
- First-run and add-server flows now expose server name/icon fields prefilled from detected provider type; re-auth remains credential-only and preserves existing edited identities.
- UI unit test coverage is not available in this package; the helper is covered by TypeScript validation and daemon contract tests cover the persisted/RPC fields.

### File List

- hifimule-daemon/src/api.rs
- hifimule-daemon/src/db.rs
- hifimule-daemon/src/rpc.rs
- hifimule-daemon/src/server_manager.rs
- hifimule-i18n/catalog.json
- hifimule-ui/src/components/BasketSidebar.ts
- hifimule-ui/src/components/ServerHub.ts
- hifimule-ui/src/login.ts
- hifimule-ui/src/rpc.ts
- hifimule-ui/src/serverIdentity.ts
- hifimule-ui/src/styles.css

## Change Log

- 2026-06-09: Story created from approved server identity sprint-change proposal. Status set to ready-for-dev.
- 2026-06-09: Implemented server identity name/icon persistence, RPC update contract, shared UI labels, Server Hub editor, i18n/CSS, and tests. Status set to review.
- 2026-06-09: Refined login/add-server to collect server identity during connect with provider-type defaults.
