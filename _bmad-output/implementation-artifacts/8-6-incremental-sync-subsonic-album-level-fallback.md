# Story 8.6: Incremental Sync - Subsonic Album-Level Fallback

Status: ready-for-dev

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

- [ ] Extend Subsonic client support for paged full-library song enumeration (AC: 2, 3)
  - [ ] Replace or overload the local `SubsonicClient::search3(&self, query: &str)` wrapper so it can pass `songCount` and `songOffset` query params while preserving existing browse/search behavior.
  - [ ] Add `SubsonicProvider` helper code that treats `None`, empty token, and `"0"` as initial/full dump and calls `search3` with `query=""`, `songCount=500`, `songOffset=0,500,...`.
  - [ ] Emit `ChangeEvent { item: ItemRef { id: song.id, item_type: ItemType::Song }, change_type: ChangeType::Created, version: computed_version }` for every full-dump song.
  - [ ] Preserve existing `search(query)` public behavior for UI search; do not make normal user search return every song.

- [ ] Introduce a minimal change-detection context without broad sync migration (AC: 1, 5, 7)
  - [ ] Add an explicit provider-neutral context type in `hifimule-daemon/src/providers/mod.rs` or an adjacent module, carrying only data needed for fallback: synced song ID, album ID when known, size, content type, suffix, and any existing version/etag.
  - [ ] Update the `MediaProvider` trait and both provider implementations deliberately if the chosen API is `changes_since(token, context)`. Jellyfin may ignore the context, but tests must prove its existing `minDateLastSaved` behavior is unchanged.
  - [ ] If a separate helper is chosen instead of changing the trait, keep it provider-owned and explicit; do not use process globals, `AppState` back references, or hidden reads of the current device from inside `SubsonicProvider`.
  - [ ] Update only the RPC/sync caller path needed to pass the current manifest snapshot into change detection. Do not migrate streaming/download execution in this story.

- [ ] Implement album-level fallback inside `providers/subsonic.rs` only (AC: 1, 4, 5)
  - [ ] Keep Subsonic-specific fallback mechanics inside the Subsonic provider/client layer; outside callers should provide context and process returned `ChangeEvent`s, not call `getAlbum` directly.
  - [ ] For non-initial numeric tokens, keep calling `getIndexes` with `ifModifiedSince` as epoch milliseconds first.
  - [ ] When `getIndexes` returns changed artists, continue emitting conservative artist-level updates as today unless a stronger album mapping is available.
  - [ ] When `getIndexes` returns no changed artists, re-fetch albums known from the supplied change context and compare their fresh track IDs and metadata to the context snapshot.
  - [ ] Add a provider-local comparison helper that detects added, removed, and updated songs without depending on query parameter ordering or response ordering.

- [ ] Add a backward-compatible source for album fallback inputs (AC: 1, 5)
  - [ ] Audit `SyncedItem` in `hifimule-daemon/src/device/mod.rs`; it currently has `jellyfin_id`, `name`, `album`, `artist`, `local_path`, `size_bytes`, `synced_at`, `original_name`, and `etag`, but no album ID.
  - [ ] Add manifest fields only with `#[serde(default)]` so old `.hifimule.json` files keep deserializing. Prefer explicit provider-neutral names for new fields, e.g. `provider_album_id`, `provider_content_type`, `provider_suffix`, or a compact metadata struct if it fits existing style.
  - [ ] Update sync writes in `hifimule-daemon/src/sync.rs` or the provider-to-sync mapping path only if needed to persist those fields for future syncs; preserve all existing Jellyfin semantics and JSON compatibility.
  - [ ] If current runtime paths cannot populate album IDs for pre-existing manifest rows, implement a focused fallback that can derive candidate album IDs through Subsonic `search3`/`getAlbum` data, and document/test the limitation for old manifests.
  - [ ] Do not rename `jellyfin_id` in this story; it is a legacy field name used broadly as the provider item ID.

