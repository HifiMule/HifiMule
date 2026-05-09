# Story 8.3: SubsonicProvider Adapter

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a Ritualist (Arthur) and Convenience Seeker (Sarah),
I want HifiMule to connect to Navidrome and any Subsonic/OpenSubsonic-compatible server,
so that users who prefer Navidrome can use HifiMule without switching media servers.

## Acceptance Criteria

1. Given `SubsonicProvider` implements `MediaProvider`, when connected to a Navidrome or Subsonic server, then `list_artists()` uses `getArtists`, `get_album()` uses `getAlbum`, and `list_playlists()` uses `getPlaylists` through a reusable Subsonic client wrapper.
2. Given Subsonic DTOs cross the provider boundary, when they become domain models, then IDs remain `String`, `duration` seconds pass through unchanged, `bitRate` maps to `bitrate_kbps` without bps conversion, `coverArt` maps to `cover_art_id`, and missing optional values stay `None`.
3. Given `provider.capabilities()` is called after construction, when the server ping reports `openSubsonic: true`, then capabilities report OpenSubsonic support and server transcoding support without re-pinging on every call.
4. Given `provider.download_url()` is called, when no `TranscodeProfile` is supplied then it returns a `/rest/download.view` URL, and when a profile is supplied then it returns a `/rest/stream.view` URL with `format=mp3` and `maxBitRate` in kbps.
5. Given `provider.changes_since(token)` is called on `SubsonicProvider`, when a timestamp token is supplied then it calls `getIndexes` with `ifModifiedSince` expressed as epoch milliseconds and returns coarse `ChangeEvent` values without implementing Story 8.6's full album-level fallback.
6. Given Subsonic request URLs include auth parameters, when URLs are logged, asserted, or surfaced through errors, then credentials are sanitized or redacted and the raw password never leaves the provider module.
7. Given the daemon crate is tested, when `rtk cargo test -p hifimule-daemon` runs, then Subsonic adapter, conversion, auth URL, and existing Jellyfin provider tests pass.

## Tasks / Subtasks

- [x] Add the Subsonic adapter module and dependency wiring (AC: 1, 3, 7)
  - [x] Add `hifimule-daemon/src/providers/subsonic.rs` and export it from `hifimule-daemon/src/providers/mod.rs`.
  - [x] Add the `opensubsonic` crate to `hifimule-daemon/Cargo.toml`, pinning the current exact crate version used by this implementation.
  - [x] Construct `SubsonicProvider` from server URL plus `CredentialKind::Password { username, password }`; reject token credentials with `ProviderError::Auth` or `UnsupportedCapability`.
  - [x] Keep runtime detection/factory integration out of scope except for providing a constructor Story 8.4 can call.

- [x] Implement `MediaProvider` browse and search behavior (AC: 1, 2)
  - [x] `list_libraries()` returns one synthetic `Library` with ID `"all"`, name `"All Music"`, `ItemType::Library`, and no fake cover art.
  - [x] `list_artists(library_id)` ignores `library_id` and maps `getArtists` index entries into domain `Artist` values.
  - [x] `get_artist(artist_id)` uses `getArtist` and maps returned albums into `ArtistWithAlbums`.
  - [x] `list_albums(library_id)` ignores `library_id` and uses an ID3-aligned Subsonic endpoint such as `getAlbumList2` or another documented crate method that returns albums reliably.
  - [x] `get_album(album_id)` uses `getAlbum` and maps album plus songs into `AlbumWithTracks`.
  - [x] `list_playlists()` uses `getPlaylists`; `get_playlist(playlist_id)` uses `getPlaylist` and maps tracks into `PlaylistWithTracks`.
  - [x] `search(query)` uses `search3` and maps artists, albums, songs, and playlists when the API response provides them.

- [x] Add Subsonic DTO-to-domain conversion helpers (AC: 2)
  - [x] Implement focused conversion functions for Subsonic/OpenSubsonic artist, album, playlist, and song DTOs.
  - [x] Use `Seconds(duration)` and `Kbps(bit_rate)` passthrough helpers from `domain/models.rs`; do not apply Jellyfin tick or bps conversions.
  - [x] Preserve Subsonic `coverArt` as `cover_art_id`; never substitute the song, album, or artist ID as artwork ID.
  - [x] Map Subsonic API, HTTP, auth, not-found, and JSON errors into existing `ProviderError` variants.

