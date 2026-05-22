# Story 9.1: Provider Browse Modes and Capability Contract

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a System Admin (Alexis),
I want the daemon provider layer to expose supported browse modes explicitly,
so that the UI can show Jellyfin-like navigation without hardcoding server-specific API behavior.

## Acceptance Criteria

1. Given a provider is connected, when the UI requests available browse modes, then the daemon returns only the modes that the active provider can service from the canonical set: `artists`, `albums`, `playlists`, `genres`, `recentlyAdded`, `frequentlyPlayed`, `recentlyPlayed`, and `favorites`.
2. Given a browse mode is unsupported by the active provider, when the UI asks for available modes, then that mode is absent from the returned mode list and any direct request for that mode fails with `ProviderError::UnsupportedCapability` mapped to a JSON-RPC error.
3. Given browse data is requested through the new `browse.*` RPC methods, then every request obtains the active `Arc<dyn MediaProvider>` via `require_provider()` and no UI or RPC handler constructs Jellyfin, Subsonic, or OpenSubsonic server URLs directly.
4. Given the provider/domain contract is extended, when daemon tests run, then existing Jellyfin/Subsonic browse, sync, auto-fill, scrobble, image proxy, and legacy `jellyfin_get_*` compatibility behavior remains unchanged.

## Tasks / Subtasks

- [ ] Add provider-neutral browse domain types (AC: 1, 2)
  - [ ] Update `hifimule-daemon/src/domain/models.rs` with a `Genre` struct using provider-neutral fields: `id: String`, `name: String`, `song_count: Option<u32>`, and `cover_art_id: Option<String>`.
  - [ ] Extend `Song` with optional browse metadata needed by history/favorites modes: `date_added: Option<String>`, `last_played_at: Option<String>`, `play_count: Option<u32>`, and `is_favorite: Option<bool>`.
  - [ ] Preserve existing `Song` field meanings and all current conversion tests; new optional fields must default to `None` for existing provider responses.
  - [ ] Add `BrowseMode` and `BrowseCapabilities` in `hifimule-daemon/src/providers/mod.rs`. Serialize RPC-facing mode values as the exact camelCase strings in AC #1.
  - [ ] Extend existing `Capabilities` without removing or renaming `open_subsonic`, `supports_changes_since`, or `supports_server_transcoding`.

- [ ] Extend `MediaProvider` with explicit browse capabilities and methods (AC: 1, 2, 3)
  - [ ] Add `fn browse_capabilities(&self) -> BrowseCapabilities` or nest `browse: BrowseCapabilities` inside `capabilities()`. Use one source of truth for `browse.listModes`.
  - [ ] Add explicit async methods rather than generic string dispatch:
    - `list_genres(library_id: Option<&str>) -> Result<Vec<Genre>, ProviderError>`
    - `get_genre_tracks(genre_id_or_name: &str, offset: u32, limit: u32) -> Result<Vec<Song>, ProviderError>`
    - `list_recently_added(library_id: Option<&str>, offset: u32, limit: u32) -> Result<Vec<Song>, ProviderError>`
    - `list_frequently_played(library_id: Option<&str>, offset: u32, limit: u32) -> Result<Vec<Song>, ProviderError>`
    - `list_recently_played(library_id: Option<&str>, offset: u32, limit: u32) -> Result<Vec<Song>, ProviderError>`
    - `list_favorites(library_id: Option<&str>, offset: u32, limit: u32) -> Result<Vec<Song>, ProviderError>`
  - [ ] Provide default trait implementations that return `ProviderError::UnsupportedCapability` only if doing so keeps implementors and tests focused; otherwise implement each method explicitly on both providers.
  - [ ] Keep all provider-specific API details inside `providers/jellyfin.rs`, `providers/subsonic.rs`, or named `JellyfinClient` helpers in `api.rs`. Do not build provider URLs in `rpc.rs` or TypeScript.

- [ ] Implement Jellyfin browse capability support (AC: 1, 3, 4)
  - [ ] Update `JellyfinProvider::capabilities()`/browse capabilities to expose modes only after the provider methods are implemented and tested.
  - [ ] Extend `JellyfinClient` with named methods for new query shapes instead of assembling ad hoc URLs in `rpc.rs`.
  - [ ] Use Jellyfin `/Items` queries through the existing authenticated client path for genre-filtered tracks, recently added, frequently played, recently played, and favorites.
  - [ ] Include fields needed for metadata mapping, especially `DateCreated` and user data fields such as favorite/play count/last played where available.
  - [ ] Map `JellyfinItem` into the extended `Song` fields without changing existing title, album, artist, duration, bitrate, or cover art behavior.

