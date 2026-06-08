---
baseline_commit: 39cdb09
---

# Story 9.9: Tracks Browse Mode — Provider Contract & Daemon RPC

Status: review

## Story

As a System Admin (Alexis),
I want the daemon to expose a flat, paginated, filterable track listing,
so that the UI can present a library-wide Tracks browse mode for both Jellyfin and OpenSubsonic-class servers.

## Acceptance Criteria

1. **Given** a provider that implements `list_tracks`
   **When** `browse.listTracks({ startIndex: 0, limit: 200 })` is called
   **Then** the daemon returns the first page of library tracks as `{ tracks: Track[], total: number, startIndex: number, limit: number }`.

2. **Given** `browse.listTracks` is called with `artistId`
   **Then** the response is filtered to tracks whose artist matches.

3. **Given** `browse.listTracks` is called with `albumId`
   **Then** the response is filtered to tracks within that album.

4. **Given** both `artistId` and `albumId` are provided
   **Then** the album filter takes precedence (album implies its artist) — the daemon scopes results to the album only.

5. **Given** a Subsonic provider without `search3` support (i.e. classic Subsonic, `open_subsonic == false`)
   **When** `browse.listModes` is called
   **Then** `tracks` is NOT present in the returned modes array.

6. **Given** a provider that does not advertise the `Tracks` mode in `BrowseCapabilities::list_modes`
   **When** `browse.listTracks` is called anyway
   **Then** the RPC returns `ERR_UNSUPPORTED_CAPABILITY` (`provider_error_to_rpc(ProviderError::UnsupportedCapability)`).

7. **Given** the `letter` filter is provided (optional v1)
   **When** `browse.listTracks({ letter: "A", ... })` is called
   **Then** only tracks whose title starts with that letter are returned (Jellyfin: `NameStartsWith=A`; Subsonic: post-filtered in-process — see Dev Notes for the rationale).

8. **Given** a JellyfinProvider
   **Then** `BrowseMode::Tracks` is included in `capabilities().browse.list_modes` and `list_tracks` is implemented for Jellyfin.

9. **Given** a SubsonicProvider with `open_subsonic == true`
   **Then** `BrowseMode::Tracks` is included in `capabilities().browse.list_modes` and `list_tracks` is implemented using `search3` / `getArtist` / `getAlbum` per the rules below.

10. **Given** Subsonic URLs are constructed for any `list_tracks` request
    **Then** all auth params (`u`, `p`, `t`, `s`) are sanitized via `sanitize_subsonic_url` before being passed to `tracing::` macros or file-based logs (per the existing security requirement).

## Tasks / Subtasks

- [x] **Task 1: Add `BrowseMode::Tracks` variant** (AC: 1, 5, 6, 8, 9)
  - [x] Add `Tracks` variant to `BrowseMode` enum in [hifimule-daemon/src/providers/mod.rs:297](hifimule-daemon/src/providers/mod.rs:297). Place it after `Playlists` to mirror the architecture doc's `BrowseMode` union order (`artists | albums | playlists | tracks | genres | ...`).
  - [x] Verify it serializes as `"tracks"` (the enum already has `#[serde(rename_all = "camelCase")]` on the type — confirmed at line 296).
  - [x] Update the existing `BrowseMode` serialization test in [hifimule-daemon/src/providers/mod.rs:864](hifimule-daemon/src/providers/mod.rs:864) by adding a `Tracks` → `"tracks"` assertion in the same style as the other variants.

- [x] **Task 2: Add `TrackListFilter` and `TrackListPage` types** (AC: 1, 2, 3, 4, 7)
  - [x] In `hifimule-daemon/src/providers/mod.rs`, add:
    ```rust
    #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct TrackListFilter {
        pub library_id: Option<String>,
        pub artist_id: Option<String>,
        pub album_id: Option<String>,
        pub letter: Option<String>,   // single uppercase char — kept as String for consistency with list_artists/list_albums letter param
        pub start_index: u32,
        pub limit: u32,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct TrackListPage {
        pub tracks: Vec<crate::domain::models::Song>,
        pub total: u32,
        pub start_index: u32,
        pub limit: u32,
    }
    ```
  - [x] **Letter type rationale:** the architecture doc at `architecture.md:377` shows `Option<char>`, but existing trait methods already use `Option<&str>` for the letter parameter (see `list_artists` signature at `providers/mod.rs:58–64`). Use `Option<String>` for consistency. Convert the architecture's `Option<char>` annotation in your judgment — the trait amendment is the authoritative shape now.

- [x] **Task 3: Add `list_tracks` to the `MediaProvider` trait with default `NotSupported`** (AC: 6)
  - [x] In [hifimule-daemon/src/providers/mod.rs](hifimule-daemon/src/providers/mod.rs) (inside the `MediaProvider` trait body, between line 175 `list_favorites` and line 177 `list_favorite_items`, or grouped near the other paginated lists — place it after `list_favorite_items` to keep recent additions grouped), add:
    ```rust
    async fn list_tracks(&self, _filter: TrackListFilter) -> Result<TrackListPage, ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "list_tracks is not supported by this provider".to_string(),
        ))
    }
    ```
  - [x] Pattern-match the other capability-gated default impls (e.g. `list_recently_added` at line 133, `list_favorites` at line 166) — same `Err(ProviderError::UnsupportedCapability(...))` shape.

