# Story 8.4: Runtime Server-Type Detection Factory

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a System Admin (Alexis),
I want the daemon to auto-detect the server type when I enter a URL,
so that I do not need to manually specify "Jellyfin" or "Navidrome" during setup.

## Acceptance Criteria

1. Given a user enters a server URL in setup, when `server.connect` is called with `serverType: "auto"`, then the daemon detects server type in this order: Subsonic `GET /rest/ping.view`, Subsonic/OpenSubsonic success with `openSubsonic: true`, Subsonic classic success without the flag, then Jellyfin `GET /System/Info`; if all fail, it returns `Unknown server type at this URL`.
2. Given Subsonic detection succeeds, when the provider is stored, then the active provider is a `SubsonicProvider` held as `Arc<dyn MediaProvider>` and its `server_type()` is `ServerType::OpenSubsonic` when `openSubsonic` is true, otherwise `ServerType::Subsonic`.
3. Given Jellyfin detection succeeds, when the provider is stored, then the active provider is a `JellyfinProvider` held as `Arc<dyn MediaProvider>` and existing Jellyfin behavior continues to work.
4. Given `server.connect` is called with `serverType: "jellyfin"` or `"subsonic"`, then auto-detection is skipped and only the specified provider path is attempted.
5. Given a connection succeeds, when daemon state is queried, then `get_daemon_state` includes `serverType: "jellyfin" | "subsonic" | "openSubsonic" | null` and `serverVersion: string | null`.
6. Given a connection succeeds, when the daemon restarts, then server URL, detected server type, username, and server version are restored from persistent config and credentials are read only from keyring.
7. Given a connection attempt fails, when errors are logged, returned, or tested, then raw Jellyfin tokens, Subsonic passwords, and Subsonic `u`, `p`, `t`, `s` query values are not exposed.
8. Given the daemon crate is tested, when `rtk cargo test -p hifimule-daemon` runs, then factory, persistence, RPC, and existing provider tests pass.

## Tasks / Subtasks

- [x] Add shared provider factory types and connection entry point (AC: 1, 2, 3, 4, 7)
  - [x] Add `ServerTypeHint::{Auto, Jellyfin, Subsonic}` in `hifimule-daemon/src/providers/mod.rs`; do not add parallel enums elsewhere.
  - [x] Add `pub async fn connect(url: &str, creds: &ProviderCredentials, hint: ServerTypeHint) -> Result<Arc<dyn MediaProvider>, ProviderError>`.
  - [x] For `Auto`, attempt Subsonic first by constructing `SubsonicProvider::connect(...)`; classify `ServerType::OpenSubsonic` from the provider's cached ping result and `ServerType::Subsonic` otherwise.
  - [x] If Subsonic fails in `Auto`, attempt Jellyfin detection via `/System/Info`, then construct `JellyfinProvider` using the existing `JellyfinClient` and authenticated Jellyfin token.
  - [x] For explicit hints, skip unrelated probes entirely: `Jellyfin` must not ping Subsonic; `Subsonic` must not call Jellyfin.
  - [x] Map "all detection paths failed" to a focused provider/RPC error message: `Unknown server type at this URL`.

- [x] Update server connection RPC and credential flow (AC: 1, 4, 5, 7)
  - [x] Add `server.connect` to `hifimule-daemon/src/rpc.rs` with params `{ url, serverType, username, password }`.
  - [x] Keep the existing `login` RPC compatible by delegating to `server.connect` with `serverType: "auto"` or by retaining it as a Jellyfin-compatible wrapper; do not silently remove current UI callers.
  - [x] For Jellyfin, authenticate with `JellyfinClient::authenticate_by_name`, store the access token in keyring, and construct `JellyfinProvider` from the returned token and user ID.
  - [x] For Subsonic/OpenSubsonic, store the password in keyring, construct `SubsonicProvider` with `CredentialKind::Password`, and never store the raw password in SQLite or JSON config.
  - [x] Return `{ ok: true, serverType, serverVersion }` on success and JSON-RPC errors with existing error codes on failure.

