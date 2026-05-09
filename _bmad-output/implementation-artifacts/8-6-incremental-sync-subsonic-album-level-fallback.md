# Story 8.6: Incremental Sync - Subsonic Album-Level Fallback

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a Ritualist (Arthur),
I want incremental sync to correctly detect new songs added to existing albums on Navidrome,
so that a new track added to an album I have synced is picked up on the next incremental sync.

## Acceptance Criteria

1. Given Navidrome/Subsonic `getIndexes?ifModifiedSince={epoch_ms}` returns no changed artist entries for an incremental token, when a new song was added to an album already represented in the device manifest/change context, then the provider change-detection path still emits a song-level `ChangeEvent` for the new song through an album re-fetch fallback.
2. Given an initial sync/full-library dump is requested, when `SubsonicProvider::changes_since(None)`, `changes_since(Some(""))`, or `changes_since(Some("0"))` runs, then it uses `search3?query=&songCount=500&songOffset={n}` pagination to enumerate all tracks and emits song `ChangeEvent`s without using the index-only path.
3. Given `search3` returns exactly a full page of 500 songs, when more songs exist, then the provider requests the next `songOffset`; when a page returns fewer than 500 songs, then pagination stops.
4. Given album fallback compares the supplied manifest-derived context with fresh `getAlbum` results, when song IDs differ, song count differs, or available Subsonic file metadata changes (`size`, `contentType`, `suffix`), then it emits appropriate `Created`, `Updated`, or `Deleted` song-level `ChangeEvent`s.
5. Given the existing `MediaProvider::changes_since(token)` signature has no manifest argument and `SyncedItem` currently lacks a first-class album ID, when this story needs album fallback input, then it introduces an explicit, minimal, provider-neutral change context and a backward-compatible manifest strategy rather than relying on hidden globals or artist/title string guesses alone.
6. Given Subsonic URLs include `u`, `p`, `t`, and `s` auth params, when this story adds requests, logs, or test failure messages, then it reuses the existing sanitizer behavior and does not leak raw username, password, token, or salt values.
7. Given Jellyfin provider behavior and the existing sync engine are still in use, when daemon tests run, then Subsonic fallback tests, existing Subsonic provider tests, Jellyfin provider tests, and sync delta tests pass.

## Tasks / Subtasks

- [x] Extend Subsonic client support for paged full-library song enumeration (AC: 2, 3)
  - [x] Replace or overload the local `SubsonicClient::search3(&self, query: &str)` wrapper so it can pass `songCount` and `songOffset` query params while preserving existing browse/search behavior.
  - [x] Add `SubsonicProvider` helper code that treats `None`, empty token, and `"0"` as initial/full dump and calls `search3` with `query=""`, `songCount=500`, `songOffset=0,500,...`.
  - [x] Emit `ChangeEvent { item: ItemRef { id: song.id, item_type: ItemType::Song }, change_type: ChangeType::Created, version: computed_version }` for every full-dump song.
  - [x] Preserve existing `search(query)` public behavior for UI search; do not make normal user search return every song.

- [x] Introduce a minimal change-detection context without broad sync migration (AC: 1, 5, 7)
  - [x] Add an explicit provider-neutral context type in `hifimule-daemon/src/providers/mod.rs` or an adjacent module, carrying only data needed for fallback: synced song ID, album ID when known, size, content type, suffix, and any existing version/etag.
  - [x] Update the `MediaProvider` trait and both provider implementations deliberately if the chosen API is `changes_since(token, context)`. Jellyfin may ignore the context, but tests must prove its existing `minDateLastSaved` behavior is unchanged.
  - [x] If a separate helper is chosen instead of changing the trait, keep it provider-owned and explicit; do not use process globals, `AppState` back references, or hidden reads of the current device from inside `SubsonicProvider`.
  - [x] Update only the RPC/sync caller path needed to pass the current manifest snapshot into change detection. Do not migrate streaming/download execution in this story.

- [x] Implement album-level fallback inside `providers/subsonic.rs` only (AC: 1, 4, 5)
  - [x] Keep Subsonic-specific fallback mechanics inside the Subsonic provider/client layer; outside callers should provide context and process returned `ChangeEvent`s, not call `getAlbum` directly.
  - [x] For non-initial numeric tokens, keep calling `getIndexes` with `ifModifiedSince` as epoch milliseconds first.
  - [x] When `getIndexes` returns changed artists, continue emitting conservative artist-level updates as today unless a stronger album mapping is available.
  - [x] When `getIndexes` returns no changed artists, re-fetch albums known from the supplied change context and compare their fresh track IDs and metadata to the context snapshot.
  - [x] Add a provider-local comparison helper that detects added, removed, and updated songs without depending on query parameter ordering or response ordering.

