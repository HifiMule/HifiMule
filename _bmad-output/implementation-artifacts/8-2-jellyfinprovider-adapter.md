# Story 8.2: JellyfinProvider Adapter

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a System Admin (Alexis),
I want the existing Jellyfin API client wrapped as a `JellyfinProvider` implementing `MediaProvider`,
so that no existing functionality regresses and all callers can move toward the unified provider interface.

## Acceptance Criteria

1. Given `JellyfinProvider` wraps the existing Jellyfin API logic, when callers browse libraries, artists, albums, playlists, search results, artwork, downloads, streams, scrobbles, or changes, then the behavior is exposed through the `MediaProvider` trait from Story 8.1.
2. Given Jellyfin DTOs cross the provider boundary, when they become domain models, then all normalization is correct: Jellyfin IDs remain `String`, `runTimeTicks / 10_000_000` becomes `duration_seconds`, bps becomes kbps, `MediaSources[0].Size` becomes `size_bytes` only where the domain supports it, and artwork references stay as `cover_art_id`.
3. Given `provider.changes_since(token)` is called on `JellyfinProvider`, when the token is a previous sync timestamp, then Jellyfin is queried with `/Items?minDateLastSaved={ISO}` and returned items become `ChangeEvent` values without leaking Jellyfin DTOs to callers.
4. Given the existing UI, sync engine, auto-fill, and scrobble bridge rely on Jellyfin today, when this story is complete, then those paths still pass their existing tests and no user-visible Jellyfin behavior regresses.
5. Given Story 8.4 will own runtime server detection and full provider lifecycle, when this story is complete, then provider construction is available and testable without prematurely replacing the whole app state with `Arc<dyn MediaProvider>`.
6. Given the daemon crate is tested, when `rtk cargo test -p hifimule-daemon` runs, then the Jellyfin adapter and DTO-to-domain conversion tests pass.

## Tasks / Subtasks

- [x] Add `JellyfinProvider` adapter module (AC: 1, 5)
  - [x] Create `hifimule-daemon/src/providers/jellyfin.rs` and export it from `hifimule-daemon/src/providers/mod.rs`.
  - [x] Wrap the existing `JellyfinClient`; do not duplicate HTTP request logic in a second client.
  - [x] Store the provider's server URL, token credential, and user ID in the provider instance or an adjacent constructor type.
  - [x] Return `ServerType::Jellyfin` and `Capabilities { open_subsonic: false, supports_changes_since: true, supports_server_transcoding: true }`.

- [x] Map existing Jellyfin calls to the `MediaProvider` trait (AC: 1, 2)
  - [x] `list_libraries()` should reuse `JellyfinClient::get_views()` and map music collection views into domain `Library`.
  - [x] `list_artists(library_id)` should reuse or extend `get_items()` with `IncludeItemTypes=MusicArtist`; preserve alphabetical quick-nav behavior if a trait signature amendment for `letter` is introduced.
  - [x] `list_albums(library_id)` and `get_album(album_id)` should reuse or extend `get_items()` / `get_child_items_with_sizes()` so album tracks are returned as domain `Song` values.
  - [x] `get_artist(artist_id)` should return a domain `ArtistWithAlbums`; if existing `/Items?ParentId={artist}` behavior is insufficient, extend `JellyfinClient` with the minimal Jellyfin query needed instead of doing ad hoc HTTP in the provider.
  - [x] `search(query)` should wrap `search_audio_items()` initially and map song results; add artist/album search only if the existing API surface already supports it safely.
  - [x] `download_url()` should resolve the same effective Jellyfin download/stream endpoint used by existing sync, including `PlaybackInfo` transcoding fallback when a `TranscodeProfile` is supplied.
  - [x] `cover_art_url()` should produce the Jellyfin primary image URL used by `image_proxy`; do not let UI code build Jellyfin URLs itself.
  - [x] `scrobble()` should preserve current `report_item_played()` semantics for the `Played` submission path.