- [x] Add active provider lifecycle to daemon state (AC: 2, 3, 5)
  - [x] Add `provider: Arc<tokio::sync::RwLock<Option<Arc<dyn MediaProvider>>>>` to `AppState`.
  - [x] Add a shared `require_provider(&AppState) -> Result<Arc<dyn MediaProvider>, JsonRpcError>` helper that clones the provider under a read lock and releases the lock before async provider calls.
  - [x] On successful `server.connect`, acquire the write lock and replace the active provider; the old provider should be dropped naturally when outstanding `Arc` clones finish.
  - [x] Add lightweight state fields for active `server_type` and `server_version`, or derive them from persisted config plus active provider where practical.
  - [x] Invalidate `last_connection_check` when the provider is replaced so `serverConnected` does not report stale Jellyfin-only status.

- [x] Persist server configuration and restore on startup (AC: 5, 6)
  - [x] Add `server_config` table and migrations in `hifimule-daemon/src/db.rs`:
    `id INTEGER PRIMARY KEY CHECK (id = 1)`, `url TEXT NOT NULL`, `server_type TEXT NOT NULL`, `username TEXT NOT NULL`, `server_version TEXT`, `updated_at INTEGER NOT NULL`.
  - [x] Add typed helpers such as `upsert_server_config`, `get_server_config`, and tests using `Database::memory()`.
  - [x] Replace or extend the current `CredentialManager` config JSON behavior so URL/user metadata does not remain Jellyfin-only; preserve backward compatibility with existing `config.json` where possible.
  - [x] On daemon startup, if `server_config` exists, fetch credentials from keyring and restore `AppState.provider` before serving RPC requests, or clearly mark the daemon disconnected if credentials are unavailable.
  - [x] Do not persist credentials in SQLite; only URL/type/username/version belong in database config.

- [x] Surface provider state through daemon state and image routing (AC: 3, 5)
  - [x] Extend `get_daemon_state` to include `serverType` and `serverVersion` while preserving all existing device fields.
  - [x] Keep existing `/jellyfin/image/{id}` route working for current UI code.
  - [x] If adding provider-aware image proxy support in this story, route through `provider.cover_art_url()` and sanitize Subsonic URLs before logs; otherwise document that full UI browse/image migration remains out of scope.

- [x] Add focused tests and verification (AC: 1-8)
  - [x] Factory tests: auto detects OpenSubsonic ping, classic Subsonic ping, Jellyfin fallback, explicit Jellyfin skips Subsonic, explicit Subsonic skips Jellyfin, all-fail returns unknown-type error.
  - [x] RPC tests: `server.connect` accepts all three `serverType` values, returns normalized `serverType`, replaces the active provider, and rejects invalid params.
  - [x] Persistence tests: `server_config` round-trips URL/type/username/version and migration is idempotent.
  - [x] Credential tests: Subsonic password and Jellyfin token are redacted in debug/error paths and are not written to database config.
  - [x] State tests: `get_daemon_state` includes `serverType`/`serverVersion` before and after provider replacement.
  - [x] Run `rtk cargo test -p hifimule-daemon`.

## Dev Notes

### Current Codebase State

- Provider trait and shared types live in `hifimule-daemon/src/providers/mod.rs`; `ServerType` already includes `Jellyfin`, `Subsonic`, `OpenSubsonic`, and `Unknown`. Reuse those variants. [Source: hifimule-daemon/src/providers/mod.rs:49]
- `SubsonicProvider::connect(ProviderCredentials)` already pings once and caches `open_subsonic`; use it for both explicit Subsonic connection and auto-detection. [Source: hifimule-daemon/src/providers/subsonic.rs:36]
- `JellyfinProvider::new(JellyfinClient, server_url, token, user_id)` exists, but it does not authenticate by itself; `server.connect` must authenticate first or use an already stored token on restore. [Source: hifimule-daemon/src/providers/jellyfin.rs:28]
- `AppState` currently stores `jellyfin_client: JellyfinClient` and has no active provider lock. Story 8.4 is the first story that should add app-wide `Arc<dyn MediaProvider>` lifecycle. [Source: hifimule-daemon/src/rpc.rs:55]
- Existing RPC methods are Jellyfin-first: `login`, `test_connection`, `jellyfin_get_views`, `jellyfin_get_items`, image proxy, sync, auto-fill, and scrobble still use `JellyfinClient`/`CredentialManager`. Keep compatibility where full migration is too broad for this story. [Source: hifimule-daemon/src/rpc.rs:129; hifimule-daemon/src/main.rs]
- There is no `server_config` table today. `Database::init()` currently creates only `devices` and `scrobble_history`, then applies column migrations. Add the new table in that same migration style. [Source: hifimule-daemon/src/db.rs]
- `CredentialManager` currently writes URL/user ID to `config.json` and stores one keyring entry named `jellyfin-token`. This is Jellyfin-specific and must be generalized or wrapped carefully for Subsonic password storage. [Source: hifimule-daemon/src/api.rs]