- [x] Add a backward-compatible source for album fallback inputs (AC: 1, 5)
  - [x] Audit `SyncedItem` in `hifimule-daemon/src/device/mod.rs`; it currently has `jellyfin_id`, `name`, `album`, `artist`, `local_path`, `size_bytes`, `synced_at`, `original_name`, and `etag`, but no album ID.
  - [x] Add manifest fields only with `#[serde(default)]` so old `.hifimule.json` files keep deserializing. Prefer explicit provider-neutral names for new fields, e.g. `provider_album_id`, `provider_content_type`, `provider_suffix`, or a compact metadata struct if it fits existing style.
  - [x] Update sync writes in `hifimule-daemon/src/sync.rs` or the provider-to-sync mapping path only if needed to persist those fields for future syncs; preserve all existing Jellyfin semantics and JSON compatibility.
  - [x] If current runtime paths cannot populate album IDs for pre-existing manifest rows, implement a focused fallback that can derive candidate album IDs through Subsonic `search3`/`getAlbum` data, and document/test the limitation for old manifests.
  - [x] Rename the serialized `.hifimule.json` synced-item ID to provider-neutral `providerItemId`.

- [x] Compute stable Subsonic change versions (AC: 4)
  - [x] Add a small helper that builds `ChangeEvent.version` from available Subsonic song fields: at minimum `id`, `size`, `contentType`, and `suffix`; include a deterministic separator/format.
  - [x] Treat missing metadata conservatively: if ID is new, emit `Created`; if an existing ID lacks metadata needed for version comparison, do not invent false updates.
  - [x] Preserve Subsonic unit rules from Story 8.3: duration is seconds, bitrate is kbps, IDs are `String`, and `coverArt` is distinct from item ID.

- [x] Protect security and boundaries (AC: 6, 7)
  - [x] Reuse `sanitize_subsonic_url()` / `sanitize_subsonic_message()` for any new logging or provider errors that can include Subsonic URLs.
  - [x] Do not add `opensubsonic`, `wiremock`, `insta`, or a new sync database layer for this story; the existing hand-rolled client and `mockito` tests are the current project pattern.
  - [x] Do not migrate `execute_sync()` to provider-neutral streaming unless the minimum manifest metadata path absolutely requires a small compatibility change.
  - [x] Do not change UI copy, Tauri routes, `server.connect`, or browse RPC contracts.

- [x] Add focused tests and verification (AC: 1-7)
  - [x] Add `providers::subsonic` tests for initial full dump: first page 500 songs, second page fewer than 500, all emitted as song `Created` changes.
  - [x] Add a test proving `changes_since(Some("0"))` uses `search3` and does not call `getIndexes`.
  - [x] Add a test where `getIndexes` returns no artists, existing manifest album data is re-fetched via `getAlbum`, and a new song ID emits `ChangeType::Created`.
  - [x] Add tests for removed song IDs and metadata-only updates using `size`, `contentType`, and `suffix`.
  - [x] Add manifest serialization/deserialization tests proving new fields default cleanly on old manifests and serialize in the existing camelCase style where applicable.
  - [x] Run `rtk cargo test -p hifimule-daemon subsonic --no-fail-fast`, `rtk cargo test -p hifimule-daemon sync --no-fail-fast`, and `rtk cargo test -p hifimule-daemon`.

## Dev Notes

### Current Codebase State