- [ ] Implement Subsonic/OpenSubsonic browse capability support (AC: 1, 2, 3, 4)
  - [ ] Add local `SubsonicClient` helpers for official Subsonic endpoints needed by the contract; reuse `signed_url`, sanitizer, envelope parsing, and existing DTO conversion style.
  - [ ] Use `getGenres` for `list_genres` and `getSongsByGenre` for `get_genre_tracks`.
  - [ ] Use only endpoints whose return shape can satisfy the requested contract. If classic Subsonic can return albums but not track-level results for a mode, keep that mode disabled until Story 9.4 defines the album-to-track behavior.
  - [ ] Reuse `getStarred2` for favorites if it can return songs directly; preserve the `coverArt != id` rule.
  - [ ] Ensure all new Subsonic request logging or errors use the existing sanitization behavior for `u`, `p`, `t`, and `s`.

- [ ] Add provider-neutral `browse.*` JSON-RPC methods (AC: 1, 2, 3, 4)
  - [ ] Add handler cases in `hifimule-daemon/src/rpc.rs` for:
    - `browse.listModes`
    - `browse.listArtists`
    - `browse.getArtist`
    - `browse.listAlbums`
    - `browse.getAlbum`
    - `browse.listPlaylists`
    - `browse.getPlaylist`
    - `browse.listGenres`
    - `browse.getGenre`
    - `browse.listRecentlyAdded`
    - `browse.listFrequentlyPlayed`
    - `browse.listRecentlyPlayed`
    - `browse.listFavorites`
  - [ ] Each handler must call `require_provider(state).await?`, clone the provider, release locks before awaits, and call a `MediaProvider` method.
  - [ ] Return provider-neutral camelCase response shapes from the architecture: `{ modes }`, `{ artists, total }`, `{ albums, total }`, `{ playlists }`, `{ genres, total }`, `{ tracks, total }`, and detail wrappers such as `{ artist, albums }`.
  - [ ] Keep existing `jellyfin_get_views`, `jellyfin_get_items`, `jellyfin_get_item_details`, `jellyfin_get_item_counts`, and `jellyfin_get_item_sizes` stable for the current UI.
  - [ ] Do not use `active_non_jellyfin_provider()` for new browse methods; that helper intentionally bypasses `JellyfinProvider` for legacy compatibility and would violate the new provider-neutral contract.

- [ ] Add focused tests and verification (AC: 1, 2, 3, 4)
  - [ ] Add pure tests for `BrowseMode` wire values and `BrowseCapabilities` -> mode list ordering.
  - [ ] Add provider tests for Jellyfin capability reporting and all implemented new methods using `mockito`.
  - [ ] Add provider tests for Subsonic/OpenSubsonic capability reporting and every implemented endpoint using `mockito`.
  - [ ] Add RPC tests using a fake or mock provider proving `browse.listModes` and at least one data method route through `Arc<dyn MediaProvider>` instead of `JellyfinClient`.
  - [ ] Add unsupported-mode tests proving hidden modes are omitted and direct requests map `UnsupportedCapability` cleanly.
  - [ ] Run `rtk cargo test -p hifimule-daemon browse --no-fail-fast`.
  - [ ] Run `rtk cargo test -p hifimule-daemon providers --no-fail-fast`.
  - [ ] Run `rtk cargo test -p hifimule-daemon`.

## Dev Notes

### Current Codebase State

