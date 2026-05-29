---
title: 'Auto-sync for Subsonic/Navidrome providers'
type: 'feature'
created: '2026-05-29'
status: 'done'
baseline_commit: '55e50f1'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** `auto_sync_on_connect` only works for Jellyfin. For Subsonic/Navidrome users, the device-connect handler calls `CredentialManager::get_credentials()` which returns stale Jellyfin credentials (or fails entirely), causing auto-sync to skip silently. Basket resolution and auto-fill in `run_auto_sync`/`auto_fill.rs` also call Jellyfin-specific APIs exclusively. Additionally, the interactive UI sync for Subsonic with auto-fill enabled is hard-blocked with "not available yet" in `provider_calculate_delta`.

**Approach:** Add a parallel provider-based auto-sync path for Subsonic/Navidrome: detect the configured server type via the DB at device-connect time, create a `MediaProvider`, resolve basket items using the provider trait (`get_album`, `get_artist`, `get_playlist`, `get_song`, `list_favorite_items`), implement a provider-neutral auto-fill in `auto_fill.rs` (`run_auto_fill_provider`), and wire it into both the device-connect auto-sync and `provider_calculate_delta`. The Jellyfin auto-sync path remains unchanged.

## Boundaries & Constraints

**Always:**
- Jellyfin auto-sync path (`run_auto_sync` / `execute_sync`) is not touched — zero regression risk.
- Provider-based auto-sync uses `execute_provider_sync`, not `execute_sync`.
- `run_auto_fill_provider` uses only song-level list methods: `list_favorites`, `list_frequently_played`, `list_recently_played`. Skip `list_recently_added` (returns albums, requires expansion — defer).
- Song size is estimated as `(bitrate_kbps * 1_000 / 8) * duration_seconds`. Skip songs with estimated size = 0 (unknown bitrate), same as existing `provider_track_size` logic.
- The existing `DESTRUCTIVE_CLEANUP_THRESHOLD` guard applies to the provider path too.
- Provider connection at auto-sync time: read from `db.get_server_config()` + `CredentialManager::get_server_secret(server_type)`. If either fails, log and skip auto-sync.

**Ask First:**
- If auto-fill returns zero items (all songs have unknown bitrate on a server), ask whether to skip silently or error — do not make that call unilaterally.

