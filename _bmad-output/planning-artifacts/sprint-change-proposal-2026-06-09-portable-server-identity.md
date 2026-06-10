# Sprint Change Proposal — Portable Server Identity

**Project:** HifiMule
**Date:** 2026-06-09
**Author:** Alexis (via bmad-correct-course workflow)
**Mode:** Incremental
**Classification:** Moderate
**Status:** Approved by Alexis on 2026-06-09 — pending implementation handoff

---

## Section 1: Issue Summary

### Problem Statement

Stories 2.11 (Multi-Server Hub) and 2.12 (Server Identity Name & Icon) are implemented and
merged. They introduced a single random `Uuid::new_v4()` per server (`server_config.id`,
[db.rs:388](../../hifimule-daemon/src/db.rs)) and reused that one id for **everything**: the DB row
primary key, the credentials vault key, the provider-cache key, the device-manifest `server_id`,
the basket `serverId`, and sync routing.

Because that id is **machine-local and random**, two problems follow:

1. **Manifests are not portable.** The `server_id` written into a device's `.hifimule.json`
   ([device/mod.rs](../../hifimule-daemon/src/device/mod.rs)) is meaningless on any other machine —
   the same physical device synced from a second computer cannot recognize its own items.
2. **Remove/re-add triggers spurious resync.** Removing a server and re-adding the same
   logical server/user mints a new UUID, so existing manifest items (tagged with the old id) no
   longer match → unnecessary full resync and orphaned manifest entries.

### Context

Discovered during review of the just-completed multi-server work. Notably, **pre-2.11 the code
used a deterministic composite identity** `type|url|user` (still present as
`legacy_composite_server_id()`, [rpc.rs:377](../../hifimule-daemon/src/rpc.rs), and the UI's
`reconcileServerIds()` shim). Story 2.11 regressed that deterministic identity into a random UUID.
This change restores and strengthens portability rather than inventing a new concept.

Issue type: **Technical limitation discovered during/after implementation.**

### Desired End State

- A stable, machine-independent **portable `server_id`**, derived deterministically, tags manifest
  items, basket items, and drives sync routing.
- A separate **machine-local `id`** continues to key the DB row, credentials vault, and provider cache.
- The same logical server/user resolves to the same `server_id` on any machine and across remove/re-add.
- The change is invisible to users — manifest portability and resync-avoidance only.

---

## Section 2: Impact Analysis

### Checklist Status

- [x] 1.1 Triggering stories: 2.11 Multi-Server Hub + 2.12 Server Identity (both `done`).
- [x] 1.2 Core problem defined: technical limitation — machine-local random id used as portable identity.
- [x] 1.3 Evidence: UUID minted at [db.rs:388](../../hifimule-daemon/src/db.rs); written verbatim into manifest; 2.11 regressed prior deterministic composite.
- [x] 2.1 Epic 2 stays `done`; reopened only for a focused follow-up (already `in-progress` for 2.12).
- [x] 2.2 Add Story 2.13 Portable Server Identity.
- [x] 2.3 Epic 3 (basket `serverId`) and force-sync routing change in *meaning* (id becomes portable); ACs hold.
- [N/A] 2.4 No epics invalidated; no new epics needed.
- [x] 2.5 Priority: before further multi-server polish.
- [x] 3.1 PRD: add FR46 (portable, machine-independent identity).
- [x] 3.2 Architecture: dual-id model, new columns, derivation fn, routing translation, reconciliation, RPC additions.
- [N/A] 3.3 UX: no user-visible change.
- [x] 3.4 Other: tests + sprint-status entry.
- [x] 4.1 Direct Adjustment viable — effort Medium, risk Low-Medium.
- [N/A] 4.2 Rollback not needed. [N/A] 4.3 MVP unaffected.
- [x] 4.4 Recommended path: Direct Adjustment via new Story 2.13.
- [x] 5.1–5.5 Proposal components completed.
- [!] 6.3 User approval pending.
- [!] 6.4 Sprint status update pending approval.

### Epic Impact

| Epic | Status | Change |
|---|---|---|
| Epic 2: Connection & Verification | Modified | Add Story 2.13 (Epic 2 already `in-progress` for 2.12) |
| Epic 3: Curation Hub (Basket) | Semantics | Basket `serverId` becomes portable id; ACs unchanged |
| Force-sync / device manifest | Semantics | Manifest `server_id` becomes portable id; reconciliation added |
| All other epics | No change | — |

### Artifact Impact