- `SubsonicProvider` and its local REST client live in `hifimule-daemon/src/providers/subsonic.rs`. The current `changes_since(token)` parses a numeric epoch-millisecond token, calls `getIndexes(ifModifiedSince)`, and maps returned artists to `ChangeType::Updated` artist events only. This is the exact behavior this story must harden. [Source: hifimule-daemon/src/providers/subsonic.rs]
- `SubsonicClient::search3(&self, query: &str)` currently sends only `query`; it does not expose `songCount` or `songOffset`. Add pagination support without breaking existing `SubsonicProvider::search(query)`. [Source: hifimule-daemon/src/providers/subsonic.rs]
- `Search3Dto` already includes `song`, and `song_from_dto` already maps Subsonic song DTOs into domain `Song`. Reuse these conversion paths instead of adding a second DTO model. [Source: hifimule-daemon/src/providers/subsonic.rs]
- `MediaProvider::changes_since` currently accepts only `Option<&str>`, so it cannot see the device manifest. The album fallback requirement needs either a deliberate trait/context change across providers or an explicit provider-owned helper called from a place that already has the manifest. Do not implement a fake fallback that has no manifest input. [Source: hifimule-daemon/src/providers/mod.rs]
- `ChangeEvent` uses `item: ItemRef`, `change_type: ChangeType::{Created, Updated, Deleted}`, and `version: Option<String>`. Use these exact variants; do not introduce new event enums. [Source: hifimule-daemon/src/domain/models.rs]
- Device manifests store synced track state in `DeviceManifest.synced_items`. The Rust field is still internally named `SyncedItem.jellyfin_id`, but the `.hifimule.json` key is provider-neutral `providerItemId`. [Source: hifimule-daemon/src/device/mod.rs]
- `calculate_delta()` compares desired provider item IDs to manifest `synced_items` and treats metadata-equal ID changes as `id_changes`. If this story adds metadata fields, keep existing delta semantics and tests intact. [Source: hifimule-daemon/src/sync.rs]

### Architecture Compliance

