# Story 9.3: Genre Browsing and Genre Entity Basket Item

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a Ritualist (Arthur),
I want to browse by genre and add a genre to the basket as a single entity,
so that my device can receive a dynamic genre-based selection without manually picking every album.

## Acceptance Criteria

1. **Given** the active provider supports genres, **When** I open the Genres browse mode, **Then** I see a music-only grid of genres (already rendering from Story 9.2 — this story enables the basket toggle on genre cards).

2. **Given** I click a genre, **Then** I can view the tracks associated with that genre (already working from Story 9.2's `navigateToGenre`/`loadGenreTracks` flow — no change needed).

3. **Given** I click (+) on a genre card, **Then** a single Genre basket item is added (type `'MusicGenre'`) with estimated track count; the add button toggles to remove (same overlay behavior as artist/album cards).

4. **Given** a Genre basket item is in the basket, **Then** the BasketSidebar renders it as a dedicated Genre card: icon + "Genre · ~N tracks · ~X MB" — mirroring the Artist card layout.

5. **Given** sync starts with a Genre basket item, **Then** the daemon resolves the current track list for that genre at sync time (genre ID → full track list via provider API).

6. **Given** genre tracks overlap with other basket items (individual tracks, albums, artists), **Then** duplicates are removed during sync planning (already handled by the existing `seen_ids` dedup in both Jellyfin and Subsonic sync paths — no new code needed).

## Tasks / Subtasks

- [x] Task 1: Enable basket toggle for genre cards in library.ts (AC: 1, 3)
  - [x] In `hifimule-ui/src/library.ts`, in the `renderGrid()` function (lines ~309–315), **remove** the `MusicGenre` guard:
    ```typescript
    // REMOVE this block:
    // Genre container items have no basket toggle until Story 9.3
    const isGenreContainer = item.type === 'MusicGenre';
    const selEnabled = !isGenreContainer;
    // REPLACE with:
    const selEnabled = true;
    ```
  - [x] Remove the comment `// Genre container items have no basket toggle until Story 9.3`
  - [x] All other `renderGrid()` logic remains unchanged — device lock is handled by the CSS class `.device-locked` toggled by `BasketSidebar`, not by `selEnabled`
  - [x] Verify: `MediaCard.create(item, 'items', false, ..., selEnabled)` now receives `true` for `MusicGenre` items — no other changes to `MediaCard.ts` are required (it already creates the correct `BasketItem` from `BrowseDisplayItem`)

- [x] Task 2: Add Genre card renderer to BasketSidebar (AC: 4)
  - [x] In `hifimule-ui/src/components/BasketSidebar.ts`, add a `renderGenreCard(item: BasketItem)` private method **after** `renderArtistCard()` (around line 1160):
    ```typescript
    private renderGenreCard(item: BasketItem): string {
        return `
            <div class="basket-item-card basket-item-genre" data-id="${this.escapeHtml(item.id)}">
                <div class="basket-item-genre-icon">
                    <sl-icon name="music-note-beamed"></sl-icon>
                </div>
                <div class="basket-item-info">
                    <div class="basket-item-name">${this.escapeHtml(item.name)}</div>
                    <div class="basket-item-meta">
                        Genre · ~${item.childCount ?? 0} tracks · ~${formatSize(item.sizeBytes ?? 0)}
                    </div>
                </div>
                <sl-icon-button name="x" class="remove-item-btn" data-id="${this.escapeHtml(item.id)}" label="Remove"></sl-icon-button>
            </div>
        `;
    }
    ```
  - [x] In `renderItem()`, add dispatch for genre **before** the auto-fill check (after the `AUTO_FILL_SLOT_ID` check, before the `MusicArtist` check):
    ```typescript
    private renderItem(item: BasketItem): string {
        if (item.id === AUTO_FILL_SLOT_ID) {
            return this.renderAutoFillSlotCard(item);
        }
        if (item.type === 'MusicGenre') {        // ADD THIS BLOCK
            return this.renderGenreCard(item);
        }
        if (item.type === 'MusicArtist') {
            return this.renderArtistCard(item);
        }
        // ... existing default renderer ...
    }
    ```
  - [x] No CSS changes required — reuse the same structure as `.basket-item-artist`; the `.basket-item-genre` class will inherit existing `.basket-item-card` styles without needing new CSS rules (can optionally add minimal distinct styling, but not required)

- [x] Task 3: Add genre sync resolution — non-Jellyfin path (AC: 5)
  - [x] In `hifimule-daemon/src/rpc.rs`, in function `provider_sync_items_for_id` (around line 1440), add genre resolution **before** the final `Err(...)`:
    ```rust
    // After the artist check and before the final Err:
    if let Ok((tracks, _)) = provider.get_genre_tracks(item_id, 0, 10_000).await {
        return Ok((
            tracks.iter().map(provider_song_to_desired_item).collect(),
            None,
        ));
    }

    Err(JsonRpcError {
        code: ERR_CONNECTION_FAILED,
        message: format!("Sync aborted: Failed to fetch item {item_id}: Not found"),
        data: None,
    })
    ```
  - [x] The `get_genre_tracks` trait method has a default `Err(UnsupportedCapability)` impl — if the provider doesn't support genres, `if let Ok(...)` won't match and the existing `Err` is returned (correct behavior: sync aborted with informative error)
  - [x] Limit `10_000` is intentional: genre track counts above 10,000 are unrealistic; if needed, a future story can add pagination loops

- [x] Task 4: Add genre sync resolution — Jellyfin path (AC: 5)
  - [x] In `hifimule-daemon/src/rpc.rs`, in function `handle_sync_calculate_delta`, in the inner loop processing Jellyfin items (around line 2053), add genre detection and expansion **after** the `is_playlist` variable declaration and **before** the `match state.jellyfin_client.get_child_items_with_sizes(...)` call:
    ```rust
    let is_playlist = item.item_type == "Playlist";
    let item_id = item.id.clone();
    let item_name = item.name.clone();

    // ADD: Genre items use GenreIds query — ParentId expansion doesn't work for Jellyfin genre entities
    if item.item_type == "Genre" {
        match state
            .jellyfin_client
            .get_songs_by_genre(&url, &token, &user_id, &item_id, 0, 10_000)
            .await
        {
            Ok(response) => {
                for track in response.items {
                    if is_downloadable_item_type(&track.item_type) {
                        results.push(Ok(to_desired_item(track)));
                    }
                }
            }
            Err(e) => {
                results.push(Err(format!("Failed to expand genre {item_id}: {e}")));
            }
        }
        continue;
    }

    // Existing get_child_items_with_sizes call follows unchanged...
    match state
        .jellyfin_client
        .get_child_items_with_sizes(&url, &token, &user_id, &item.id)
        .await
    { ...
    ```
  - [x] `get_songs_by_genre` is on `state.jellyfin_client` (type `JellyfinClient`) — it's already used elsewhere in the same file (e.g. the browse genre handler); no import changes needed
  - [x] Return type of `get_songs_by_genre` is `anyhow::Result<JellyfinItemsResponse>` where `JellyfinItemsResponse` has `.items: Vec<JellyfinItem>` — use `response.items` to iterate
  - [x] The `continue` at the end skips the `get_child_items_with_sizes` call (which uses `ParentId` and does NOT work for genre entities)
  - [x] Jellyfin genre type string is `"Genre"` (confirmed from test data in `jellyfin.rs:1265` and from `genre_from_item()` which reads `item.id` from a Jellyfin item returned by the genres endpoint)

- [x] Task 5: Enrich genre cover art in the daemon's `browse.listGenres` handler (AC: 1)
  - [x] In `hifimule-daemon/src/rpc.rs`, in `handle_browse_list_genres` (line ~610), change `genres` to `mut` and add parallel cover art enrichment after the `list_genres` call:
    ```rust
    async fn handle_browse_list_genres(
        state: &AppState,
        params: Option<Value>,
    ) -> Result<Value, JsonRpcError> {
        let library_id = params.as_ref().and_then(|p| p["libraryId"].as_str()).map(str::to_owned);
        let provider = require_provider(state).await?;
        let mut genres = provider
            .list_genres(library_id.as_deref())
            .await
            .map_err(provider_error_to_rpc)?;

        // Enrich genres without cover art: fetch first track's art in parallel
        let needs_art: Vec<(usize, String)> = genres
            .iter()
            .enumerate()
            .filter(|(_, g)| g.cover_art_id.is_none())
            .map(|(i, g)| (i, g.id.clone()))
            .collect();

        if !needs_art.is_empty() {
            let art_futures: Vec<_> = needs_art
                .iter()
                .map(|(_, genre_id)| {
                    let p = provider.clone();
                    let id = genre_id.clone();
                    async move {
                        p.get_genre_tracks(&id, 0, 1)
                            .await
                            .ok()
                            .and_then(|(tracks, _)| tracks.into_iter().next())
                            .and_then(|t| t.cover_art_id)
                    }
                })
                .collect();

            let art_results = futures::future::join_all(art_futures).await;
            for ((idx, _), art) in needs_art.iter().zip(art_results) {
                genres[*idx].cover_art_id = art;
            }
        }

        let total = genres.len() as u64;
        Ok(serde_json::json!({ "genres": genres, "total": total }))
    }
    ```
  - [x] `futures::future::join_all` is already imported and used in `rpc.rs` (line 1841) — no new dependency
  - [x] `provider.clone()` clones the `Arc<dyn MediaProvider>` — cheap, already done elsewhere in the file
  - [x] `get_genre_tracks(id, 0, 1)` fetches exactly 1 track — minimal network overhead per genre
  - [x] Failures are silently ignored via `.ok()` — a genre with no tracks or a provider error simply keeps `cover_art_id: None`; `None` serialises to `"coverArtId": null` and `MediaCard` renders with no image (identical to the current behaviour)
  - [x] No changes to `browse.getGenre`, `browse.listModes`, or any other handler

- [x] Task 6: TypeScript compile check and smoke test (AC: 1–6)
  - [x] Run `rtk tsc` from `hifimule-ui/` — must pass with zero type errors
  - [x] Run `rtk cargo build` from workspace root — must compile with zero errors
  - [ ] Smoke test: genre cards in the Genres browse mode show an album cover image (first track's art) rather than blank
  - [ ] Smoke test: add a genre card to basket, verify it appears as Genre card in BasketSidebar (not the default image card)
  - [ ] Smoke test: verify basket toggle button on genre card in library browser is enabled (not greyed out) when a device is selected
  - [ ] Smoke test: verify basket toggle is still CSS-locked (greyed out via `.device-locked`) when no device is selected — this is handled by the existing CSS rule, NOT by `selEnabled`

## Dev Notes

### Current Codebase State (post Story 9.2)

**`hifimule-ui/src/library.ts` — What exists today:**
- `mapGenres()` maps `BrowseGenre[]` → `BrowseDisplayItem[]` with `type: 'MusicGenre'`, `childCount: genre.trackCount ?? 0`, `sizeBytes: 0`, `sizeTicks: 0`, `subtitle: "N tracks"` (or null)
- `renderGrid()` at lines ~309–315 has the single change needed: `const isGenreContainer = item.type === 'MusicGenre'; const selEnabled = !isGenreContainer;` — replace this with `const selEnabled = true;`
- `loadGenres()`, `loadGenreTracks()`, `navigateToGenre()` all exist and work from Story 9.2 — **do NOT touch these**
- `fetchBrowseGenres()` and `fetchBrowseGenre()` wrappers exist in `rpc.ts` — **do NOT touch**

**`hifimule-ui/src/components/MediaCard.ts` — No changes required:**
- The basket toggle for `BrowseDisplayItem` already adds `type: bi.type` ('MusicGenre') to the `BasketItem`
- `childCount: bi.childCount ?? 0`, `sizeBytes: bi.sizeBytes ?? 0`, `sizeTicks: bi.sizeTicks ?? 0` are already used
- `artist: bi.subtitle ?? undefined` will be set to `"42 tracks"` (the subtitle string) — this is harmless noise; `renderGenreCard()` ignores `item.artist` and reads `item.childCount` directly
- `showSelection: true` (always set to `true` for `isBrowseItem`) is already correct

**Genre cover art enrichment — daemon-side, in `handle_browse_list_genres`:**
- Providers typically return `cover_art_id: None` for genres (Jellyfin genre entities carry no image; Subsonic's genres API omits art).
- The daemon enriches the response before sending it: for each genre with `cover_art_id == None`, fire `provider.get_genre_tracks(id, 0, 1)` and use the first track's `cover_art_id`. All requests run in parallel via `futures::future::join_all` (already in use at rpc.rs:1841).
- The UI receives complete `coverArtId` values in the `browse.listGenres` response and renders them via the existing `getImageUrl` call in MediaCard — no extra frontend logic needed.
- `loadGenres()` and `mapGenres()` in `library.ts` need **no changes** — they already pass `g.coverArtId` through to `BrowseDisplayItem`. The cover art will simply be present in the response.

**`hifimule-ui/src/components/BasketSidebar.ts` — Artist card as the exact model:**
- `renderArtistCard()` at line ~1145 is the exact template to follow for `renderGenreCard()`
- Artist uses `basket-item-artist` CSS class and `sl-icon name="person-fill"` — Genre uses `basket-item-genre` and `sl-icon name="music-note-beamed"`
- `renderItem()` dispatch order: `AUTO_FILL_SLOT_ID` → `MusicGenre` (new) → `MusicArtist` → default

**`hifimule-daemon/src/rpc.rs` — Sync resolution:**
- `provider_sync_items_for_id` (line ~1393): tries album → playlist → artist → error. Add genre before the final error.
- `handle_sync_calculate_delta` Jellyfin inner loop (line ~2050): tries `is_downloadable` → container expansion via `get_child_items_with_sizes`. Add genre before `get_child_items_with_sizes`.
- Deduplication of `desired_items` is already handled at lines ~2116–2129 via `seen_ids` HashSet — no additional dedup code needed.
- `get_songs_by_genre` signature: `(&self, url: &str, token: &str, user_id: &str, genre_id: &str, offset: u32, limit: u32) -> Result<JellyfinItemsResponse>` — called the same way as in `handle_browse_list_genres`/`handle_browse_get_genre`.

### Key Architecture Constraints

- All basket items are sent to `sync_calculate_delta` as raw IDs in `itemIds[]`. The daemon resolves them to actual tracks. Genre IDs (Jellyfin GUIDs or Subsonic genre names) must be resolved via `get_songs_by_genre` / `get_genre_tracks` respectively.
- `BasketItem.type` is `string` in `basket.ts` — no type widening needed; `'MusicGenre'` is valid.
- Device lock state (no device selected) is handled exclusively by `BasketSidebar` toggling the `.device-locked` CSS class on `#library-content`. The `selEnabled` flag in `renderGrid()` is only for the "no server connected" edge case and the legacy PascalCase items path — for browse items, it doesn't gate device lock.
- `manifest_save_basket` serializes basket items with `type: 'MusicGenre'` to the device manifest. On reload, `hydrateFromDaemon()` restores them. `renderItem()` will dispatch to `renderGenreCard()` after hydration — correct.

### Genre ID Format by Provider

- **Jellyfin**: `Genre.id` is a Jellyfin GUID (e.g., `"a5b7c3d2-..."`). `get_songs_by_genre` uses `GenreIds={id}` query param. Jellyfin's `get_items_by_ids` fetches the genre entity with `Type: "Genre"` — this is the string to check for in `handle_sync_calculate_delta`.
- **Subsonic**: `Genre.id == Genre.name` (e.g., `"Rock"`). `get_genre_tracks` uses `genre={name}` in the Subsonic API call. The artist/album/playlist lookups in `provider_sync_items_for_id` will return errors for a genre-name string, so the genre check falls through naturally.

### Story Boundaries

**In scope:**
- Enable basket toggle for `MusicGenre` cards in `renderGrid()` (1-line change in `library.ts`)
- `renderGenreCard()` in `BasketSidebar.ts`
- `renderItem()` dispatch for `MusicGenre` in `BasketSidebar.ts`
- Genre-to-tracks resolution in daemon sync paths (both Jellyfin and Subsonic)
- Cover art enrichment in `handle_browse_list_genres` daemon handler (parallel `get_genre_tracks` limit-1 calls)

**Out of scope:**
- Story 9.4: History and Favorites browse modes special UX
- Genre quick-nav (A-Z bar for genres) — not required; the existing `>= 20 items` threshold and quick-nav bar is artists-only per Story 9.2
- Genre count/size resolution at add-time: `sizeBytes: 0` at add-time is accepted; "~0 MB" in the genre card is expected MVP behavior. The daemon resolves tracks (and thus actual sizes) at sync time.
- No changes to the sync engine's `calculate_delta` function itself — dedup is already handled

**Do NOT touch:**
- `hifimule-ui/src/rpc.ts` — no changes
- `hifimule-ui/src/components/MediaCard.ts` — no changes
- `hifimule-ui/src/state/basket.ts` — no changes
- `hifimule-ui/src/login.ts` — no changes
- `loadGenres()`, `loadGenreTracks()`, `navigateToGenre()` in `library.ts` — no changes

### Testing Guidance

No vitest/jest setup in `hifimule-ui/` — verification is TypeScript compilation + manual smoke test.

- `rtk tsc` from `hifimule-ui/` — primary quality gate; must pass with zero errors
- `rtk cargo build` from workspace root — must compile with zero errors (only `rpc.rs` has Rust changes)
- Key checks to reason through in code:
  - After removing the `isGenreContainer` guard, every `BrowseDisplayItem` in `renderGrid()` gets `selEnabled = true` — confirm no regression on artist/album/playlist/track basket toggles
  - `renderItem()` dispatch order must put `'MusicGenre'` check before the default renderer to avoid genre items rendering as image cards
  - In `provider_sync_items_for_id`, the genre check is `if let Ok(...)` — if `get_genre_tracks` returns `Err(UnsupportedCapability)`, the `Ok` arm doesn't match and the `Err(...)` at the end fires (correct: sync aborts with "item not found" for unsupported genre provider)
  - In the Jellyfin genre expansion, `response.items` may include non-Audio items if Jellyfin returns mixed types — the `is_downloadable_item_type(&track.item_type)` filter handles this correctly

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story-9.3-Genre-Browsing-and-Genre-Entity-Basket-Item]
- [Source: _bmad-output/implementation-artifacts/9-2-browse-mode-navigation-ui.md#Story-Boundaries] (out-of-scope list from 9.2 defines exactly what 9.3 must implement)
- [Source: _bmad-output/implementation-artifacts/9-2-browse-mode-navigation-ui.md#Dev-Agent-Record] (Task 7 completion note: "Genre container items always pass `deviceSelectionEnabled: false` to MediaCard (Story 9.3 scope)")
- [Source: hifimule-ui/src/library.ts:309-315] (the change point for enabling genre basket toggle)
- [Source: hifimule-daemon/src/rpc.rs:610-621] (handle_browse_list_genres — replace with enriched version; `futures::future::join_all` already used at line 1841)
- [Source: hifimule-ui/src/components/BasketSidebar.ts:1145-1160] (renderArtistCard — the exact model for renderGenreCard)
- [Source: hifimule-ui/src/components/BasketSidebar.ts:1162-1194] (renderItem — where to add MusicGenre dispatch)
- [Source: hifimule-daemon/src/rpc.rs:1393-1454] (provider_sync_items_for_id — add genre resolution before final Err)
- [Source: hifimule-daemon/src/rpc.rs:2034-2100] (handle_sync_calculate_delta container expansion — add genre before get_child_items_with_sizes)
- [Source: hifimule-daemon/src/rpc.rs:2116-2129] (dedup via seen_ids already present — no new dedup needed)
- [Source: hifimule-daemon/src/providers/jellyfin.rs:1252-1297] (genre provider tests confirming Type="Genre" and get_genre_tracks usage)
- [Source: hifimule-daemon/src/api.rs:1075-1097] (get_songs_by_genre signature and return type)

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

- Task 1: Removed `isGenreContainer` guard and `selEnabled = !isGenreContainer` from `renderGrid()` in `library.ts`. Replaced with `const selEnabled = true;` — genre cards now receive a basket toggle exactly like artist/album cards. Device lock remains handled exclusively by `.device-locked` CSS class.
- Task 2: Added `renderGenreCard()` private method to `BasketSidebar.ts` modelled on `renderArtistCard()`. Added `MusicGenre` dispatch in `renderItem()` between the `AUTO_FILL_SLOT_ID` check and `MusicArtist` check. No CSS changes needed — `.basket-item-card` base styles apply automatically.
- Task 3: Added genre resolution in `provider_sync_items_for_id` (non-Jellyfin path) before the final `Err(...)`. Uses `get_genre_tracks(item_id, 0, 10_000)` — if provider doesn't support genres, the `if let Ok` arm doesn't match and the existing error fires.
- Task 4: Added genre detection block in `handle_sync_calculate_delta` Jellyfin inner loop after `item_name` declaration and before `get_child_items_with_sizes`. Uses `get_songs_by_genre` (via `GenreIds` query) and `continue`s to skip the `ParentId`-based expansion that doesn't work for genre entities.
- Task 5: Replaced `handle_browse_list_genres` with enriched version that fetches first track's cover art in parallel (via `futures::future::join_all`) for each genre with `cover_art_id: None`. Zero extra dependencies — `join_all` already imported, `provider.clone()` clones the `Arc`.
- Task 6: `rtk tsc` from `hifimule-ui/` — 0 errors. `rtk cargo build` from workspace root — 0 errors, 2 pre-existing warnings (unrelated to this story). Smoke tests are manual (no vitest setup in hifimule-ui).

### File List

- hifimule-ui/src/library.ts
- hifimule-ui/src/components/BasketSidebar.ts
- hifimule-daemon/src/rpc.rs

## Change Log

- 2026-05-22: Implemented Story 9.3 — enabled genre basket toggle in library.ts, added renderGenreCard/renderItem dispatch in BasketSidebar.ts, added genre sync resolution for both non-Jellyfin (provider_sync_items_for_id) and Jellyfin (handle_sync_calculate_delta) paths, added parallel cover art enrichment in handle_browse_list_genres. tsc: 0 errors, cargo build: 0 errors.