- **PRD:** add FR46 — portable, machine-independent server identity (manifest portability + resync avoidance).
- **Epics:** add Story 2.13 Portable Server Identity after Story 2.12.
- **Architecture:** new subsection "Server Identity Model — Portable vs Machine-Local (Story 2.13)":
  dual-id table, `derive_server_id`, schema (`server_id`, `server_reported_id` columns), `ServerRecord`
  amendment, `get_provider_by_server_id` routing translation, idempotent reconciliation, additive contract
  changes, enforcement rules.
- **UX:** none.
- **Sprint status:** add `2-13-portable-server-identity: backlog`.

### Technical Impact

- New SQLite columns `server_config.server_id` and `server_config.server_reported_id`, backfilled on migration.
- New `derive_server_id(server_type, canonical_base_url, username, server_reported_id)` —
  `sha256("v1|<type>|url:<canonicalUrl>|<user>")`, preferring `rid:<serverReportedId>` when available
  (Jellyfin `System/Info.Id`; Subsonic/OpenSubsonic uses URL basis).
- `ServerManager::get_provider_by_server_id` maps portable → local before reusing the existing
  per-local-id provider cache; vault and cache stay keyed by `local_id`.
- Idempotent manifest + UI-basket reconciliation maps old random UUIDs and the pre-2.11 composite to the
  portable `server_id`.
- Additive RPC fields: `server.connect` returns `serverId` + `localId`; `server.list` /
  `get_daemon_state.servers[]` add `serverId`; `get_daemon_state` adds `selectedServerPortableId`.
  `server.select/remove/update` keep keying on local `id`.

---

## Section 3: Recommended Approach

**Option selected: Direct Adjustment** (new Story 2.13; existing reviewed stories not rewritten).

**Rationale:**
- The deterministic basis already exists (`legacy_composite_server_id`, `reconcileServerIds`); this
  promotes it into a hashed, persisted, routed canonical identity rather than building from scratch.
- A follow-up story preserves the reviewed 2.11/2.12 history (same pattern used for 2.12 itself).
- MVP is unaffected; no rollback required.

**Effort:** Medium — schema migration, derivation + routing translation, and idempotent
manifest/basket reconciliation across daemon and UI.

**Risk:** Low-to-Medium. Highest risk is reconciliation correctness (must avoid both spurious resync
and orphaned tags); mitigated by idempotent reconciliation, the existing composite shim, and targeted
tests (cross-machine equality, remove/re-add no-resync). Vault layout is untouched (stays on `local_id`),
so no credential-loss risk is introduced.

**Timeline impact:** One focused story.

---

## Section 4: Detailed Change Proposals

### 4.1 PRD — add FR46

Section: Functional Requirements → Server & Profile Management. Add after FR44:

```markdown
FR46: Each configured media server has a stable, machine-independent logical identity
      ("portable server id") used to tag synced items and basket items in the device
      manifest (.hifimule.json) and to route sync. The portable id is derived
      deterministically from the server's identity (server type, canonical base URL,
      username; preferring a server-reported id when available), so the same logical
      server/user resolves to the same id on any machine and across remove/re-add cycles.
      A separate machine-local id continues to key local storage, credentials vault, and
      the provider cache. Changing this identity is invisible to users; it only affects
      manifest portability and avoidance of unnecessary re-sync.
```

### 4.2 Epics — add Story 2.13

Insert after Story 2.12:

```markdown
### Story 2.13: Portable Server Identity

As a System Admin (Alexis) and multi-server user,
I want each media server to have a stable, machine-independent identity used in device
manifests and sync routing,
So that a device synced on one machine is recognized on another, and removing then
re-adding the same server does not trigger a needless full resync.

**Acceptance Criteria:**

**Given** a server is added or reconnected
**When** `server.connect` / upsert runs
**Then** the daemon derives a deterministic portable `server_id` and persists it in
  `server_config.server_id`, while the existing random `id` is retained as the machine-local id.
**And** the basis is `sha256("v1|" + serverType + "|" + canonicalBaseUrl + "|" + username)`,
  preferring `sha256("v1|" + serverType + "|rid:" + serverReportedId + "|" + username)` when a
  server-reported id is available (Jellyfin `System/Info.Id`); Subsonic/OpenSubsonic uses the
  canonical-URL basis.

**Given** the same logical server/user is configured on two different machines
**Then** both machines derive an identical `server_id`.

**Given** a server is removed and later re-added with the same type/URL/username
**Then** the re-derived `server_id` is identical to the previous one.
**And** existing manifest items tagged with that `server_id` are still recognized — no full resync.

**Given** the credentials vault and provider cache
**Then** they remain keyed by the machine-local `id` (credentials are machine-local).

**Given** sync runs with basket/manifest items tagged by portable `server_id`
**When** the daemon needs a provider for an item
**Then** it resolves `server_id -> local id` and uses the existing per-local-id provider cache.

**Given** a device manifest or UI basket holds items tagged with an old random `server_id`
  (a machine-local UUID written by Story 2.11) or the pre-2.11 composite `type|url|user`
**When** the server is loaded/connected
**Then** those tags are reconciled in place to the new deterministic `server_id`.
**And** reconciliation is idempotent and never blocks startup.

**Given** the canonical base URL changes but a server-reported id is available
**Then** `server_id` remains stable.
**And** if no server-reported id is available, a URL change may yield a new logical identity
  (documented fallback behavior).

**Technical Notes:**
- `server_config` adds `server_id TEXT` (deterministic) and `server_reported_id TEXT NULL`
  (captured at connect for stable re-derivation). Existing `id` becomes the documented
  machine-local id (vault key, provider-cache key, select/remove/update key).
- New daemon helper `derive_server_id(server_type, canonical_base_url, username, reported_id)`.
  Reuse `normalized_server_url()` for the canonical base URL.
- `ServerManager` gains `get_provider_by_server_id(server_id)` mapping portable -> local
  before delegating to the existing `get_provider(local_id)`.
- Device manifest `SyncedItem.server_id` / `BasketItem.server_id` and UI basket `serverId`
  store the portable `server_id`. Extend `reconcileServerIds()` to map random-UUID -> portable.
- A migration backfills `server_id` for existing rows by deriving from stored type/url/username.
- Contracts: `server.list` / `get_daemon_state.servers[]` return both `id` (local) and
  `serverId` (portable); `get_daemon_state` adds `selectedServerPortableId`; `server.connect`
  returns `serverId` (portable) and `localId`. `server.select/remove/update` keep using local `id`.
- Tests: derivation determinism, cross-machine equality, remove/re-add no-resync, manifest +
  basket reconciliation idempotency, schema migration/backfill.

**Prerequisites:** Story 2.11, Story 2.12.
```

### 4.3 Architecture — add "Server Identity Model — Portable vs Machine-Local (Story 2.13)"

Insert after the `sync.start — Multi-Provider Routing` block, before `Enforcement — All AI Agents MUST`:

````markdown
### Server Identity Model — Portable vs Machine-Local (Story 2.13)

**Problem:** Stories 2.11/2.12 used a single random `Uuid::new_v4()` (`server_config.id`) for
*everything* — DB row PK, vault key, provider-cache key, AND the manifest/basket/sync `serverId`.
Because that id is machine-local and random, `.hifimule.json` is not portable across machines,
and remove/re-add of the same logical server mints a new id → spurious full resync and orphaned
manifest items. (Pre-2.11 used a deterministic composite `type|url|user`; 2.11 regressed it.)

**Resolution — two distinct identities:**

| Identity | Column | Used for | Stability |
|---|---|---|---|
| `local_id` | `server_config.id` (unchanged) | DB row PK, **vault key**, **provider-cache key**, `server.select/remove/update` | Random UUID, machine-local |
| `server_id` (portable) | `server_config.server_id` (NEW) | device manifest `SyncedItem.server_id` / `BasketItem.server_id`, UI basket `serverId`, **sync routing** | Deterministic, identical across machines & re-adds |

**Derivation (daemon, at connect/upsert):**
```rust
fn derive_server_id(
    server_type: &str,
    canonical_base_url: &str,   // normalized_server_url(): scheme+host+port+path, lowercased host, no trailing slash
    username: &str,
    server_reported_id: Option<&str>,  // Jellyfin System/Info.Id; None for Subsonic/OpenSubsonic
) -> String {
    let basis = match server_reported_id {
        Some(rid) if !rid.is_empty() => format!("v1|{server_type}|rid:{rid}|{username}"),
        _                            => format!("v1|{server_type}|url:{canonical_base_url}|{username}"),
    };
    sha256_hex(basis.as_bytes())   // lowercase hex
}
```
- The `v1|` prefix and `rid:` / `url:` basis tags allow future versioning without collisions.
- `server_reported_id` is captured at connect into a new `server_config.server_reported_id TEXT NULL`
  so the basis is recomputable and stable. Jellyfin populates it from `System/Info.Id`; Subsonic/
  OpenSubsonic has no server-id concept → URL basis. **Consequence:** for URL-basis servers, a base-URL
  change yields a new logical identity (documented fallback); for `rid`-basis servers, identity survives URL changes.

**Schema amendment:**
```sql
ALTER TABLE server_config ADD COLUMN server_id           TEXT;  -- deterministic portable id
ALTER TABLE server_config ADD COLUMN server_reported_id  TEXT;  -- nullable; basis input
-- Backfill on migration: server_id = derive_server_id(server_type, url, username, NULL) for existing rows.
```

