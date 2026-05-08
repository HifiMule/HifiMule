# Story 8.1: MediaProvider Trait & Domain Models

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a System Admin (Alexis),
I want all server communication routed through a shared `MediaProvider` trait,
so that the sync engine, auto-fill, and scrobble bridge never depend on server-specific API details.

## Acceptance Criteria

1. Given `hifimule-daemon` is built, when provider-aware code needs to browse, download, search, scrobble, inspect capabilities, or query media server changes, then the contract exists on `Arc<dyn MediaProvider>` and is usable without direct server API coupling.
2. Given the `domain/models.rs` module is defined, when a provider returns library data, then it returns API-neutral domain types (`Library`, `Song`, `Album`, `Artist`, `Playlist`, `SearchResult`, `ChangeEvent`) with normalized fields:
   - IDs are always `String`, never integer types.
   - Duration is `u32` seconds.
   - Bitrate is `u32` kbps.
   - Cover art references are `Option<String>` and are separate from item IDs.
3. Given provider errors are surfaced to callers, when a provider operation fails, then failures are represented by a shared `ProviderError` with HTTP, auth, not found, unsupported capability, deserialization, and catch-all variants.
4. Given this is Story 8.1, when implementation is complete, then it creates the provider/domain foundation and compile-time tests without migrating existing Jellyfin callers; the full `api.rs` to `JellyfinProvider` adapter and caller migration belongs to Story 8.2.
5. Given the workspace builds, when `rtk cargo test -p hifimule-daemon` runs, then the new modules compile and unit tests prove the normalization traps that would break Navidrome/Subsonic support.

## Tasks / Subtasks

- [x] Add daemon domain model module (AC: 2)
  - [x] Create `hifimule-daemon/src/domain/mod.rs` and `hifimule-daemon/src/domain/models.rs`.
  - [x] Add `mod domain;` to `hifimule-daemon/src/main.rs` beside the existing top-level modules.
  - [x] Define API-neutral structs/enums: `Library`, `Song`, `Album`, `Artist`, `Playlist`, `ArtistWithAlbums`, `AlbumWithTracks`, `PlaylistWithTracks`, `SearchResult`, `ChangeEvent`, `ItemRef`, and `ItemType`.
  - [x] Use `String` for every server-originated identifier, including library, artist, album, song, playlist, and item refs.
  - [x] Include `cover_art_id: Option<String>` on visible media entities that can render artwork.

- [x] Add explicit unit newtypes for DTO boundary conversions (AC: 2, 5)
  - [x] Add small newtypes/helpers for `JellyfinTicks`, `Seconds`, `Bps`, and `Kbps` in the domain layer or a tightly adjacent conversion module.
  - [x] Implement conversions that make Jellyfin ticks-to-seconds and bps-to-kbps explicit.
  - [x] Add tests for tick conversion, seconds passthrough, bps-to-kbps conversion, kbps passthrough, string ID preservation, and cover art ID preservation.

- [x] Add provider contract module (AC: 1, 3)
  - [x] Create `hifimule-daemon/src/providers/mod.rs`.
  - [x] Add `mod providers;` to `hifimule-daemon/src/main.rs`.
  - [x] Define `MediaProvider` using `#[async_trait]` with `Send + Sync`.
  - [x] Define `ServerType`, `Capabilities`, `ProviderError`, `TranscodeProfile`, and any required provider-neutral credential/profile placeholders needed for the trait signature to compile.
  - [x] Include the full method surface from architecture: browse libraries/artists/albums/playlists, search, download URL, cover art URL, changes since, scrobble, server type, and capabilities.
  - [x] Return `url::Url` only if the crate is already present; otherwise use `String` URLs for this story and leave the `Url` crate decision for the adapter story to avoid adding an unplanned dependency.

- [x] Keep current Jellyfin behavior intact (AC: 4)
  - [x] Do not move `hifimule-daemon/src/api.rs` in this story.
  - [x] Do not update `rpc.rs`, `sync.rs`, `auto_fill.rs`, `main.rs` call sites from `JellyfinClient` to `Arc<dyn MediaProvider>` yet, except for adding module declarations.
  - [x] Do not introduce `opensubsonic` in this story; Story 8.3 owns the Subsonic adapter dependency.
  - [x] Do not introduce `jellyfin-sdk` in this story; Story 8.2 owns the Jellyfin adapter decision.

- [x] Verify build and tests (AC: 5)
  - [x] Run `rtk cargo test -p hifimule-daemon`.
  - [x] If compile errors appear in existing unrelated tests, document them in the story's Dev Agent Record instead of broadening this story's scope.

## Dev Notes

### Current Codebase State

- The daemon crate is `hifimule-daemon`, not the older `jellysync-daemon` name. Source files live under `hifimule-daemon/src/`. [Source: Cargo.toml; hifimule-daemon/Cargo.toml]
- Current Jellyfin-specific server API code is concentrated in `hifimule-daemon/src/api.rs`, with `JellyfinClient`, `JellyfinItem`, `JellyfinView`, `MediaSource`, `JellyfinUserData`, request construction, download/transcoding methods, and many mockito tests. Preserve it for Story 8.2. [Source: hifimule-daemon/src/api.rs]
- Current `rpc::AppState` still stores `jellyfin_client: JellyfinClient`; current `main.rs` creates `Arc<api::JellyfinClient>` for auto-sync. These are known Jellyfin-specific call sites, but Story 8.1 should only lay the trait/model foundation. [Source: hifimule-daemon/src/rpc.rs; hifimule-daemon/src/main.rs]
- `async-trait = "0.1"` already exists in `hifimule-daemon/Cargo.toml`; do not add a duplicate dependency line. [Source: hifimule-daemon/Cargo.toml]
- Workspace dependency versions currently include `tokio ~1.49`, `reqwest ~0.12`, `serde ~1.0`, `serde_json ~1.0`, `thiserror ~2.0`, and Rust `1.93.0`. Keep new code compatible with the existing workspace. [Source: Cargo.toml]

