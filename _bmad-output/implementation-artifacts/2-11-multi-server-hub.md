---
baseline_commit: 95791633a3c511eed4aafee1945bd911d6c0657a
---

# Story 2.11 — Multi-Server Hub

Status: ready-for-dev

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
- **AC35:** Basket items render with locked state where `item.serverId !== selectedServerId`: CSS class `basket-item--locked`, a server-name badge, and the `(×)` remove control hidden. Selected-server items render normally with `(+)`/`(×)`. *(epics.md:502, 554–556; architecture.md:761; render site `BasketSidebar.ts:1563–1598`)*
- **AC36:** When the basket contains items from multiple servers, an informational note is shown (basket header/status zone, `BasketSidebar.ts:958–968`/`809–824`): `"Items from other servers are read-only until you switch back to that server."` A clear visual grouping (section divider or label) separates items by server. *(epics.md:556; proposal Story 3.2 ACs)*

## Tasks / Subtasks

### Phase 1 — Daemon infrastructure (no UI)

- [ ] **T1: `server_config` DB migration** (AC16, AC17)
  - [ ] T1.1: In `db.rs` (schema ~116–126), detect INTEGER/`CHECK (id = 1)`; if present, migrate; if already TEXT, skip (idempotent).
  - [ ] T1.2: Recreate table per `architecture.md:674–681` (TEXT PK + `selected INTEGER NOT NULL DEFAULT 0`); copy existing row; generate UUID (confirm/add the `uuid` crate in `Cargo.toml`); set `selected = 1`.
  - [ ] T1.3: Match the inline-`ALTER TABLE` style already in `db.rs:73–95` (`auto_sync_on_connect`, `transcoding_profile_id`) — no version-tracked framework exists.
  - [ ] T1.4: Update `ServerConfig` struct + `db.rs` CRUD to use String/UUID id and multiple rows; add `set_selected(id)`, `list_servers()`, `remove_server(id)`.
- [ ] **T2: `ServerManager`** (AC12–AC14)
  - [ ] T2.1: Define `ServerManager` + `ServerRecord` (`architecture.md:634–646`) in a new module `hifimule-daemon/src/server_manager.rs`.
  - [ ] T2.2: Replace `AppState.provider`/`server_type`/`server_version` (`rpc.rs:66–78`) with `server_manager`; update `AppState` construction in daemon bootstrap.
  - [ ] T2.3: Rewrite `require_provider()` (`rpc.rs:473–479`) per `architecture.md:660–664` (selected id → cached provider; lazy-connect if absent).
  - [ ] T2.4: On startup, load all rows into `ServerManager.servers`; set `selected_server_id` from the `selected = 1` row; **no eager provider connects**.
  - [ ] T2.5: Audit & fix every other read of `state.provider`/`server_type`/`server_version` across `rpc.rs`, `sync.rs`, `api.rs`, scrobbler (architecture.md:782–789 — `AppState.provider` no longer exists). Add `ServerManager::get_provider(serverId)` (lazy) for the sync/playlist routing tasks.
- [ ] **T3: Vault migration** (AC18, AC19)
  - [ ] T3.1: Define `VaultContents = HashMap<String, ServerCredentials>`, `ServerCredentials { token_or_password: String }` (`architecture.md:697–704`).
  - [ ] T3.2: In `api.rs` (`load_secrets`~1828, `save_secrets`~1847) implement deserialize-new → fallback-legacy → migrate → re-encrypt (`architecture.md:706–711`). Real legacy shape is `{ token, server_secrets: HashMap<server_type,String> }` (`api.rs:1815–1821`), not the arch doc's idealized struct.
  - [ ] T3.3: Replace `save_server_secret`/`get_server_secret` (`api.rs:1918–1936`, keyed by `server_type`) with UUID-keyed equivalents; add `remove_server_secret(uuid)`.
  - [ ] T3.4: Preserve the `#[cfg(test)]` `TEST_SECRETS` mock seam (Story 7.5 AC7).
- [ ] **T4: IPC — connect/list/select/remove** (AC5, AC6, AC8, AC20)
  - [ ] T4.1: `server.connect` (`rpc.rs:1269–1382`): generate/upsert UUID, store vault by UUID, return `serverId` (currently only `serverType`/`serverVersion`, `rpc.rs:1377–1381`); auto-select if none.
  - [ ] T4.2: Add `server.list`/`server.select`/`server.remove` dispatch arms (`rpc.rs:296–370`) + handlers.
  - [ ] T4.3: `server.select`: update in-memory `selected_server_id` + DB `selected` flag + lazy-connect provider.
  - [ ] T4.4: `server.remove`: DB delete + vault delete + provider eviction; reselect first remaining or `None` (AC8); reconcile with `server.logout` (`rpc.rs:299`).