- [x] Add Jellyfin DTO-to-domain conversion helpers (AC: 2)
  - [x] Implement focused conversion functions or `TryFrom` impls for `JellyfinView -> Library`, `JellyfinItem -> Artist`, `JellyfinItem -> Album`, and `JellyfinItem -> Song`.
  - [x] Use Story 8.1 newtypes/helpers (`JellyfinTicks`, `Seconds`, `Bps`, `Kbps`) for duration and bitrate conversion.
  - [x] Preserve optional fields instead of inventing placeholder values; missing Jellyfin size, bitrate, track number, artist ID, or cover art should remain `None`.
  - [x] Translate adapter failures into `ProviderError` variants; do not bubble raw `anyhow` text when a specific HTTP/auth/not-found/deserialization variant is available.

- [x] Implement Jellyfin incremental changes (AC: 3)
  - [x] Extend `JellyfinClient::get_items()` or add a minimal method for `/Items?minDateLastSaved={ISO}` with `Fields=MediaSources`.
  - [x] Treat the current `MediaProvider::changes_since(token: Option<&str>)` token as an ISO timestamp string for Jellyfin in this story; document invalid or missing token behavior in tests.
  - [x] Map returned Jellyfin item types into `ItemType::{Song, Album, Artist, Playlist}` and `ChangeType::Updated` unless a reliable created/deleted signal exists.
  - [x] Do not add Subsonic fallback behavior here; Story 8.3 and 8.6 own Subsonic incremental semantics.

- [x] Migrate low-risk call sites or add compatibility seams (AC: 4, 5)
  - [x] Keep existing JSON-RPC method names such as `jellyfin_get_items` stable unless a compatibility wrapper is added.
  - [x] Prefer updating internal helpers to call `JellyfinProvider` where behavior is already 1:1 with the trait.
  - [x] Defer wholesale `AppState.provider: Arc<RwLock<Option<Arc<dyn MediaProvider>>>>` replacement to Story 8.4 unless it is required for compilation.
  - [x] Preserve current sync streaming behavior in `sync.rs`; if `download_url()` only returns a URL, ensure the code still uses the same auth headers and streaming body path as before.

- [x] Test and verify (AC: 2, 3, 4, 6)
  - [x] Add unit tests for every Jellyfin DTO-to-domain conversion, including ticks-to-seconds, bps-to-kbps, UUID string passthrough, missing optional fields, and cover art ID preservation.
  - [x] Add mockito-backed adapter tests for list libraries, list albums/get album tracks, search, cover art URL, scrobble/report played, and `minDateLastSaved`.
  - [x] Keep or update existing `api.rs` tests so current HTTP request shapes are still covered.
  - [x] Run `rtk cargo test -p hifimule-daemon`.

### Review Findings

- [x] [Review][Patch] `get_artist` uses `ParentId=<artist_id>` — Jellyfin returns 0 albums; must use `AlbumArtistIds=<artist_id>&IncludeItemTypes=MusicAlbum&Recursive=true` [hifimule-daemon/src/providers/jellyfin.rs:135-153]
- [x] [Review][Patch] `transcode_profile_to_device_profile` missing `TranscodingProfiles` and `DirectPlayProfiles` arrays — transcoding silently falls through to direct play [hifimule-daemon/src/providers/jellyfin.rs:448-455]
- [x] [Review][Patch] `song_from_item`: `album_id.or(parent_id)` fallback incorrect for search results — search endpoint never populates `AlbumId`, so `parent_id` (library folder) is always used as album ID [hifimule-daemon/src/providers/jellyfin.rs:409]
- [x] [Review][Patch] `status_from_message` extracts any first 3-digit decimal run from error string — can return IP octets or path components instead of HTTP status code [hifimule-daemon/src/providers/jellyfin.rs:457-467]
- [x] [Review][Patch] `map_error` uses substring matching on "401"/"403"/"404" — misclassifies errors whose text coincidentally contains those digits (e.g., item IDs, path components) [hifimule-daemon/src/providers/jellyfin.rs:42-70]
- [x] [Review][Patch] `map_not_found` re-wraps error via `anyhow!(message)` — discards original error chain and causes data loss [hifimule-daemon/src/providers/jellyfin.rs:72-82]
- [x] [Review][Patch] Variable shadowing in `get_items_changed_since`: `if let Some(token) = min_date_last_saved` reuses name `token`, obscuring intent and creating a refactor trap [hifimule-daemon/src/api.rs:468]
- [x] [Review][Patch] No tests for `list_artists` or `get_artist` — AC1/AC6 violation [hifimule-daemon/src/providers/jellyfin.rs]
- [x] [Review][Patch] No error-mapping tests (401→Auth, 404→NotFound, malformed JSON→Deserialization) — AC6 violation [hifimule-daemon/src/providers/jellyfin.rs]
- [x] [Review][Patch] No invalid/malformed token test for `changes_since` — AC3 constraint violation [hifimule-daemon/src/providers/jellyfin.rs]
- [x] [Review][Patch] No test for `scrobble(ScrobbleSubmission::Playing)` returning `UnsupportedCapability` — AC6 violation [hifimule-daemon/src/providers/jellyfin.rs]
- [x] [Review][Defer] `download_url` without profile returns unauthenticated URL — Story 8.4 owns provider integration; sync.rs still uses JellyfinClient directly [hifimule-daemon/src/providers/jellyfin.rs:271-276] — deferred, Story 8.4 scope
- [x] [Review][Defer] Token stored as plain `String` without `CredentialKind` wrapper — Story 8.4 owns constructor interface and provider lifecycle [hifimule-daemon/src/providers/jellyfin.rs:20-25] — deferred, Story 8.4 scope
- [x] [Review][Defer] `user_id` not url-encoded in `get_items_changed_since` — pre-existing pattern across all JellyfinClient methods; Jellyfin UUIDs are hex+hyphen so no encoding needed in practice [hifimule-daemon/src/api.rs:458] — deferred, pre-existing