- Epic 8 requires all server communication through the `MediaProvider` trait and provider modules. Album-level drift detection belongs inside the Subsonic provider/client layer; the sync/RPC layer may provide a manifest-derived context but must not issue Subsonic HTTP calls directly. [Source: _bmad-output/planning-artifacts/architecture.md#Subsonic-Incremental-Sync---Album-Level-Fallback]
- For initial Subsonic sync, architecture requires `search3?query=&songCount=500&songOffset={n}` pagination to enumerate all tracks. [Source: _bmad-output/planning-artifacts/architecture.md#Subsonic-Incremental-Sync---Album-Level-Fallback]
- Subsonic `getIndexes?ifModifiedSince` uses milliseconds since Unix epoch and only indicates artist collection changes; it is not a reliable song-level delta feed. [Source: _bmad-output/planning-artifacts/research/technical-compare-jellyfin-navidrome-subsonic-opensubsonic-api-research-2026-05-08.md#Incremental-Sync]
- Subsonic has no ETag equivalent for file-level comparison. Use available metadata such as `size`, `contentType`, and `suffix` as the change signal where present. [Source: _bmad-output/planning-artifacts/epics.md#Story-8.6-Incremental-Sync---Subsonic-Album-Level-Fallback]
- Subsonic URLs containing auth params must be sanitized before logging; Story 8.5 implemented `sanitize_subsonic_url()` and `sanitize_subsonic_message()` and added tests around this. [Source: _bmad-output/implementation-artifacts/8-5-subsonic-url-credential-sanitization.md]

### Story Boundaries

- In scope: Subsonic provider change-detection hardening, explicit change context plumbing, `search3` song pagination, album fallback comparison helpers, minimal backward-compatible manifest metadata needed for future fallback accuracy, and tests.
- In scope if necessary: small `SyncedItem` metadata extension with `#[serde(default)]` and corresponding sync write plumbing.
- Out of scope: broad provider-neutral sync streaming migration, UI changes, runtime server detection, browse RPC changes, API-key auth, adding the `opensubsonic` crate back, or replacing `mockito`.
- Do not call Subsonic HTTP APIs outside `hifimule-daemon/src/providers/subsonic.rs`; passing manifest-derived context into the provider is allowed.
- Do not use artist/title string matching as the only implementation for album fallback; it can support migration for old manifests, but new writes need explicit provider metadata.

### Previous Story Intelligence

- Story 8.3 intentionally deferred full album-level fallback and full-library `search3` pagination to Story 8.6. It established the local hand-rolled Subsonic client and the conservative `getIndexes` implementation now present in code. [Source: _bmad-output/implementation-artifacts/8-3-subsonicprovider-adapter.md]
- Story 8.3 removed the unused `opensubsonic` crate after review. Do not add it back unless there is a clear, tested reason that outweighs the current local-client pattern. [Source: _bmad-output/implementation-artifacts/8-3-subsonicprovider-adapter.md#Review-Findings]
- Story 8.4 established active provider lifecycle and server detection, but kept sync, auto-fill, scrobble, browse, and image proxy paths mostly Jellyfin-first for compatibility. Keep this story focused on provider-level change detection. [Source: _bmad-output/implementation-artifacts/8-4-runtime-server-type-detection-factory.md]
- Story 8.5 completed Subsonic credential sanitization and added `tracing::debug!` in `signed_url` using `sanitize_subsonic_url()`. New request code should follow that exact pattern. [Source: _bmad-output/implementation-artifacts/8-5-subsonic-url-credential-sanitization.md]
- Recent commits are `4f0a2cf Review 8.5`, `3d6f966 Dev 8.5`, `a36079a Story 8.5`, `784250f Change domain`, and `50a0d79 Review 8.4`; implementation should build on the provider/factory/security hardening rather than reworking older Jellyfin flows.

### Latest Technical Context

- The official Subsonic API documents `getIndexes.ifModifiedSince` as milliseconds since 1 Jan 1970 and says the response is only returned if the artist collection changed. This confirms why song-level changes inside an existing album need client fallback. [Source: https://subsonic.org/pages/api.jsp]
- OpenSubsonic's `getIndexes` docs preserve the same `ifModifiedSince` contract and show the JSON `subsonic-response.indexes` shape HifiMule already parses. [Source: https://opensubsonic.netlify.app/docs/endpoints/getindexes/]
- OpenSubsonic documents that ID3 browsing should use `getArtists`, `getArtist`, and `getAlbum`, while file-structure browsing uses `getIndexes` and `getMusicDirectory`. Prefer `getAlbum` for album fallback rather than file-structure traversal. [Source: https://opensubsonic.netlify.app/docs/opensubsonic-api/]
- The official Subsonic API documents `search3` paging params `songCount` and `songOffset`; use those for the initial full-library dump. [Source: https://subsonic.org/pages/api.jsp]

### Testing Guidance

- Keep provider HTTP tests in `hifimule-daemon/src/providers/subsonic.rs` using existing `mockito` helpers and `auth_matchers()`.
- Use query matchers for `songCount`, `songOffset`, `ifModifiedSince`, and `id`; do not assert full URL strings or query param ordering.
- Add focused pure unit tests for album diff helpers so edge cases are readable without full HTTP setup.
- Manifest compatibility tests belong near existing `device` or `sync` tests depending on where the new fields are declared and written.
- Verify no raw auth values appear in new failure messages; use the same sentinel strings from Story 8.5 tests where practical.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story-8.6-Incremental-Sync---Subsonic-Album-Level-Fallback]
- [Source: _bmad-output/planning-artifacts/architecture.md#Subsonic-Incremental-Sync---Album-Level-Fallback]
- [Source: _bmad-output/planning-artifacts/research/technical-compare-jellyfin-navidrome-subsonic-opensubsonic-api-research-2026-05-08.md#Incremental-Sync]
- [Source: _bmad-output/implementation-artifacts/8-3-subsonicprovider-adapter.md]
- [Source: _bmad-output/implementation-artifacts/8-4-runtime-server-type-detection-factory.md]
- [Source: _bmad-output/implementation-artifacts/8-5-subsonic-url-credential-sanitization.md]
- [Source: hifimule-daemon/src/providers/subsonic.rs]
- [Source: hifimule-daemon/src/providers/mod.rs]
- [Source: hifimule-daemon/src/domain/models.rs]
- [Source: hifimule-daemon/src/device/mod.rs]
- [Source: hifimule-daemon/src/sync.rs]

## Dev Agent Record

### Agent Model Used

GPT-5 Codex

### Debug Log References

- 2026-05-09: `rtk cargo test -p hifimule-daemon subsonic --no-fail-fast` — 35 passed, 241 filtered out.
- 2026-05-09: `rtk cargo test -p hifimule-daemon sync --no-fail-fast` — 56 passed, 220 filtered out.
- 2026-05-09: `rtk cargo test -p hifimule-daemon` — 276 passed.

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- Implemented backward-compatible `MediaProvider::changes_since_with_context()` and provider-neutral `ProviderChangeContext`; existing `changes_since(token)` remains a no-context wrapper.
- Implemented Subsonic full-library initial dump via paged `search3?query=&songCount=500&songOffset={n}`, preserving normal UI `search(query)` behavior.
- Implemented Subsonic album fallback that runs only after `getIndexes(ifModifiedSince)` returns no changed artists, re-fetches known context albums via `getAlbum`, and emits song-level `Created`, `Updated`, and `Deleted` changes.
- Added deterministic Subsonic song versions from `id`, `size`, `contentType`, and `suffix`, with conservative behavior when metadata is missing.
- Added defaulted provider metadata fields to `SyncedItem`, sync desired/add/id-change plumbing, manifest-to-change-context conversion, and compatibility tests for old manifests.
- Renamed the serialized synced-item manifest ID to `providerItemId`.

### File List

- _bmad-output/implementation-artifacts/8-6-incremental-sync-subsonic-album-level-fallback.md
- _bmad-output/implementation-artifacts/sprint-status.yaml
- hifimule-daemon/src/device/mod.rs
- hifimule-daemon/src/device/tests.rs
- docs/architecture-jellyfinsync-daemon.md
- docs/data-models-jellyfinsync-daemon.md
- hifimule-daemon/src/main.rs
- hifimule-daemon/src/providers/jellyfin.rs
- hifimule-daemon/src/providers/mod.rs
- hifimule-daemon/src/providers/subsonic.rs
- hifimule-daemon/src/rpc.rs
- hifimule-daemon/src/sync.rs

### Review Findings

- [x] [Review][Decision] `provider_content_type` never stored in manifest — `to_desired_item` and auto-sync path hardcode `provider_content_type: None`; requires a decision on where to source `contentType` in the Subsonic sync write path. Until resolved, `song_metadata_changed` fires false Updated events for any song the server returns with a `contentType`, and real content-type changes are never detected in subsequent syncs. Decision: do not synthesize or persist `contentType` from the current Jellyfin-shaped sync write path; compare content type only when both manifest and provider values are present, and carry authoritative Subsonic metadata through `sync_detect_changes` until provider-neutral sync execution can persist it directly.
- [x] [Review][Patch] SHOWSTOPPER: sync engine never calls `changes_since_with_context` with manifest context — `provider_change_context()` is only used in `device/mod.rs` and its tests; `rpc.rs` and `sync.rs` call only `changes_since()` which passes empty `ProviderChangeContext::default()`, so `album_fallback_changes` always runs with an empty `by_album` map and returns no events. AC1 is non-functional end-to-end. [hifimule-daemon/src/rpc.rs + sync.rs]
- [x] [Review][Patch] Infinite loop risk in `full_song_dump_changes` — loop breaks only when `count < 500`; a server that caps responses at 500 or a library with exactly N×500 songs causes the loop to spin indefinitely with no max-iterations guard. [hifimule-daemon/src/providers/subsonic.rs]
- [x] [Review][Patch] Mid-loop `get_album` 404 aborts entire album fallback, including Jellyfin IDs — any `get_album` error propagates with `?` and skips all remaining albums; Jellyfin album UUIDs included in `provider_change_context()` cause guaranteed 404s on Subsonic, aborting the fallback on mixed-provider or provider-switched devices. [hifimule-daemon/src/providers/subsonic.rs] — superseded by fail-fast error handling so callers do not advance an incomplete token or convert stale IDs into deletes
- [x] [Review][Patch] `provider_change_context()` unconditionally sets `size: Some(size_bytes)` — `size_bytes` is the downloaded file size, not a server-reported value; when Subsonic's `getAlbum` omits `size` (optional field), `song_metadata_changed` compares `Some(n) != None` → emits spurious Updated events for every song in the library on every incremental sync. [hifimule-daemon/src/device/mod.rs]
- [x] [Review][Patch] Auto-sync path hardcodes `provider_album_id: None` — `run_auto_sync` in `main.rs:558` sets all provider fields to None, so items synced via auto-fill never have a provider album ID and cannot participate in album fallback once the sync flow is wired (see showstopper patch). [hifimule-daemon/src/main.rs:558]
- [x] [Review][Defer] Songs without `album_id` excluded from album fallback — acknowledged limitation per spec ("document/test the limitation for old manifests"); those songs' deletions/metadata changes are invisible in the fallback path [hifimule-daemon/src/providers/subsonic.rs] — deferred, pre-existing
- [x] [Review][Defer] Songs moved between albums not detected — when a song's album_id changes on the server, the context groups by old album_id; old album emits Deleted, new album not in context so Create is never emitted; architectural limitation of ID-based grouping [hifimule-daemon/src/providers/subsonic.rs] — deferred, pre-existing
- [x] [Review][Defer] No integration test through full sync engine path — album fallback tests call `changes_since_with_context` directly on the provider; no test exercises manifest → `provider_change_context()` → incremental sync → events [hifimule-daemon/src/providers/subsonic.rs] — deferred, pre-existing
- [x] [Review][Defer] Serial `getAlbum` calls in `album_fallback_changes` — O(n) round trips with no concurrency; performance concern for large libraries [hifimule-daemon/src/providers/subsonic.rs] — deferred, pre-existing
- [x] [Review][Defer] `getIndexes.lastModified` not used for next token — server timestamp ignored; caller constructs next token from wall-clock time; clock skew can cause fallback to be bypassed when it should run [hifimule-daemon/src/providers/subsonic.rs] — deferred, pre-existing

### Review Findings

- [x] [Review][Patch] `sync_detect_changes` is not wired into the current sync workflow — the new RPC handler calls `changes_since_with_context`, but the UI still invokes only `sync_calculate_delta` and `sync_execute`, so AC1 remains non-functional through the existing user sync path. [hifimule-daemon/src/rpc.rs:213]
- [x] [Review][Patch] `sync_detect_changes` has no regression tests — `rtk cargo test -p hifimule-daemon sync_detect_changes --no-fail-fast` runs zero tests, leaving the new manifest-context RPC, error mapping, and response contract unverified. [hifimule-daemon/src/rpc.rs:1242]
- [x] [Review][Patch] RPC layer parses Subsonic internals from `ChangeEvent.version` — `handle_sync_detect_changes` splits the literal `subsonic:{id}|{size}|{contentType}|{suffix}` format, moving provider-specific metadata parsing into `rpc.rs` instead of keeping the provider contract explicit and neutral. [hifimule-daemon/src/rpc.rs:1296]
- [x] [Review][Patch] Missing manifest metadata can still produce false updates — `provider_change_context()` sends `content_type: None` and `suffix: None` for old/current manifest rows, while `song_metadata_changed` treats server-side `Some(contentType)`/`Some(suffix)` as a difference, so legacy rows can still emit spurious `Updated` events. [hifimule-daemon/src/device/mod.rs:100]
- [x] [Review][Patch] Album 404s are converted into mass delete events — `album_fallback_changes` maps `ProviderError::NotFound` for an album fetch to `Deleted` for every expected song, but a stale/wrong/provider-switched album ID does not prove every track was removed. [hifimule-daemon/src/providers/subsonic.rs:114]
- [x] [Review][Patch] Album fetch failures are silently downgraded to partial success — non-404 `get_album` errors are logged and skipped, so change detection can return success with incomplete album coverage and callers may advance tokens after missing changes. [hifimule-daemon/src/providers/subsonic.rs:129]
- [x] [Review][Patch] Full-dump pagination silently truncates at `MAX_SONG_DUMP_PAGES` — the loop exits at 2000 pages without an error or warning, so very large libraries or servers returning endless full pages produce incomplete initial changes while appearing successful. [hifimule-daemon/src/providers/subsonic.rs:89]
- [x] [Review][Patch] `sync_detect_changes` treats missing or malformed `syncToken` as an initial dump — invalid params fall through to `None`, which can trigger an expensive full-library `search3` path instead of returning an invalid-params error. [hifimule-daemon/src/rpc.rs:1246]
- [x] [Review][Patch] `sync_detect_changes` returns debug-formatted enum names as API strings — `format!("{:?}", ...)` is not a stable wire contract for `itemType` or `changeType`; use explicit serialized values. [hifimule-daemon/src/rpc.rs:1320]
- [x] [Review][Patch] Subsonic version parsing treats empty metadata fields as present — `parse_subsonic_version` returns `Some("")` for empty content type or suffix fields, which can persist empty provider metadata and cause later comparisons to treat blanks as real values. [hifimule-daemon/src/rpc.rs:1301]
- [x] [Review][Patch] Detected created songs do not carry `provider_album_id` forward — `DetectedChange` includes content type and suffix but no album ID, while `ChangeEvent` only carries the song ID, so songs added through change detection cannot seed the album context needed for later fallback runs. [hifimule-daemon/src/rpc.rs:1287]

### Change Log

- 2026-05-09: Implemented Story 8.6 Subsonic incremental sync album-level fallback and moved story to review.
- 2026-05-09: Applied code-review patch set for Story 8.6; daemon tests pass, UI build blocked by missing Node/npm on PATH.
- 2026-05-09: Continued review remediation by carrying auto-fill provider album metadata into auto-sync desired items.
- 2026-05-09: Closed content-type source decision conservatively: no synthetic manifest content type, compare only known values.
- 2026-05-09: Renamed serialized manifest synced-item IDs from `jellyfinId` to provider-neutral `providerItemId`.