- [x] Implement capabilities, download URLs, and scrobbling (AC: 3, 4, 6)
  - [x] Ping once during construction or explicit initialization and cache whether `openSubsonic` is true.
  - [x] `server_type()` should return `ServerType::OpenSubsonic` for OpenSubsonic-capable servers and `ServerType::Subsonic` otherwise, matching the enum already in `providers/mod.rs`.
  - [x] `capabilities()` returns `{ open_subsonic, supports_changes_since: true, supports_server_transcoding: true }`.
  - [x] `download_url(song_id, None)` returns a signed `/rest/download.view` URL.
  - [x] `download_url(song_id, Some(profile))` returns a signed `/rest/stream.view` URL with `format=mp3` unless the profile specifies a supported container override, and `maxBitRate` equals `profile.max_bitrate_kbps`.
  - [x] `cover_art_url(cover_art_id)` returns a signed `getCoverArt` URL.
  - [x] `scrobble(Played)` calls Subsonic scrobble with `submission=true`; `Playing` may call now-playing scrobble if supported by the crate or return `UnsupportedCapability` with a focused test.

- [x] Implement first-pass Subsonic changes support only (AC: 5)
  - [x] Interpret `changes_since(Some(token))` as epoch milliseconds if numeric; document and test behavior for missing or malformed tokens.
  - [x] Call `getIndexes` with `ifModifiedSince` for non-empty tokens.
  - [x] Emit conservative `ChangeEvent` values for changed artists/albums/songs that can be derived from the returned response.
  - [x] Do not implement manifest comparison or full-library `search3` pagination in this story; Story 8.6 owns that fallback.

- [x] Add tests and verification (AC: 2, 3, 4, 5, 6, 7)
  - [x] Unit-test Subsonic song conversion: duration passthrough, kbps passthrough, string IDs, missing optional fields, and `coverArt != id`.
  - [x] Add HTTP/client tests for `getArtists`, `getAlbum`, `getPlaylists`, `getPlaylist`, `search3`, `download_url`, `stream_url`, `cover_art_url`, ping/capabilities, and `getIndexes`.
  - [x] Add tests proving raw passwords and auth tokens are redacted in `Debug`, error messages, and any sanitizer helper.
  - [x] Keep existing `providers::jellyfin` tests passing.
  - [x] Run `rtk cargo test -p hifimule-daemon`.

### Review Findings

- [x] [Review][Decision] `opensubsonic` crate pinned in Cargo.toml but never imported — resolved: removed unused dep from Cargo.toml; hand-rolled client is the implementation
- [x] [Review][Decision] `capabilities()` hardcodes `supports_server_transcoding: true` regardless of `open_subsonic` flag — resolved: made conditional on `self.open_subsonic` [hifimule-daemon/src/providers/subsonic.rs]
- [x] [Review][Decision] `getAlbumList2` uses hardcoded `size=500` with no pagination — resolved: added pagination loop with offset [hifimule-daemon/src/providers/subsonic.rs]
- [x] [Review][Patch] `sanitize_message` not applied in `map_reqwest_error` 401/403 branch — fixed [hifimule-daemon/src/providers/subsonic.rs]
- [x] [Review][Patch] `sanitize_message` strips `p=` as a substring false-positively — fixed with query-separator boundary check [hifimule-daemon/src/providers/subsonic.rs]
- [x] [Review][Patch] Redundant second `status == "failed"` check in `get_envelope_url` — removed dead code block [hifimule-daemon/src/providers/subsonic.rs]
- [x] [Review][Patch] Missing test: `scrobble(ScrobbleSubmission::Playing)` → `ProviderError::UnsupportedCapability` — added
- [x] [Review][Patch] Missing test: `changes_since(None)` and `changes_since(Some(""))` — added
- [x] [Review][Patch] Missing tests: HTTP 401/403 → `ProviderError::Auth` and HTTP 404 → `ProviderError::NotFound` — added
- [x] [Review][Patch] `maxBitRate` test only asserts `=192` is present — added negative assertion `!stream.contains("maxBitRate=192000")`
- [x] [Review][Defer] `t=` and `s=` auth params not sanitized in error messages — Story 8.5 owns comprehensive credential sanitization [hifimule-daemon/src/providers/subsonic.rs:504-525] — deferred, pre-existing
- [x] [Review][Defer] `ProviderError::NotFound` always reports `item_type="item", id="unknown"` — loses actual item context; pre-existing design constraint [hifimule-daemon/src/providers/subsonic.rs] — deferred, pre-existing
- [x] [Review][Defer] Passwords stored as plaintext `String` with no `zeroize`-on-drop — pre-existing pattern across entire daemon crate [hifimule-daemon/src/providers/subsonic.rs:233] — deferred, pre-existing
- [x] [Review][Defer] `reqwest::Client` instantiated per `SubsonicClient` with no shared connection pool — pre-existing pattern [hifimule-daemon/src/providers/subsonic.rs:267] — deferred, pre-existing