- [ ] **T5: `get_daemon_state` extension** (AC15)
  - [ ] T5.1: Add `servers[]` + `selectedServerId` to the returned JSON (build object `rpc.rs:1624–1640`); keep existing fields.

### Phase 2 — Server Hub UI (`hifimule-ui`, Lit)

- [ ] **T6: RPC client wrappers** (AC1, AC2, AC6, AC20)
  - [ ] T6.1: Add `server.list`/`server.select`/`server.remove` wrappers in `rpc.ts` (generic `rpcCall` 75–85; existing `server.probe`/`server.connect` at 61/86).
- [ ] **T7: Server Hub component** (AC1, AC2, AC4, AC6, AC8)
  - [ ] T7.1: New `ServerHub` Lit component modeled on the **Device Hub** card pattern (`BasketSidebar.ts:693–714, 410–438`): clickable cards, highlighted-selected, switch-on-click.
  - [ ] T7.2: Mount as a compact header selector (extend/replace `server-connection-chip`, `main.ts:146–180`) **and** a Settings → Servers tab.
  - [ ] T7.3: Add a minimal Settings view/tab host (none exists today).
  - [ ] T7.4: "Add Server" → present `login.ts` form inline (AC4).
  - [ ] T7.5: "Remove" → confirmation dialog (extra warning for selected server, AC8) → `server.remove` → AC7 removal notification.
- [ ] **T8: First-run vs add-server flow** (AC4, AC10, AC11)
  - [ ] T8.1: In `main.ts` (gating ~71–89), drive UI mode from `servers.length` + `selectedServerId`: 0 servers → full-screen `login.ts`; ≥1 & none selected → AC9 empty state; selected → library.
  - [ ] T8.2: `login.ts` gains inline "add" mode vs first-run full-screen mode.
  - [ ] T8.3: Detect 401 from browse RPC for selected server → targeted re-auth modal (AC11).
- [ ] **T9: Library / Start-Sync gating** (AC9, AC26)
  - [ ] T9.1: No-server empty state in `library.ts`; disable `(+)` (`MediaCard.ts:66` / `library.ts:550`) and Start Sync when `selectedServerId === null`. **Keep Start Sync enabled** for a mixed/non-selected-server basket as long as a device is selected and items exist (AC26) — do not gate sync on `selectedServerId` when the basket is non-empty.

### Phase 3 — Mixed-server basket, read-only rendering & sync routing

- [ ] **T10: Basket store — retention + serverId** (AC3, AC22, AC25)
  - [ ] T10.1: **Stop deleting other-server items.** Replace `removeItemsFromOtherServers()` (`basket.ts:60–83`) and the `hydrateFromDaemon` filter (`basket.ts:107`) so non-active-server items are **retained** and flagged for locked rendering.
  - [ ] T10.2: Keep setting `item.serverId = activeServerId` on add (`basket.ts:214`); ensure `basket.add`/`basket.remove`/`manifest_save_basket` carry `serverId`; daemon validates it against `server_config` (AC25).
  - [ ] T10.3: **Composite→UUID reconciliation** (AC22): on first load after migration, remap items whose `serverId` matches the old composite form of the migrated server to its new UUID, so they aren't locked/wiped. Coordinate with `setActiveServerId` (`basket.ts:50–58`, called `main.ts:141`, `BasketSidebar.ts:256–259, 648–649`).
- [ ] **T11: Read-only basket rendering** (AC35, AC36)
  - [ ] T11.1: In `renderItem()` (`BasketSidebar.ts:1563–1598`) add `basket-item--locked` class + server-name badge + hide `(×)` where `item.serverId !== selectedServerId`. Apply equivalently in `renderAutoFillSlotCard`/`renderGenreCard`/`renderArtistCard`.
  - [ ] T11.2: Mixed-server informational note + visual server grouping in the header/status zone (`BasketSidebar.ts:958–968`/`809–824`).