## Dev Notes

### Current Codebase State

- The daemon crate is `hifimule-daemon`; source files live under `hifimule-daemon/src/`. [Source: hifimule-daemon/Cargo.toml]
- Story 8.1 is complete and added `hifimule-daemon/src/domain/models.rs` plus `hifimule-daemon/src/providers/mod.rs`. Build on those types; do not create parallel domain structs. [Source: _bmad-output/implementation-artifacts/8-1-mediaprovider-trait-and-domain-models.md]
- `MediaProvider` currently has this relevant surface: `list_libraries`, `list_artists`, `get_artist`, `list_albums`, `get_album`, `list_playlists`, `get_playlist`, `search`, `download_url`, `cover_art_url`, `changes_since`, `scrobble`, `server_type`, and `capabilities`. [Source: hifimule-daemon/src/providers/mod.rs]
- `MediaProvider::changes_since` currently takes `Option<&str>`, not `SystemTime`; use ISO timestamp strings for Jellyfin in this story and do not broaden the signature unless tests and all implementors are updated. [Source: hifimule-daemon/src/providers/mod.rs; _bmad-output/implementation-artifacts/8-1-mediaprovider-trait-and-domain-models.md#Review-Findings]
- Current Jellyfin HTTP/DTO code is concentrated in `hifimule-daemon/src/api.rs`: `JellyfinClient`, `JellyfinItem`, `JellyfinView`, `MediaSource`, `JellyfinUserData`, auth, item browsing, image fetching, search, scrobble reporting, stream negotiation, and many mockito tests. [Source: hifimule-daemon/src/api.rs]
- Current direct `JellyfinClient` call sites remain in `rpc.rs`, `sync.rs`, `auto_fill.rs`, `scrobbler.rs`, and `main.rs`. This story should reduce direct coupling where practical but must not destabilize the app before Story 8.4 owns runtime provider lifecycle. [Source: hifimule-daemon/src/rpc.rs; hifimule-daemon/src/sync.rs; hifimule-daemon/src/auto_fill.rs; hifimule-daemon/src/scrobbler.rs; hifimule-daemon/src/main.rs]

### Architecture Compliance

- All server communication must ultimately be mediated through `MediaProvider`; the daemon should eventually hold an `Arc<dyn MediaProvider>` resolved at connect time. [Source: _bmad-output/planning-artifacts/architecture.md#Media-Provider-Layer]
- Domain models are API-neutral. DTO-to-domain conversion belongs at the provider boundary, not in UI, sync, or RPC code. [Source: _bmad-output/planning-artifacts/architecture.md#Media-Provider-Layer]
- Normalization rules are mandatory: all IDs are `String`; Jellyfin `runTimeTicks` becomes seconds; Jellyfin bitrate bps becomes kbps; cover art IDs remain separate fields. [Source: _bmad-output/planning-artifacts/architecture.md#Media-Provider-Layer]
- The architecture describes `providers/jellyfin.rs` as the adapter location and `providers/mod.rs` as the shared trait/error/type location. [Source: _bmad-output/planning-artifacts/architecture.md#Project-structure-additions]
- Browse RPC responses use camelCase and should remain compatible with existing UI expectations. `image_proxy` remains the UI artwork path; callers should not construct provider artwork URLs directly outside provider/RPC plumbing. [Source: _bmad-output/planning-artifacts/architecture.md#Library-Browsing-Multi-Provider-RPC-Contract]

### Story Boundaries

- Story 8.2 owns the Jellyfin adapter and safe caller migration. Story 8.3 owns `SubsonicProvider`; Story 8.4 owns runtime server type detection/factory and full `AppState.provider`; Story 8.5 owns Subsonic URL sanitization; Story 8.6 owns Subsonic incremental album-level fallback.
- Do not add `opensubsonic` in this story.
- Do not add a new Jellyfin SDK dependency unless the implementation deliberately replaces the existing manual client. The epic notes mention pinning `jellyfin-sdk` because it is pre-1.0, but the current code already has a working reqwest client on workspace `reqwest ~0.12`; wrapping that client is the lower-regression path. If `jellyfin-sdk` is introduced anyway, pin an exact version and update this story's file list and tests.
- Keep existing credential storage behavior unless a small constructor adapter is needed. Broader server config persistence belongs to Story 8.4.
- Do not change Tauri UI routes or visible copy for this story.

### Implementation Guardrails

- Avoid a mechanical move of `api.rs` that breaks tests and imports. A safe path is to add `providers/jellyfin.rs` that wraps `crate::api::JellyfinClient`, then optionally move internals after tests pass.
- If a method must add new Jellyfin query parameters, extend `JellyfinClient` with a named method rather than assembling raw URLs in several places.
- Existing `JellyfinClient` uses `X-Emby-Token` for many authenticated requests. Centralizing auth header construction is useful, but a broad auth-header migration is not required for this story unless all affected tests are updated.
- Preserve streaming semantics: existing sync calls `get_item_stream()` and consumes `bytes_stream()` with auth headers. A provider method that returns only a URL is not enough unless the caller still has a safe way to stream with the same headers.
- Treat playlist support carefully. If current `api.rs` does not expose dedicated playlist endpoints, implement the minimal Jellyfin client methods with mock tests rather than faking playlists through generic item queries.
- `list_artists` in current code does not accept `letter`, while architecture later amends the trait. If the trait is amended here, update all implementors/tests and wire Jellyfin `NameStartsWith` / `NameLessThan` through deliberately.

### Previous Story Intelligence

- Story 8.1 deliberately did not move `api.rs` or migrate `rpc.rs`, `sync.rs`, `auto_fill.rs`, `main.rs` call sites. This story is the first adapter/migration step. [Source: _bmad-output/implementation-artifacts/8-1-mediaprovider-trait-and-domain-models.md#Story-Boundaries]
- Review for 8.1 replaced flat credentials with `CredentialKind::{Token, Password}` and redacted `Debug`; preserve that security posture when constructing a Jellyfin provider. [Source: _bmad-output/implementation-artifacts/8-1-mediaprovider-trait-and-domain-models.md#Review-Findings]
- 8.1 deferred the untyped `changes_since` token semantics to later stories. For this story, explicitly test the Jellyfin ISO timestamp interpretation so the ambiguity is contained. [Source: _bmad-output/implementation-artifacts/8-1-mediaprovider-trait-and-domain-models.md#Review-Findings]
- 8.1 test command passed with 209 daemon tests. Keep this story's changes narrow enough that the same package-level command remains the primary verification. [Source: _bmad-output/implementation-artifacts/8-1-mediaprovider-trait-and-domain-models.md#Debug-Log-References]

### Latest Technical Context

- The Jellyfin OpenAPI index currently lists stable OpenAPI artifacts dated 2025-12-15 and unstable artifacts dated 2025-12-20, so prefer the checked-in/current client behavior plus focused mock tests over assuming older endpoint examples are authoritative. [Source: https://api.jellyfin.org/openapi/]
- Jellyfin auth uses the `MediaBrowser` authorization scheme; `X-Emby-Token` exists as a fallback in common clients but should remain centralized so it can be replaced safely later. [Source: https://jmshrv.com/posts/jellyfin-api/]
- Jellyfin's relevant HifiMule endpoints remain: `/UserViews`, `/Items`, `/Items/{id}`, `/Items/{id}/Images/Primary`, `/Items/{id}/Download`, `/Items/{id}/PlaybackInfo`, `/UserPlayedItems/{id}`, and `/Items?minDateLastSaved={ISO}`. [Source: _bmad-output/planning-artifacts/research/technical-compare-jellyfin-navidrome-subsonic-opensubsonic-api-research-2026-05-08.md#Full-API-Surface-Reference]

### Testing Guidance

- Keep tests close to the adapter. Conversion tests should not require HTTP.
- Use existing `mockito` dev dependency for HTTP behavior; do not add `wiremock` just for this story unless there is a clear benefit.
- Test `minDateLastSaved` by asserting the query parameter is sent and returned DTOs become `ChangeEvent` entries.
- Test failure mapping: 401/403 should become auth-ish `ProviderError`; 404 should become `NotFound` where item identity is known; malformed JSON should become `Deserialization`.
- Run `rtk cargo test -p hifimule-daemon` before moving the story to review.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story-8.2-JellyfinProvider-Adapter]
- [Source: _bmad-output/planning-artifacts/architecture.md#Media-Provider-Layer]
- [Source: _bmad-output/planning-artifacts/architecture.md#Library-Browsing-Multi-Provider-RPC-Contract]
- [Source: _bmad-output/planning-artifacts/architecture.md#Provider-Type-Definitions]
- [Source: _bmad-output/planning-artifacts/research/technical-compare-jellyfin-navidrome-subsonic-opensubsonic-api-research-2026-05-08.md#Jellyfin-Endpoints-HifiMule-Relevant]
- [Source: _bmad-output/implementation-artifacts/8-1-mediaprovider-trait-and-domain-models.md]
- [Source: hifimule-daemon/src/api.rs]
- [Source: hifimule-daemon/src/providers/mod.rs]
- [Source: hifimule-daemon/src/domain/models.rs]

## Dev Agent Record

### Agent Model Used

GPT-5 Codex

### Debug Log References

- 2026-05-09: Started Story 8.2 implementation; sprint status moved to in-progress.
- 2026-05-09: Added failing Jellyfin adapter/conversion tests, then implemented adapter against existing `JellyfinClient`.
- 2026-05-09: Ran `rtk cargo test -p hifimule-daemon providers::jellyfin` after adapter implementation; provider-focused tests passed.
- 2026-05-09: Ran `rtk cargo fmt -p hifimule-daemon`.
- 2026-05-09: Ran `rtk cargo test -p hifimule-daemon`; 223 tests passed.

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- Implemented `JellyfinProvider` as a `MediaProvider` wrapper around the existing `JellyfinClient`, including Jellyfin capabilities, constructor state, browse/search/download/artwork/scrobble/change methods, and DTO-to-domain conversion helpers.
- Extended the Jellyfin DTO/client surface only where needed for adapter normalization and incremental changes: optional artist/album/image/bitrate fields, public stream URL resolution, and `/Items?minDateLastSaved={ISO}` support.
- Kept existing JSON-RPC names and sync streaming path stable; wholesale provider lifecycle replacement remains deferred to Story 8.4.
- Added conversion and mockito-backed adapter tests covering libraries, albums/tracks, playlists, search, cover art URL, scrobble played submission, download URL/transcoding resolution, and changes since.

### File List

- _bmad-output/implementation-artifacts/8-2-jellyfinprovider-adapter.md
- _bmad-output/implementation-artifacts/sprint-status.yaml
- hifimule-daemon/src/api.rs
- hifimule-daemon/src/auto_fill.rs
- hifimule-daemon/src/providers/mod.rs
- hifimule-daemon/src/providers/jellyfin.rs
- hifimule-daemon/src/sync.rs

### Change Log

- 2026-05-09: Added JellyfinProvider adapter and Jellyfin DTO/domain normalization tests; daemon test suite passes.