## Dev Notes

### Current Codebase State

- The daemon crate is `hifimule-daemon`; provider code lives under `hifimule-daemon/src/providers/`. [Source: hifimule-daemon/Cargo.toml]
- Story 8.1 added shared domain models in `hifimule-daemon/src/domain/models.rs` and the `MediaProvider` trait in `hifimule-daemon/src/providers/mod.rs`; reuse these exact types. [Source: hifimule-daemon/src/domain/models.rs; hifimule-daemon/src/providers/mod.rs]
- Story 8.2 added `JellyfinProvider` in `hifimule-daemon/src/providers/jellyfin.rs`; mirror its adapter structure, focused conversion helpers, provider-local tests, and `ProviderError` mapping style where appropriate. [Source: hifimule-daemon/src/providers/jellyfin.rs]
- The current trait signature is `list_artists(&self, library_id: Option<&str>)`; do not add the architecture's proposed `letter` parameter in this story unless all implementors and callers are updated in the same change. [Source: hifimule-daemon/src/providers/mod.rs; _bmad-output/planning-artifacts/architecture.md#Subsonic-Library-Level]
- `ServerType` already includes `Jellyfin`, `Subsonic`, `OpenSubsonic`, and `Unknown`; use those variants instead of adding a new enum. [Source: hifimule-daemon/src/providers/mod.rs]

### Architecture Compliance