- [ ] **T12: Auto-fill per server** (AC30–AC32)
  - [ ] T12.1: `insertAutoFillSlot()` (`BasketSidebar.ts:348–355`) sets `serverId = currentServerId`; remove any existing slot first (AC30).
  - [ ] T12.2: Toggle ON/OFF state (`BasketSidebar.ts:373–408`) derives ON only when `slot.serverId === selectedServerId`; locked rendering for foreign-server slot (AC31).
  - [ ] T12.3: Add `serverId` to the `autoFill` sync param; route `run_auto_fill`/`run_auto_fill_provider` via `get_provider(serverId)` (parse `rpc.rs:3360–3390`; Jellyfin `auto_fill.rs:54`; provider `rpc.rs:2227–2293`, `auto_fill.rs:349`) (AC32).
- [ ] **T13: Multi-provider sync routing** (AC27–AC29)
  - [ ] T13.1: Change `itemIds` shape to `Array<{id, serverId}>` in the three RPCs' parse sites (`rpc.rs:3037–3046`; `provider_calculate_delta` `rpc.rs:2179–2225`; execute dispatch `rpc.rs:3736–3780`) and update UI callers.
  - [ ] T13.2: Add `server_id` to `SyncAddItem` (`sync.rs:89–120`); populate during delta calc.
  - [ ] T13.3: **Group items by `serverId`**; for each group resolve provider via `get_provider(serverId)`; run delta calc + container expansion (genre `rpc.rs:3185–3281`, album/playlist `rpc.rs:3283–3317`) + download per group. Remove `active_non_jellyfin_provider` single-global assumption (`rpc.rs:1738–1745`, `3059–3061`).
  - [ ] T13.4: `execute_sync` (Jellyfin, `sync.rs:1596`) and `execute_provider_sync` (`sync.rs:2243`, `download_url` per item `sync.rs:2390`): drive each from its group's provider/credentials. Groups may run concurrently (existing async task model).
  - [ ] T13.5: Aggregate per-group results (synced items + errors) back into one sync result/progress stream.

### Phase 4 — Playlist scope

- [ ] **T14: Daemon cross-server validation** (AC33)
  - [ ] T14.1: Add `ERR_CROSS_SERVER_CONFLICT` to the `ERR_*` constants (`rpc.rs:21–37`).
  - [ ] T14.2: In `handle_playlist_create` (`rpc.rs:912–987`), after resolving via `provider_sync_items_for_id` (`rpc.rs:2107–2177`), verify every item's `serverId === selectedServerId`; on mismatch return the cross-server error and create nothing. Leave addTracks/removeTracks/delete/rename/reorder (server-scoped IDs) untouched.
- [ ] **T15: Save-as-Playlist UI pre-filter** (AC34)
  - [ ] T15.1: In `handleSaveAsPlaylist()` (`BasketSidebar.ts:1660–1747`) filter `manualIds` to `serverId === selectedServerId`; show the cross-server `sl-alert` notice (mirror auto-fill notice `:1667–1672`); reflect filtered count.

### i18n & Verification

- [ ] **T16: i18n** (AC24) — add all new keys (serverHub.*, library.selectServerEmpty, basket.mixedServerNote, basket.playlist.cross_server_notice, re-auth prompts, removal toast) to `hifimule-i18n/catalog.json` for en/fr/es (+de).
- [ ] **T17: Tests & checks** (AC23, AC29)
  - [ ] T17.1: Unit tests — DB migration (INTEGER→UUID, idempotent), vault migration (legacy→map, both credential shapes), `ServerManager` select/remove/lazy-connect/reselect, sync server-grouping, playlist cross-server rejection.
  - [ ] T17.2: `rtk cargo test` + `rtk cargo build` clean; `rtk tsc` + `rtk lint` clean.
  - [ ] T17.3: Manual: two-server (Jellyfin + Navidrome) — switch keeps each server's basket read-only; mixed-basket sync downloads each file from its correct server (AC29); cross-server Save-as-Playlist shows notice and saves only selected-server items; single-server upgrade migrates silently (AC21).

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

### Debug Log References

### Completion Notes List

Ultimate context engine analysis completed — comprehensive developer guide created. Story scoped as the COMPLETE end-to-end multi-server feature: all four proposal phases (infra, hub UI, mixed-server basket + multi-provider sync routing, playlist scope) plus the amended-but-never-implemented behaviors from Stories 3.2/3.8/11.4/11.5 folded in. Two key reconciliations against the actual code captured: (1) no `sync.start` RPC — serverId is threaded through the real 3-RPC sync pipeline; (2) JSON-RPC negative error codes, not HTTP 409. Regression trap (composite-serverId basket wipe on UUID migration) flagged with explicit handling task.

### File List
