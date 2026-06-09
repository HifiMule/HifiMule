# Story 2.13: Portable Server Identity

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a System Admin (Alexis) and multi-server user,
I want each media server to have a stable, machine-independent identity used in device manifests and sync routing,
so that a device synced on one machine is recognized on another, and removing then re-adding the same server does not trigger a needless full resync.

## Context & Problem (read first)

Stories 2.11/2.12 minted a single random `Uuid::new_v4()` (`server_config.id`) and reused it for **everything**: DB row PK, credentials-vault key, provider-cache key, device-manifest `server_id`, basket `serverId`, and sync routing. Because that id is **machine-local and random**:

1. `.hifimule.json` is **not portable** — the `server_id` written into a device's manifest is meaningless on any other machine.
2. **Remove/re-add mints a new UUID** → existing manifest items (tagged with the old id) stop matching → spurious full resync + orphaned manifest entries.

Pre-2.11 the code used a deterministic composite `type|url|user` (still present as `legacy_composite_server_id()`). This story **promotes that deterministic basis into a hashed, persisted, routed canonical identity** — it does not invent a new concept.

**The fix: two distinct identities.**

| Identity | Column | Used for | Stability |
|---|---|---|---|
| `local_id` | `server_config.id` (unchanged) | DB row PK, **vault key**, **provider-cache key**, `server.select/remove/update` keying | Random UUID, machine-local |
| `server_id` (portable) | `server_config.server_id` (NEW) | device manifest `SyncedItem.server_id` / `BasketItem.server_id`, UI basket `serverId`, **sync routing** | Deterministic, identical across machines & remove/re-add |

The change is **invisible to users** — manifest portability + resync avoidance only. No UX/screen/copy changes.

## Acceptance Criteria

1. **Deterministic derivation on connect/upsert.** Given a server is added or reconnected, when `server.connect` / `upsert_server` runs, then the daemon derives a deterministic portable `server_id` and persists it in `server_config.server_id`, while the existing random `id` is retained as the machine-local id. The basis is `sha256("v1|" + serverType + "|url:" + canonicalBaseUrl + "|" + username)`, preferring `sha256("v1|" + serverType + "|rid:" + serverReportedId + "|" + username)` when a server-reported id is available (Jellyfin `System/Info.Id`). Subsonic/OpenSubsonic has no server-id concept → URL basis. Output is lowercase hex.
2. **Cross-machine equality.** Given the same logical server/user (same type/URL/username, same server-reported id where applicable) is configured on two different machines, then both machines derive an identical `server_id`.
3. **Remove/re-add stability.** Given a server is removed and later re-added with the same type/URL/username, then the re-derived `server_id` is identical to the previous one, and existing manifest items tagged with that `server_id` are still recognized — no full resync.
4. **Credentials/cache stay machine-local.** Given the credentials vault and provider cache, then they remain keyed by the machine-local `id`. No credential loss; no vault re-encryption; no cache eviction beyond existing behavior.
5. **Portable→local routing.** Given sync runs with basket/manifest items tagged by portable `server_id`, when the daemon needs a provider for an item, then it resolves `server_id → local id` and reuses the existing per-local-id provider cache via `ServerManager::get_provider_by_server_id`.
6. **Idempotent reconciliation.** Given a device manifest or UI basket holds items tagged with an old random `server_id` (a machine-local UUID written by Story 2.11) **or** the pre-2.11 composite `type|url|user`, when the server is loaded/connected, then those tags are reconciled in place to the new deterministic `server_id`. Reconciliation is idempotent (re-running is a no-op) and never blocks startup.
7. **URL-change fallback documented.** Given the canonical base URL changes but a server-reported id is available, then `server_id` remains stable. If no server-reported id is available, a URL change may yield a new logical identity (documented fallback behavior — acceptable).
8. **Additive contract amendments.** `server.connect` returns `serverId` (portable) **and** `localId`; `server.list` / `get_daemon_state.servers[]` each add `serverId` (portable) alongside `id` (local); `get_daemon_state` adds `selectedServerPortableId` (`selectedServerId` keeps its current local-id meaning). `server.select` / `server.remove` / `server.update` continue to key on local `id`. All existing fields preserved.
9. **Schema migration/backfill.** `server_config` gains `server_id TEXT` and `server_reported_id TEXT` (nullable). A migration backfills `server_id` for existing rows by deriving from stored `server_type`/`url`/`username` (reported id NULL on backfill). Migration is idempotent and does not block startup; handles fresh installs and existing UUID tables.
10. **UI coherence — no foreign-item regression.** The UI's active-server tracking, basket item tagging, and read-only ("locked") comparison all operate on the **portable** `server_id` end-to-end. After this story, a single-server user's own items must NOT appear locked/foreign. (See Regression Risks — this is the highest-risk coherence requirement.)
11. **Tests.** Derivation determinism, cross-machine equality, remove/re-add no-resync, manifest + basket reconciliation idempotency, schema migration/backfill — all covered by tests.

