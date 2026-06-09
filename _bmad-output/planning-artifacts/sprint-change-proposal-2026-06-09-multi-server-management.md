# Sprint Change Proposal — Multi-Server Management

**Date:** 2026-06-09  
**Author:** Alexis (via bmad-correct-course workflow)  
**Classification:** Major  
**Status:** Approved by Alexis on 2026-06-09 — pending implementation handoff

---

## Section 1: Issue Summary

### Problem Statement

HifiMule currently supports exactly one configured media server at a time. The `server_config` table enforces a single-row constraint (`CHECK (id = 1)`), and `AppState` holds a single `Arc<RwLock<Option<Arc<dyn MediaProvider>>>>`. Users with multiple servers (e.g., home Jellyfin + work Navidrome) must fully reconfigure the app to switch between them, losing all basket state in the process.

### Context

This is a new product requirement — not a bug or implementation failure. The existing architecture already anticipated multi-server use: the `MediaProvider` abstraction layer was explicitly designed to be server-agnostic, and the `.hifimule.json` manifest already defines a `server_id` field described as "normalized server URL for multi-server manifests." The foundation exists; this change activates it at the user-facing level.

### Desired End State

- Users can configure and manage multiple media servers via a persistent **Server Hub**
- One server is "selected" at a time; the library browser shows that server's content
- The sync basket can hold items from any server (mixing is allowed)
- Basket items from non-selected servers are visible but read-only
- Playlist creation is scoped to items from the selected server only
- Credentials are stored per-server in the encrypted vault

---

## Section 2: Impact Analysis

### Epic Impact

| Epic | Status | Change Required |
|---|---|---|
| Epic 2: Connection & Verification | Modified | Update Stories 2.1, 2.5; add new Story 2.11 |
| Epic 3: Curation Hub (Basket) | Modified | Update Story 3.2 (serverId on basket items + read-only enforcement); Update Story 3.8 (auto-fill slot server-bound + overwrite-on-enable) |
| Epic 11: Selection-as-Playlist | Modified | Update Stories 11.4 and 11.5 (server scope validation) |
| All other epics | No change | — |

### PRD Impact

- **FR5, FR6, FR7**: Updated from single-server to multi-server scope
- **FR42 (new)**: Server Hub — add, remove, select servers
- **FR43 (new)**: Mixed-server basket — read-only for non-selected server items
- **FR44 (new)**: Playlist operations restricted to selected server

### Architecture Impact

| Component | Change |
|---|---|
| `AppState.provider` | Replaced by `ServerManager` with per-server provider cache |
| `server_config` table | Drop single-row constraint; add `selected` column; migration required |
| `secrets.enc` vault | Store `HashMap<serverId, ServerCredentials>` instead of single blob |
| IPC contract | New `server.list`, `server.select`, `server.remove` RPCs; `server.connect` returns `serverId` |
| `get_daemon_state` | Gains `servers[]` and `selectedServerId` fields |
| Basket item type | Gains `serverId: string` field |
| `sync.start` params | `itemIds` carries per-item `serverId` for multi-provider routing |

### Technical Impact

- **Vault migration**: On first daemon startup after upgrade, the existing single-credential blob is auto-migrated to `HashMap<uuid, credentials>` format. Credentials are irrecoverably lost if the hardware fingerprint changes (existing known limitation — unchanged).
- **Provider lifecycle**: Providers are lazily initialized per server (only when first selected), not on daemon startup for all servers. This keeps idle memory within the < 10MB NFR.
- **sync.start routing**: The daemon groups `itemIds` by `serverId`, calls `ServerManager.get_provider(serverId)` per group, and downloads each file from its correct server. The existing container-expansion logic (`rpc.rs:807–866`) runs per-group.
- **Database migration**: `server_config` schema change requires a SQLite migration (`ALTER TABLE` to add `selected` column + remove CHECK constraint via recreate).

---

## Section 3: Recommended Approach

**Option selected: Direct Adjustment** (new stories added, existing stories modified — no rollback, no MVP scope reduction)

**Rationale:**
- The architecture was designed for this. The `MediaProvider` trait, `server_id` in the manifest, and `require_provider()` helper are all in place. The change activates infrastructure that already exists rather than rearchitecting from scratch.
- No completed stories need to be rolled back. The changes to Stories 2.1, 2.5, 3.2, 11.4, and 11.5 are additive — existing ACs remain valid, new ACs are appended.
- MVP is unaffected. The original MVP (single-device, single-server sync) still works; this is a Growth Feature addition.