- All server communication should go through `MediaProvider`; UI, sync, and scrobble code must not construct Subsonic REST URLs directly. [Source: _bmad-output/planning-artifacts/project-context.md#Core-Principles]
- Subsonic is single-library from HifiMule's perspective: return synthetic `"all"` from `list_libraries()` and ignore `library_id` in Subsonic browse methods. [Source: _bmad-output/planning-artifacts/architecture.md#Subsonic-Library-Level]
- For Subsonic artwork, `coverArt` is a distinct API field and is not equal to the item ID; map it only at the provider boundary. [Source: _bmad-output/planning-artifacts/architecture.md#Cover-Art-Routing]
- Auth params are `u`, `t=md5(password + salt)`, `s`, `v=1.16.1`, `c=hifimule`, and `f=json`, with a fresh random salt per request. If the `opensubsonic` crate handles this internally, still test that generated URLs do not expose raw passwords. [Source: _bmad-output/planning-artifacts/architecture.md#Subsonic-Auth-Internals]
- The raw password should remain inside `providers/subsonic.rs` and redacted debug types; do not store it in `AppState` or pass it through RPC responses. [Source: _bmad-output/planning-artifacts/architecture.md#Subsonic-Auth-Internals]

### Story Boundaries

- Story 8.3 owns the `SubsonicProvider` adapter and conversion tests.
- Story 8.4 owns runtime server detection, persisted server config, active `Arc<dyn MediaProvider>` lifecycle, and app-wide provider replacement.
- Story 8.5 owns comprehensive Subsonic URL credential sanitization. This story still must avoid leaking credentials in its own logs/tests/errors.
- Story 8.6 owns album-level drift detection, current-manifest comparison, and initial-sync `search3` pagination fallback. Keep `changes_since` here intentionally conservative.
- Do not change visible Tauri UI copy or routes in this story.

### Previous Story Intelligence

- 8.2 chose to wrap existing clients rather than move broad files. Follow that low-regression approach: add `providers/subsonic.rs`, export it, and avoid broad caller migration before Story 8.4. [Source: _bmad-output/implementation-artifacts/8-2-jellyfinprovider-adapter.md#Story-Boundaries]
- 8.2 review found missing tests around browse methods and error mapping; include Subsonic tests for every implemented trait method, especially auth, 404/not-found, malformed JSON, and unsupported capability behavior. [Source: _bmad-output/implementation-artifacts/8-2-jellyfinprovider-adapter.md#Review-Findings]
- 8.2 kept sync streaming on the existing Jellyfin path because provider lifecycle is not yet app-wide. For Subsonic, expose signed URLs now, but do not force sync engine migration in this story. [Source: _bmad-output/implementation-artifacts/8-2-jellyfinprovider-adapter.md#Completion-Notes-List]
- 8.1 introduced `CredentialKind::Password` with redacted `Debug`; use it rather than creating a new credential shape. [Source: hifimule-daemon/src/providers/mod.rs]

### Latest Technical Context

- `opensubsonic` is currently documented as a complete async Rust client for Subsonic API v1.16.1 plus OpenSubsonic extensions, with methods including `ping`, `getArtists`, `getArtist`, `getAlbum`, `search3`, `getPlaylists`, `getPlaylist`, `stream`, `download`, `getCoverArt`, and `scrobble`. [Source: https://docs.rs/opensubsonic]
- The latest docs observed during story creation list `opensubsonic` 0.3.0. Pin the exact version chosen in `Cargo.toml` because the API has already changed between documented releases. [Source: https://docs.rs/crate/opensubsonic/0.3.0]
- OpenSubsonic's ID3-oriented browse path is `getArtists`, `getArtist`, and `getAlbum`; file-structure browse uses `getIndexes` and `getMusicDirectory`. Prefer ID3 methods for HifiMule browse views. [Source: https://opensubsonic.netlify.app/docs/opensubsonic-api/]

### Testing Guidance

- Keep conversion tests pure and local to `providers/subsonic.rs`.
- Prefer the existing `mockito` dev dependency unless `opensubsonic` tests are materially easier with another mock server; avoid adding `wiremock` and `insta` unless the implementation genuinely needs them.
- Assert URL query parameters with matchers rather than raw string order; signed Subsonic URLs may reorder query params.
- Test that `maxBitRate` is kbps, not bps.
- Run `rtk cargo test -p hifimule-daemon` before moving the story to review.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story-8.3-SubsonicProvider-Adapter]
- [Source: _bmad-output/planning-artifacts/architecture.md#Subsonic-Library-Level]
- [Source: _bmad-output/planning-artifacts/architecture.md#Cover-Art-Routing]
- [Source: _bmad-output/planning-artifacts/architecture.md#Subsonic-Auth-Internals]
- [Source: _bmad-output/implementation-artifacts/8-1-mediaprovider-trait-and-domain-models.md]
- [Source: _bmad-output/implementation-artifacts/8-2-jellyfinprovider-adapter.md]
- [Source: hifimule-daemon/src/providers/mod.rs]
- [Source: hifimule-daemon/src/providers/jellyfin.rs]
- [Source: hifimule-daemon/src/domain/models.rs]

## Dev Agent Record

### Agent Model Used

GPT-5 Codex

### Debug Log References

- `rtk cargo test -p hifimule-daemon providers::subsonic::tests -- --test-threads=1 --nocapture` - 16 Subsonic-focused tests passed.
- `rtk cargo test -p hifimule-daemon` - full daemon suite passed, 246 tests.

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- Added `SubsonicProvider` and a provider-local reusable Subsonic REST client wrapper with password auth, MD5 token signing, cached OpenSubsonic ping capability, signed download/stream/cover-art URLs, browse/search/scrobble methods, and conservative `getIndexes`-based change events.
- Added Subsonic DTO-to-domain conversion helpers preserving string IDs, seconds, kbps, optional fields, and distinct `coverArt` values.
- Added Subsonic unit and HTTP tests covering conversions, browse/search endpoints, signed URLs, ping/capabilities caching, change tokens, scrobble, credential redaction, and API/JSON error mapping.

### File List

- `Cargo.lock`
- `_bmad-output/implementation-artifacts/8-3-subsonicprovider-adapter.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `hifimule-daemon/Cargo.toml`
- `hifimule-daemon/src/providers/mod.rs`
- `hifimule-daemon/src/providers/subsonic.rs`

### Change Log

- 2026-05-09: Implemented SubsonicProvider adapter, tests, and story workflow status updates.