## Tasks / Subtasks

- [ ] **1. Schema + derivation helper (daemon, db.rs)** (AC: #1, #9)
  - [ ] Add `server_id: Option<String>` and `server_reported_id: Option<String>` to `ServerConfig` (`hifimule-daemon/src/db.rs:18`). Keep `id` first/unchanged.
  - [ ] Add `server_id TEXT` and `server_reported_id TEXT` to the `CREATE TABLE IF NOT EXISTS server_config` DDL (`db.rs:142`).
  - [ ] Add an idempotent inline migration in `Database::init` (follow existing `PRAGMA table_info` existence-check + `ALTER TABLE ADD COLUMN` style, `db.rs:158-289`) that adds both columns to existing UUID tables.
  - [ ] Backfill: for existing rows where `server_id IS NULL`, compute `server_id = derive_server_id(server_type, normalized_server_url(url), username, None)` and UPDATE. Idempotent; must not block startup.
  - [ ] Update `row_to_server_config` (`db.rs:320`), and the SELECT column lists in `list_servers` (`db.rs:452`), `get_server` (`db.rs:466`), `get_server_config` (`db.rs:481`) **in lockstep** so the new columns are read.
  - [ ] In `upsert_server` (`db.rs:338-421`): after URL upsert, compute and persist `server_id`. New rows: derive from type/url/username (+ reported id if supplied). Existing rows: re-derive on upsert (keeps stable). Do NOT change the `Uuid::new_v4()` minting of the local `id` (`db.rs:388`).
  - [ ] Implement `derive_server_id(server_type, canonical_base_url, username, server_reported_id: Option<&str>) -> String` exactly per the architecture basis (`v1|`, `rid:`/`url:` tags, lowercase hex sha256). Place it where `normalized_server_url` lives or a shared util; reuse `normalized_server_url` for the canonical URL.
  - [ ] **Hashing crate:** add `sha2 = "0.10"` to `hifimule-daemon/Cargo.toml`. The AC + architecture specify `sha256` for the documented basis — use SHA-256, NOT the existing `blake3` (blake3 is used only for the hardware-uid vault derivation; do not reuse it here, as the documented basis string must hash with sha256 to remain authoritative and cross-machine stable).

- [ ] **2. Capture server-reported id at connect (daemon)** (AC: #1, #2, #7)
  - [ ] Jellyfin: fetch `System/Info` (the `Id` field) during/after connect and surface it from the provider. The Jellyfin provider (`hifimule-daemon/src/providers/jellyfin.rs`) currently exposes `access_token()` and `provider_user_id()` but does NOT fetch System/Info — add a `System/Info.Id` accessor (e.g. `server_reported_id() -> Option<String>`).
  - [ ] Subsonic/OpenSubsonic: no server-id concept → `server_reported_id = None` (URL basis). Do not invent one.
  - [ ] In `handle_server_connect` (`hifimule-daemon/src/rpc.rs:1282-1419`), capture the reported id and pass it into `upsert_server` so `server_reported_id` is persisted and the `rid:` basis is used when present. Preserve all existing connect behavior (probe, vault store by local id, provider cache, URL upsert).

- [ ] **3. Portable→local routing (daemon, server_manager.rs + rpc.rs)** (AC: #4, #5)
  - [ ] Add `server_id: String` and `server_reported_id: Option<String>` to `ServerRecord` (`hifimule-daemon/src/server_manager.rs:20`) and its `From<ServerConfig>` impl (`server_manager.rs:31`).
  - [ ] Add `ServerManager::get_provider_by_server_id(...)` that maps portable `server_id` → the matching record's local `id`, then delegates to the existing `get_provider(manager, db, local_id)` (`server_manager.rs:150`). Keep the cache keyed by local id — do NOT add a second cache.
  - [ ] Add an rpc-level wrapper mirroring `get_provider_for_server` (`rpc.rs:452`) → `get_provider_by_server_id_for(...)` returning `JsonRpcError` on unknown id.
  - [ ] Route sync provider resolution by portable id: in `handle_sync_calculate_delta` (groups items by serverId, `rpc.rs:~3360-3463`) and `handle_sync_execute` multi-server dispatch (`rpc.rs:~4240-4269`) and `run_auto_fill`, resolve providers via `get_provider_by_server_id` instead of `get_provider_for_server(local_id)`. On a single machine `server_id ↔ local_id` is 1:1 (upsert-by-URL prevents dupes).

- [ ] **4. Write portable id into manifest + basket (daemon)** (AC: #5, #6)
  - [ ] Wherever `SyncedItem.server_id` / `BasketItem.server_id` (`hifimule-daemon/src/device/mod.rs:12-61`) are written during sync, write the **portable** `server_id`, never the local id.
  - [ ] Manifest reconciliation: on device load/connect, rewrite any `synced_items[].server_id` / `basket_items[].server_id` that equals a known **local_id** (2.11 random UUID) **or** the pre-2.11 composite (`legacy_composite_server_id`) → that server's portable `server_id`. Idempotent; never blocks startup. Persist the rewritten manifest.
  - [ ] Update the daemon-side `reconcile_basket_server_ids()` (`rpc.rs:~1908-1942`, called by `handle_manifest_get_basket`): its current map is composite→local UUID; extend so the resolved target is the **portable** id, and add the local_id→portable mapping. Preserve existing behavior (drop items for removed servers, adopt selected server for untagged items) but adopt the **portable** id for untagged items.

- [ ] **5. Additive RPC contract (daemon, rpc.rs)** (AC: #8)
  - [ ] `server_row_to_json` (`rpc.rs:1439-1449`): add `"serverId": config.server_id` alongside existing `"id"`.
  - [ ] `handle_server_connect` response: add `serverId` (portable) and `localId` (the local UUID). **Preserve the existing `serverId` field's wire name but change its value to the portable id, and add `localId`** — see Regression Risks; audit consumers.
  - [ ] `handle_get_daemon_state` (`rpc.rs:1776-1900`): add `selectedServerPortableId` (portable id of the selected server) next to the existing `selectedServerId` (keeps local-id meaning); ensure each entry in `servers[]` includes `serverId`.
  - [ ] `server.select` / `server.remove` / `server.update` unchanged — continue to key on local `id`.

- [ ] **6. UI coherence — switch active-server + tagging to portable** (AC: #10)
  - [ ] `hifimule-ui/src/main.ts:66-79`: read `state.selectedServerPortableId` (NEW) and pass it to `basketStore.setActiveServerId(...)` instead of the local `selectedServerId`. This is the linchpin for AC10.
  - [ ] `hifimule-ui/src/state/basket.ts:266` tags new items with `this.activeServerId` — since activeServerId becomes portable, items get tagged portable automatically. Verify no other tagging path uses a local id.
  - [ ] `hifimule-ui/src/components/BasketSidebar.ts`: `currentServerId` (used as the fallback serverId at `:1151`, `:1169`, and to build sync items at `:1901`) must be the **portable** id. Trace how `currentServerId` is set and switch its source to the portable id. The sync payload `items: [{ id, serverId }]` (`:1901`) must carry portable ids so the daemon's `get_provider_by_server_id` resolves them.
  - [ ] `basketStore.reconcileServerIds()` (`hifimule-ui/src/state/basket.ts:113-133`): currently maps composite→local UUID. Extend so it maps **both** the pre-2.11 composite **and** the local UUID → the portable `server_id`. It needs the portable id per server, so the `servers` arg (passed from `ServerHub.ts:31` and elsewhere) must include `serverId`. Persist + notify only when changed (keep idempotency).
  - [ ] `ServerHub.ts`: where it calls `setActiveServerId(id)` on select (`:212`) and `reconcileServerIds(this.servers)` (`:31`), ensure `id`/`servers` carry the portable id. `setActiveServerId(null)` on logout (`:286`) is unchanged.
  - [ ] `BasketItem` interface (`basket.ts:8-19`) and `ServerSummary`/RPC types (`hifimule-ui/src/rpc.ts`): add `serverId` (portable) to server summaries so the UI can map local↔portable. `server.connect` consumers: `login.ts:218` awaits `server.connect` without reading `serverId` (low risk), but audit anyway.

- [ ] **7. Tests** (AC: #11)
  - [ ] Rust unit: `derive_server_id` determinism; identical output for same inputs (cross-machine equality); `rid:` basis preferred when reported id present and non-empty; URL basis when reported id None/empty; URL-change-with-rid stability vs URL-change-without-rid new identity.
  - [ ] Rust: remove/re-add yields identical `server_id`; manifest items tagged with that id remain matched (delta sees them unchanged — no resync).
  - [ ] Rust: schema migration adds columns idempotently; backfill derives correct `server_id`; reconciliation (manifest + `reconcile_basket_server_ids`) is idempotent (second run no-op) and maps local_id & composite → portable.
  - [ ] Rust: contract fields present — `server.list` / `get_daemon_state.servers[]` include `serverId`; `server.connect` returns `serverId` + `localId`; `get_daemon_state` includes `selectedServerPortableId`.
  - [ ] Run `rtk cargo test -p hifimule-daemon`, then `rtk cargo test` (workspace). Run `rtk tsc` for `hifimule-ui`. Run `rtk lint` if a UI lint script exists.

## Dev Notes

### Current State and Guardrails (verified against source)

- **Hashing:** `hifimule-daemon/Cargo.toml` has `blake3`, `md-5` — but **no `sha2`**. The AC/architecture basis is `sha256`. Add `sha2 = "0.10"`; do not substitute blake3. [Source: `hifimule-daemon/Cargo.toml`; `architecture.md:803-816`]
- **`server_config` today** has `id, url, server_type, username, server_version, name, icon, updated_at, selected`. Migration style is inline/idempotent in `Database::init` (PRAGMA checks + ALTER TABLE), no migration framework. The local UUID is minted at `db.rs:388` via `uuid::Uuid::new_v4()`. [Source: `hifimule-daemon/src/db.rs:18-31,142-289,388`]
- **`normalized_server_url(url)`** = `url.trim().trim_end_matches('/').to_ascii_lowercase()` (`rpc.rs:312`). Reuse it for the canonical URL basis — do not reimplement. [Source: `hifimule-daemon/src/rpc.rs:312`]
- **`legacy_composite_server_id(config)`** = `format!("{}|{}|{}", server_type, normalized_server_url(url), username)` (`rpc.rs:377`). This is exactly the pre-2.11 composite that reconciliation must map → portable. [Source: `hifimule-daemon/src/rpc.rs:377-384`]
- **Provider cache** is keyed by local UUID in `ServerManager.providers: HashMap<String, Arc<dyn MediaProvider>>` via `get_provider(manager, db, id)` with double-checked locking (`server_manager.rs:47-52,150-183`). Keep this; add `get_provider_by_server_id` as a thin portable→local translation in front of it. [Source: `hifimule-daemon/src/server_manager.rs`]
- **Manifest items** `SyncedItem.server_id` and `BasketItem.server_id` are already `Option<String>` with `#[serde(default)]` (`device/mod.rs:12-61`). No struct change needed — only the *value* written changes (portable, not local) plus reconciliation. [Source: `hifimule-daemon/src/device/mod.rs`]
- **Jellyfin provider** does NOT currently fetch `System/Info` — only `access_token()` / `provider_user_id()` exist (`jellyfin.rs:577-581`). You must add the `System/Info.Id` fetch + accessor. [Source: `hifimule-daemon/src/providers/jellyfin.rs`]
- **Daemon basket reconciliation** already exists: `reconcile_basket_server_ids()` (`rpc.rs:~1908-1942`), called from `handle_manifest_get_basket`, maps composite→local UUID and adopts the selected server for untagged items. Extend its target to portable. [Source: `hifimule-daemon/src/rpc.rs`]

### UI coherence chain (the AC10 linchpin — get this exactly right)

For a single-server user, their own items must never render locked. "Locked" = `isItemLocked` returns true when `item.serverId && activeServerId && item.serverId !== activeServerId` (`basket.ts:64-70`). Today both sides are the local UUID. After this story **both sides must be the portable id**, end to end:

1. `main.ts:66-79` sets active server from daemon state — switch `selectedServerId` → `selectedServerPortableId`.
2. `basket.ts:266` tags items with `activeServerId` → automatically portable once (1) lands.
3. `BasketSidebar.currentServerId` (fallback tagging `:1151/:1169`, sync payload `:1901`) → must be portable.
4. `reconcileServerIds` (`basket.ts:113-133`) rewrites already-stored items (composite **and** local UUID) → portable, so existing localStorage baskets don't all appear foreign after upgrade.

If any one of these still uses the local id while the others use portable → every item appears foreign/locked. This is the single biggest regression risk; verify with a single-server smoke path.

### Regression Risks

- **`server.connect` response `serverId` semantic flip.** Today `server.connect` returns `serverId` = local UUID. This story changes that field's *value* to the portable id and adds `localId`. **Audit every consumer** of `server.connect`'s result: any code using the returned `serverId` as a local key (select/remove/update/cache) must switch to `localId`. (`login.ts:218` only awaits the call — confirm it doesn't read `serverId`.)
- **Reconciliation must be idempotent and order-independent.** Re-running on already-portable items must be a no-op (do not double-map). Mapping table should be `{composite → portable, local_id → portable}`; never map portable → anything.
- **No spurious resync / no orphaned tags.** Re-deriving must yield the same `server_id` so the differential-sync delta sees existing items as unchanged. Test remove/re-add explicitly.
- **Vault untouched.** Do NOT re-key the credentials vault or provider cache to portable id. They stay on `local_id`. No credential-loss risk should be introduced.
- **Migration must not block startup.** Backfill + reconciliation run idempotently; failures must be logged and skipped, not fatal.
- **i18n:** No new user-facing strings expected (internal change). If any are added, update all four catalogs (en/fr/es/de) in `hifimule-i18n/catalog.json` — existing convention from 2.12.
- **Single-machine 1:1 invariant.** Upsert-by-normalized-URL prevents duplicate rows, so `server_id ↔ local_id` is 1:1 per machine. `get_provider_by_server_id` can assume one match; error cleanly if none.

### Project Structure Notes

- Daemon Rust crate: `hifimule-daemon/src/{db.rs, rpc.rs, server_manager.rs, device/mod.rs, providers/jellyfin.rs}`.
- UI (TypeScript): `hifimule-ui/src/{main.ts, rpc.ts, state/basket.ts, components/ServerHub.ts, components/BasketSidebar.ts, login.ts}`.
- No new modules required; all changes are additive to existing files. Add `sha2` to `hifimule-daemon/Cargo.toml`.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` Story 2.13 (lines 546-607)]
- [Source: `_bmad-output/planning-artifacts/prd.md` FR46 (line 157)]
- [Source: `_bmad-output/planning-artifacts/architecture.md` Server Identity Model — Portable vs Machine-Local (lines 788-871)]
- [Source: `_bmad-output/planning-artifacts/sprint-change-proposal-2026-06-09-portable-server-identity.md`]
- [Source: `_bmad-output/implementation-artifacts/2-12-server-identity-name-and-icon.md` previous story intelligence]
- [Source: `_bmad-output/implementation-artifacts/2-11-multi-server-hub.md` multi-server routing/basket behavior]
- [Source: `hifimule-daemon/src/db.rs`]
- [Source: `hifimule-daemon/src/rpc.rs`]
- [Source: `hifimule-daemon/src/server_manager.rs`]
- [Source: `hifimule-daemon/src/device/mod.rs`]
- [Source: `hifimule-daemon/src/providers/jellyfin.rs`]
- [Source: `hifimule-daemon/Cargo.toml`]
- [Source: `hifimule-ui/src/main.ts`]
- [Source: `hifimule-ui/src/state/basket.ts`]
- [Source: `hifimule-ui/src/components/ServerHub.ts`]
- [Source: `hifimule-ui/src/components/BasketSidebar.ts`]
- [Source: `hifimule-ui/src/rpc.ts`]

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List

## Change Log

- 2026-06-09: Story created from approved sprint-change-proposal-2026-06-09-portable-server-identity. Comprehensive context engine analysis completed — comprehensive developer guide created. Status set to ready-for-dev.