**Effort:** High — vault restructuring, database migration, new `ServerManager`, IPC additions, and basket model changes all touch core infrastructure.

**Risk:** Medium — the vault migration is the highest-risk item (credential loss on failure); mitigated by keeping the old format detectable and the migration path explicit. Provider lazy-initialization is straightforward given the existing `require_provider()` pattern.

**Timeline impact:** Adds approximately 1–2 sprints of new story work (Story 2.11 + basket changes + IPC layer). Existing stories 2.1, 2.5, 11.4, 11.5 require partial rework only.

---

## Section 4: Detailed Change Proposals

### PRD Changes

**Update Section "2. Server & Profile Management":**

Replace FR5/FR6/FR7:

```
FR5: Users can configure multiple media server connections, each with its own URL, server type, 
     username, and credentials (API token for Jellyfin or username+password for 
     Subsonic/OpenSubsonic servers). The system auto-detects the server type by pinging the URL 
     when the user enters it.

FR6: Users can select a specific user profile from any configured media server for syncing.

FR7: The system can maintain persistent, encrypted connection state for each configured media 
     server. For Jellyfin, an access token is stored per server. For Subsonic/OpenSubsonic, the 
     user password is stored per server (encrypted) and used to sign each request stateless-style.
```

Add new FRs:

```
FR42: Users can add, remove, and switch between configured media servers via a persistent Server 
      Hub. The selected server is the active server — its library is browsable and its basket 
      items are editable. Only one server can be selected at a time.

FR43: The basket can contain items from multiple media servers simultaneously. Items from 
      non-selected servers are displayed as read-only (visible but not removable or reorderable). 
      Only items from the selected server can be added, removed, or modified in the basket.

FR44: Playlist creation and server-playlist write operations (FR37–FR41) are restricted to 
      tracks from the selected server. The UI prevents mixing items from different servers in a 
      single playlist write operation.
```

---

### Architecture Changes

**1. Replace `AppState.provider` with `ServerManager`:**

```rust
pub struct ServerManager {
    servers: Vec<ServerRecord>,
    selected_server_id: Option<String>,
    providers: HashMap<String, Arc<dyn MediaProvider>>,  // lazy, keyed by server UUID
}

pub struct ServerRecord {
    pub id: String,           // stable UUID
    pub url: String,
    pub server_type: String,  // 'jellyfin' | 'subsonic'
    pub username: String,
}

// In AppState:
pub server_manager: Arc<RwLock<ServerManager>>,

// require_provider() returns the selected server's provider or RpcError::NotConnected
async fn require_provider(state: &AppState) -> Result<Arc<dyn MediaProvider>, RpcError>
```

**2. `server_config` table migration:**

```sql
-- New schema (recreate to drop CHECK constraint)
CREATE TABLE IF NOT EXISTS server_config (
    id          TEXT    PRIMARY KEY,   -- stable UUID (was INTEGER with CHECK id=1)
    url         TEXT    NOT NULL,
    server_type TEXT    NOT NULL,
    username    TEXT    NOT NULL,
    selected    INTEGER NOT NULL DEFAULT 0,
    updated_at  INTEGER NOT NULL
);
-- Migration: existing single row gets generated UUID, selected = 1
```

**3. Vault restructuring:**

Old: `secrets.enc` decrypts to `Secrets { jellyfin_token, subsonic_password }`

New: `secrets.enc` decrypts to `HashMap<String, ServerCredentials>` where key = server UUID, value = `ServerCredentials { token_or_password: String }`

Migration path: On load, if the decrypted blob deserializes as the old `Secrets` struct, wrap it into a map using the existing server's UUID and re-encrypt.

**4. New and modified IPC methods:**

```
// Modified — now returns serverId
server.connect(params: { url, serverType, username, password })
  → { ok, serverId: string, serverType, serverVersion }

// New
server.list → Array<{ id, url, serverType, username, selected: boolean }>
server.select({ id: string }) → { ok: true }
server.remove({ id: string }) → { ok: true }

// get_daemon_state extended
{
  ...existing fields...
  servers: Array<{ id, url, serverType, username, selected: boolean }>,
  selectedServerId: string | null
}
```

**5. Basket item model:**