- [ ] Compute stable Subsonic change versions (AC: 4)
  - [ ] Add a small helper that builds `ChangeEvent.version` from available Subsonic song fields: at minimum `id`, `size`, `contentType`, and `suffix`; include a deterministic separator/format.
  - [ ] Treat missing metadata conservatively: if ID is new, emit `Created`; if an existing ID lacks metadata needed for version comparison, do not invent false updates.
  - [ ] Preserve Subsonic unit rules from Story 8.3: duration is seconds, bitrate is kbps, IDs are `String`, and `coverArt` is distinct from item ID.

- [ ] Protect security and boundaries (AC: 6, 7)
  - [ ] Reuse `sanitize_subsonic_url()` / `sanitize_subsonic_message()` for any new logging or provider errors that can include Subsonic URLs.
  - [ ] Do not add `opensubsonic`, `wiremock`, `insta`, or a new sync database layer for this story; the existing hand-rolled client and `mockito` tests are the current project pattern.
  - [ ] Do not migrate `execute_sync()` to provider-neutral streaming unless the minimum manifest metadata path absolutely requires a small compatibility change.
  - [ ] Do not change UI copy, Tauri routes, `server.connect`, or browse RPC contracts.

- [ ] Add focused tests and verification (AC: 1-7)
  - [ ] Add `providers::subsonic` tests for initial full dump: first page 500 songs, second page fewer than 500, all emitted as song `Created` changes.
  - [ ] Add a test proving `changes_since(Some("0"))` uses `search3` and does not call `getIndexes`.
  - [ ] Add a test where `getIndexes` returns no artists, existing manifest album data is re-fetched via `getAlbum`, and a new song ID emits `ChangeType::Created`.
  - [ ] Add tests for removed song IDs and metadata-only updates using `size`, `contentType`, and `suffix`.
  - [ ] Add manifest serialization/deserialization tests proving new fields default cleanly on old manifests and serialize in the existing camelCase style where applicable.
  - [ ] Run `rtk cargo test -p hifimule-daemon subsonic --no-fail-fast`, `rtk cargo test -p hifimule-daemon sync --no-fail-fast`, and `rtk cargo test -p hifimule-daemon`.

## Dev Notes

### Current Codebase State

- `SubsonicProvider` and its local REST client live in `hifimule-daemon/src/providers/subsonic.rs`. The current `changes_since(token)` parses a numeric epoch-millisecond token, calls `getIndexes(ifModifiedSince)`, and maps returned artists to `ChangeType::Updated` artist events only. This is the exact behavior this story must harden. [Source: hifimule-daemon/src/providers/subsonic.rs]
- `SubsonicClient::search3(&self, query: &str)` currently sends only `query`; it does not expose `songCount` or `songOffset`. Add pagination support without breaking existing `SubsonicProvider::search(query)`. [Source: hifimule-daemon/src/providers/subsonic.rs]
- `Search3Dto` already includes `song`, and `song_from_dto` already maps Subsonic song DTOs into domain `Song`. Reuse these conversion paths instead of adding a second DTO model. [Source: hifimule-daemon/src/providers/subsonic.rs]
- `MediaProvider::changes_since` currently accepts only `Option<&str>`, so it cannot see the device manifest. The album fallback requirement needs either a deliberate trait/context change across providers or an explicit provider-owned helper called from a place that already has the manifest. Do not implement a fake fallback that has no manifest input. [Source: hifimule-daemon/src/providers/mod.rs]
- `ChangeEvent` uses `item: ItemRef`, `change_type: ChangeType::{Created, Updated, Deleted}`, and `version: Option<String>`. Use these exact variants; do not introduce new event enums. [Source: hifimule-daemon/src/domain/models.rs]
- Device manifests store synced track state in `DeviceManifest.synced_items`. `SyncedItem.jellyfin_id` is the legacy provider item ID field; it has no album ID today, so album fallback needs a backward-compatible manifest metadata strategy. [Source: hifimule-daemon/src/device/mod.rs]
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

{{agent_model_name_version}}

### Debug Log References

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.

### File List

- _bmad-output/implementation-artifacts/8-6-incremental-sync-subsonic-album-level-fallback.md
- _bmad-output/implementation-artifacts/sprint-status.yaml
