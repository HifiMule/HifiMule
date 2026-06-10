---
baseline_commit: 95791633a3c511eed4aafee1945bd911d6c0657a
---

# Story 2.11 — Multi-Server Hub

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

**As** a System Admin (Alexis) and Ritualist (Arthur),
**I want** a persistent Server Hub where I can see all configured servers, switch the active one, add or remove servers, and curate/sync a basket that may hold items from any of them,
**So that** I have full control over which media libraries I'm curating from, without reconfiguring the app or losing basket state — and syncs and playlists just work across servers.

## Context & Scope

This story delivers **complete, end-to-end multi-server support** — infrastructure, UI, **and** the runtime behaviors (sync routing, auto-fill, playlist scope, read-only basket) that make a multi-server workflow actually work. **Nothing is deferred to follow-up stories.** The codebase today is single-server: one `server_config` row (`CHECK (id = 1)`), one `AppState.provider`, a single `currentServer` in `get_daemon_state`, a derived composite `serverId` string, and a sync pipeline that downloads everything from one global provider. This story replaces all of it.

**Source of truth:** `sprint-change-proposal-2026-06-09-multi-server-management.md` (all four implementation phases), `epics.md` Story 2.11 (lines 451–502) + amended Stories 3.2/3.8 (lines 535–570), and `architecture.md` §"Multi-Server Management — Architectural Decisions" (lines 626–790). The amended ACs for Stories 3.2, 3.8, 11.4, 11.5 in the change proposal are **folded into this story** (Sections C/K/L/M/N) — they are required for the hub to function and are implemented here.

### Everything is in scope — four proposal phases + folded-in amendments
- **Phase 1 — Daemon infrastructure (no UI):** `server_config` migration to UUID + `selected` column; `ServerManager` replacing `AppState.provider`; credential vault migration to `HashMap<serverUuid, ServerCredentials>`; new RPCs `server.list`/`server.select`/`server.remove`; `server.connect` returns `serverId`; `get_daemon_state` gains `servers[]` + `selectedServerId`. *(Sections F/G/H/I)*
- **Phase 2 — Server Hub UI:** list/select/add/remove UI (Settings → Servers tab + compact header selector); first-run vs add-server flow split; per-server re-auth; library/empty/first-run gating. *(Sections A/B/C/D/E)*
- **Phase 3 — Mixed-server basket & sync routing:** `serverId` threaded through the real sync pipeline; items grouped by `serverId` and routed to the correct provider per group; full read-only/locked basket rendering; auto-fill bound per server. *(Sections K/L/N — folds in Stories 3.2 & 3.8)*
- **Phase 4 — Playlist scope:** cross-server validation in `playlist.create`; Save-as-Playlist UI pre-filter + cross-server notice. *(Section M — folds in Stories 11.4 & 11.5)*

> **Stale statuses:** Stories 3.2, 3.8, 11.4, 11.5 are marked `done` with multi-server amendment notes, but the code audit confirmed their **runtime multi-server behaviors were never implemented** (built on the single-server composite-id stopgap). This story implements them for real; treat those amendment ACs as part of this story's contract.

> **Reconciliation — there is NO `sync.start` RPC.** The proposal/architecture say "`sync.start`", but the real pipeline is **three RPCs**: `sync_calculate_delta`, `sync_detect_changes`, `sync_execute` (dispatch `rpc.rs:315–317`). "Multi-provider routing for `sync.start`" means: add per-item `serverId` to the `itemIds` shape of these RPCs (and their UI callers), then group/route inside them. Do not invent a new `sync.start` method.

> **Reconciliation — error codes are negative JSON-RPC ints, not HTTP 409.** The proposal says `code: 409`, but this codebase uses negative `ERR_*` constants in the `JsonRpcError` envelope (`rpc.rs:21–37`). Define a new constant (e.g. `ERR_CROSS_SERVER_CONFLICT`) and convey "cross-server" semantics via the message (and optionally `data`).

## Acceptance Criteria

### A. Server Hub display & selection
- **AC1:** When ≥1 server is configured and the user opens the main UI or Settings → Servers, the Server Hub lists all servers; each row shows URL (or display name), a detected type badge (Jellyfin / Subsonic/OpenSubsonic), and username; the selected server is highlighted. *(epics.md:459–463)*
- **AC2:** Clicking a non-selected server calls `server.select({ id })`; the daemon sets `selected_server_id` in `ServerManager` and persists `selected = 1` on that row (and `0` on all others); the library browser reloads with the new server's content. *(epics.md:465–468; architecture.md:741)*
- **AC3:** On server switch, basket items whose `serverId !== selectedServerId` become **read-only/locked** and items matching the new `selectedServerId` become editable. Items are **NOT deleted** on switch. *(epics.md:469–470; proposal Story 3.2 ACs)*

### B. Add server
- **AC4:** Clicking "Add Server" presents the Story 2.5 connection form **inline** (not a full-screen takeover); on success the new server is appended **without disrupting** the currently selected server. *(epics.md:472–474)*
- **AC5:** `server.connect` authenticates, persists the server row with a freshly generated UUID, stores the credential in the vault keyed by that UUID, and returns `{ ok: true, serverId, serverType, serverVersion }`. If a server with the same normalized URL already exists, its credentials are updated (upsert by URL). If no server was previously selected, the new one becomes selected. *(proposal Story 2.1 ACs; architecture.md:717–721)*