```typescript
type BasketItem = {
  id: string;
  type: BasketItemType;
  name: string;
  sizeBytes: number | null;
  serverId: string;  // NEW — UUID of originating server
  // ...rest unchanged
}
```

`sync.start` params: `itemIds` becomes `Array<{ id: string, serverId: string }>`. Daemon groups by `serverId`, routes each group to `ServerManager.get_provider(serverId)`.

---

### Story Changes

#### Story 2.1 — Secure Media Server Link (MODIFIED)

**Updated Acceptance Criteria** (replace existing):

```
Given the UI is open in "Settings" → "Servers"
When I enter a server URL, username, and password and click "Add Server"
Then the daemon auto-detects the server type (Story 8.4 factory).
And for Jellyfin: authenticates and stores the access token in the vault keyed by the new 
  server's UUID.
And for Subsonic/Navidrome: stores the password in the vault keyed by the new server's UUID 
  for per-request MD5 signing.
And the connection is validated by a successful ping/library query.
And the new server appears in the Server Hub with its detected type and username.
And if a server with the same URL already exists, its credentials are updated (upsert by URL).
And the newly added server becomes the selected server if no server was previously selected.
```

**Updated Technical Notes** (append):
```
- server.connect RPC now returns { serverId, serverType, serverVersion }.
- Vault key format: server UUID (not URL) to survive URL edits.
- Migration: existing single-server vault entry migrated to { [existingServerId]: credentials } 
  format on first daemon startup after upgrade.
```

---

#### Story 2.5 — Interactive Login & Identity Management (MODIFIED)

**Updated user story framing:**
```
As a Ritualist (Arthur) and System Admin (Alexis),
I want a clear, guided server connection screen where I can add a new server or re-authenticate 
an existing one,
So that I can manage multiple media server connections without manually copying API tokens.
```

**Updated/appended Acceptance Criteria:**
```
Given the application has no configured servers (first run or all removed)
When the Login View is displayed
Then I can enter a server URL, username, and password.
And the UI shows a live type badge as before.
When I click "Connect"
Then the daemon calls server.connect, authenticates, and adds the server to the Server Hub.
And the UI transitions to the main Library Browser with that server selected.

Given the application already has configured servers
When I open Settings → Servers → "Add Server"
Then the same connection form is presented inline (not a full-screen takeover).
And on success, the new server is appended to the hub without disrupting the currently 
  selected server.

Given a previously configured server has an expired or invalid token
When the user selects that server in the Server Hub
Then the UI surfaces a re-authentication prompt for that server specifically.
And re-authentication replaces only that server's credential in the vault.
```

**Updated Technical Notes** (append):
```
- Login screen is shown full-screen only on first run (no servers configured).
- Post-first-run: server addition is an inline form in the Server Hub settings panel.
- Re-auth prompt: UI detects 401 from browse RPC for the selected server → shows targeted 
  re-auth modal for that server's URL.
```

---

#### Story 2.11 — Multi-Server Hub (NEW)

```
### Story 2.11: Multi-Server Hub

As a System Admin (Alexis) and Ritualist (Arthur),
I want a persistent Server Hub where I can see all configured servers, switch the active one, 
and add or remove servers,
So that I have full control over which media library I'm curating from at any time.

**Acceptance Criteria:**

**Given** one or more servers are configured
**When** I open the main UI or Settings → Servers
**Then** the Server Hub is displayed listing all configured servers.
**And** each server shows its URL (or a user-friendly display name), detected type badge 
  (Jellyfin / Subsonic), and username.
**And** the currently selected server is highlighted.

**Given** I click a server in the hub that is not currently selected
**When** the UI calls server.select({ id })
**Then** the daemon updates selectedServerId in ServerManager and persists the selected flag 
  in server_config.
**And** the library browser reloads with the newly selected server's content.
**And** basket items from the previous server become read-only (locked visual state).
**And** basket items from the newly selected server become editable.

**Given** I click "Add Server" in the hub
**Then** the server connection form (Story 2.5) is presented inline.
**And** on success, the new server is appended to the hub.

**Given** I click "Remove" on a server that is not currently selected
**When** I confirm removal in a confirmation dialog
**Then** server.remove({ id }) is called.
**And** the server's credentials are deleted from the vault.
**And** the server is removed from the server_config table.
**And** basket items that originated from that server are removed from the basket with a 
  notification: "X items from [server] were removed from your basket."

**Given** I click "Remove" on the currently selected server
**Then** the confirmation dialog warns that the active server will be deselected.
**And** on confirmation, removal proceeds and selectedServerId is set to the first remaining 
  server (or null if none remain).
**And** if no servers remain, the UI enters the first-run state.

**Given** no server is selected (selectedServerId === null)
**Then** the library browser shows an empty state: "Select a server to browse your library."
**And** all (+) add buttons in the library browser are disabled.

**Technical Notes:**
- server.list → Array<{ id, url, serverType, username, selected }>
- server.select({ id }) → { ok: true } — updates ServerManager + DB
- server.remove({ id }) → { ok: true } — removes from DB + vault; evicts provider from cache
- get_daemon_state gains servers[] and selectedServerId fields
- Provider initialization is lazy: ServerManager calls providers::connect() only when a 
  server is first selected (not at daemon startup for all servers).
- Credential vault migration: existing single-credential blob is auto-migrated to 
  HashMap<serverId, creds> on first startup after upgrade.
- UI: Server Hub lives in a "Servers" tab in Settings AND as a compact selector in the 
  main layout header (analogous to the device hub in the sidebar).
```