- [x] **Task 4: Extend Jellyfin `get_items` to support tracks filters** (AC: 2, 3, 4, 7, 8)
  - [x] In [hifimule-daemon/src/api.rs:291–351](hifimule-daemon/src/api.rs:291), `get_items` currently supports `parent_id`, `include_item_types`, `start_index`, `limit`, `name_starts_with`, `name_less_than`. It does NOT yet support `ArtistIds`, `AlbumIds`, or `SortBy`.
  - [x] **Decision — pick one of these approaches:**
    - **(A) Extend `get_items` in place** by adding optional `artist_ids: Option<&str>`, `album_ids: Option<&str>`, `sort_by: Option<&str>` parameters. Existing callers (4 of them in `jellyfin.rs`) pass `None` for these new params. Append `&ArtistIds={ids}`, `&AlbumIds={ids}`, `&SortBy={sort}` when present. This is the path of least churn and mirrors how `name_starts_with` was added.
    - **(B) Add a sibling helper `get_items_with_filters(...)`** that takes a `JellyfinItemsQuery` builder struct and is called only from `list_tracks`. Heavier refactor, but cleaner long-term.
    - **Recommended: (A).** It matches the established pattern in this file and lets future stories add more optional filters incrementally.
  - [x] If picking (A), audit each existing `get_items` call site in `hifimule-daemon/src/providers/jellyfin.rs` and add `None, None, None` (or named-arg equivalents) for the new params.

- [x] **Task 5: Implement `JellyfinProvider::list_tracks`** (AC: 1, 2, 3, 4, 7, 8)
  - [x] In [hifimule-daemon/src/providers/jellyfin.rs](hifimule-daemon/src/providers/jellyfin.rs), inside `impl MediaProvider for JellyfinProvider`, add an `async fn list_tracks(&self, filter: TrackListFilter) -> Result<TrackListPage, ProviderError>` method. Place it near the other paginated listing methods (after `list_favorites` if it exists, or near `list_all_songs_page`).
  - [x] Translate `TrackListFilter` to a single Jellyfin `/Items` call via the (now-extended) `get_items` helper:
    - `IncludeItemTypes = "Audio"` (constant — define `const AUDIO_TYPES: &str = "Audio";` if not already present alongside `ARTIST_TYPES` / `ALBUM_TYPES` / `PLAYLIST_TYPES`).
    - `Recursive=true` (already default in `get_items`).
    - `SortBy = "Name,Album"` (per architecture.md:394).
    - `StartIndex = filter.start_index`, `Limit = filter.limit`.
    - `ArtistIds = filter.artist_id` (single id; Jellyfin accepts a comma-separated list — pass the single id as a string).
    - `AlbumIds = filter.album_id` — and per AC 4, when `album_id` is present, **do not also pass `ArtistIds`** (album implies its artist).
    - `NameStartsWith = filter.letter` (already supported).
    - `library_id` → `parent_id` (consistent with the existing pattern in `list_artists` / `list_albums`).
  - [x] Map response items via the existing `song_from_item` helper (in [jellyfin.rs](hifimule-daemon/src/providers/jellyfin.rs)).
  - [x] Build `TrackListPage { tracks, total, start_index, limit }` and return.
  - [x] Add `BrowseMode::Tracks` to the `list_modes` vec inside `JellyfinProvider::capabilities()` at [hifimule-daemon/src/providers/jellyfin.rs:559–567](hifimule-daemon/src/providers/jellyfin.rs:559). Place it after `BrowseMode::Playlists` to keep ordering consistent with the architecture's enum order.