**Never:**
- Do not re-authenticate Jellyfin from `main.rs` at device-connect time (no network password auth in the hot path).
- Do not modify `execute_sync` or `execute_provider_sync` signatures.
- Do not change how `BasketItem` is stored or what IDs Subsonic items use.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Subsonic, manual basket | `auto_sync=true`, basket=[album1, artist2], Subsonic configured | Provider resolves album→tracks, artist→albums→tracks; delta runs; `execute_provider_sync` transfers files | Error state + OS notification |
| Subsonic, auto-fill enabled | `auto_sync=true`, basket=[], `auto_fill.enabled=true`, Subsonic configured | `run_auto_fill_provider` fetches favorites+frequently_played; truncates to capacity; sync executes | If zero items resolved, idle state + log |
| Classic Subsonic auto-fill | `auto_sync=true`, `auto_fill=true`, classic Subsonic (no OpenSubsonic) | `list_frequently_played` returns `UnsupportedCapability`, gracefully skipped; favorites only used | Falls back to favorites-only |
| Subsonic credentials missing | Subsonic in DB but password not in keyring | Auto-sync skipped, daemon log "no credentials" | — |
| Subsonic, FavoriteAlbum basket | `item_type="FavoriteAlbum"`, id="favorites:album:abc" | `list_favorite_items()` → filter songs by album_id="abc" | — |
| Subsonic, FavoriteArtist basket | `item_type="FavoriteArtist"`, id="favorites:artist:xyz" | `list_favorite_items()` → filter albums by artist_id="xyz" → expand each via `get_album` | — |
| UI sync, Subsonic, auto-fill | `provider_calculate_delta` called with `autoFill.enabled=true`, Subsonic provider active | `run_auto_fill_provider` called with `max_fill_bytes` from params or device storage; delta returned | Returns JsonRpcError if provider connection fails |
| Already in sync | Delta has 0 adds + 0 deletes + 0 id_changes | Skip sync, idle state, no notification | — |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/main.rs:530` -- `run_auto_sync`: Jellyfin-only auto-sync; add `run_auto_sync_via_provider` alongside it
- `hifimule-daemon/src/main.rs:214-302` -- device-connect event handler: add db capture + Subsonic provider detection + routing
- `hifimule-daemon/src/auto_fill.rs:47` -- `run_auto_fill`: Jellyfin-only auto-fill; add `run_auto_fill_provider` in same file
- `hifimule-daemon/src/rpc.rs:1741-1752` -- `provider_calculate_delta`: hard-blocks Subsonic auto-fill; wire `run_auto_fill_provider` here
- `hifimule-daemon/src/rpc.rs:1566` -- `provider_favorite_sync_items_for_basket_item`: reference implementation for FavoriteArtist/FavoriteAlbum resolution; replicate in `main.rs`
- `hifimule-daemon/src/rpc.rs:1662` -- `provider_sync_items_for_id`: reference for normal basket item resolution; replicate in `main.rs`
- `hifimule-daemon/src/rpc.rs:1493` -- `provider_track_size` + `provider_song_to_desired_item`: reference for Song→DesiredItem mapping; replicate in `main.rs`
- `hifimule-daemon/src/sync.rs:1871` -- `execute_provider_sync`: provider-agnostic sync executor; used by the new path
- `hifimule-daemon/src/providers/mod.rs:379` -- `connect()`: factory to create provider from credentials; used at device-connect time
- `hifimule-daemon/src/db.rs` -- `get_server_config()`: returns `ServerConfig { url, username, server_type, server_version }`
- `hifimule-daemon/src/api.rs:1528` -- `CredentialManager::get_server_secret(server_type)`: retrieves stored Subsonic password

## Tasks & Acceptance

**Execution:**

- [x] `hifimule-daemon/src/auto_fill.rs` -- Add `pub async fn run_auto_fill_provider(provider: Arc<dyn crate::providers::MediaProvider>, params: AutoFillParams) -> Result<Vec<AutoFillItem>>`: fetch `list_favorites(None, 0, 2000)`, then `list_frequently_played(None, 0, 2000)` (skip if `UnsupportedCapability`), then `list_recently_played(None, 0, 2000)` (skip if `UnsupportedCapability`); dedup by song ID; map to `AutoFillItem` using `(bitrate_kbps * 1_000 / 8) * duration_seconds` for size; skip zero-size songs; truncate to `max_fill_bytes` in priority order; apply `exclude_item_ids` filter same as existing function -- enables provider-neutral auto-fill for Subsonic/Navidrome

- [x] `hifimule-daemon/src/main.rs` -- Capture `let db_auto = Arc::clone(&db)` in the device-connect event spawn closure alongside existing `jellyfin_client` capture -- gives auto-sync access to server config

- [x] `hifimule-daemon/src/main.rs` -- Add private async fn `get_non_jellyfin_provider(db: &Arc<crate::db::Database>) -> Option<Arc<dyn crate::providers::MediaProvider>>`: call `db.get_server_config()`, return `None` if not subsonic/openSubsonic; try `CredentialManager::get_server_secret(server_type)`, return `None` if missing; call `crate::providers::connect(&url, &credentials, ServerTypeHint::Subsonic).await`, return `None` on failure with log -- isolates Subsonic provider acquisition for auto-sync

- [x] `hifimule-daemon/src/main.rs:271-302` -- In device-connect event handler, before the existing Jellyfin `if let Ok((url, token, user_id))` auto-sync block: call `get_non_jellyfin_provider(&db_auto).await`; if `Some(provider)`, spawn a new task calling `run_auto_sync_via_provider(provider, dm, som, state_tx, path)`; use `else if let Ok(...)` to fall through to the existing Jellyfin path -- routes Subsonic/Navidrome to provider path without touching Jellyfin path

- [x] `hifimule-daemon/src/main.rs` -- Add private async fn `run_auto_sync_via_provider(provider, device_manager, sync_op_manager, state_tx, device_path)`: mirror `run_auto_sync` structure — read manifest, resolve basket items via provider helpers (`resolve_provider_basket_item` private fn), call `run_auto_fill_provider` if basket empty + auto-fill enabled, compute delta, check destructive threshold, call `execute_provider_sync`, send notifications and state updates -- provider-based equivalent of `run_auto_sync`

- [x] `hifimule-daemon/src/main.rs` -- Add private fns `resolve_provider_basket_items`, `resolve_provider_favorite_item`, `provider_song_to_desired` (local copies of `rpc.rs` counterparts returning `anyhow::Result` instead of `JsonRpcError`): `resolve_provider_basket_items` iterates basket items; FavoriteArtist/FavoriteAlbum calls `resolve_provider_favorite_item`; others call `provider.get_album → get_playlist → get_artist → get_song` in sequence (skip if not found) -- basket resolution for provider auto-sync path

- [x] `hifimule-daemon/src/rpc.rs:1741-1752` -- Replace the `"Auto-fill sync is not available for Subsonic servers yet"` error block with: extract `max_fill_bytes` from `params["autoFill"]["maxBytes"]` or from `device_manager.get_device_storage().await`; build `AutoFillParams`; call `crate::auto_fill::run_auto_fill_provider(provider.clone(), fill_params).await`; map error to `JsonRpcError`; convert items to `DesiredItem` via local `provider_song_to_desired_item`; set `desired_items` and continue delta calculation -- enables interactive UI sync with auto-fill for Subsonic/Navidrome

**Acceptance Criteria:**
- Given a Subsonic device with `auto_sync_on_connect=true` and manual basket items, when the device is connected, then the daemon auto-syncs using the Subsonic provider (resolves basket items via `get_album`/`get_artist` etc.) without calling any Jellyfin API.
- Given a Subsonic/Navidrome device with `auto_sync_on_connect=true` and `auto_fill.enabled=true`, when the device is connected, then `run_auto_fill_provider` fetches favorites and (for OpenSubsonic) frequently-played tracks, truncates to device capacity, and syncs.
- Given a classic Subsonic server where `list_frequently_played` returns `UnsupportedCapability`, when auto-fill runs, then only favorites are used (no error, no crash).
- Given a Jellyfin device with `auto_sync_on_connect=true`, when the device is connected, then the existing `run_auto_sync` path is used (unchanged behavior).
- Given a Subsonic device with auto-fill enabled and the user initiates an interactive sync from the UI, when `sync.calculateDelta` is called, then `run_auto_fill_provider` is used and the response is a valid delta (no longer returns "not available" error).

## Design Notes

**Provider-based basket resolution** in `main.rs` mirrors `rpc.rs`'s `provider_sync_items_for_id` but returns `anyhow::Result`. Do not extract to a shared module — the two call sites have different error types and the logic is small enough to inline safely. If a third call site appears, extract then.

**Auto-fill priority order**: favorites first (is_favorite=true), then frequently_played (sorted by play_count desc, already sorted by SubsonicProvider), then recently_played (sorted by last_played_at desc). Stop filling as soon as capacity is reached. Songs appearing in multiple lists: keep the first (highest priority) occurrence via a `HashSet<String>` dedup.

**Size estimate for provider songs**: `(bitrate_kbps * 1_000 / 8) * duration_seconds`. Songs missing bitrate get size=0 and are skipped, consistent with `provider_track_size` in `rpc.rs`.

**Max page size for auto-fill lists**: fetch up to 2000 songs per list via `limit=2000, offset=0`. This avoids pagination complexity for auto-fill while covering realistic library sizes. Server enforces its own hard cap.

## Verification

**Commands:**
- `rtk cargo check` -- expected: zero errors
- `rtk cargo test --package hifimule-daemon` -- expected: all tests pass, no new failures
- `rtk cargo clippy --package hifimule-daemon -- -D warnings` -- expected: clean

**Manual checks:**
- With a Navidrome server configured and a device with `auto_sync_on_connect=true` + manual basket items: connect device → daemon log shows `[AutoSync] Starting auto-sync via provider` and proceeds without Jellyfin 401 errors.
- With auto-fill enabled + Navidrome: connect device → daemon log shows `[AutoSync] Auto-fill resolved N items via provider`.

## Spec Change Log

## Suggested Review Order

**Routing — device-connect event handler**

- Server-type check + provider/Jellyfin routing split; entry point for the whole change
  [`main.rs:282`](../../src/main.rs#L282)

**Provider acquisition**

- Creates Subsonic provider from DB config + keyring; returns None for Jellyfin or on failure
  [`main.rs:1050`](../../src/main.rs#L1050)

**Provider auto-sync orchestration**

- Top-level flow: manifest read → auto-fill or basket → delta → execute_provider_sync
  [`main.rs:1089`](../../src/main.rs#L1089)

**Provider-based auto-fill**

- Fetches favorites/frequently-played/recently-played; deduplicates; truncates to capacity
  [`auto_fill.rs:267`](../../src/auto_fill.rs#L267)

**Basket resolution — normal items**

- Tries get_album → get_playlist → get_artist → get_song in sequence
  [`main.rs:1446`](../../src/main.rs#L1446)

- FavoriteArtist/FavoriteAlbum resolution via list_favorite_items()
  [`main.rs:1396`](../../src/main.rs#L1396)

- Song → DesiredItem conversion using bitrate estimate for size
  [`main.rs:1497`](../../src/main.rs#L1497)

**Interactive sync auto-fill (RPC path)**

- Replaced "not available" stub with actual run_auto_fill_provider call
  [`rpc.rs:1741`](../../src/rpc.rs#L1741)