---

#### Story 3.8 — Lazy Auto-Fill Virtual Slot (MODIFIED)

**Appended Acceptance Criteria** (existing ACs unchanged):

```
Given the basket sidebar is visible and a server is selected
When I enable the "Auto-Fill" toggle
Then a single Auto-Fill Slot card appears in the basket, bound to the currently selectedServerId.
And if an Auto-Fill Slot already exists (from any server), it is replaced by the new slot.

Given an Auto-Fill Slot is in the basket from server A and I switch to server B
When the basket renders
Then the Auto-Fill Slot is shown as read-only (locked, no toggle affordance) with a server name 
  badge indicating it belongs to server A.
And the auto-fill toggle for server B is shown as OFF.

Given an Auto-Fill Slot is in the basket from server A and I switch to server B and enable 
  auto-fill
When the toggle is turned ON
Then the server A slot is removed and replaced by a new Auto-Fill Slot bound to server B.
And the UI shows no confirmation prompt — overwrite is silent and immediate.

Given I toggle Auto-Fill OFF while any server is selected
Then the Auto-Fill Slot is removed from the basket regardless of which server owns the slot.
```

**Appended Technical Notes:**
```
- AutoFillSlot virtual item gains serverId: string (set to selectedServerId at toggle time).
- Auto-fill toggle state in the UI checks item.serverId === selectedServerId to decide 
  whether to show the toggle as ON or OFF.
- On toggle ON: dispatch removes any existing __auto_fill_slot__ item first, then inserts 
  a new one with serverId = selectedServerId.
- sync.start: autoFill param gains serverId; daemon routes run_auto_fill() to 
  ServerManager.get_provider(serverId) instead of the globally selected provider.
- Multi-slot auto-fill (one per server) is deferred to a future change.
```

**Rationale**: Keeps existing single-slot simplicity. The overwrite-on-enable model is predictable — the user explicitly chose a new server and enabled auto-fill, signalling intent to replace.

---

#### Story 3.2 — The Live Selection Basket (MODIFIED)

**Appended Acceptance Criteria** (existing ACs unchanged):

```
Given the Library Browser and a server is selected
When I click (+) on an album, artist, playlist, or track
Then the item is added to the basket with its serverId set to the currently selectedServerId.

Given the basket contains items from multiple servers
When the basket renders
Then items from the selected server render with normal (+)/(×) controls.
And items from non-selected servers render with a server name badge and no remove (×) control.
And a section divider or label clearly groups items by server.

Given I switch the selected server via the Server Hub
When the basket re-renders
Then previously-selected-server items become locked (read-only).
And newly-selected-server items (if any) become editable.
And the basket header shows an informational note if mixed-server items are present:
  "Items from other servers are read-only until you switch back to that server."

Given the basket contains only items from non-selected servers
Then the "Start Sync" button remains enabled (sync can execute items from any server).
And the storage projection includes all items regardless of server.

Given sync starts with a mixed-server basket
When sync.start is called
Then the daemon groups itemIds by their originating server, instantiates the appropriate 
  provider per group, and downloads each file from its correct server.
```