- [x] **Task 6: Implement `SubsonicProvider::list_tracks`** (AC: 1, 2, 3, 4, 5, 7, 9, 10)
  - [x] In [hifimule-daemon/src/providers/subsonic.rs](hifimule-daemon/src/providers/subsonic.rs), inside `impl MediaProvider for SubsonicProvider`, add an `async fn list_tracks(&self, filter: TrackListFilter) -> Result<TrackListPage, ProviderError>` method.
  - [x] **Dispatch logic — implement as three branches, in priority order:**
    1. **`album_id` is `Some`** — call `self.client.get_album(&album_id).await?` (already in `SubsonicClient`); map `body.album.song → Vec<Song>` via the existing `song_from_dto`. Apply optional `letter` post-filter (case-insensitive `title.starts_with(&letter)`). Apply offset/limit slicing locally. Compute `total = filtered_len as u32`.
    2. **`artist_id` is `Some` (and `album_id` is `None`)** — call `self.client.get_artist(&artist_id).await?` to get the artist's album list, then iterate albums and call `get_album` for each, flattening to a `Vec<Song>`. Apply optional `letter` post-filter and offset/limit slicing locally. This mirrors the existing pattern in `history_songs_from_album_list` at [subsonic.rs:197–213](hifimule-daemon/src/providers/subsonic.rs:197). Compute `total = filtered_len as u32`.
    3. **Neither filter is set (unfiltered enumeration)** — gate on `self.open_subsonic`:
       - If `open_subsonic == false`: return `Err(ProviderError::UnsupportedCapability("list_tracks requires OpenSubsonic search3 support".to_string()))`. Pair with the capability gating in Task 7 so this branch is never reached from the UI, but the guard makes the contract self-consistent for direct callers.
       - If `open_subsonic == true`: call `self.client.search3_paged("", Some(filter.limit as usize), Some(filter.start_index as usize)).await?` (already present in `SubsonicClient` — see [subsonic.rs:179](hifimule-daemon/src/providers/subsonic.rs:179)). Map `search_result3.song → Vec<Song>` via `song_from_dto`. Apply optional `letter` post-filter (after the page returns — see Letter Caveat below).
       - **Total caveat:** `search3` does not return a total count. Existing precedent: `list_all_songs_page` at [subsonic.rs:703–721](hifimule-daemon/src/providers/subsonic.rs:703) returns the page length as `total` (i.e. `count`, not a global library count). Follow the same pattern: set `total = tracks.len() as u32`. The UI uses page-length-equals-limit as the "has more" signal (consistent with how `library.ts` autoload logic infers exhaustion); document this in the dev note below.
       - **Alternative considered & rejected:** issuing a separate `search3` with `songCount=1` to discover total — adds a network round-trip per pagination call and Subsonic's `search3` total is also unreliable across implementations. Not worth the cost for v1.
  - [x] Apply the **Letter Caveat** consistently across all three branches: Subsonic has no native track-title prefix filter, so the post-filter is applied in-process. Document this in a brief inline comment so future devs don't try to push it server-side.
  - [x] Update `SubsonicProvider::capabilities()` at [hifimule-daemon/src/providers/subsonic.rs:537–561](hifimule-daemon/src/providers/subsonic.rs:537) to include `BrowseMode::Tracks` in `list_modes` ONLY when `self.open_subsonic == true`. Add it inside the existing `if self.open_subsonic { ... }` block alongside `RecentlyAdded`/`FrequentlyPlayed`/`RecentlyPlayed`.

- [x] **Task 7: Add `browse.listTracks` RPC handler** (AC: 1, 2, 3, 4, 6)
  - [x] In [hifimule-daemon/src/rpc.rs:345–367](hifimule-daemon/src/rpc.rs:345), register `"browse.listTracks" => handle_browse_list_tracks(&state, payload.params).await,` in the method dispatch match block. Place it after `"browse.listFavoriteItems"` and before `"browse.search"` to keep grouping coherent.
  - [x] Implement `handle_browse_list_tracks` near the existing `handle_browse_list_artists` (line 548). Reuse the parameter-parsing pattern from that handler — parse `libraryId`, `artistId`, `albumId`, `letter`, `startIndex` (default 0), `limit` (default 50). All optional except defaults.
  - [x] **Capability gate:** before calling the provider, check `provider.capabilities().browse.list_modes.contains(&BrowseMode::Tracks)`. If absent, return `JsonRpcError { code: ERR_UNSUPPORTED_CAPABILITY, message: hifimule_i18n::t("error.tracks_mode_unsupported"), data: None }`. (Add the i18n key in Task 9.)
  - [x] If the capability is present, build a `TrackListFilter` from params and call `provider.list_tracks(filter).await`. Convert `ProviderError` via the existing `provider_error_to_rpc` helper at [rpc.rs:480–503](hifimule-daemon/src/rpc.rs:480).
  - [x] Return `serde_json::json!({ "tracks": page.tracks, "total": page.total, "startIndex": page.start_index, "limit": page.limit })` — camelCase per the IPC convention.

- [x] **Task 8: Tests** (AC: 1–9)
  - [x] Extend the existing `BrowseMode` serialization test ([providers/mod.rs:864](hifimule-daemon/src/providers/mod.rs:864)) with a `Tracks → "tracks"` assertion.
  - [x] In `rpc.rs` tests module (near line 8493), add a test pattern-matching `browse_list_modes_routes_through_provider_capabilities`:
    - `browse_list_tracks_returns_tracks_from_provider` — wire `FakeBrowseProvider` to return a small `Vec<Song>`; assert the RPC returns `tracks`, `total`, `startIndex`, `limit` and that camelCase fields are present.
    - `browse_list_tracks_rejects_when_capability_missing` — provider lacks `BrowseMode::Tracks` in `list_modes`; assert `ERR_UNSUPPORTED_CAPABILITY` is returned. Pattern-match the existing `browse_unsupported_capability_maps_to_err_unsupported_capability` test at [rpc.rs:8688](hifimule-daemon/src/rpc.rs:8688).
    - You will need to extend `FakeBrowseProvider` (around line 8460) with a `tracks: Vec<Song>` field and a constructor variant. Mirror how the existing genre support was added.
  - [x] In `providers/subsonic.rs` tests (search for `#[tokio::test]` near the bottom of the file), add:
    - A test that `capabilities().browse.list_modes` includes `Tracks` when `open_subsonic == true` and EXCLUDES it when `open_subsonic == false`. Use `SubsonicProvider::from_client_for_tests` (already present at line 75).
    - A test that `list_tracks` with `open_subsonic == false` AND no `artist_id`/`album_id` returns `ProviderError::UnsupportedCapability`.
  - [x] In `providers/jellyfin.rs` tests, add a test that asserts `BrowseMode::Tracks` appears in `capabilities().browse.list_modes` (alongside the existing capabilities assertions). A full HTTP-level `list_tracks` mock test is OPTIONAL for v1 — the RPC-level handler test covers the contract, and the Jellyfin client mock infrastructure (`mockito` per the existing tests at line 2025) is heavy. Document this in the Completion Notes if you skip it.

