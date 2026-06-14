---
baseline_commit: 8b4ff3781ad53fc07d1fcc86338dc2f4f3d5d0eb
---

# Story 12.2: Auto-Fill Manifest Schema & DB History Scaffolding

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer,
I want `manifest.autoFill` to become a per-server `Map<serverId, AutoFillPipeline>` — actively migrating any legacy `{ enabled, maxBytes }` block onto the **selected server's portable `serverId`** on load — and a daemon DB `autofill_history` table to exist,
so that per-server pipeline config becomes the real on-disk shape (visible, not hidden behind a shim) while today's single-server auto-fill behavior is preserved, and Epic 13 strategies have a machine-local place to store runtime state.

## Acceptance Criteria

1. **Manifest field becomes a per-server pipeline map.** Given the device manifest, when `auto_fill` is modeled, then `DeviceManifest.auto_fill` is a type holding `pipelines: HashMap<String, AutoFillPipeline>` keyed by the **portable `server_id`** (the same id used on `SyncedItem.server_id` / `BasketItem.server_id`), where `AutoFillPipeline` is the Story 12.1 type (`crate::auto_fill::AutoFillPipeline`). [Source: architecture.md#Auto-Fill-Pipeline-Model (lines 792–807); device/mod.rs:163-168]
2. **Legacy block is actively migrated onto the selected server.** Given an existing on-disk manifest whose `autoFill` is the legacy shape `{ "enabled": <bool>, "maxBytes": <u64|null> }` **and** a server is currently selected with a portable `server_id`, when the device is detected/loaded, then the legacy block is converted to `pipelines[selectedServerId] = AutoFillPipeline::default_legacy(maxBytes)` with `enabled` carried over, the legacy block is cleared, and the manifest is **persisted** (best-effort, mirroring the existing `reconcile_manifest_server_ids` persist path). After migration the on-disk `autoFill` is the per-server map shape. The migration is idempotent (a manifest already in map shape is untouched) and only runs for a **meaningful** legacy block (`enabled == true` OR `maxBytes` is set) — a fresh/default `{ enabled: false, maxBytes: null }` is treated as "no config" and not migrated. [Source: device/mod.rs:343-357 (reconcile+persist pattern); architecture.md lines 805–807; auto_fill/pipeline.rs `default_legacy`]
3. **Both JSON shapes deserialize unambiguously.** Given either the legacy object `{ enabled, maxBytes }` **or** the new per-server map `{ "<serverId>": { …AutoFillPipeline… }, … }`, when `serde_json::from_str::<DeviceManifest>(...)` runs, then each shape deserializes into the correct internal representation (legacy → `legacy: Some(prefs)`, where an empty default `{ false, null }` deserializes to `legacy: None`; map → `pipelines`), and an empty/absent `autoFill` yields an empty, disabled config (via `#[serde(default)]`). The discriminator must be robust: legacy values are scalars/null under `enabled`/`maxBytes`; per-server values are pipeline **objects**.
4. **Single-server behavior is preserved (server-aware reads).** Given the current single-server auto-fill code paths, when the manifest type changes + the legacy block migrates, then every existing consumer produces **identical** behavior and identical `get_daemon_state` JSON (`autoFill: { enabled, maxBytes }`):
   - Consumers **without** server context (`main.rs:578,581,1119,1141,1143` — `run_auto_sync` / `run_auto_sync_via_provider`, which take a client/provider, not a db handle) read via server-agnostic accessors `legacy_enabled()` / `legacy_max_bytes()` that resolve the effective single pipeline (the migrated entry, since single-server installs hold exactly one) and fall back to the legacy block.
   - Consumers **with** server context (`rpc.rs:1855-1860` `get_daemon_state`, `rpc.rs:5810-5811` `setAutoFill`, both have `state.db`) resolve the selected portable id via `db.get_server_config()?.server_id` and read/write `pipelines[selectedServerId]` (read via `enabled_for(Some(id))` / `max_bytes_for(Some(id))`; write via `set_for(id, …)`), keeping the emitted JSON shape `{ enabled, maxBytes }` unchanged.
   No behavior change ships: single-slot, selected-server auto-fill works exactly as today; **multi-slot sync-time expansion remains Story 12.3.** [Source: rpc.rs:1855-1860,5805-5813; main.rs:548-591,1095-1153]
5. **`autofill_history` table created (schema only).** Given the daemon DB `init()`, when the daemon starts, then a `CREATE TABLE IF NOT EXISTS autofill_history` runs idempotently (matching the existing `db.rs` migration style) with columns covering the architecture's runtime-state needs — at minimum `device_id`, `server_id`, `track_id`, `last_synced_at`, `tier` — and a primary key on `(device_id, server_id, track_id)`. The table is **scaffolding only**: no story-12.2 code reads or writes rows (consumed by Epic 13). [Source: architecture.md (lines 809–812); db.rs:135-216]
6. **Storage-split enforcement holds.** Given the all-agents enforcement rule, when this story is implemented, then pipeline **config** lives only in the manifest (portable, `server_id`-keyed) and runtime **history** lives only in the daemon DB — never mixed: no pipeline config is written to `autofill_history`, and no cooldown/rotation/pity-timer state is written to the manifest. [Source: architecture.md#Enforcement (line 922)]
7. **Scope boundary — schema + migration only, no expansion/wiring.** Given the epic sequencing, when implementation is complete, then this story does **NOT**: change sync-time expansion or `sync.start` (Story 12.3), make auto-fill run against more than the selected server's slot, add `autoFill.setPipeline` / `basket.autoFill` serverId params (Story 12.7), build any UI (Story 12.6), fetch from providers, read/write `autofill_history` rows, or add a new crate dependency. The auto-fill *behavior* remains exactly today's single-slot algorithm against the selected server. [Source: epics.md#Epic-12 sequencing; sprint-change-proposal-2026-06-14-configurable-auto-fill.md:132-135]
8. **Build & tests green.** Given the workspace, when `rtk cargo test -p hifimule-daemon` runs, then all existing daemon tests pass (no regression — note ~30 `auto_fill: AutoFillPrefs::default()` fixture initializers must be updated to the new type), new migration / round-trip / deserialization tests pass, and `rtk cargo clippy -p hifimule-daemon --all-targets` introduces no new warnings in touched modules.

## Tasks / Subtasks

- [x] **Design the manifest config type** (`hifimule-daemon/src/device/mod.rs`) (AC: 1, 3)
  - [x] Add `use crate::auto_fill::AutoFillPipeline;` (`HashMap` is already used in the module for `folder_ids`).
  - [x] Introduce a public type `AutoFillConfig` holding both the new map and a transient legacy carrier:
    ```rust
    #[derive(Debug, Clone, Default, PartialEq)]
    pub struct AutoFillConfig {
        /// Per-portable-serverId pipeline configs (the new model). Empty = none configured.
        pub pipelines: HashMap<String, AutoFillPipeline>,
        /// Legacy single-block read from an old manifest, pending migration onto the
        /// selected server's portable id (see migrate_legacy_to_selected). `None` once
        /// migrated, or when the legacy block was the empty default.
        pub legacy: Option<AutoFillPrefs>,
    }
    ```
  - [x] Keep `AutoFillPrefs { enabled, max_bytes }` (`device/mod.rs:163-168`) — it is the legacy on-disk shape and the migration carrier. Do not delete it.
  - [x] Implement **custom `Serialize`/`Deserialize`** for `AutoFillConfig`:
    - **Deserialize:** Distinguish shapes robustly (recommended: an internal `#[serde(untagged)]` enum `{ PerServer(HashMap<String, AutoFillPipeline>), Legacy(AutoFillPrefs) }` with `PerServer` tried first, or a `serde_json::Value` peek). Per-server map values are pipeline **objects**; legacy `enabled`/`maxBytes` values are scalar/null, so `HashMap<String, AutoFillPipeline>` deserialization fails and falls back to `AutoFillPrefs`. Then normalize: a legacy block equal to the empty default (`enabled == false && max_bytes.is_none()`) → `legacy: None`; otherwise `legacy: Some(prefs)`. Empty `{}` → empty `pipelines`.
    - **Serialize:** If `pipelines` non-empty → serialize the map (serverId keys verbatim, values `AutoFillPipeline`). Else if `legacy` is `Some` → serialize the legacy block as `AutoFillPrefs` does (`{ "enabled", "maxBytes" }`). Else → serialize the empty default `{ "enabled": false, "maxBytes": null }`. **Verify the empty/default case matches today's `AutoFillPrefs::default()` JSON byte-for-byte** (so `get_daemon_state` and new-device manifests are unchanged).
  - [x] Change the manifest field to `pub auto_fill: AutoFillConfig,` (keep `#[serde(default)]`). [device/mod.rs:101-102]

- [x] **Implement migration onto the selected server** (`hifimule-daemon/src/device/mod.rs`) (AC: 2)
  - [x] On `AutoFillConfig`, add `pub fn migrate_legacy_to(&mut self, server_id: &str) -> bool`: if `legacy` is `Some(prefs)` and `!self.pipelines.contains_key(server_id)`, insert `pipelines.insert(server_id.to_string(), pipeline_from_legacy(&prefs))`, set `legacy = None`, return `true` (changed). `pipeline_from_legacy` builds `AutoFillPipeline::default_legacy(prefs.max_bytes)` and sets its `enabled = prefs.enabled`. Idempotent: returns `false` when there is nothing to migrate.
  - [x] Wire it into `DeviceManager::handle_device_detected` (`device/mod.rs:329-357`), immediately after the existing `reconcile_manifest_server_ids` + best-effort `write_manifest` block. Resolve the selected portable id and migrate:
    ```rust
    if let Ok(Some(sel)) = self.db.get_server_config()      // selected = 1 row
        && let Some(portable) = sel.server_id
        && manifest.auto_fill.migrate_legacy_to(&portable)
        && let Err(e) = write_manifest(std::sync::Arc::clone(&device_io), &manifest).await
    {
        daemon_log!("[Device] Auto-fill legacy migration persist failed (continuing): {}", e);
    }
    ```
    (Match the surrounding best-effort style — never block device load on a write failure. `self.db` is available on `DeviceManager`; `get_server_config()` returns the selected server, whose `.server_id` is the portable id — `db.rs:630-643,30-39`.)
  - [x] **Caveat to document:** the legacy block carries no serverId, so it is attributed to the **currently selected** server (the only faithful target). If no server is selected / no portable id exists yet, leave `legacy` in place — migration runs on a later detect once a server is selected.

- [x] **Add behavior-preserving accessors** (`hifimule-daemon/src/device/mod.rs`) (AC: 4)
  - [x] `pub fn enabled_for(&self, server_id: Option<&str>) -> bool` and `pub fn max_bytes_for(&self, server_id: Option<&str>) -> Option<u64>`: resolve `pipelines.get(server_id)` when `server_id` is `Some` and present; else if `pipelines.len() == 1` use that single entry (single-server install / no-context callers); else fall back to the `legacy` block. Read `enabled` from the pipeline's `enabled`, max-bytes from `pipeline.budget.max_bytes`.
  - [x] `pub fn legacy_enabled(&self) -> bool { self.enabled_for(None) }` and `pub fn legacy_max_bytes(&self) -> Option<u64> { self.max_bytes_for(None) }` — thin wrappers for the no-server-context call sites.
  - [x] `pub fn set_for(&mut self, server_id: &str, enabled: bool, max_bytes: Option<u64>)`: upsert `pipelines[server_id]` from `pipeline_from_legacy(&AutoFillPrefs { enabled, max_bytes })`; clear `legacy`. Used by `setAutoFill` (which has db context to resolve the selected portable id).
  - [x] `pub fn set_legacy(&mut self, enabled: bool, max_bytes: Option<u64>)`: sets the `legacy` block (fallback used only when no selected portable id is available at write time).
  - [x] Optional but cheap: `pub fn pipeline_for(&self, server_id: &str) -> Option<&AutoFillPipeline>` for Story 12.3.

- [x] **Update consumers (behavior-preserving)** (AC: 4, 8)
  - [x] `main.rs:225` → `manifest.auto_fill.legacy_enabled()`
  - [x] `main.rs:578` `if manifest.auto_fill.enabled` → `if manifest.auto_fill.legacy_enabled()`
  - [x] `main.rs:581` `manifest.auto_fill.max_bytes` → `manifest.auto_fill.legacy_max_bytes()`
  - [x] `main.rs:1119` `!manifest.auto_fill.enabled` → `!manifest.auto_fill.legacy_enabled()`
  - [x] `main.rs:1141,1143` → `legacy_enabled()` / `legacy_max_bytes()`
  - [x] `rpc.rs:1855-1860` (`get_daemon_state`): resolve selected portable id via `state.db.get_server_config()` (or the daemon-state field already computed nearby — check for `selectedServerPortableId` resolution already present in this RPC) and emit `{ "enabled": d.auto_fill.enabled_for(portable), "maxBytes": d.auto_fill.max_bytes_for(portable) }`. JSON shape unchanged.
  - [x] `rpc.rs:5805-5813` (`setAutoFill`): resolve selected portable id; if `Some(id)` → `m.auto_fill.set_for(&id, auto_fill_enabled, max_fill_bytes)`; else `m.auto_fill.set_legacy(auto_fill_enabled, max_fill_bytes)`.
  - [x] After edits: `grep -rn "auto_fill\.\(enabled\|max_bytes\)" hifimule-daemon/src` must return zero direct field accesses.

- [x] **Update all struct-literal initializers** (AC: 8)
  - [x] Replace every `auto_fill: crate::device::AutoFillPrefs::default(),` / `auto_fill: AutoFillPrefs::default(),` with `AutoFillConfig::default()`. Sites: `sync.rs:3442`, `device/mod.rs:815`, `rpc.rs` (6037, 6819, 7000, 7941, 8050, 8131, 8187, 8260, 8402, 8504, 8766, 8862, 9014, 9031, 9094), `tests.rs:71,149,217`, `device/tests.rs` (many — grep the full list). `grep -rn "AutoFillPrefs::default()" hifimule-daemon/src` should return zero in non-test field positions afterward (`AutoFillPrefs` itself is still referenced by `AutoFillConfig`'s serde/migration).

- [x] **Scaffold the `autofill_history` DB table** (`hifimule-daemon/src/db.rs`) (AC: 5, 6)
  - [x] In `Database::init()` (`db.rs:135-216`), after the `server_config`/`migrate_server_config_to_multi` block and before `Ok(())`, add:
    ```rust
    conn.execute(
        "CREATE TABLE IF NOT EXISTS autofill_history (
            device_id TEXT NOT NULL,
            server_id TEXT NOT NULL,
            track_id TEXT NOT NULL,
            last_synced_at INTEGER,
            tier TEXT,
            PRIMARY KEY (device_id, server_id, track_id)
        )",
        [],
    )
    .map_err(|e| anyhow!("Failed to create autofill_history table: {}", e))?;
    ```
  - [x] Doc-comment: Epic-12.2 scaffolding consumed by Epic 13 (cooldown windows, stable-core, pity-timer); `server_id` is the **portable** id (matches manifest keys), keyed machine-local per device+server. No reads/writes in 12.2. [Source: architecture.md lines 809-812, 922]

- [x] **Tests** (`device/tests.rs` and `db` tests) (AC: 2, 3, 4, 8)
  - [x] Deserialize legacy `{ "enabled": true, "maxBytes": 8000000000 }` → `legacy == Some(AutoFillPrefs { true, Some(8_000_000_000) })`, `pipelines` empty.
  - [x] Deserialize empty default `{ "enabled": false, "maxBytes": null }` → `legacy == None`, `pipelines` empty (no spurious migration trigger).
  - [x] Migration: build a config with a meaningful legacy block, call `migrate_legacy_to("srv-portable")` → `pipelines["srv-portable"]` is `default_legacy(maxBytes)` with `enabled` carried, `legacy == None`, returns `true`; calling again returns `false` (idempotent); calling on a config that already has `pipelines["srv-portable"]` does not overwrite it and returns `false`.
  - [x] Round-trip map: deserialize `{ "<serverId>": { "enabled": true, "ordering": ["favorite"], "budget": { "maxBytes": 1000 } } }` → `pipelines["<serverId>"]` as expected; serialize re-emits the map shape.
  - [x] Default serialize: `AutoFillConfig::default()` serializes `autoFill` as `{ "enabled": false, "maxBytes": null }` (pin so `get_daemon_state` stays stable).
  - [x] Accessor parity: `enabled_for(Some(id))`, single-entry fallback `enabled_for(None)`, and legacy fallback all return the expected enabled/max_bytes; `set_for` upserts the pipeline; `legacy_enabled()`/`legacy_max_bytes()` match.
  - [x] DB: temp/in-memory `Database::new(...)` → `SELECT … FROM autofill_history LIMIT 0` succeeds; calling `init()` twice does not error (idempotent).
  - [x] Run `rtk cargo test -p hifimule-daemon`; zero regressions, all new tests pass.

## Dev Notes

### What this story is (and is not)

This is a **persistence/schema + migration** story bridging the pure engine (Story 12.1, `done`) and per-server sync-time expansion (Story 12.3, next). It does three things:
1. Turns the manifest's single `autoFill` block into a per-`serverId` map of `AutoFillPipeline`.
2. **Actively migrates** a legacy `{ enabled, maxBytes }` block onto the selected server's portable `serverId` on device-detect and persists it, so the new map shape becomes the real on-disk reality (the change is *visible*, not hidden behind a compat shim).
3. Creates the daemon DB `autofill_history` table as empty scaffolding for Epic 13.

It must **not** change how auto-fill *behaves* today: single slot, selected server, favorites→playCount→dateCreated. The behavior change (running a pipeline per server, multi-slot expansion) is Story 12.3; RPC/UI is 12.6/12.7. After this story, a single-server install still auto-fills exactly as before — but its config is stored as `pipelines[selectedServerId]` instead of the legacy block. [Source: sprint-change-proposal-2026-06-14-configurable-auto-fill.md:132-135; epics.md#Epic-12]

### The central design problem (read this first)

The manifest field is **`pub auto_fill: AutoFillPrefs`** (`device/mod.rs:101-102, 163-168`), the legacy `{ enabled, maxBytes }` block. The architecture wants `autoFill : Map<serverId, AutoFillPipeline>`. The trap: a `HashMap<String, AutoFillPipeline>` and the legacy object **collide in JSON** — both are JSON objects. A naive type change breaks deserialization of every existing manifest and breaks ~30 call sites/fixtures.

**Solution:** a dedicated `AutoFillConfig` type with **custom serde** that reads either shape (legacy object → `legacy: Some(prefs)`; serverId-keyed object-of-objects → `pipelines`), plus a **migration step** that, when a device is detected and a server is selected, converts the legacy block into `pipelines[selectedServerId]` and persists it. The discriminator is reliable because legacy values under `enabled`/`maxBytes` are scalars/null (a `HashMap<String, AutoFillPipeline>` deserialize fails on them and falls back), whereas per-server values are pipeline objects.

**Why migrate to the *selected* server (and not eagerly key it any other way)?** The legacy single-slot bound to `selectedServerId` at sync time, and the manifest never recorded which server it was. The only faithful target is the **currently selected** server's portable id. So migration resolves that at detect-time (`db.get_server_config()?.server_id`) — exactly "legacy read as the default pipeline" from the architecture, made concrete and visible. [Source: architecture.md lines 805-807, 840-841]

**Why a `meaningful`-only trigger?** New manifests created by the daemon use `AutoFillConfig::default()` (no legacy block), but on serialize the empty config emits `{ enabled:false, maxBytes:null }`, which on the *next* load would otherwise look like a legacy block. Treating that empty default as `legacy: None` on deserialize prevents littering every device with a disabled `pipelines[serverId]` entry. Migration fires only when `enabled || max_bytes.is_some()`.

### Behavior preservation — the accessor strategy (AC #4)

Once `auto_fill` is a map, "is auto-fill enabled?" needs a server id. Two classes of consumer:

| Consumer | Has server context? | How it reads |
|---|---|---|
| `main.rs` `run_auto_sync` (`:548-591`), `run_auto_sync_via_provider` (`:1095-1153`) | **No** — take a `JellyfinClient`/`MediaProvider`, not a db handle | `legacy_enabled()` / `legacy_max_bytes()` → `enabled_for(None)`: single-entry fallback (single-server install has exactly one pipeline) → legacy block |
| `rpc.rs` `get_daemon_state` (`:1855-1860`), `setAutoFill` (`:5805-5813`) | **Yes** — `state.db` | resolve `db.get_server_config()?.server_id`, read `enabled_for(Some(id))`/`max_bytes_for(Some(id))`, write `set_for(id, …)` |

For the overwhelmingly common single-server install, `enabled_for(None)` resolves the one migrated pipeline → identical behavior. `get_daemon_state` keeps emitting `{ enabled, maxBytes }` so the UI (`BasketSidebar.ts:284-285,301-302,689-690`) is untouched (UI is Story 12.6). **Multi-slot expansion (honoring more than the selected server's slot at sync time) is explicitly Story 12.3 — do not add it here.**

### Current code being changed (read before writing)

- **`DeviceManifest`** — `device/mod.rs:73-123`. Field `pub auto_fill: AutoFillPrefs` at `:101-102` (`#[serde(default)]`). Manifest derives `Serialize, Deserialize, Clone, Default`.
- **`AutoFillPrefs`** — `device/mod.rs:163-168`: `#[serde(rename_all = "camelCase")] { enabled: bool, max_bytes: Option<u64> }` → `{ "enabled", "maxBytes" }`. **Keep it.**
- **Migration host** — `DeviceManager::handle_device_detected` (`device/mod.rs:329-357`): already does post-load `reconcile_manifest_server_ids` + best-effort `write_manifest`. Add the auto-fill migration right after, in the same best-effort style. `self.db` is available; `write_manifest` (`:209-218`) and `DeviceProber::probe` (`:297-306`) are the persist/read paths.
- **Selected-server lookup** — `Database::get_server_config()` (`db.rs:630-643`) returns the `selected = 1` row; `ServerConfig.server_id: Option<String>` is the **portable** id (`db.rs:30-39`). This is the migration target and the `get_daemon_state`/`setAutoFill` resolution source.
- **`SyncedItem.server_id` / `BasketItem.server_id`** — `device/mod.rs:12-61`, both `Option<String>`, the **portable** id (Story 2.11/2.13). The `pipelines` key must be this same portable id. [Source: architecture.md lines 840-841, 909]
- **Serde-evolution patterns already in this file** (reuse the style): `#[serde(default)]` everywhere; `#[serde(default, rename = "playlistPath", alias = "playlist_path", skip_serializing_if = "Option::is_none")]`; and the `reconcile_manifest_server_ids` post-load + persist pattern (`:192-207, 343-357`) — your migration should feel native beside it.

### DB scaffolding (AC #5)

- Engine: **SQLite via `rusqlite ~0.38` (bundled)** — `Cargo.toml:18`, `hifimule-daemon/Cargo.toml:17`. `Database` wraps `Arc<Mutex<Connection>>` (`db.rs:104-117`); held in `AppState` as `pub db: Arc<crate::db::Database>`.
- Migration convention (no `schema_version` table): everything in `init()` is idempotent — `CREATE TABLE IF NOT EXISTS` and column-existence probes. See `db.rs:135-216`. The new table is a plain `CREATE TABLE IF NOT EXISTS` (brand new, no data migration). `server_id` = portable id (AC #6). No reads/writes in 12.2.

### Architecture compliance (non-negotiable)

- Manifest holds **config** (portable, per `device×server`); DB holds **runtime history** (machine-local). Never mix. [Source: architecture.md line 922]
- Reuse Story 12.1's `crate::auto_fill::AutoFillPipeline` (`auto_fill/pipeline.rs:52-73`, re-exported via `auto_fill/mod.rs:17-24`, already `#[serde(rename_all="camelCase", default)]`, `pub`). **Do not redefine** any pipeline/stage type. Use `AutoFillPipeline::default_legacy(max_bytes)` as the legacy→pipeline mapping (set `enabled` on the result). [Source: auto_fill/pipeline.rs]
- Do not put selection config in `domain/models.rs` (reserved for provider-neutral entities). The config type lives in `device/mod.rs` with the other manifest sub-types. [Source: 12-1 story Project Structure Notes]

### Previous story intelligence (Story 12.1)

- 12.1 delivered the pure engine + model only; intentionally **unreferenced by the binary** under a module-level `#![allow(dead_code)]`. Referencing `AutoFillPipeline` from the manifest in 12.2 is the first real use — some dead-code allowance becomes live; that's fine, don't strip the blanket allow (other internals stay unused until 12.3/12.4).
- 12.1's `AutoFillPipeline` serde shape (camelCase + empty-object default) is already test-pinned in `auto_fill/pipeline.rs`. Build manifest tests on it; don't re-test pipeline internals.
- Sandbox caveat: full `rtk cargo test -p hifimule-daemon` may not finish where mockito/local networking is blocked. Your new tests are pure serde + in-memory SQLite — run targeted if needed: `rtk cargo test -p hifimule-daemon device::tests` and `rtk cargo test -p hifimule-daemon db::`.

### Git intelligence

Recent commits (`8b4ff37 Review 12.1`, `3af9768 Dev 12.1`, `aefee3f Story 12.1`, `87c5990 Correct course auto-fill`) confirm Epic 12 just started — 12.1 committed and reviewed; this is the immediate follow-on. No competing in-flight changes to `device/mod.rs` or `db.rs`.

### Latest technical context

- **No new crate dependency** (AC #7). `serde`/`serde_json` (`~1.0`) and `rusqlite ~0.38` cover everything. Custom serde is hand-written (`serde::{Serializer, Deserializer}` or `serde_json::Value` peek) — no `serde_with`.
- Rust edition is 2024-era (let-chains in use — the migration snippet uses one). Target the existing workspace toolchain.

### Project Structure Notes

- Manifest + migration + accessors: `hifimule-daemon/src/device/mod.rs`. DB: `hifimule-daemon/src/db.rs`. Consumer edits: `main.rs`, `rpc.rs`. Fixture edits: `sync.rs`, `tests.rs`, `device/tests.rs`. Binary crate (no `lib.rs`); tests are `#[cfg(test)] mod tests`, run via `rtk cargo test -p hifimule-daemon`.
- No UI/TS changes (12.6 owns UI). `BasketSidebar.ts` reads the unchanged `autoFill: { enabled, maxBytes }` JSON — leave it.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Epic-12 (lines 3002-3028 — Story 12.2)]
- [Source: _bmad-output/planning-artifacts/architecture.md#Auto-Fill-Pipeline-Model (lines 788-826); #Enforcement (line 922)]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-06-14-configurable-auto-fill.md (Sections 2, 4.2, 5)]
- [Source: _bmad-output/implementation-artifacts/12-1-autofill-pipeline-domain-model-and-engine.md (AutoFillPipeline, default_legacy, scope handoff)]
- [Source: hifimule-daemon/src/device/mod.rs:73-123 (DeviceManifest), :163-168 (AutoFillPrefs), :192-207 + :343-357 (reconcile+persist pattern), :209-218 (write), :297-306 (read), :329-357 (handle_device_detected)]
- [Source: hifimule-daemon/src/auto_fill/pipeline.rs:52-73 (AutoFillPipeline); auto_fill/mod.rs:17-24 (re-export)]
- [Source: hifimule-daemon/src/db.rs:104-216 (Database, init), :630-643 (get_server_config), :30-39 (ServerConfig.server_id portable)]
- [Source: hifimule-daemon/src/main.rs:548-591,1095-1153 (auto-sync consumers, no db context); rpc.rs:1855-1860,5805-5813 (consumers with state.db)]
- [Source: hifimule-ui/src/components/BasketSidebar.ts:284-285,301-302,689-690 (UI reads autoFill JSON — must stay stable)]

## Dev Agent Record

### Agent Model Used

Opus 4.8 (claude-opus-4-8[1m]) — BMad dev-story workflow.

### Debug Log References

- `rtk cargo check -p hifimule-daemon` — clean (only pre-existing `rename_item` dead-code warning in `api.rs`, untouched module).
- `rtk cargo clippy -p hifimule-daemon --all-targets` — no new warnings in touched modules (`device/mod.rs`, `db.rs`); all reported warnings pre-existing (vault, api, device_io, mtp, jellyfin, sync + the pre-existing redundant `use serde_json;` at `device/tests.rs:2`).
- `rtk cargo test -p hifimule-daemon` — 483 passed, 0 failed (full suite ran in sandbox; mockito-gated tests included).

### Completion Notes List

- **AutoFillConfig type (AC 1, 3):** Added `AutoFillConfig { pipelines: HashMap<String, AutoFillPipeline>, legacy: Option<AutoFillPrefs> }` in `device/mod.rs` with hand-written `Serialize`/`Deserialize`. Deserialize uses an internal `#[serde(untagged)]` enum (`PerServer` tried first, then `Legacy`); per-server values are pipeline objects while legacy `enabled`/`maxBytes` are scalars/null, so the map deserialize fails and falls through unambiguously. An empty default `{ enabled:false, maxBytes:null }` normalizes to `legacy: None` (no spurious migration). Serialize emits the map when non-empty, else the legacy block, else the empty default — pinned **byte-for-byte** equal to `AutoFillPrefs::default()` so `get_daemon_state` and new-device manifests are unchanged.
- **Migration (AC 2):** `migrate_legacy_to(server_id)` maps a parked legacy block onto the selected server's portable id (via `default_legacy(max_bytes)` with `enabled` carried over), clears `legacy`, returns `true`. Idempotent; never overwrites an existing pipeline. Wired into `DeviceManager::handle_device_detected` immediately after the `reconcile_manifest_server_ids` block, resolving the selected portable id via `db.get_server_config()?.server_id`, with the same best-effort persist style (never blocks device load).
- **Accessors (AC 4):** `enabled_for`/`max_bytes_for` resolve the keyed pipeline → single-entry fallback → legacy block. `legacy_enabled()`/`legacy_max_bytes()` are the no-server-context wrappers; `set_for`/`set_legacy` are the write paths; `pipeline_for` reserved for 12.3 (`#[allow(dead_code)]`).
- **Consumers (AC 4):** `main.rs` auto-sync paths (no db context) use `legacy_enabled()`/`legacy_max_bytes()`. `rpc.rs` `get_daemon_state` and `setAutoFill` (both have `state.db`) resolve the selected portable id and use `enabled_for(Some(id))`/`max_bytes_for(Some(id))` / `set_for(id,…)` (falling back to `set_legacy` when no server selected). Emitted `{ enabled, maxBytes }` JSON shape unchanged — UI untouched (Story 12.6). Zero direct `auto_fill.enabled`/`.max_bytes` field accesses remain.
- **Fixtures (AC 8):** All ~37 `auto_fill: AutoFillPrefs::default()` struct-literal initializers across `sync.rs`, `tests.rs`, `device/tests.rs`, `rpc.rs`, and `device/mod.rs:815` updated to `AutoFillConfig::default()`. `AutoFillPrefs` retained (legacy on-disk shape + migration carrier + serde fallback).
- **DB scaffolding (AC 5, 6):** `CREATE TABLE IF NOT EXISTS autofill_history (device_id, server_id, track_id, last_synced_at, tier, PRIMARY KEY(device_id, server_id, track_id))` added to `Database::init()`. `server_id` documented as the portable id matching manifest keys; no reads/writes in 12.2 (Epic 13 consumes). Storage split honored — config only in manifest, history only in DB.
- **Scope (AC 7):** No sync-time expansion, no `sync.start` change, no new RPC params, no UI, no provider fetch, no `autofill_history` reads/writes, no new crate dependency. Behavior frozen to today's single-slot selected-server algorithm.

### File List

- `hifimule-daemon/src/device/mod.rs` — `AutoFillConfig` type + custom serde + `migrate_legacy_to`/accessors/`pipeline_from_legacy`; manifest field `auto_fill: AutoFillConfig`; migration wired into `handle_device_detected`; `initialize_device` fixture updated.
- `hifimule-daemon/src/db.rs` — `autofill_history` table in `init()`; `test_autofill_history_table_exists_and_init_idempotent`.
- `hifimule-daemon/src/main.rs` — auto-sync consumers use `legacy_enabled()`/`legacy_max_bytes()`.
- `hifimule-daemon/src/rpc.rs` — `get_daemon_state` + `setAutoFill` resolve selected portable id; server-aware read/write; fixture initializers updated.
- `hifimule-daemon/src/sync.rs` — fixture initializer updated.
- `hifimule-daemon/src/tests.rs` — fixture initializers updated.
- `hifimule-daemon/src/device/tests.rs` — fixture initializers updated; new AutoFillConfig serde/migration/accessor + manifest round-trip tests.

## Change Log

- 2026-06-14 — Story 12.2 created via create-story workflow (ready-for-dev). Scope: manifest `auto_fill` → `AutoFillConfig` (per-server `Map<serverId, AutoFillPipeline>`), **active migration of the legacy `{ enabled, maxBytes }` block onto the selected server's portable serverId on device-detect (persisted)**, behavior-preserving server-aware accessors, and `autofill_history` DB table scaffolding (schema only). Behavior frozen (single-slot, selected server); multi-slot expansion deferred to 12.3; no UI/RPC-contract/new deps.
- 2026-06-14 — Dev complete (status → review). Implemented `AutoFillConfig` with custom dual-shape serde, legacy→selected-server migration wired into `handle_device_detected`, server-aware accessors, consumer + ~37 fixture updates, and the `autofill_history` table. All 8 ACs satisfied; `rtk cargo test -p hifimule-daemon` = 483 passed; no new clippy warnings in touched modules.