**ServerRecord amendment:**
```rust
pub struct ServerRecord {
    pub id: String,                      // machine-local id (was "stable UUID")
    pub server_id: String,               // NEW — deterministic portable id
    pub server_reported_id: Option<String>, // NEW
    pub url: String,
    pub server_type: String,
    pub username: String,
    pub name: Option<String>,
    pub icon: Option<String>,
}
```

**Routing translation:** vault and provider cache stay keyed by `local_id`. Manifest/basket/sync carry
`server_id`. `ServerManager` gains:
```rust
// Resolve a portable server_id to the local record, then reuse the existing per-local-id cache.
pub async fn get_provider_by_server_id(state: &AppState, server_id: &str)
    -> Result<Arc<dyn MediaProvider>, RpcError>;
```
`sync.start` grouping and `run_auto_fill()` route via `get_provider_by_server_id` instead of
`get_provider(local_id)`. On a single machine `server_id ↔ local_id` is 1:1 (upsert-by-URL prevents dupes).

**Reconciliation (idempotent, on startup/connect — no spurious resync):**
- Device manifests: rewrite any `synced_items[].server_id` / `basket_items[].server_id` that equals a known
  `local_id` (2.11 random UUID) **or** the pre-2.11 composite `type|url|user` → that server's portable `server_id`.
- UI: extend `reconcileServerIds()` to map `local_id → server_id` in addition to the existing composite mapping.
- Because re-deriving yields the same `server_id`, remove/re-add leaves manifest tags valid → delta sees items as unchanged.

**Contract amendments (additive — existing fields preserved):**
- `server.connect` → adds `serverId` (portable) and `localId` to the existing response.
- `server.list` / `get_daemon_state.servers[]` → each record adds `serverId` (portable) alongside `id` (local).
- `get_daemon_state` → adds `selectedServerPortableId`. `selectedServerId` keeps its current meaning (local id).
- `server.select` / `server.remove` / `server.update` → unchanged; continue to key on local `id`.
- UI basket `setActiveServerId()` switches to compare against `selectedServerPortableId`; basket items tag with portable `serverId`.

**Enforcement additions:**
- Never write a `local_id` into the device manifest or basket `serverId` — always the portable `server_id`.
- Keep the vault and provider cache keyed by `local_id`; translate portable→local at the routing boundary.
- `derive_server_id` is the single source of truth for portable identity — do not reconstruct the basis ad hoc.
````

### 4.4 Sprint Status

Add to the Epic 2 block:

```yaml
  2-13-portable-server-identity: backlog  # added via sprint-change-proposal-2026-06-09-portable-server-identity
```

### 4.5 UX

No change — the portable identity is internal; no screen, flow, or copy is affected.

---

## Section 5: Implementation Handoff

### Scope Classification: **Moderate**

Adds a focused story plus schema migration, derivation/routing logic, and idempotent reconciliation
across daemon and UI — but no architectural restructuring and no credential-vault changes.

### Handoff Recipients

| Role | Responsibility |
|---|---|
| **Product Owner / Developer** | Apply FR46 to `prd.md`, Story 2.13 to `epics.md`, and the sprint-status entry. |
| **Developer** | Implement Story 2.13 per the sequence below. |

### Recommended Implementation Sequence

```
1. Schema: add server_config.server_id + server_reported_id; migration backfill.
2. derive_server_id() helper + capture server_reported_id at connect (Jellyfin System/Info.Id).
3. ServerManager::get_provider_by_server_id (portable → local); route sync.start + run_auto_fill via it.
4. Write portable server_id into manifest (SyncedItem/BasketItem) and basket tagging.
5. Reconciliation: manifest rewrite + extend UI reconcileServerIds() (local_id & composite → portable).
6. Additive RPC fields (serverId/localId/selectedServerPortableId); UI active-server comparison switch.
7. Tests: determinism, cross-machine equality, remove/re-add no-resync, reconciliation idempotency, migration.
```

### Success Criteria

- Two machines configured for the same logical server/user derive an identical `server_id`.
- Remove then re-add of the same server preserves manifest item recognition — no full resync.
- A device's `.hifimule.json` synced on machine A is recognized on machine B.
- Credentials and provider cache remain keyed by machine-local `id`; no credential loss.
- Existing single- and multi-server users see no behavior change beyond a one-time idempotent reconcile.

---

## Section 6: Approval

Approved by Alexis on 2026-06-09.

Applied artifact updates:

- Added FR46 to `prd.md` (Server & Profile Management + FR-to-epic map).
- Added Story 2.13 and the FR46 mapping to `epics.md`.
- Added "Server Identity Model — Portable vs Machine-Local (Story 2.13)" to `architecture.md`.
- Added `2-13-portable-server-identity: backlog` to `sprint-status.yaml`.
- UX specification: no change (internal identity only).