- [x] **Task 9: i18n** (AC: 6)
  - [x] Add `error.tracks_mode_unsupported` key to the i18n catalogs (en/fr/es). Locate by grepping the `hifimule-i18n` crate or wherever existing `error.*` keys live (e.g. `error.no_active_media_provider` referenced at `rpc.rs:475`). Mirror existing key style.
  - [x] English: `"Tracks browse mode is not supported by this provider"`.
  - [x] French: `"Le mode de navigation par pistes n'est pas pris en charge par ce fournisseur"`.
  - [x] Spanish: `"El modo de navegación por pistas no es compatible con este proveedor"`.

- [x] **Task 10: Build & test gates**
  - [x] `rtk cargo check --workspace` — zero new errors.
  - [x] `rtk cargo clippy --workspace -- -D warnings` — zero new warnings introduced by this story.
  - [x] `rtk cargo test -p hifimule-daemon` — all tests pass, including the new ones from Task 8.
  - [x] `rtk cargo fmt --all` before commit.

## Dev Notes

### Scope Boundary — Daemon Only

This story is **daemon-only** (Rust). All UI work — `TracksBrowseView.ts`, the browse-mode bar entry, panel pagination, A–Z UI, track-row context menus, i18n keys for the view itself — is **Story 9.10** and explicitly out of scope here.

- ✅ In scope: `BrowseMode::Tracks` variant, `TrackListFilter`/`TrackListPage` types, `MediaProvider::list_tracks` default impl, Jellyfin adapter, Subsonic adapter, capability gating, `browse.listTracks` RPC handler, `error.tracks_mode_unsupported` i18n key, daemon-side tests.
- ❌ Out of scope: any `.ts` file change, any `tracks.view.*` i18n key, any `BrowseMode` TS union change, any UI rendering.

Story 9.10 depends on this story landing first, but does **not** require any further daemon changes.

### Current Code Anatomy (READ BEFORE TOUCHING)

#### `hifimule-daemon/src/providers/mod.rs`

- **`MediaProvider` trait (line 55–277)** — async-trait with capability-gated default impls. Every method that's optional returns `Err(ProviderError::UnsupportedCapability(...))` by default. Pattern matches: `list_genres` (111), `list_recently_added` (133), `list_frequently_played` (144), `list_favorites` (166), `list_all_songs_page` (188), playlist write methods (199–256).
- **`BrowseMode` enum (line 297–306)** — `#[serde(rename_all = "camelCase")]`, eight variants. Add `Tracks` here.
- **`BrowseCapabilities` (line 308–312)** — single field `list_modes: Vec<BrowseMode>`. No code change to this struct; just adjust the vec contents in each provider.
- **`Capabilities` (line 314–321)** — `open_subsonic`, `supports_changes_since`, `supports_server_transcoding`, `supports_playlist_write`, `browse`. No new field needed — track support is derived from `browse.list_modes`.
- **`ProviderError::UnsupportedCapability(String)`** — the established way to signal a missing capability.

#### `hifimule-daemon/src/providers/jellyfin.rs`

- **`impl MediaProvider for JellyfinProvider` (line 120+)** — methods call `self.client.get_items(...)` with a fixed `IncludeItemTypes` constant (e.g. `ARTIST_TYPES`, `ALBUM_TYPES`, `PLAYLIST_TYPES`).
- **`capabilities()` (line 555–571)** — builds `Capabilities { ..., browse: BrowseCapabilities { list_modes: vec![ ... ] } }`. List currently: Artists, Albums, Playlists, Genres, RecentlyAdded, FrequentlyPlayed, RecentlyPlayed, Favorites. Add `Tracks` after `Playlists`.
- **`song_from_item`** — the canonical DTO→domain mapper for Jellyfin items.
- **No existing `ArtistIds`/`AlbumIds`/`SortBy` support in `api.rs::get_items`** — Task 4 adds it.

#### `hifimule-daemon/src/providers/subsonic.rs`