### Architecture Compliance

- All future server communication must be mediated through `MediaProvider`; the daemon should eventually hold `Arc<dyn MediaProvider>` resolved at connect time. [Source: _bmad-output/planning-artifacts/architecture.md#Media-Provider-Layer]
- Architecture method surface:
  - `list_libraries`
  - `list_artists`
  - `get_artist`
  - `get_album`
  - `search`
  - `download_url`
  - `cover_art_url`
  - `list_playlists`
  - `get_playlist`
  - `changes_since`
  - `scrobble`
  - `server_type`
  - `capabilities`
- Domain models must be independent of server DTOs. DTO-to-domain conversion happens at provider adapter boundaries, not in UI, sync, or RPC code. [Source: _bmad-output/planning-artifacts/architecture.md#Media-Provider-Layer]
- Normalization rules are non-negotiable: IDs are `String`; Jellyfin `runTimeTicks` becomes seconds; Jellyfin bitrates in bps become kbps; Subsonic `coverArt` stays a separate cover art ref and must not be assumed equal to the song ID. [Source: _bmad-output/planning-artifacts/architecture.md#Media-Provider-Layer]
- `ProviderError` should include at least HTTP status/message, auth failure, not found, unsupported capability, deserialization, and `Other(#[from] anyhow::Error)`. [Source: _bmad-output/planning-artifacts/architecture.md#ProviderError]
- `Capabilities` should include `open_subsonic`, `supports_changes_since`, and `supports_server_transcoding`. Architecture later expects providers to cache capabilities and reset on replacement. [Source: _bmad-output/planning-artifacts/architecture.md#Capabilities]

### Story Boundaries

- This story intentionally does not satisfy the final system-wide "no direct HTTP outside providers" end state by itself. Existing direct Jellyfin HTTP calls remain until Story 8.2 wraps/moves `api.rs` into `providers/jellyfin.rs` and migrates callers.
- Do not add Subsonic URL sanitization implementation here. The architecture mandates it, but Story 8.5 owns the hardening. The trait should leave room for provider-owned URL construction so future sanitization is enforceable.
- Do not add runtime server detection or `connect()` factory implementation here. Story 8.4 owns `ServerTypeHint`, `connect(url, creds, hint)`, `AppState.provider`, and `require_provider`.
- Do not alter UI or Tauri code. Epic 8.1 is daemon domain/trait groundwork only.

### Latest Technical Context

- `async-trait` remains required for `dyn MediaProvider` because native async functions in traits do not make async methods dyn-compatible by themselves. Current docs list `async-trait` latest as `0.1.89`; the crate dependency can stay as `"0.1"` unless the lockfile or workspace policy requires an exact version. [Source: https://docs.rs/async-trait]
- `opensubsonic` docs currently show `0.3.0` as the latest docs.rs version and describe a complete async OpenSubsonic/Subsonic client supporting Subsonic API v1.16.1 and OpenSubsonic extensions. Do not add it in 8.1; capture this only as context for Story 8.3. [Source: https://docs.rs/opensubsonic]

### Testing Guidance

- Add focused unit tests in the new module(s). The highest-value tests are conversion and shape tests, not HTTP tests.
- Test that Navidrome-style IDs such as MD5 strings remain `String` unchanged.
- Test that Jellyfin ticks convert using `ticks / 10_000_000` and do not overflow for realistic audio durations.
- Test that Subsonic-style durations and kbps values pass through unchanged.
- Test that `cover_art_id` can differ from `id` and remains preserved.
- Run `rtk cargo test -p hifimule-daemon` before handing off.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story-8.1-MediaProvider-Trait-Domain-Models]
- [Source: _bmad-output/planning-artifacts/architecture.md#Media-Provider-Layer]
- [Source: _bmad-output/planning-artifacts/architecture.md#ProviderError]
- [Source: _bmad-output/planning-artifacts/architecture.md#Library-Browsing-Multi-Provider-RPC-Contract]
- [Source: _bmad-output/planning-artifacts/research/technical-compare-jellyfin-navidrome-subsonic-opensubsonic-api-research-2026-05-08.md#Async-Architecture]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-05-08.md#Technical-Impact]

## Dev Agent Record

### Agent Model Used

GPT-5 Codex

### Debug Log References

- `rtk cargo test -p hifimule-daemon` - passed, 209 tests.

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- Added API-neutral daemon domain models with `String` server IDs, seconds/kbps units, and separate optional cover art references.
- Added explicit DTO boundary conversion helpers for Jellyfin ticks, seconds, bps, and kbps with focused unit tests for normalization traps.
- Added the `MediaProvider` async trait surface, provider capability/server metadata, scrobble placeholders, transcode profile placeholder, and shared `ProviderError`.
- Preserved existing Jellyfin API/call-site behavior; only top-level module declarations were added in `main.rs`.

### File List

- `hifimule-daemon/src/main.rs`
- `hifimule-daemon/src/domain/mod.rs`
- `hifimule-daemon/src/domain/models.rs`
- `hifimule-daemon/src/providers/mod.rs`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `_bmad-output/implementation-artifacts/8-1-mediaprovider-trait-and-domain-models.md`

### Change Log

- 2026-05-08: Implemented MediaProvider trait/domain model foundation and moved story to review.