- The daemon crate is `hifimule-daemon`; provider code lives under `hifimule-daemon/src/providers/`, shared domain models under `hifimule-daemon/src/domain/models.rs`, and JSON-RPC handlers under `hifimule-daemon/src/rpc.rs`.
- `MediaProvider` already exists with library/artist/album/playlist/search/download/artwork/change/scrobble methods, plus `server_type`, `server_version`, `access_token`, `provider_user_id`, and `capabilities`. It does not yet expose genres, browse modes, history/favorites lists, or browse metadata fields.
- `Capabilities` currently has only `open_subsonic`, `supports_changes_since`, and `supports_server_transcoding`. Existing provider tests assert this exact shape, so update tests deliberately when adding browse capability fields.
- `domain::models::Song` currently has ID/title/artist/album/duration/bitrate/track/disc/cover art only. `JellyfinItem` already parses `date_created` and `JellyfinUserData { is_favorite, play_count }`, but `song_from_item()` does not map those fields and `JellyfinUserData` does not yet include last-played metadata.
- Many tests and provider conversion helpers construct `Song` with direct struct literals. Adding optional fields will cause compile errors until every literal is updated with `None` or routed through a small helper; do this deliberately and keep conversion tests explicit.
- `SubsonicProvider` uses a hand-rolled local client. It already has `get_artists`, `get_artist`, `get_album_list2`, `get_album`, `get_playlists`, `get_playlist`, `search3_paged`, `get_indexes`, `download`, `stream`, `getCoverArt`, and `scrobble`. It does not yet have `getGenres`, `getSongsByGenre`, or `getStarred2` helpers.
- `rpc.rs` currently has legacy Jellyfin-shaped methods and an `active_non_jellyfin_provider()` helper. That helper returns `None` for Jellyfin so the old handlers can fall through to direct `JellyfinClient` behavior. New `browse.*` handlers must not use that helper.
- `require_provider()` already exists in `rpc.rs` and is the right entry point for provider-neutral browse RPCs.
- The UI currently calls `jellyfin_get_views` and `jellyfin_get_items` from `hifimule-ui/src/library.ts`; Story 9.2 owns visible browse-mode navigation. Story 9.1 may add TypeScript wrappers only if useful, but should not refactor the UI rendering flow.
- Architecture mentions `ts-rs`, but the current daemon Cargo manifests do not include a `ts-rs` dependency and current RPC contracts are manually shaped/tested. Do not turn Story 9.1 into a broad type-generation migration; if `ts-rs` is introduced, keep it narrowly justified and tested.

### Architecture Compliance

- The architecture requires all media server API calls to go through `Arc<dyn MediaProvider>` and provider modules. No RPC or UI layer should construct Jellyfin or Subsonic API URLs for new browse behavior.
- The architecture's provider-neutral RPC inventory already names `browse.listModes`, `browse.listGenres`, `browse.getGenre`, `browse.listRecentlyAdded`, `browse.listFrequentlyPlayed`, `browse.listRecentlyPlayed`, and `browse.listFavorites`.
- Keep response fields camelCase at the provider-neutral RPC boundary. Existing legacy Jellyfin-shaped responses use PascalCase and should remain unchanged for compatibility.
- For artwork, continue the existing rule: provider entities carry `cover_art_id`; the UI/RPC image proxy resolves the actual image. Do not teach the UI to call provider-specific artwork URLs.
- Use `String` for all provider IDs. Do not introduce numeric IDs for Subsonic/Navidrome compatibility.

### Story Boundaries

- In scope: provider/domain browse contract, capability reporting, provider method additions, provider-neutral `browse.*` RPC handlers, and tests proving unsupported modes are hidden/fail cleanly.
- In scope only if needed for contract tests: small TypeScript RPC type/wrapper additions. Visible UI navigation is Story 9.2.
- Out of scope: segmented browse-mode control, per-mode scroll/cache UI behavior, genre basket entity/sync resolution, favorites/history UX, sync playlist generation changes, and broad replacement of legacy `jellyfin_get_*` methods.
- Do not remove or rename current legacy RPC methods. Existing UI and tests still depend on them.
- Do not add `opensubsonic`, `wiremock`, or a Jellyfin SDK dependency for this story. The current project pattern is `reqwest`, provider-local DTOs, and `mockito`.

### Previous Work Intelligence

- Story 8.1 established provider-neutral domain models and `MediaProvider`; reuse those exact modules instead of adding parallel DTOs.
- Story 8.2 wrapped the existing `JellyfinClient` rather than introducing a Jellyfin SDK. Continue that low-regression approach: add named client helpers where new Jellyfin query shapes are required.
- Story 8.3 intentionally removed the unused `opensubsonic` crate and kept a local Subsonic client. Add new Subsonic methods to that client rather than reintroducing the dependency.
- Story 8.4 added `AppState.provider`, `server.connect`, persisted server config, and `require_provider()`. This story should build on that lifecycle.
- Story 8.5 hardened Subsonic URL/message sanitization. Reuse its sanitizer for any new Subsonic URL-bearing logs or errors.
- Story 8.6 added `changes_since_with_context` and manifest provider metadata. Do not parse provider-specific version strings in new browse RPCs; keep provider details at the provider boundary.
- The current `spec-fix-subsonic-playlist-browse.md` deliberately preserved the old Jellyfin-shaped browse path for playlists. Do not undo that fix while adding the new provider-neutral path.