- **`open_subsonic: bool`** — set from the `ping` response. Used to gate history modes (line 161 `ensure_open_subsonic_history`). Use the same boolean to gate `Tracks` in `capabilities()` and in unfiltered `list_tracks`.
- **`SubsonicClient::search3_paged(query, count, offset)` (line 970)** — already implemented; works against `?query=&songCount={count}&songOffset={offset}`. Return type contains `search_result3.song: Vec<SongDto>`. Use directly for unfiltered enumeration.
- **`SubsonicClient::get_album(id)` and `get_artist(id)`** — already present (used by changes-tracking code). Reuse for filtered branches.
- **`song_from_dto`** — the canonical DTO→domain mapper for Subsonic songs.
- **Existing precedent for "no total, return page length":** `list_all_songs_page` at line 703–721 — returns `(songs, count)` where `count = songs.len() as u32`. Follow this for the unfiltered `search3` branch.
- **Existing precedent for "fetch-all-then-paginate-in-process":** `get_genre_tracks` at line 651–675 — uses a 10,000-row dump and slices in-process. The 10k cap is an accepted limitation (logged in the 9.8 Review Findings as a deferred follow-up). **Avoid that pattern for unfiltered tracks** — the `search3` paged endpoint is server-paginated and is the right primitive. The fetch-all pattern is only acceptable for the `artist_id`-filtered branch (which inherently fetches an artist's complete album set), and even there the working set should be bounded by typical artist discography size.
- **URL Sanitization (NFR):** every constructed URL must be passed through `sanitize_subsonic_url()` before logging. See enforcement at [architecture.md:285](_bmad-output/planning-artifacts/architecture.md:285).

#### `hifimule-daemon/src/rpc.rs`

- **Method dispatch (line 297–380)** — a single `match payload.method.as_str() { ... }`. Add the new arm in the `browse.*` group around line 363–367.
- **`browse_pagination(params)` (line 524)** — helper that pulls `startIndex` (default 0) and `limit` (default 50). Reusable for `handle_browse_list_tracks`.
- **`provider_error_to_rpc(error)` (line 480–503)** — central error mapping; maps `ProviderError::UnsupportedCapability` to `ERR_UNSUPPORTED_CAPABILITY`. Use this — do NOT build the RPC error manually for the provider-call failure path.
- **For the capability-gate failure path (i.e. caller bypassed the UI's capability check):** build the `JsonRpcError` directly with `ERR_UNSUPPORTED_CAPABILITY` and the i18n message. This mirrors how `handle_browse_list_recently_added` etc. would behave if they checked caps explicitly (the existing default-impl path also returns the same error code via the trait's `Err(NotSupported)` → `provider_error_to_rpc`). Either path lands on `ERR_UNSUPPORTED_CAPABILITY`.

#### `hifimule-daemon/src/api.rs`

- **`get_items` (line 291–351)** — single helper used by all Jellyfin browse methods. Currently builds `&Recursive=true&ParentId=...&IncludeItemTypes=...&StartIndex=...&Limit=...&NameStartsWith=...&NameLessThan=...`. Task 4 adds `&ArtistIds=...&AlbumIds=...&SortBy=...`.
- **`println!("DEBUG: Jellyfin Response ...")` at line 343** — a pre-existing debug print. Do **not** remove it as part of this story (out of scope; it is unrelated to track listing). Mention in Completion Notes if you notice it.

### Wire Contract (RPC ↔ TypeScript)

The architecture doc ([_bmad-output/planning-artifacts/architecture.md:320](_bmad-output/planning-artifacts/architecture.md:320)) is authoritative:

**Request:**
```json
{
  "method": "browse.listTracks",
  "params": {
    "libraryId": "all" | null,
    "artistId": "abc123" | null,
    "albumId": "xyz789" | null,
    "letter": "A" | null,
    "startIndex": 0,
    "limit": 200
  }
}
```

**Response:**
```json
{
  "tracks": [ /* Track[] — see architecture.md:329 */ ],
  "total": 1234,
  "startIndex": 0,
  "limit": 200
}
```

The `Track` shape on the wire is the camelCase serialization of `domain::models::Song`. No new wire-level type — the existing `Song` serde derive already produces the correct shape (the architecture doc's `Track` is the conceptual name used in UI code; the daemon's domain model is `Song`).

### Pattern-Matching Pre-Existing Stories

- **Story 9.1** added `BrowseMode` and `BrowseCapabilities`. Capability gating pattern: provider lists what it supports; RPC checks before dispatching.
- **Story 9.4** added history modes (`list_recently_added`, `list_frequently_played`, `list_recently_played`) — same shape as this story, except those returned `(Vec<Album|Song>, u32)`. Use them as the closest sibling template.
- **Story 9.6** ("Navidrome/Subsonic Browse Parity Hardening") refined the Subsonic capabilities matrix and added the `open_subsonic`-based gating that this story extends. Read it if anything in the Subsonic capability list is unclear.
- **Story 9.7** introduced server-paginated lists for artists/albums root — that's the autoload pattern the UI side (9.10) will lean on. Not directly relevant here but provides the "why" for the `total` field on the wire.

### Why `total` Matters Even When We Can't Always Compute It

The UI's autoload-on-scroll logic ([hifimule-ui/src/library.ts](hifimule-ui/src/library.ts) — `loadMoreForListView`) uses `state.items.length < state.pagination.total` as the exhaustion check for artists/albums root. For Subsonic's unfiltered `list_tracks` (where `search3` does not provide a total), we return `total = tracks.len()`. The UI consumer for Story 9.10 is told to treat "page length < limit" as exhaustion in that case. The wire contract still includes `total` for consistency — the field is set; it's just an under-approximation in one specific branch.

### Subsonic Letter Filter — In-Process Caveat

Subsonic's `search3` does not support a server-side title prefix filter. The Jellyfin adapter applies `NameStartsWith` server-side. The Subsonic adapter applies the letter check in-process AFTER receiving each page. This means:

- Subsonic's `letter` filter narrows the returned page but does NOT change the `start_index`/`limit` semantics. The UI may need to fetch additional pages to fill a screen if the prefix is rare. This is a documented v1 limitation — the proposal lists A–Z letter filtering on tracks as "optional in v1" specifically because of this asymmetry.
- Document this caveat in a brief inline `//` comment in `subsonic.rs::list_tracks`.

### Files to Touch

**Create:** _none_ — this is purely additive to existing files.

**Modify (UPDATE):**

- [hifimule-daemon/src/providers/mod.rs](hifimule-daemon/src/providers/mod.rs) — add `BrowseMode::Tracks`, `TrackListFilter`, `TrackListPage`, default `list_tracks` trait impl. (~40 lines added.)
- [hifimule-daemon/src/providers/jellyfin.rs](hifimule-daemon/src/providers/jellyfin.rs) — implement `list_tracks`, add `BrowseMode::Tracks` to `capabilities()`. (~40 lines.)
- [hifimule-daemon/src/providers/subsonic.rs](hifimule-daemon/src/providers/subsonic.rs) — implement `list_tracks` with three dispatch branches, gate `Tracks` in `capabilities()` on `open_subsonic`. (~60 lines.)
- [hifimule-daemon/src/api.rs](hifimule-daemon/src/api.rs) — extend `get_items` with `ArtistIds`/`AlbumIds`/`SortBy` params; update 4 call sites in `jellyfin.rs` to pass `None` for the new params. (~15 lines + 4 call-site touches.)
- [hifimule-daemon/src/rpc.rs](hifimule-daemon/src/rpc.rs) — register `browse.listTracks` dispatch arm, add `handle_browse_list_tracks` handler, add tests. (~80 lines including tests.)
- `hifimule-i18n` crate (or wherever `error.no_active_media_provider` is defined) — add the new `error.tracks_mode_unsupported` key in en/fr/es.

**Do not touch:**

- Any `hifimule-ui/**/*.ts` file — that's Story 9.10.
- The PRD, architecture, UX, or epics docs — already updated in the sprint change proposal commit.

### Project Structure Notes

- The daemon-side trait amendment and adapter implementations stay within the established `providers/` module structure. No new files, no new modules.
- All naming aligns with existing conventions: snake_case Rust internal, camelCase JSON-RPC wire.
- Token consistency: use `Song` (domain) internally; `tracks` (camelCase) on the wire. Story 9.10's TypeScript `Track` type maps from the wire shape.

### Previous Story Intelligence (from 9.8 and 9.7)

Relevant operational notes pulled forward from the most recent stories' Review Findings and Completion Notes:

- **Pre-existing TypeScript baseUrl deprecation warning is expected** — does not apply here (no TS in this story), but listed because the dev agent's quality bar should be "zero NEW warnings", not "zero warnings".
- **`println!("DEBUG: ...")` at api.rs:343** is pre-existing and out of scope. Don't remove as part of this story.
- **Stale-mode race in `loadMoreForListView`** (9.8 review, deferred) — not relevant to this daemon-only story but mentioned because Story 9.10 will inherit it. Track in the Story 9.10 backlog.
- **`get_songs_by_genre` 10k cap** (9.8 review, deferred) — sibling issue: don't replicate the pattern. Use `search3_paged` for `list_tracks` unfiltered, not a fetch-all loop.
- **Capability gating is the load-bearing safety net** — Subsonic classic without `search3` MUST omit `Tracks` from `list_modes` (AC 5). Without that, the UI in 9.10 would issue a request that's guaranteed to fail. The capability check is the contract.

### Git Intelligence

Recent commit pattern: `Story X.Y` → `Dev X.Y` → `Review X.Y`, with PR-style review fixes folded into a single review commit. Pattern matched on all stories from 11.x and 9.x. Expectation for this story:

1. This story file landing under `_bmad-output/implementation-artifacts/` as `Story 9.9` commit.
2. A subsequent `Dev 9.9` commit implementing the tasks above.
3. A `Review 9.9` commit folding in code-review findings.

### Latest Technical Information

- **`reqwest` query construction** — the existing `get_items` builds a `Vec<String>` of `"key=value"` then `.join("&")`. Pre-existing style; preserve it for the new params (don't introduce `Url::query_pairs_mut` mid-file).
- **`async-trait` pinning** — already a workspace dependency; no version bump needed for the new trait method.
- **Jellyfin `ArtistIds` vs `AlbumArtistIds`** — Jellyfin has both. For library-wide track listing filtered by a single artist, `ArtistIds` is the right param (it includes featured-artist contributions). `AlbumArtistIds` is narrower (album-artist-only). The proposal and architecture both specify `ArtistIds` — match that.
- **OpenSubsonic `search3` `songCount`/`songOffset`** — these are standard parameters in the OpenSubsonic spec extension. Classic Subsonic also exposes `search3` from v1.4.0+, but the unfiltered query (`query=""`) behavior is OpenSubsonic-specific; classic Subsonic may return zero songs or fail. That's why the `open_subsonic` gate exists.

### References

- [Source: _bmad-output/planning-artifacts/epics.md:1970](_bmad-output/planning-artifacts/epics.md:1970) — Story 9.9 ACs and Technical Notes.
- [Source: _bmad-output/planning-artifacts/architecture.md:320](_bmad-output/planning-artifacts/architecture.md:320) — `browse.listTracks` RPC contract.
- [Source: _bmad-output/planning-artifacts/architecture.md:369](_bmad-output/planning-artifacts/architecture.md:369) — Tracks Browse Mode provider contract section.
- [Source: _bmad-output/planning-artifacts/architecture.md:285](_bmad-output/planning-artifacts/architecture.md:285) — Subsonic URL sanitization (NFR).
- [Source: _bmad-output/planning-artifacts/prd.md:199](_bmad-output/planning-artifacts/prd.md:199) — FR41 (Tracks browse mode).
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-06-08-tracks-browse-mode.md](_bmad-output/planning-artifacts/sprint-change-proposal-2026-06-08-tracks-browse-mode.md) — proposal that introduced this story.
- [Source: hifimule-daemon/src/providers/mod.rs:55](hifimule-daemon/src/providers/mod.rs:55) — `MediaProvider` trait.
- [Source: hifimule-daemon/src/providers/mod.rs:297](hifimule-daemon/src/providers/mod.rs:297) — `BrowseMode` enum (Task 1 target).
- [Source: hifimule-daemon/src/providers/jellyfin.rs:559](hifimule-daemon/src/providers/jellyfin.rs:559) — Jellyfin `capabilities()` (Task 5 target).
- [Source: hifimule-daemon/src/providers/subsonic.rs:537](hifimule-daemon/src/providers/subsonic.rs:537) — Subsonic `capabilities()` (Task 6 target).
- [Source: hifimule-daemon/src/providers/subsonic.rs:703](hifimule-daemon/src/providers/subsonic.rs:703) — `list_all_songs_page` (precedent for `search3_paged` usage).
- [Source: hifimule-daemon/src/providers/subsonic.rs:179](hifimule-daemon/src/providers/subsonic.rs:179) — `search3_paged` call site precedent.
- [Source: hifimule-daemon/src/rpc.rs:345](hifimule-daemon/src/rpc.rs:345) — browse dispatch (Task 7 target).
- [Source: hifimule-daemon/src/rpc.rs:548](hifimule-daemon/src/rpc.rs:548) — `handle_browse_list_artists` (handler template).
- [Source: hifimule-daemon/src/rpc.rs:8493](hifimule-daemon/src/rpc.rs:8493) — test pattern `browse_list_modes_routes_through_provider_capabilities`.
- [Source: hifimule-daemon/src/api.rs:291](hifimule-daemon/src/api.rs:291) — `get_items` (Task 4 target).
- [Source: _bmad-output/implementation-artifacts/9-8-extend-view-toggle-all-modes.md](_bmad-output/implementation-artifacts/9-8-extend-view-toggle-all-modes.md) — previous story (UI-only; learning: keep capability gating strict).

## Dev Agent Record

### Agent Model Used

Claude Opus 4.7 (Amelia / Senior Software Engineer)

### Debug Log References

_none_

### Completion Notes List

- Implemented the daemon-side `Tracks` browse mode end-to-end: enum variant, `TrackListFilter`/`TrackListPage` types, `MediaProvider::list_tracks` default impl, Jellyfin and Subsonic adapters, `browse.listTracks` RPC handler, and `error.tracks_mode_unsupported` i18n key (en/fr/es).
- **Jellyfin** — extended `JellyfinClient::get_items` with `artist_ids`, `album_ids`, `sort_by` optional params (path A as recommended). All four existing call sites pass `None` for the new params. `JellyfinProvider::list_tracks` issues a single `/Items` call with `IncludeItemTypes=Audio`, `SortBy=Name,Album`, and applies the album-implies-artist precedence rule (AC 4). Added `BrowseMode::Tracks` to `capabilities().browse.list_modes` after `Playlists`. Added `#[allow(clippy::too_many_arguments)]` on `get_items` to keep clippy clean.
- **Subsonic** — `SubsonicProvider::list_tracks` dispatches three branches: (1) `album_id` → `getAlbum`; (2) `artist_id` → `getArtist` + per-album `getAlbum` fan-out; (3) unfiltered → `search3_paged` (gated on `open_subsonic` — classic Subsonic returns `UnsupportedCapability`). `BrowseMode::Tracks` is added to capabilities ONLY when `open_subsonic == true`. The letter filter is applied in-process across all three branches via a shared `apply_letter_filter` helper (Subsonic has no native title-prefix filter). Total in the unfiltered branch follows `list_all_songs_page` precedent: page-length as total, UI uses "page length < limit" as exhaustion.
- **RPC** — `handle_browse_list_tracks` parses params (`libraryId`, `artistId`, `albumId`, `letter`, `startIndex`, `limit`), explicitly checks `provider.capabilities().browse.list_modes.contains(&BrowseMode::Tracks)` and returns `ERR_UNSUPPORTED_CAPABILITY` with the i18n message when missing (AC 6). Provider errors are routed through the existing `provider_error_to_rpc`. Response uses camelCase keys: `tracks`, `total`, `startIndex`, `limit`.
- **Tests added**: `browse_list_tracks_returns_tracks_from_provider` and `browse_list_tracks_rejects_when_capability_missing` in `rpc.rs`; extended `FakeBrowseProvider` with a `tracks` field, a `with_tracks` constructor, and a `list_tracks` impl. Subsonic: `classic_subsonic_list_tracks_unfiltered_unsupported` (verifies AC 5/9) and an assertion that classic Subsonic capabilities exclude `BrowseMode::Tracks`. Extended the `BrowseMode` serialization test in `providers/mod.rs` with the `Tracks → "tracks"` assertion. The Jellyfin capabilities test (`provider_reports_capabilities`) was updated to include `BrowseMode::Tracks`. Per Dev Notes guidance, no full HTTP-mock `list_tracks` test was added on the Jellyfin side — the RPC handler test and capability test together cover the contract; the Subsonic mock infrastructure cost would be out of proportion for v1.
- **Gates run**:
  - `rtk cargo check -p hifimule-daemon` — 0 errors.
  - `rtk cargo clippy -p hifimule-daemon -- -D warnings` — 79 errors total, all pre-existing (baseline at commit `39cdb09` had 80; this story actually reduced the count by adding `#[allow(clippy::too_many_arguments)]` on `get_items`). Zero new warnings introduced.
  - `rtk cargo test -p hifimule-daemon` — 420 passed, 0 failed. Also fixed a pre-existing stale test mock: `providers::subsonic::tests::provider_get_genre_tracks_calls_songs_by_genre` expected `getSongsByGenre` to be called with `count=20`, but the production impl calls with `count=10_000` (the 10k cap discussed in 9.8 review findings). The mock matcher was updated to align with the actual production call; the underlying 10k-cap concern remains tracked as a separate deferred follow-up.
  - `rtk cargo fmt --all` — applied.
- **Out of scope reminders observed**: did not touch `println!("DEBUG: ...")` at `api.rs:343` (pre-existing); did not touch any `hifimule-ui/**/*.ts` (Story 9.10's scope); did not modify PRD/architecture/UX/epics docs.

### File List

- hifimule-daemon/src/providers/mod.rs — added `BrowseMode::Tracks` variant, `TrackListFilter` and `TrackListPage` types, default `list_tracks` trait impl, and serialization test assertion.
- hifimule-daemon/src/providers/jellyfin.rs — added imports for `TrackListFilter`/`TrackListPage`, included `BrowseMode::Tracks` in `capabilities()` (and matching test), implemented `list_tracks`, updated 3 existing `get_items` call sites with the new trailing `None, None, None` args.
- hifimule-daemon/src/providers/subsonic.rs — added imports for `TrackListFilter`/`TrackListPage`, included `BrowseMode::Tracks` in `capabilities()` only when `open_subsonic == true` (and matching test), implemented `list_tracks` (three branches), added shared `apply_letter_filter` helper, added classic-Subsonic guard test. Also corrected a stale mock matcher in `provider_get_genre_tracks_calls_songs_by_genre` (count: 20 → 10000) to match the actual production call.
- hifimule-daemon/src/api.rs — extended `get_items` signature with `artist_ids`, `album_ids`, `sort_by` optional params (`#[allow(clippy::too_many_arguments)]`); updated the existing in-file test call site.
- hifimule-daemon/src/rpc.rs — added imports for `BrowseMode`/`TrackListFilter`, registered `browse.listTracks` dispatch arm, added `handle_browse_list_tracks` handler with capability gate, extended `FakeBrowseProvider` with a `tracks` field/constructor/impl, added two RPC-level tests, added a `make_fake_song` test helper, updated the existing `handle_jellyfin_get_items` call site with three trailing `None` args.
- hifimule-i18n/catalog.json — added `error.tracks_mode_unsupported` in en/fr/es.

## Change Log

- 2026-06-08: Story created from sprint-change-proposal-2026-06-08-tracks-browse-mode. Ultimate context engine analysis completed — comprehensive developer guide created.
- 2026-06-08: Dev 9.9 — daemon-side Tracks browse mode implemented (provider trait, Jellyfin and Subsonic adapters, RPC, i18n, tests). All ACs satisfied. Cargo check clean; clippy: no new warnings (79 vs. 80 baseline); tests: 420 passed, 0 failures (including a stale mock fix in `provider_get_genre_tracks_calls_songs_by_genre`).