### Architecture Compliance

- All new server-type detection and provider instantiation belongs inside `providers/` and RPC orchestration. Do not let UI, sync, scrobble, or device code manually probe Subsonic/Jellyfin endpoints. [Source: _bmad-output/planning-artifacts/architecture.md#Provider-Factory]
- `AppState.provider` should be an `Arc<RwLock<Option<Arc<dyn MediaProvider>>>>`. RPC handlers should clone the provider and release the lock before awaiting provider work. [Source: _bmad-output/planning-artifacts/architecture.md#Provider-Lifecycle]
- Persist server URL, detected type, username, and server version in SQLite; credentials stay in OS keyring only. [Source: _bmad-output/planning-artifacts/architecture.md#Server-Config-Persistence]
- JSON-RPC external payloads use camelCase. `server.connect` params are `{ url, serverType, username, password }`; daemon state fields are `serverType` and `serverVersion`. [Source: _bmad-output/planning-artifacts/architecture.md#Server-Connect-IPC]
- Provider identity should normalize to string values for the UI: `jellyfin`, `subsonic`, `openSubsonic`, or `null`. Keep Rust enum serialization internal if it would emit capitalized variants by default.

### Story Boundaries

- In scope: provider factory, runtime detection, `server.connect`, active provider storage, server config persistence, `get_daemon_state` server fields, startup restore if feasible without broad UI churn.
- In scope only as needed for compatibility: keep `login`, `save_credentials`, `get_credentials`, and current Jellyfin RPCs working.
- Out of scope: full browse RPC migration to provider-neutral `browse.*`, full sync/auto-fill/scrobble migration off `JellyfinClient`, visible UI changes beyond contract support, Story 8.5's comprehensive URL sanitization sweep, and Story 8.6's Subsonic album-level incremental fallback.
- Do not add the `opensubsonic` crate back. Story 8.3 intentionally removed the unused dependency and implemented a focused local Subsonic client.

### Previous Story Intelligence

- Story 8.2 deferred provider lifecycle and noted that direct Jellyfin streaming still uses `JellyfinClient`; do not break that path while adding the active provider. [Source: _bmad-output/implementation-artifacts/8-2-jellyfinprovider-adapter.md#Review-Findings]
- Story 8.2 review also deferred the unauthenticated direct Jellyfin `download_url(None)` concern because sync still streams through `JellyfinClient`; do not claim sync is provider-neutral unless the call sites are actually migrated. [Source: _bmad-output/implementation-artifacts/8-2-jellyfinprovider-adapter.md#Review-Findings]
- Story 8.3 established that `SubsonicProvider` is constructed from `CredentialKind::Password`, rejects token credentials, and keeps raw password inside `providers/subsonic.rs`. Preserve this boundary. [Source: _bmad-output/implementation-artifacts/8-3-subsonicprovider-adapter.md#Current-Codebase-State]
- Story 8.3 review deferred full sanitization of `t=` and `s=` in all error/log paths to Story 8.5, but this story must not introduce new leaks in factory/RPC/persistence code. [Source: _bmad-output/implementation-artifacts/8-3-subsonicprovider-adapter.md#Review-Findings]

### Latest Technical Context

- OpenSubsonic `ping` returns a `subsonic-response` envelope; successful OpenSubsonic-capable servers may include `openSubsonic: true`, `type`, and `serverVersion`, while classic Subsonic success can omit the OpenSubsonic flag. [Source: https://opensubsonic.netlify.app/docs/endpoints/ping/]
- The Subsonic/OpenSubsonic recommended token auth is `u`, `t=md5(password + salt)`, `s`, `v`, `c`, and `f=json`; `p` plaintext auth exists but should not be used for HifiMule runtime requests. [Source: https://opensubsonic.netlify.app/docs/api-reference/]
- Jellyfin's system API exposes `getSystemInfo` and public system info operations; for HifiMule detection, `/System/Info` is the architecture-approved Jellyfin fallback probe. [Source: https://kotlin-sdk.jellyfin.org/dokka/jellyfin-api/org.jellyfin.sdk.api.operations/-system-api/index.html]

### Testing Guidance

- Keep provider factory tests inside `providers/mod.rs` or a provider-factory test module so HTTP mock setup is close to the detection logic.
- Use the existing `mockito` dev dependency; do not add `wiremock` unless a specific test cannot be expressed with `mockito`.
- Assert probe order with mock expectations: in `Auto`, Subsonic ping must be attempted before Jellyfin fallback; in explicit hints, unrelated probes should have zero hits.
- Test errors by class, not exact full strings, except for the required user-facing unknown-type message.
- Use test-only config/keyring indirection if modifying `CredentialManager`; avoid tests that write to the developer's real keyring.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story-8.4-Runtime-Server-Type-Detection-Factory]
- [Source: _bmad-output/planning-artifacts/architecture.md#Provider-Factory]
- [Source: _bmad-output/planning-artifacts/architecture.md#Provider-Lifecycle]
- [Source: _bmad-output/planning-artifacts/architecture.md#Server-Config-Persistence]
- [Source: _bmad-output/planning-artifacts/architecture.md#Server-Connect-IPC]
- [Source: _bmad-output/planning-artifacts/prd.md#Server-Profile-Management]
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#Server-Type-Badge-Login-Screen]
- [Source: _bmad-output/implementation-artifacts/8-2-jellyfinprovider-adapter.md]
- [Source: _bmad-output/implementation-artifacts/8-3-subsonicprovider-adapter.md]
- [Source: hifimule-daemon/src/providers/mod.rs]
- [Source: hifimule-daemon/src/providers/jellyfin.rs]
- [Source: hifimule-daemon/src/providers/subsonic.rs]
- [Source: hifimule-daemon/src/rpc.rs]
- [Source: hifimule-daemon/src/db.rs]
- [Source: hifimule-daemon/src/api.rs]

### Review Findings

- [x] [Review][Patch] Double Jellyfin auth in `handle_server_connect` — `authenticate_by_name` is called twice: once inside `connect_jellyfin` (factory) and again at `rpc.rs:347` to get the token for keyring storage. The provider holds the first auth's token; the keyring stores the second. Fix: return the auth result (token + user_id) from `connect_jellyfin`, or add a `connect_jellyfin_with_token` variant so the RPC layer authenticates once and passes the token in. Also store the username (not the UUID `auth.user.id`) in `persisted_username` for display purposes. [`hifimule-daemon/src/rpc.rs:343`]
- [x] [Review][Patch] `sanitize_secret_message` in `providers/mod.rs` lacks `preceded_by_separator` guard — single-char keys (`u`, `p`, `t`, `s`) will match mid-word (e.g. `"status="` matches `s=`), causing over-redaction of diagnostic messages and an **infinite loop** when the replacement `[redacted]` is re-found in the next `while` iteration. Copy the `preceded_by_separator` pattern from `subsonic.rs::sanitize_message`. [`hifimule-daemon/src/providers/mod.rs:232`]
- [x] [Review][Patch] Keyring key collision breaks restore for users previously logged in via `login` RPC — `login` writes the token to the legacy `"jellyfin-token"` keyring key; `restore_provider_from_config` reads from `"jellyfin-token-jellyfin"`. A user who logged in before this story will get a keyring miss on restart and start in a disconnected state even though a valid token exists. Fix: in `restore_provider_from_config`, fall back to `get_credentials()` legacy key if `get_server_secret("jellyfin")` fails, or update `handle_login` to also call `save_server_secret("jellyfin", &token)`. [`hifimule-daemon/src/rpc.rs:133`, `hifimule-daemon/src/api.rs:944`]
- [x] [Review][Patch] `restore_provider_from_config` re-pings Subsonic on every daemon restart — AC 6 requires restoring from persistent config; the current implementation calls `providers::connect(…, Subsonic)` which performs a live `GET /rest/ping.view`. If the server is offline at startup, the provider is silently not restored (violating AC 6). Fix: add a `SubsonicProvider::from_stored_config(url, username, password, open_subsonic: bool, server_version: Option<String>)` constructor (mirroring the Jellyfin restore path) and use it instead of re-connecting. [`hifimule-daemon/src/rpc.rs:152`]
- [x] [Review][Patch] No RPC-level tests for `handle_server_connect` — AC 8 and story spec explicitly require "RPC tests: `server.connect` accepts all three `serverType` values, returns normalized `serverType`, replaces the active provider, and rejects invalid params." `test_parse_server_type_hint_accepts_supported_values` only tests the parsing helper; no test calls `handle_server_connect` end-to-end with a mock server. [`hifimule-daemon/src/rpc.rs`]
- [x] [Review][Patch] No credential-redaction test for `handle_server_connect` / factory error paths — AC 7 and Required Tests: "Credential tests: Subsonic password and Jellyfin token are redacted in debug/error paths." Only `subsonic.rs` has internal redaction tests. No test asserts that `JsonRpcError.message` from a failed `server.connect` (Jellyfin or Subsonic path) does not leak raw passwords or tokens. [`hifimule-daemon/src/providers/mod.rs`, `hifimule-daemon/src/rpc.rs`]
- [x] [Review][Patch] No persistence idempotency test for `db.init()` — AC 8 / Required Tests: "Persistence tests: migration is idempotent." Call `db.init()` a second time on an already-initialized in-memory DB and assert no error (all `CREATE TABLE IF NOT EXISTS` statements should be safe). [`hifimule-daemon/src/db.rs`]

- [x] [Review][Defer] Auto mode discards Subsonic and Jellyfin error details — on all-fail, caller gets only "Unknown server type at this URL" with no indication of whether errors were network, auth, or protocol failures. Pre-existing design choice per AC 1; consider improving diagnostics in Story 8.6+. [`hifimule-daemon/src/providers/mod.rs:163`] — deferred, pre-existing
- [x] [Review][Defer] `check_server_connection_cached` falls back to Jellyfin-only credentials check when `state.provider` is None — ignores Subsonic servers entirely for connectivity status when no provider is loaded. Pre-existing behavior not introduced by this story. [`hifimule-daemon/src/rpc.rs`] — deferred, pre-existing
- [x] [Review][Defer] Three separate `RwLock`s for `provider`/`server_type`/`server_version` allow inconsistent intermediate reads — a reader can observe `server_type = "jellyfin"` while `provider` is `None`. Pre-existing architectural pattern; fix requires a composite lock. [`hifimule-daemon/src/rpc.rs:60`] — deferred, pre-existing
- [x] [Review][Defer] `restore_provider_from_config` populates `state.server_type` from the raw DB string instead of deriving it from the restored provider's `server_type()` — harmless today since DB strings are written by `server_type_slug`, but could drift if DB was written by an older version. [`hifimule-daemon/src/rpc.rs:174`] — deferred, pre-existing

## Dev Agent Record

### Agent Model Used

GPT-5 Codex

### Debug Log References

- `rtk cargo test -p hifimule-daemon providers:: --no-fail-fast` - 46 provider tests passed.
- `rtk cargo test -p hifimule-daemon --no-fail-fast` - 257 tests passed.
- `rtk cargo test -p hifimule-daemon` - 257 tests passed.

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- Implemented provider factory runtime detection with Subsonic/OpenSubsonic-first auto detection, Jellyfin fallback, explicit hint skipping, normalized server type slugs, and focused unknown-type errors.
- Added active provider lifecycle to RPC state, `server.connect`, provider-aware daemon state fields, startup restore from persisted server config, and cache invalidation on provider replacement.
- Added `server_config` SQLite persistence, generalized keyring secret helpers, Subsonic/Jellyfin version reporting, and tightened Subsonic query-secret redaction for `u`, `p`, `t`, and `s`.
- Preserved existing Jellyfin login, credentials, image proxy, sync, and browse compatibility while adding the provider-neutral connection path.

### File List

- `_bmad-output/implementation-artifacts/8-4-runtime-server-type-detection-factory.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `hifimule-daemon/src/api.rs`
- `hifimule-daemon/src/db.rs`
- `hifimule-daemon/src/paths.rs`
- `hifimule-daemon/src/providers/jellyfin.rs`
- `hifimule-daemon/src/providers/mod.rs`
- `hifimule-daemon/src/providers/subsonic.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-daemon/src/tests.rs`

### Change Log

- 2026-05-09: Implemented runtime server-type detection factory, `server.connect`, active provider state, server config persistence, startup restore, daemon state fields, redaction hardening, and focused tests.