**Appended Technical Notes:**
```
- BasketItem gains serverId: string (populated from selectedServerId at add time).
- basketStore persists serverId per item in the basket manifest section of .hifimule.json.
- sync.start params: itemIds becomes Array<{ id: string, serverId: string }>; daemon routes 
  each group to ServerManager.get_provider(serverId).
- basket.add / basket.remove RPCs gain serverId in params; daemon validates serverId exists 
  in server_config before accepting.
- Read-only rendering: CSS class basket-item--locked on items where 
  item.serverId !== state.selectedServerId; (×) button hidden.
```

---

#### Story 11.4 — Daemon RPCs (MODIFIED)

**Appended Acceptance Criteria** (after existing playlist.create AC):

```
Given playlist.create is called with itemIds
When the daemon resolves entities to track IDs
Then it verifies all resolved tracks originated from the currently selected server.
And if any track's serverId does not match selectedServerId, the RPC returns:
  { status: "error", code: 409, message: "Playlist creation requires all items to be from 
    the selected server. Switch server or remove cross-server items." }
And no playlist is created on the server.
```

**Appended Technical Notes:**
```
- Daemon validates serverId per itemId against selectedServerId before entity resolution.
- Error code 409 (Conflict) — consistent with existing RPC error envelope.
- playlist.addTracks / removeTracks / delete / rename / reorder operate on server-side 
  playlist IDs which are already server-scoped; no additional validation needed.
```

---

#### Story 11.5 — Basket "Save as Playlist" (MODIFIED)

**Appended Acceptance Criteria** (alongside existing Auto-Fill notice AC):

```
Given I click "Save selection as playlist" and the basket contains items from non-selected 
  servers
When the dialog opens
Then an inline notice informs me: "Only items from [selected server name] will be saved. 
  Items from other servers are not included."
And the item count reflects only the selected-server items.
And I can proceed; cross-server items are silently excluded from the itemIds sent to 
  playlist.create.
```

**Appended Technical Notes:**
```
- UI filters basketStore items to serverId === selectedServerId before building the itemIds 
  array for playlist.create.
- This prevents the 409 error from surfacing in normal flow.
```

---

## Section 5: Implementation Handoff

### Scope Classification: **Major**

Requires architectural changes (new `ServerManager`), database migration, credential vault restructuring, new IPC surface, and basket model changes across multiple epics.

### Handoff Recipients

| Role | Responsibility |
|---|---|
| **Solution Architect** | Review and sign off on `ServerManager` design, vault migration strategy, and `sync.start` multi-provider routing. Amend architecture doc with approved content from Section 4. |
| **Product Manager** | Update PRD with FR5/FR6/FR7 changes and new FR42/FR43/FR44. |
| **Product Owner / Developer** | Update epics.md with all story changes in Section 4. Sequence new Story 2.11 within the Epic 2 backlog (after Story 2.10). Mark modified stories with amendment notes. |
| **Developer** | Implement in the following sequence: (1) Database migration + `ServerManager` struct, (2) Vault restructuring + migration, (3) New IPC methods, (4) Story 2.11 UI, (5) Basket `serverId` + sync routing, (6) Playlist scope validation. |

### Recommended Implementation Sequence

```
Phase 1 — Infrastructure (no UI changes)
  1a. server_config migration + ServerManager struct (replaces AppState.provider)
  1b. Vault migration (HashMap<serverId, creds>)
  1c. New IPC: server.list, server.select, server.remove (extend server.connect)
  1d. get_daemon_state extended fields

Phase 2 — Story 2.11: Server Hub UI
  2a. Server Hub component (Settings → Servers tab + compact header selector)
  2b. First-run / re-auth flows updated (Story 2.5)

Phase 3 — Basket & Sync
  3a. BasketItem.serverId + basket.add/remove RPC changes
  3b. sync.start multi-provider routing
  3c. Read-only basket rendering (Story 3.2)

Phase 4 — Playlist scope
  4a. Server scope validation in playlist.create daemon handler (Story 11.4)
  4b. UI pre-filter + cross-server notice in Save as Playlist dialog (Story 11.5)
```

### Success Criteria

- A user with two configured servers (Jellyfin + Navidrome) can switch between them without losing basket content from either.
- A sync with a mixed-server basket completes successfully, downloading files from the correct server for each item.
- Attempting to save a cross-server basket as a playlist surfaces an informational notice, and the resulting playlist contains only selected-server items.
- Existing single-server users experience zero behavior change after the upgrade (credentials auto-migrated, single server auto-selected).
- Daemon idle memory remains < 10MB (lazy provider initialization ensures only the active provider is live).