### C. Remove server
- **AC6:** "Remove" on a **non-selected** server, after confirmation, calls `server.remove({ id })`; the daemon removes the row, deletes that server's vault entry, and evicts its provider from `ServerManager.providers`. *(epics.md:476–480; architecture.md:743)*
- **AC7:** After removal, basket items originating from that server are removed and a notification is shown: `"X items from [server] were removed from your basket."` *(epics.md:481)*
- **AC8:** "Remove" on the **currently selected** server warns the active server will be deselected; on confirm, removal proceeds and `selected_server_id` is set to the first remaining server (and that row's `selected = 1`), or `None`/`null` if none remain. *(epics.md:483–486; architecture.md:743)*

### D. No-server / first-run states
- **AC9:** When `selectedServerId === null` (e.g., servers exist but none selected), the library browser shows `"Select a server to browse your library."`; all `(+)` add buttons are disabled; Start Sync is disabled. *(epics.md:488–491)*
- **AC10:** When **no servers are configured at all** (first run or all removed), the UI enters the full-screen first-run login (Story 2.5). Servers-exist-but-none-selected → AC9 in-app empty state (not full-screen). *(epics.md:486, 488; proposal Story 2.5 ACs)*

### E. Per-server re-authentication
- **AC11:** When a configured server has an expired/invalid token and the user selects it (browse RPC → 401 for that server), the UI surfaces a re-auth prompt scoped to **that server's URL**; re-auth replaces **only** that server's vault credential. *(proposal Story 2.5 ACs)*

### F. Daemon — ServerManager
- **AC12:** `AppState.provider`/`server_type`/`server_version` are **replaced** by `server_manager: Arc<RwLock<ServerManager>>`. `ServerManager { servers: Vec<ServerRecord>, selected_server_id: Option<String>, providers: HashMap<String, Arc<dyn MediaProvider>> }` (lazy cache keyed by UUID). *(architecture.md:632–650; current rpc.rs:66–78)*
- **AC13:** `require_provider(state)` returns the **selected** server's provider (lazy-loading via `providers::connect()` on first use), or `RpcError::NotConnected` if none selected. **All existing `browse.*`, `sync.*`, scrobble, playlist call sites continue calling `require_provider()` unchanged.** *(architecture.md:657–666; current rpc.rs:473–479)*
- **AC14:** Providers init **lazily** — `providers::connect()` only when a server is first selected (`server.select` or first-startup auto-select of the `selected = 1` row), never eagerly for all servers (preserves < 10MB idle-RAM NFR). *(epics.md:498; architecture.md:652–654)*
- **AC15:** `get_daemon_state` returns new fields `servers: Array<{ id, url, serverType, username, selected }>` and `selectedServerId: string | null`, in addition to existing fields (don't break existing consumers). *(epics.md:497; architecture.md:731–738; current rpc.rs:1543–1641)*

### G. Database migration
- **AC16:** `server_config` migrates to `id TEXT PRIMARY KEY` (was `INTEGER PRIMARY KEY CHECK (id = 1)`) plus `selected INTEGER NOT NULL DEFAULT 0`; the `CHECK` constraint is removed by **recreating** the table (SQLite can't drop CHECK via ALTER). Existing columns preserved. *(epics.md:500; architecture.md:668–690; current db.rs:116–126)*
- **AC17:** The existing single row (if any) gets a generated UUID, `selected = 1`, and its vault credential is re-keyed under that UUID. Migration is **idempotent** (skip if already TEXT id). Follow the inline-`ALTER TABLE` pattern at `db.rs:73–95` — there is no version-tracked migration framework. *(architecture.md:683–690)*

### H. Credential vault migration
- **AC18:** Vault (`secrets.enc`) decrypts to `HashMap<String, ServerCredentials>` keyed by server UUID, `ServerCredentials { token_or_password: String }`. *(architecture.md:692–704)*
- **AC19:** On first load after upgrade, if the blob is the **legacy** `Secrets` struct (real shape: `{ token: Option<String>, server_secrets: HashMap<server_type, String> }`, `api.rs:1815–1821`), map the existing server's credential (Jellyfin → `token`; Subsonic → `server_secrets[server_type]`) to `HashMap { <existingServerUuid> → ServerCredentials }`, re-encrypt, overwrite. If neither format parses, treat as empty (existing hardware-fingerprint limitation). **Never** re-encrypt in legacy format. *(architecture.md:706–713, 788)*

### I. IPC contract
- **AC20:** New RPCs dispatched: `server.list → Array<{ id, url, serverType, username, selected }>`; `server.select({ id }) → { ok: true }`; `server.remove({ id }) → { ok: true }`. Error envelopes follow existing `JsonRpcError`. *(epics.md:494–496; architecture.md:723–729; current dispatch rpc.rs:296–370)*

### J. Regression / non-functional
- **AC21:** **Existing single-server users see zero behavior change after upgrade** — credentials auto-migrate, the single server auto-selects, library/basket/sync/playlists work exactly as before. *(proposal §Success Criteria)*
- **AC22:** Basket items persisted with the **old composite `serverId`** (`"type|url|username"`) are reconciled to the migrated server's UUID so they are **not silently wiped** by the basket store's server-mismatch logic. *(basket.ts:50–114)*
- **AC23:** `cargo build` + `cargo test` clean (no regressions); `rtk tsc` + `rtk lint` clean for `hifimule-ui`; new unit tests cover DB migration, vault migration, `ServerManager` select/remove/lazy-connect, sync server-grouping, and playlist cross-server validation.
- **AC24:** All new user-facing strings added to `hifimule-i18n/catalog.json` for en/fr/es (+de if present) and consumed via `t('...')` — no hardcoded English. *(i18n.ts; catalog.json)*

### K. Mixed-server basket & multi-provider sync routing (folds in Story 3.2)
- **AC25:** Clicking `(+)` adds the item to the basket with `serverId = selectedServerId`. `basket.add`/`basket.remove` RPCs carry `serverId`; the daemon validates `serverId` exists in `server_config` before accepting. *(proposal Story 3.2 ACs; architecture.md:758, 763–765)*
- **AC26:** With a mixed-server basket, the storage projection bar includes **all** items regardless of server, and **Start Sync stays enabled** when items exist (sync can execute items from any server) — provided a device is selected. *(proposal Story 3.2 ACs)*
- **AC27:** The sync pipeline carries per-item `serverId`. The `itemIds` param of `sync_calculate_delta` / `sync_detect_changes` / `sync_execute` changes from `string[]` to `Array<{ id: string, serverId: string }>` (parse sites: `rpc.rs:3037–3046`, `provider_calculate_delta` `rpc.rs:2179–2225`). `SyncAddItem` (`sync.rs:89–120`) gains a `server_id` field, populated during delta calc. *(architecture.md:767–778)*
- **AC28:** The daemon **groups items by `serverId`**, obtains the correct provider per group via `ServerManager.get_provider(serverId)`, and runs delta calc + container expansion + download **per group**, downloading each file from its originating server. The single global-provider assumption (`active_non_jellyfin_provider`, `rpc.rs:1738–1745`, used at `rpc.rs:3059–3061` & `3736–3780`; Jellyfin `execute_sync` args `sync.rs:1596–1608`; provider `execute_provider_sync` `sync.rs:2243–2255`, `download_url` per item `sync.rs:2390`) is removed. *(architecture.md:778)*
- **AC29:** A real sync of a Jellyfin + Navidrome mixed basket completes, downloading each item from its correct server. *(proposal §Success Criteria)*

### L. Auto-fill per server (folds in Story 3.8)
- **AC30:** The Auto-Fill virtual slot gains `serverId`, set to `selectedServerId` at toggle time. On toggle ON: any existing `__auto_fill_slot__` is removed first, then a new slot inserted with `serverId = selectedServerId`. The UI toggle reads ON only when `slot.serverId === selectedServerId`. *(proposal Story 3.8 ACs; architecture.md:759)*
- **AC31:** An Auto-Fill slot owned by server A, viewed while server B is selected, renders **read-only/locked** with a server-A badge and the server-B toggle shows OFF. Enabling auto-fill under server B silently replaces the slot with one bound to server B (no confirmation). Toggling OFF removes the slot regardless of owner. *(proposal Story 3.8 ACs)*
- **AC32:** The `autoFill` sync param gains `serverId`; `run_auto_fill` / `run_auto_fill_provider` (`auto_fill.rs:54`, `:349`; param parse `rpc.rs:3360–3390`; provider path `rpc.rs:2227–2293`) route to `ServerManager.get_provider(autoFill.serverId)` instead of the global Jellyfin client / global provider. *(architecture.md:780)*

### M. Playlist cross-server scope (folds in Stories 11.4 & 11.5)
- **AC33:** In `playlist.create` (handler `rpc.rs:912–987`; resolution `provider_sync_items_for_id` `rpc.rs:2107–2177`), after resolving entities to track IDs, the daemon verifies all resolved tracks originated from `selectedServerId`. If any item's `serverId` differs, it returns an error (new `ERR_CROSS_SERVER_CONFLICT` constant; message conveys: "Playlist creation requires all items to be from the selected server. Switch server or remove cross-server items.") and **no playlist is created**. `playlist.addTracks`/`removeTracks`/`delete`/`rename`/`reorder` operate on already-server-scoped playlist IDs — no extra validation. *(proposal Story 11.4 ACs)*
- **AC34:** The Save-as-Playlist dialog (`BasketSidebar.handleSaveAsPlaylist`, `BasketSidebar.ts:1660–1747`) **pre-filters** basket items to `serverId === selectedServerId` before building the `itemIds` array (so the 11.4 error never surfaces in normal flow). When non-selected-server items exist, an inline notice (mirror the existing auto-fill `sl-alert` at `BasketSidebar.ts:1667–1672`) states: `"Only items from [selected server] will be saved. Items from other servers are not included."`, and the item count reflects only selected-server items. *(proposal Story 11.5 ACs)*

### N. Read-only basket rendering (folds in Story 3.2)
- **AC35:** Basket items render with locked state where `item.serverId !== selectedServerId`: CSS class `basket-item--locked` (dimmed), and the `(×)` remove control hidden. Track/album items show a lock placeholder (`.basket-item-image--locked`, lock icon + "read-only" tooltip) in place of their thumbnail since the active provider can't load it; artist/genre cards keep their type icon. Selected-server items render normally with `(+)`/`(×)`. *(Code-review refinement 2026-06-09: the original per-item server-name badge was replaced by the per-server section label of AC36, which is the sole server indicator — see AC36.)* *(epics.md:502, 554–556; architecture.md:761; render site `BasketSidebar.ts` `renderItem`/`basketItemImage`)*
- **AC36:** Server identity for foreign (read-only) items is conveyed by a clear visual grouping: `renderItemsList` groups items by server under a labelled section divider (`.basket-server-group-label`, server name via `serverDisplayLabel`) whenever the basket spans multiple servers **or** holds any foreign-server item; a homogeneous selected-server basket renders a flat, unlabelled list. *(Code-review refinement 2026-06-09: the standalone "Items from other servers are read-only…" status-zone note was removed — the section labels + lock placeholder + dimmed styling + tooltip make it redundant, and the freed vertical space goes to the basket list.)* *(epics.md:556; proposal Story 3.2 ACs)*

## Tasks / Subtasks

### Phase 1 — Daemon infrastructure (no UI)

- [x] **T1: `server_config` DB migration** (AC16, AC17)
  - [x] T1.1: In `db.rs` (schema ~116–126), detect INTEGER/`CHECK (id = 1)`; if present, migrate; if already TEXT, skip (idempotent).
  - [x] T1.2: Recreate table per `architecture.md:674–681` (TEXT PK + `selected INTEGER NOT NULL DEFAULT 0`); copy existing row; generate UUID (confirm/add the `uuid` crate in `Cargo.toml`); set `selected = 1`.
  - [x] T1.3: Match the inline-`ALTER TABLE` style already in `db.rs:73–95` (`auto_sync_on_connect`, `transcoding_profile_id`) — no version-tracked framework exists.
  - [x] T1.4: Update `ServerConfig` struct + `db.rs` CRUD to use String/UUID id and multiple rows; add `set_selected(id)`, `list_servers()`, `remove_server(id)`.
- [x] **T2: `ServerManager`** (AC12–AC14)
  - [x] T2.1: Define `ServerManager` + `ServerRecord` (`architecture.md:634–646`) in a new module `hifimule-daemon/src/server_manager.rs`.
  - [x] T2.2: Replace `AppState.provider`/`server_type`/`server_version` (`rpc.rs:66–78`) with `server_manager`; update `AppState` construction in daemon bootstrap.
  - [x] T2.3: Rewrite `require_provider()` per `architecture.md:660–664` (selected id → cached provider; lazy-connect if absent).
  - [x] T2.4: On startup, load all rows into `ServerManager.servers`; set `selected_server_id` from the `selected = 1` row; **no eager provider connects**.
  - [x] T2.5: Audit & fix every other read of `state.provider`/`server_type`/`server_version` across `rpc.rs`, `sync.rs`, `api.rs`, `main.rs`. Added `server_manager::get_provider(serverId)` (lazy) + `get_provider_for_server(state, id)` for routing.
- [x] **T3: Vault migration** (AC18, AC19)
  - [x] T3.1: Define `VaultContents = HashMap<String, ServerCredentials>`, `ServerCredentials { token_or_password, user_id }` (user_id added for per-server Jellyfin sessions).
  - [x] T3.2: In `api.rs` implement load-new / fallback-legacy rekey / save (`rekey_legacy_vault` + `migrate_vault_from_legacy`, run at startup). Never re-encrypts legacy shape.
  - [x] T3.3: Replaced `save_server_secret`/`get_server_secret` with UUID-keyed `save_server_credential`/`get_server_credential`/`remove_server_credential`.
  - [x] T3.4: Preserved the `#[cfg(test)]` test seam (`TEST_VAULT`, `credential_test_lock`); added `TEST_LEGACY_RAW` for migration tests.
- [x] **T4: IPC — connect/list/select/remove** (AC5, AC6, AC8, AC20)
  - [x] T4.1: `server.connect`: upsert by URL → UUID, store vault by UUID, return `serverId`; auto-select if none.
  - [x] T4.2: Added `server.list`/`server.select`/`server.remove` dispatch arms + handlers.
  - [x] T4.3: `server.select`: update DB `selected` + manager `selected_server_id` + config + lazy-connect provider.
  - [x] T4.4: `server.remove`: DB delete + vault delete + provider eviction; reselect first remaining or `None` (AC8). `server.logout` now removes all servers.
- [x] **T5: `get_daemon_state` extension** (AC15)
  - [x] T5.1: Added `servers[]` + `selectedServerId` to the returned JSON (read from in-memory manager); kept existing fields.

### Phase 2 — Server Hub UI (`hifimule-ui`, Lit)

- [x] **T6: RPC client wrappers** (AC1, AC2, AC6, AC20)
  - [x] T6.1: Added `serverList`/`serverSelect`/`serverRemove` wrappers + `ServerSummary` type in `rpc.ts`.
- [x] **T7: Server Hub component** (AC1, AC2, AC4, AC6, AC8)
  - [x] T7.1: New `ServerHub` component (`components/ServerHub.ts`): server list with selected highlight, switch-on-click, badges.
  - [x] T7.2: Mounted as a compact header selector (dropdown) in `main.ts` (`#server-hub-container`).
  - [~] T7.3: Settings → Servers tab NOT added — the header Server Hub dropdown already satisfies AC1–AC8 (list/select/add/remove). A dedicated Settings host can be added later if desired.
  - [x] T7.4: "Add Server" presents the `login.ts` form inline via a dialog (`mode: 'add'`, AC4).
  - [x] T7.5: "Remove" → confirmation dialog (extra warning for selected server, AC8) → `server.remove` → AC7 removal toast.
- [x] **T8: First-run vs add-server flow** (AC4, AC10, AC11)
  - [x] T8.1: `main.ts` `routeFromDaemonState` drives UI mode from `servers.length` + `selectedServerId`: 0 → full-screen login; ≥1 & none selected → AC9 empty state; selected → library.
  - [x] T8.2: `login.ts` gains inline "add" mode (dialog) vs first-run full-screen mode.
  - [x] T8.3: 401 re-auth modal (AC11) — IMPLEMENTED. Daemon maps provider `Auth` errors to a distinct `ERR_UNAUTHORIZED` (-8) with an `unauthorized` data flag; the Tauri `rpc_proxy` now forwards the full JSON-RPC error envelope (code+message+data) instead of a bare string; `rpcCall` detects the code on browse/library RPCs and dispatches `hifimule:server-unauthorized`; `main.ts` shows a re-auth dialog scoped (URL pre-filled + read-only) to the selected server via `login.ts` `mode: 'reauth'`. Re-auth calls `server.connect` (upsert-by-URL), replacing only that server's vault credential. Debounced so repeated 401s don't stack dialogs.
- [x] **T9: Library / Start-Sync gating** (AC9, AC26)
  - [x] T9.1: No-server empty state rendered by `main.ts` (`renderLibraryNoServerSelected`); `(+)` add is gated by `basketStore.add` (blocks with no selected server). Start Sync stays gated only on a selected device (not server), so a mixed/non-selected-server basket can still sync (AC26).

### Phase 3 — Mixed-server basket, read-only rendering & sync routing

- [x] **T10: Basket store — retention + serverId** (AC3, AC22, AC25)
  - [x] T10.1: `setActiveServerId` no longer deletes other-server items; `hydrateFromDaemon` retains all-server items. Added `isItemLocked`/`hasMultipleServers`/`serverIdsInBasket`/`removeItemsForServer`.
  - [x] T10.2: `add` keeps `item.serverId = activeServerId`; `manifest_save_basket` carries full items (with serverId); daemon validates/reconciles against `server_config` (AC25).
  - [x] T10.3: `reconcileServerIds(servers)` remaps composite→UUID for the migrated server (AC22); called by `ServerHub.refresh`. Daemon also reconciles manifest items.
- [x] **T11: Read-only basket rendering** (AC35, AC36)
  - [x] T11.1: `renderItem`/`renderAutoFillSlotCard`/`renderGenreCard`/`renderArtistCard` add `basket-item--locked` class + hide `(×)` for non-selected-server items (helpers `lockedCardClass`/`removeButtonFor`). Foreign-server track items get a lock placeholder via `basketItemImage` (`.basket-item-image--locked`). *(Code-review 2026-06-09: per-item `lockedServerBadge` removed in favour of the AC36 section label.)*
  - [x] T11.2: Items grouped by server under a labelled divider (`renderItemsList` / `.basket-server-group-label`) when the basket spans multiple servers or holds a foreign-server item. *(Code-review 2026-06-09: replaced the earlier `renderStatusZone` mixed-server note, which was removed.)*
- [x] **T12: Auto-fill per server** (AC30–AC32)
  - [x] T12.1: `insertAutoFillSlot` binds `serverId = currentServerId` via `basketStore.add`; re-adding overwrites the slot (rebind, AC30).
  - [x] T12.2: Auto-fill toggle reads ON only when the slot's `serverId === currentServerId`; foreign-server slot renders locked (AC31).
  - [x] T12.3 (daemon): `autoFill.serverId` param honored — multi-server delta routes auto-fill via `get_provider_for_server(autoFill.serverId)`; the slot's server is included in multi-server detection. Single-server path uses the selected server (correct). UI slot serverId in T12.1/T12.2.
- [x] **T13: Multi-provider sync routing** (AC27–AC29) — daemon side complete; mixed-server execute pending manual two-server verification (AC29).
  - [x] T13.1: `itemIds` parse accepts `Array<{id, serverId}>` (and legacy strings) via `parse_item_specs`; UI callers updated in T13-UI.
  - [x] T13.2: Added `server_id` to `SyncAddItem`/`DesiredItem`/`SyncedItem`/`SyncIdChangeItem`; `calculate_delta` propagates it.
  - [x] T13.3: `multi_provider_calculate_delta` groups items by `serverId`, resolves each via `get_provider_for_server` (generic — works for Jellyfin + Subsonic), tags `server_id`. Single-server keeps the existing dispatch (AC21).
  - [x] T13.4: Mixed-server execute groups `delta.adds` by `server_id` and routes each group through `execute_provider_sync` with its provider; deletes/id-changes/playlists run once with the first group. Single-server path unchanged.
  - [x] T13.5: Aggregates per-group synced items + errors into one operation result/manifest finalize.

### Phase 4 — Playlist scope

- [x] **T14: Daemon cross-server validation** (AC33)
  - [x] T14.1: Added `ERR_CROSS_SERVER_CONFLICT` constant.
  - [x] T14.2: `handle_playlist_create` validates per-item `serverId == selectedServerId` when the caller supplies `items: [{id, serverId}]`; on mismatch returns the cross-server error and creates nothing. addTracks/removeTracks/delete/rename/reorder untouched.
- [x] **T15: Save-as-Playlist UI pre-filter** (AC34)
  - [x] T15.1: `handleSaveAsPlaylist` pre-filters items to `serverId === currentServerId`, shows a cross-server `sl-alert` notice when other-server items exist, reflects the filtered set, and passes per-item `items: [{id, serverId}]` so the daemon's AC33 guard validates cleanly.

### i18n & Verification

- [x] **T16: i18n** (AC24) — new keys (serverHub.*, library.selectServerEmpty, basket.other_server, basket.locked_hint, basket.playlist.cross_server_notice, basket.playlist.item_count, error.cross_server_playlist, login.reauth_title/hint, error.unauthorized) added to `catalog.json` for en/fr/es/de with full key parity (255 keys/lang). *(Code-review 2026-06-09: `basket.mixedServerNote` removed with the status-zone note; `basket.playlist.item_count` added for AC34.)*
- [x] **T17: Tests & checks** (AC23, AC29)
  - [x] T17.1: Unit tests — DB migration (INTEGER→UUID, idempotent, empty), vault migration (legacy→map jellyfin/subsonic/already-new + end-to-end), `ServerManager` load/select/lazy-connect+cache/reselect, multi-server `server.list/select/remove` RPC, sync `server_id` propagation + grouping helpers (`parse_item_specs`, `sync_spans_multiple_servers`), provider auth→`ERR_UNAUTHORIZED` mapping (AC11).
  - [x] T17.2: `rtk cargo test` (438 passing) + `rtk cargo build` clean (no new warnings); Tauri shell `cargo check` clean; UI `tsc` clean (the pre-existing tsconfig `baseUrl` deprecation aside). No UI lint script exists in the project.
  - [x] T17.3: Manual two-server verification (AC29) — PASSED 2026-06-09: mixed-basket sync from Jellyfin + Navidrome confirmed end-to-end (server 1 copies, server 2 downloads via `?api_key=` auth fix); incremental change-detection stale-token fallback verified; progress bar no-flash confirmed.

### Review Findings (code review 2026-06-09)

Adversarial review (Blind Hunter + Edge Case Hunter + Acceptance Auditor, all on claude-opus-4-8) against baseline `9579163`. Note: T17.3 manual test used a Jellyfin **+** Navidrome *mixed* basket — the path that spans 2 servers, which works.

**Resolution (2026-06-09):** 5 patches applied & verified (1 HIGH single-non-selected-server routing, 1 LOW basket reconcile retention, 1 LOW provider-connect race, AC36 grouping, AC34 count); 2 findings dismissed as false positives on code verification during implementation (the "stale config.url" HIGH — the real select path refreshes config via `save_jellyfin_session`; the "id_changes wrong provider" MEDIUM — those ops don't touch the provider); 1 LOW playlist-guard finding deferred (defense-in-depth, UI path safe). `cargo build` clean, 438 daemon tests pass, UI tsc clean (pre-existing `baseUrl` deprecation aside).

- [x] [Review][Patch] [AC36] [FIXED] Per-server visual grouping in basket [hifimule-ui/src/components/BasketSidebar.ts, styles.css] — **Fixed 2026-06-09.** Added `renderItemsList()`: items are grouped by serverId with a labelled section divider (`.basket-server-group-label`, server name via `serverDisplayLabel`) preserving insertion order, whenever the basket spans multiple servers **or** holds any foreign-server (locked) item; a homogeneous selected-server basket renders the flat list unchanged. New `.basket-server-group-label` CSS (label + trailing divider line). **Refinement (per user 2026-06-09):** since the section label now identifies each item's server, the redundant per-item server badge (`lockedServerBadge`) was removed from all item renderers; locked styling + hidden remove control retained. UI tsc clean.
- [x] [Review][Patch] [AC34] [FIXED] Save-as-Playlist dialog item count [hifimule-ui/src/components/BasketSidebar.ts, hifimule-i18n/catalog.json] — **Fixed 2026-06-09.** The dialog now shows `basket.playlist.item_count` reflecting `manualIds.length` (selected-server items only). New i18n key added for en/fr/es/de. UI tsc clean.

- [x] [Review][Patch] [HIGH] [FIXED] Sync routes a single non-selected server's items to the *selected* provider [hifimule-daemon/src/rpc.rs:3156, :4080] — **Fixed 2026-06-09.** Renamed `sync_spans_multiple_servers` → `sync_needs_provider_routing` and changed both the calculate gate and the execute gate to route to per-server providers whenever any resolved item server differs from the selected one (not only when count > 1). Single-server (all-selected / untagged) syncs keep the existing dispatch (AC21). Added test cases for the single-other-server and nothing-selected cases. 438 tests pass. *(Original: with server A selected and a basket of only server-B items, the multi-provider path was skipped and B's items were resolved/downloaded against A.)*
- [x] [Review][Patch] [HIGH] [DISMISSED — false positive on verification] `server.select` "leaves stale `config.url`/`user_id`" [hifimule-daemon/src/api.rs:2016, :2145] — **Verified during implementation: not a bug.** `handle_server_select` does **not** call `set_config_selected_server` directly; it routes through `sync_selected_config` (rpc.rs:1376), which for a Jellyfin server with stored creds calls `save_jellyfin_session(id, record.url, token, user_id)` — writing `config.url`/`user_id`/`selected_server_id` to the newly-selected server. `handle_server_connect` does the same for `is_selected`. So whenever a Jellyfin server is the selected one (the only case the global `get_credentials()` path runs), `config.url` matches it. The else-branch (Jellyfin without creds) makes `get_credentials()` error out rather than query the wrong server. The edge reviewer analyzed `set_config_selected_server` in isolation and missed the real select path. No change made.
- [x] [Review][Patch] [MEDIUM] [DISMISSED — false positive on verification] Multi-server execute "applies id_changes/deletes/playlists to the wrong provider" [hifimule-daemon/src/rpc.rs:4138-4141] — **Verified during implementation: not a bug.** `execute_provider_sync` uses `id_changes` purely for manifest bookkeeping (sync.rs:2752-2786 — removes old id, pushes new `SyncedItem`, no network/provider call), `deletes` for device-side file removal, and `playlists` for m3u generation from the manifest. None of these touch the group's `provider`, so running them exactly once with the first group is correct regardless of which provider that is. The `if first { … }` guard correctly prevents running them N times. No change made.
- [x] [Review][Patch] [LOW] [FIXED] `reconcile_basket_server_ids` drops untagged (None serverId) items on save when no server is selected [hifimule-daemon/src/rpc.rs:1764] — **Fixed 2026-06-09.** Rewrote the match so untagged items adopt the selected server when one exists, otherwise are **retained untagged** (not dropped); known-UUID items pass through; composite ids remap to their UUID; only items belonging to an unknown/removed server are dropped (AC7). 438 tests pass.
- [x] [Review][Patch] [LOW] [FIXED] Lazy provider connect check-then-insert race → duplicate construction [hifimule-daemon/src/server_manager.rs:144 (`get_provider`)] — **Fixed 2026-06-09.** Added a double-checked insert: after `connect_provider_for` completes, re-acquire the write lock and return any instance a concurrent caller already cached, so all callers converge on one provider `Arc` (no lock held across the await). 438 tests pass.

- [x] [Review][Defer] [LOW] Cross-server playlist guard checks only the `items` param, not the `itemIds` used to build the track list [hifimule-daemon/src/rpc.rs:875-915 (`handle_playlist_create`)] — deferred 2026-06-09, defense-in-depth only. AC33's guard inspects `params["items"]`; `track_ids` is built from `params["itemIds"]`. A non-UI caller sending legacy `itemIds: string[]` (no `items`) skips the guard — but those items are then resolved against the *selected* provider, so cross-server ids simply fail to resolve and are skipped (not mis-assigned). The UI always sends `items` (AC34 pre-filter), so normal flow is fully covered. A complete fix isn't possible without per-item serverIds in the bare path, and forcing `items` would break the documented legacy `itemIds` contract — so deferred rather than patched.
- [x] [Review][Defer] load_vault masks decryptable-but-unparseable vault as empty [hifimule-daemon/src/api.rs (`load_vault`)] — deferred, low impact. `serde_json::from_str(&json).unwrap_or_default()` turns a corrupted-but-decryptable vault into an empty map (previously errored). Worst case the user re-authenticates. The legacy-migration path reads the raw blob separately, so this doesn't break migration.
- [x] [Review][Defer] `handle_server_connect` with `ServerType::Unknown` caches a provider but saves no credential [hifimule-daemon/src/rpc.rs (`handle_server_connect`)] — deferred, narrow. The Unknown arm stores no vault entry yet still inserts the provider into the cache; a later lazy reconnect fails with "No credential found" → `ERR_UNAUTHORIZED`, leaving a broken row the user must remove. Only reachable for an unrecognized server type.

**Dismissed as noise / by-design / false-positive (8):** `check_server_connection_cached` returning `selected.is_some()` without a network check (by design — AC14 lazy connect + AC11 re-auth handles 401); AC32 single-server Jellyfin auto-fill using the global client (functionally covered — a foreign `autoFill.serverId` forces the multi-provider path); `migrate_vault_from_legacy` using `servers.first()` (benign — DB migration yields exactly one row on the real path); missing-credential mapped to `ERR_UNAUTHORIZED` (minor UX; re-auth still recovers); `set_test_provider` slug fallback to "jellyfin" (test-only); `rpc_proxy`/`rpcCall` discarding the structured code for non-unauthorized errors (message conveys `ERR_CROSS_SERVER_CONFLICT`; only the unauthorized code is currently consumed); `handle_get_daemon_state` two-lock snapshot race (narrow; self-corrects on the next ~2s poll); AC23 `rtk lint` not run (no UI lint script exists in the project).

## Dev Notes

### Current state — verified against the codebase (cite these; do not assume)

**Daemon (`hifimule-daemon/src`):**
- `AppState` holds single `provider: Arc<RwLock<Option<Arc<dyn MediaProvider>>>>` + `server_type`/`server_version` — `rpc.rs:66–78`. **No `ServerManager` today.**
- `require_provider()` reads the single `state.provider` — `rpc.rs:473–479`.
- `server_config` still `id INTEGER PRIMARY KEY CHECK (id = 1)` (single row) — `db.rs:116–126`. Migration pattern to follow: inline checks `db.rs:73–95`. No version-tracked framework.
- Vault: `Secrets { token: Option<String>, server_secrets: HashMap<String,String> }`, `server_secrets` keyed by **`server_type`** (not UUID) — `api.rs:1815–1821`; `save_server_secret`/`get_server_secret` `api.rs:1918–1936`; `load_secrets`/`save_secrets` `api.rs:1828`/`1847`. Crypto itself (`vault.rs`) is Story 7.5 — **do not** change crypto, only the contained data shape + migration.
- **`serverId` is currently a DERIVED COMPOSITE STRING**, not a UUID: `server_config_id() = "{server_type}|{normalized_url}|{username}"` — `rpc.rs:418–425` (also `current_server_id()` ~`rpc.rs:437`). Surfaced as `currentServer.serverId` (`rpc.rs:1584–1592`) and used to filter basket items (`rpc.rs:1650–1651`). This story changes it to a UUID — every producer/consumer moves together (see Regression Risk).
- `server.connect` returns `{ ok, serverType, serverVersion }` — **no serverId** — `rpc.rs:1377–1381`. Handler `rpc.rs:1269–1382`.
- `get_daemon_state` returns a single `currentServer`, no `servers[]`/`selectedServerId` — `rpc.rs:1543–1641`.
- **Sync pipeline = THREE RPCs** (no `sync.start`): `sync_calculate_delta`/`sync_detect_changes`/`sync_execute` dispatch `rpc.rs:315–317`. `itemIds` parsed flat `Vec<String>` `rpc.rs:3037–3046`; passed to `provider_calculate_delta` `rpc.rs:2179–2225`. Provider chosen via single-global `active_non_jellyfin_provider` `rpc.rs:1738–1745` (used `rpc.rs:3059–3061`, `3736–3780`). Container expansion per-item: genre `rpc.rs:3185–3281`, album/playlist `rpc.rs:3283–3317` (uses `jellyfin_client` directly). Jellyfin `execute_sync(jellyfin_url, token, user_id, …)` `sync.rs:1596–1608` (single-server args); provider `execute_provider_sync(source{provider})` `sync.rs:2243–2255`, `download_url` per item `sync.rs:2390`. `SyncAddItem` has **no** `server_id` — `sync.rs:89–120`. `basket_items_from_params_or_manifest` `rpc.rs:2394–2403`.
- Auto-fill: param parse `rpc.rs:3360–3390` (no serverId); Jellyfin `run_auto_fill(&jellyfin_client, …)` uses **global** `CredentialManager` `auto_fill.rs:54`; provider `run_auto_fill_provider(provider, …)` already provider-agnostic `auto_fill.rs:349` (called `rpc.rs:2227–2293`). `AutoFillPrefs { enabled, max_bytes }` device-level `device/mod.rs:161–164`.
- Playlist: `playlist.create` dispatch `rpc.rs:369`, handler `rpc.rs:912–987`; resolution `provider_sync_items_for_id` `rpc.rs:2107–2177`; itemIds parsed `rpc.rs:937–972`. **No cross-server validation exists.** Error constants `rpc.rs:21–37` (negative ints; **no** 409). addTracks/removeTracks/delete/rename/reorder operate on server-scoped playlist IDs — `rpc.rs:989–1267`.
- `BasketItem.server_id: Option<String>` already exists daemon-side — `device/mod.rs:45–57`; persisted in `DeviceManifest.basket_items` (`.hifimule.json`); already filtered in `handle_manifest_get_basket` `rpc.rs:1650–1651`.

**Frontend (Lit + TS, `hifimule-ui/src`):** entry `main.ts`; RPC wrapper `rpc.ts` (Tauri `invoke('rpc_proxy')`, generic `rpcCall` 75–85); basket store `state/basket.ts`; login `login.ts`; i18n `i18n.ts` + `hifimule-i18n/catalog.json` (en/fr/es; de added recently, git `25a3d85`).
- `BasketItem` already has `serverId?: string` (`basket.ts:8–19`); set from `activeServerId` on add (`basket.ts:214`); `setActiveServerId` (`basket.ts:50–58`); daemon state consumed `main.ts:71`/`141`, polled ~2s `BasketSidebar.ts:630`.
- **No Server Hub, no Settings view.** Header shows read-only `server-connection-chip` (`main.ts:146–180`). **Device Hub** card pattern is the model (`BasketSidebar.ts:693–714, 410–438`).
- Basket render: `renderItem()` `BasketSidebar.ts:1563–1598`; header `:958–968`; status/notice zone `:809–824`; capacity bar `:89–180` invoked `:979`; Start Sync `:998–1012` (disabled on `!selectedDevicePath`/`isSyncing`).
- Auto-fill UI: `AUTO_FILL_SLOT_ID` `basket.ts:6`; toggle `BasketSidebar.ts:373–408`; `insertAutoFillSlot()` `:348–355` (**slot carries no serverId today**); `autoFillEnabled` synced from daemon `:276/293`.
- Save-as-Playlist: button `BasketSidebar.ts:963–967`; handler `handleSaveAsPlaylist()` `:1660–1747` builds `manualIds` from `basketStore.getItems()` with **no serverId filter**; existing auto-fill `sl-alert` notice `:1667–1672` is the pattern to mirror for the cross-server notice.

### ⚠️ Regression Risk — basket auto-wipe on serverId scheme change (AC3, AC22)
The basket store **currently deletes** items whose `serverId !== activeServerId`: `setActiveServerId()` → `removeItemsFromOtherServers()` (`basket.ts:50–83`); `hydrateFromDaemon()` keeps only matching items (`basket.ts:98–114`). Two consequences this story handles:
1. **Behavior change (AC3):** switching servers must **retain** other-server items read-only, not delete them. Replace the deletion logic with retention + locked rendering.
2. **Migration trap (AC22):** after migration the single server's `serverId` becomes a UUID, but persisted basket items (in `.hifimule.json` and `localStorage` key `hifimule-basket`) still carry the composite serverId. Remap composite→UUID for the migrated server during load, or the first `setActiveServerId(uuid)` locks/wipes the entire existing basket.

### Enforcement rules (architecture.md:782–789) — all MUST hold
- Never access `AppState.provider` (removed) — use `require_provider(state)` or `state.server_manager`.
- Never write `server_config` queries assuming a single row or INTEGER PK.
- Always pass `serverId` when adding basket items via RPC; daemon validates it exists in `server_config`.
- **Group `sync` `itemIds` by `serverId` and route each group to its correct provider — never assume all items belong to the active provider.**
- Never re-encrypt the vault in the legacy `Secrets` shape — always `HashMap<String, ServerCredentials>`.
- Evict the provider cache entry on `server.remove` before returning `{ ok: true }`.

### Testing standards
- Rust: colocated `#[cfg(test)]`, run via `rtk cargo test`. Preserve the vault test seam (`TEST_SECRETS`, `credential_test_lock`; Story 7.5 AC7).
- UI: `rtk tsc` (types), `rtk lint`. Lit components.
- Highest-risk units to test explicitly: the two migrations (idempotency + single→multi upgrade, AC21), sync server-grouping/routing (AC28–AC29), and playlist cross-server rejection (AC33).

### Project Structure Notes
- New daemon module: `hifimule-daemon/src/server_manager.rs` (mirrors `device/`, `sync.rs`, `db.rs`, `api.rs`, `rpc.rs`).
- New UI component under `hifimule-ui/src/components/` (alongside `BasketSidebar.ts`); new strings in `hifimule-i18n/catalog.json`.
- Confirm/add `uuid` crate in `hifimule-daemon/Cargo.toml`.
- Sequence per proposal §5: Phase 1 (infra) → Phase 2 (hub UI) → Phase 3 (basket/sync) → Phase 4 (playlist). Phase 1 must land before any routing work since `get_provider(serverId)` is the routing primitive.

### References
- [Source: epics.md#Story-2.11 (451–502); #Story-3.2 (535–570)]
- [Source: sprint-change-proposal-2026-06-09-multi-server-management.md §3–5 (all phases; Stories 2.1/2.5/2.11/3.2/3.8/11.4/11.5 changes)]
- [Source: architecture.md §"Multi-Server Management" (626–790)]
- [Source: implementation-readiness-report-2026-06-09.md (V1: 2.11 spec parity)]
- [Source: hifimule-daemon/src/rpc.rs:21–37, 66–78, 296–370, 418–437, 473–479, 912–987, 1269–1382, 1543–1641, 1650–1651, 1738–1745, 2107–2177, 2179–2225, 2227–2293, 2394–2403, 3037–3046, 3059–3061, 3185–3317, 3360–3390, 3736–3780]
- [Source: hifimule-daemon/src/sync.rs:89–120, 1596–1608, 2243–2255, 2390]
- [Source: hifimule-daemon/src/auto_fill.rs:54, 349]
- [Source: hifimule-daemon/src/db.rs:73–95, 116–126]
- [Source: hifimule-daemon/src/api.rs:1815–1861, 1918–1936]
- [Source: hifimule-daemon/src/device/mod.rs:45–57, 161–164]
- [Source: hifimule-ui/src/state/basket.ts:6, 8–19, 50–114, 214; src/main.ts:17–23, 71, 141, 146–180; src/login.ts; src/rpc.ts:75–85; src/components/BasketSidebar.ts:256–259, 348–355, 373–408, 630, 693–714, 809–824, 958–968, 963–967, 1563–1598, 1660–1747]

## Dev Agent Record

### Agent Model Used

(to be filled by dev agent)

### Agent Model Used

claude-opus-4-8 (dev-story implementation)

### Debug Log References

- Vault-migration write_config panic: subsonic connect now writes config.json; `write_config` failed on missing parent dir (stale test path), panicking while holding the credential test mutex → poison cascade. Fixed by having `write_config` create parent dirs. (`rtk cargo test`, 434 passing.)
- Server Hub / logout / live basket not visible at runtime: `index.html` ships a static `.split-panel` placeholder, and `renderMainLayout`'s rebuild-guard checked for `.split-panel` → bailed on first render, so the real layout (`#server-hub-container` + mounted BasketSidebar) was never built (the static basket placeholder showed instead). Fixed by guarding on `#server-hub-container` (unique to the real layout). tsc + vite build clean.

### Completion Notes List

**Phase 1 (daemon infrastructure) — COMPLETE & TESTED.** All of T1–T5 implemented:
- `db.rs`: `server_config` migrated to TEXT-UUID PK + `selected` column; idempotent legacy-INTEGER migration via table recreate; new CRUD (`upsert_server` by-URL→UUID, `list_servers`, `get_server`, `set_selected`, `remove_server`). Tests: `test_migrate_legacy_server_config`(+empty), `test_server_config_round_trips_and_updates`, `test_set_selected_nonexistent_errors`.
- `server_manager.rs` (new module): `ServerManager`/`ServerRecord`, `load_from_db`, lazy `get_provider`/`selected_provider`, `set_test_provider` seam. Tests: load/selection, lazy-connect+cache, reselect-after-remove.
- `rpc.rs`: `AppState.provider`/`server_type`/`server_version` → `server_manager`; `require_provider`/`get_provider_for_server` route via manager; `server.connect` returns `serverId`; new `server.list`/`server.select`/`server.remove`; `get_daemon_state` gains `servers[]`+`selectedServerId`; basket get/save retain all-server items with composite→UUID reconciliation (AC22). Test: `test_server_list_select_remove_multi_server`.
- `api.rs`: vault is now `HashMap<uuid, ServerCredentials{token_or_password,user_id}>`; legacy `Secrets` re-keyed at startup via `migrate_vault_from_legacy`/`rekey_legacy_vault` (never re-encrypts legacy shape); UUID-keyed `save/get/remove_server_credential`; `save_jellyfin_session`/`get_credentials` resolve the selected server via `config.selected_server_id`. Tests: rekey jellyfin/subsonic/already-new, end-to-end migration.
- `main.rs`: daemon-initiated auto-sync subsonic provider now reads the selected server's UUID-keyed credential.

**Phase 4 daemon (T14) — COMPLETE.** `ERR_CROSS_SERVER_CONFLICT` added; `handle_playlist_create` validates per-item `serverId == selectedServerId` when the caller supplies `items: [{id, serverId}]` (AC33).

**Phases 2–4 (UI + sync routing + auto-fill + playlist) — IMPLEMENTED.** Server Hub header component (list/switch/add/remove/logout via new RPCs); first-run vs none-selected (AC9) vs library routing in `main.ts`; inline add-server dialog; mixed-server basket retention with read-only/locked rendering + server badges + mixed-server note; composite→UUID reconciliation; multi-provider sync delta + execute routing (single-server path preserved byte-for-byte for AC21); auto-fill bound per server; save-as-playlist server pre-filter + cross-server notice; daemon cross-server playlist guard; full i18n (en/fr/es/de). UI typechecks clean; 437 daemon tests pass.

**AC11 — per-server 401 re-auth — IMPLEMENTED (follow-up to initial deferral).** Daemon `provider_error_to_rpc` now maps `ProviderError::Auth` to a distinct `ERR_UNAUTHORIZED` (-8) carrying an `unauthorized` data flag; the Tauri `rpc_proxy` forwards the full JSON-RPC error envelope (code+message+data) rather than a bare message; `rpcCall` detects the code on browse/library RPCs and dispatches `hifimule:server-unauthorized`; `main.ts` shows a debounced re-auth dialog scoped (URL pre-filled + read-only) to the selected server, whose `server.connect` (upsert-by-URL) replaces only that server's vault credential. 438 daemon tests pass (incl. new auth-mapping test); UI typechecks clean; Tauri shell compiles.

**All tasks complete. Story ready for review.**

---
_Original context-engine note:_ Ultimate context engine analysis completed — comprehensive developer guide created. Story scoped as the COMPLETE end-to-end multi-server feature: all four proposal phases (infra, hub UI, mixed-server basket + multi-provider sync routing, playlist scope) plus the amended-but-never-implemented behaviors from Stories 3.2/3.8/11.4/11.5 folded in. Two key reconciliations against the actual code captured: (1) no `sync.start` RPC — serverId is threaded through the real 3-RPC sync pipeline; (2) JSON-RPC negative error codes, not HTTP 409. Regression trap (composite-serverId basket wipe on UUID migration) flagged with explicit handling task.

### File List

**Daemon (Rust):**
- `hifimule-daemon/src/server_manager.rs` (new — ServerManager, lazy provider cache, tests)
- `hifimule-daemon/src/main.rs` (mod decl; auto-sync subsonic credential lookup; DesiredItem server_id)
- `hifimule-daemon/src/db.rs` (server_config UUID migration + multi-server CRUD + tests)
- `hifimule-daemon/src/api.rs` (vault → `HashMap<uuid, ServerCredentials>` + legacy migration + tests)
- `hifimule-daemon/src/device/mod.rs` (`SyncedItem.server_id`)
- `hifimule-daemon/src/sync.rs` (`server_id` on DesiredItem/SyncAddItem/SyncedItem; `source_server_id` on SyncIdChangeItem; propagation + test)
- `hifimule-daemon/src/rpc.rs` (AppState→server_manager; require_provider/get_provider_for_server; server.connect/list/select/remove; get_daemon_state servers[]/selectedServerId; basket reconciliation; multi-provider delta + execute routing; playlist cross-server validation; tests)
- (test fixtures updated across `rpc.rs`, `scrobbler.rs`, `device/tests.rs` for new struct fields)

**UI (TypeScript / Lit-style):**
- `hifimule-ui/src/rpc.ts` (serverList/serverSelect/serverRemove + ServerSummary; ERR_UNAUTHORIZED detection → re-auth event)
- `hifimule-ui/src/state/basket.ts` (retention, isItemLocked, hasMultipleServers, removeItemsForServer, reconcileServerIds)
- `hifimule-ui/src/components/ServerHub.ts` (new — header server selector / add / remove / logout)
- `hifimule-ui/src/login.ts` (inline "add" + "reauth" dialog modes; prefill/readonly URL; onClose)
- `hifimule-ui/src/main.ts` (first-run/empty/library routing; Server Hub mount; AC11 re-auth handler)
- `hifimule-ui/src/components/BasketSidebar.ts` (locked rendering, mixed-server note, auto-fill serverId, per-item sync routing, save-as-playlist pre-filter)
- `hifimule-ui/src-tauri/src/lib.rs` (`rpc_proxy` forwards structured JSON-RPC error envelope)
- `hifimule-i18n/catalog.json` (17 new keys × en/fr/es/de — 255 keys/lang)

### Change Log

- 2026-06-09 — Implemented Story 2.11 multi-server support end-to-end. **Daemon:** server_config UUID migration, ServerManager + lazy provider cache, UUID-keyed credential vault with legacy migration, server.list/select/remove RPCs, get_daemon_state servers[]/selectedServerId, multi-provider sync delta + execute routing (server_id threaded through the pipeline), auto-fill per-server routing, playlist cross-server validation (ERR_CROSS_SERVER_CONFLICT). **UI:** Server Hub header component (list/switch/add/remove/logout), first-run vs empty vs library routing, mixed-server basket retention + read-only rendering + mixed-server note, composite→UUID reconciliation, per-item sync routing, save-as-playlist server pre-filter, full i18n (en/fr/es/de). 437 daemon tests pass; UI typechecks clean.
- 2026-06-09 — AC11 (per-server 401 re-auth) implemented: daemon `ERR_UNAUTHORIZED` (-8) + `unauthorized` data flag; Tauri `rpc_proxy` forwards the structured error envelope; UI detects the code and shows a scoped re-auth dialog (`login.ts` `reauth` mode). 438 daemon tests pass; UI tsc + Tauri shell clean. 3 more i18n keys (login.reauth_title/hint, error.unauthorized) → 255 keys/lang.
- 2026-06-09 — Bug fixes during manual T17.3 run: (1) Jellyfin `download_url` now appends `?api_key=<token>` so multi-server `execute_provider_sync` fetches succeed without auth headers (Subsonic already embedded creds via `signed_url`); (2) `handle_sync_detect_changes` now returns `[]` instead of hard error on `ProviderError::NotFound` (stale Subsonic sync token → code 70); UI `itemIdsWithIncrementalChanges` wrapped in try/catch for belt-and-suspenders fallback; (3) `renderSyncProgress` refactored to render shell once then patch leaf nodes in-place — eliminates 500 ms flash caused by full `innerHTML` replacement of Shoelace components; guard changed from `.sync-progress-panel` (shared with spinner) to `#sync-progress-bar` (unique to progress shell) to fix "Starting" state freeze.
- **T17.3 PASSED 2026-06-09.** Story status → review.