### External Technical Context

- Jellyfin's generated SDK docs for `getItems` include filters for `genreIds`, `genres`, `includeItemTypes`, `fields`, `startIndex`, and `limit`; use named client helpers to send these through `JellyfinClient` rather than building URLs in RPC. [Source: https://typescript-sdk.jellyfin.org/interfaces/generated-client.ItemsApiGetItemsRequest.html]
- Jellyfin sort keys include `DateCreated`, `DatePlayed`, `PlayCount`, and `IsFavoriteOrLiked`, which are relevant for recently added, recently played, frequently played, and favorites-style ordering. [Source: https://typescript-sdk.jellyfin.org/variables/generated-client.ItemSortBy.html]
- The official Subsonic API documents `getGenres` and `getSongsByGenre`, plus ID3 browse methods `getArtists`, `getArtist`, and `getAlbum`. [Source: https://subsonic.org/pages/api.jsp]
- The official Subsonic API documents `getAlbumList2` list types including `newest`, `frequent`, `recent`, and `starred`, and `getStarred2` for ID3-style starred items. Only expose a mode when the provider implementation returns the track-level contract expected by the RPC method. [Source: https://subsonic.org/pages/api.jsp]
- OpenSubsonic mirrors the Subsonic endpoint families for browse, album/song lists, search, playlists, media retrieval, and media annotation. [Source: https://opensubsonic.netlify.app/docs/opensubsonic-api/]

### File Structure Requirements

- Update likely files:
  - `hifimule-daemon/src/domain/models.rs`
  - `hifimule-daemon/src/providers/mod.rs`
  - `hifimule-daemon/src/providers/jellyfin.rs`
  - `hifimule-daemon/src/providers/subsonic.rs`
  - `hifimule-daemon/src/api.rs`
  - `hifimule-daemon/src/rpc.rs`
- Optional UI type/wrapper-only files:
  - `hifimule-ui/src/rpc.ts`
  - `hifimule-ui/src/library.ts`
- Avoid touching sync/device files unless compiler fallout from domain type changes requires small, test-covered updates.

### Testing Guidance

- Use existing `mockito` tests for HTTP behavior and keep query assertions parameter-based rather than relying on query string ordering.
- Keep conversion tests local to provider modules where possible.
- Add at least one RPC test that would fail if a `browse.*` handler used the old direct `JellyfinClient` path.
- Re-run broad daemon tests because changes to `MediaProvider`, `Capabilities`, `Song`, and RPC dispatch can affect many modules.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story-9.1-Provider-Browse-Modes-and-Capability-Contract]
- [Source: _bmad-output/planning-artifacts/prd.md#Content-Selection--Browsing]
- [Source: _bmad-output/planning-artifacts/architecture.md#Library-Browsing--Multi-Provider-RPC-Contract]
- [Source: _bmad-output/planning-artifacts/architecture.md#Enforcement-Guidelines]
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#Component-Strategy]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-05-22.md#New-Story---9.1-Provider-Browse-Modes-and-Capability-Contract]
- [Source: _bmad-output/implementation-artifacts/8-1-mediaprovider-trait-and-domain-models.md]
- [Source: _bmad-output/implementation-artifacts/8-2-jellyfinprovider-adapter.md]
- [Source: _bmad-output/implementation-artifacts/8-3-subsonicprovider-adapter.md]
- [Source: _bmad-output/implementation-artifacts/8-4-runtime-server-type-detection-factory.md]
- [Source: _bmad-output/implementation-artifacts/8-5-subsonic-url-credential-sanitization.md]
- [Source: _bmad-output/implementation-artifacts/8-6-incremental-sync-subsonic-album-level-fallback.md]
- [Source: _bmad-output/implementation-artifacts/spec-fix-subsonic-playlist-browse.md]
- [Source: hifimule-daemon/src/domain/models.rs]
- [Source: hifimule-daemon/src/providers/mod.rs]
- [Source: hifimule-daemon/src/providers/jellyfin.rs]
- [Source: hifimule-daemon/src/providers/subsonic.rs]
- [Source: hifimule-daemon/src/api.rs]
- [Source: hifimule-daemon/src/rpc.rs]
- [Source: hifimule-ui/src/library.ts]
- [Source: hifimule-ui/src/components/MediaCard.ts]

## Dev Agent Record

### Agent Model Used

{{agent_model_name_version}}

### Debug Log References

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.

### File List
