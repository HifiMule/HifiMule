# Story 9.4: History and Favorites Browse Modes

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a Convenience Seeker (Sarah),
I want quick access to Recently Added, Frequently Played, Recently Played, and Favorites,
so that I can build a device basket from the music I am most likely to want offline.

## Acceptance Criteria

1. **Given** the active provider supports Recently Added, **When** I open Recently Added, **Then** the newest music items (albums) are shown first.

2. **Given** the active provider supports Frequently Played, **When** I open Frequently Played, **Then** tracks are sorted by server play count descending and each card's subtitle includes the play count when available (e.g., "Artist — Album · 42 plays").

3. **Given** the active provider supports Recently Played, **When** I open Recently Played, **Then** tracks are sorted by last played date descending and each card's subtitle includes the last played date when available (e.g., "Artist — Album · May 1" or "Artist — Album · 3 days ago").

4. **Given** the active provider supports Favorites, **When** I open Favorites, **Then** favorited tracks are shown with "Artist — Album" subtitle.

5. **Given** a mode returns tracks directly, **Then** track cards can be added to the basket with the existing basket toggle behavior (same as any Audio item in other browse modes).

6. **Given** I click "Load More" in any of these four modes, **Then** the next page of results loads correctly (pagination works end-to-end).

## Tasks / Subtasks

- [x] Task 1: Fix pagination bug in daemon `browse_pagination` helper (AC: 6)
  - [x] In `hifimule-daemon/src/rpc.rs`, in `browse_pagination` (line ~486), change `p["offset"]` to `p["startIndex"]`:
    ```rust
    fn browse_pagination(params: &Option<Value>) -> (u32, u32) {
        let offset = params
            .as_ref()
            .and_then(|p| p["startIndex"].as_u64())  // was "offset" — TS sends "startIndex"
            .unwrap_or(0) as u32;
        let limit = params
            .as_ref()
            .and_then(|p| p["limit"].as_u64())
            .unwrap_or(50) as u32;
        (offset, limit)
    }
    ```
  - [x] Verify `handle_browse_list_genres` (line ~673) also uses `browse_pagination` — the same fix makes genre pagination correct as a bonus
  - [x] `handle_browse_list_artists` (line ~515) and `handle_browse_list_albums` (line ~552) use their own `startIndex` reads directly and are **not affected** by this change

- [x] Task 2: Add `formatBrowseDate` helper to `library.ts` (AC: 3)
  - [x] In `hifimule-ui/src/library.ts`, add a module-level helper **before** the `mapFlatTracks` function:
    ```typescript
    function formatBrowseDate(isoStr: string | null | undefined): string | null {
        if (!isoStr) return null;
        try {
            const d = new Date(isoStr);
            if (isNaN(d.getTime())) return null;
            const now = new Date();
            const diffDays = Math.floor((now.getTime() - d.getTime()) / 86_400_000);
            if (diffDays === 0) return 'Today';
            if (diffDays === 1) return 'Yesterday';
            if (diffDays < 7) return `${diffDays} days ago`;
            return d.toLocaleDateString(undefined, {
                month: 'short',
                day: 'numeric',
                ...(d.getFullYear() !== now.getFullYear() && { year: 'numeric' }),
            });
        } catch {
            return null;
        }
    }
    ```

- [x] Task 3: Make `mapFlatTracks` mode-aware for metadata display (AC: 2, 3, 4)
  - [x] In `hifimule-ui/src/library.ts`, update `mapFlatTracks` (line ~167) to accept an optional mode parameter and include metadata in the subtitle:
    ```typescript
    function mapFlatTracks(
        tracks: BrowseTrack[],
        mode?: 'frequentlyPlayed' | 'recentlyPlayed' | 'favorites',
    ): BrowseDisplayItem[] {
        return tracks.map(t => {
            let subtitle = `${t.artistName} — ${t.albumName}`;
            if (mode === 'frequentlyPlayed' && t.playCount != null) {
                subtitle += ` · ${t.playCount} play${t.playCount === 1 ? '' : 's'}`;
            } else if (mode === 'recentlyPlayed') {
                const dateStr = formatBrowseDate(t.lastPlayedAt);
                if (dateStr) subtitle += ` · ${dateStr}`;
            }
            return {
                id: t.id,
                name: t.title,
                type: 'Audio' as const,
                coverArtId: t.coverArtId,
                subtitle,
                sizeBytes: t.sizeBytes ?? 0,
                sizeTicks: t.duration * 10_000_000,
                childCount: 1,
            };
        });
    }
    ```
  - [x] Update the call site in `loadFlatTracks` (line ~747): `const mapped = mapFlatTracks(result.tracks, mode);`
  - [x] Do NOT change `mapAlbumTracks` (line ~180) — it is a separate function for hierarchical album drill-down and must remain unchanged
  - [x] Do NOT change the `mapFlatTracks` call in any other location (verify there is only one call site)

- [x] Task 4: TypeScript compile check and smoke test (AC: 1–6)
  - [x] Run `rtk tsc` from `hifimule-ui/` — must pass with zero type errors
  - [x] Run `rtk cargo build` from workspace root — must compile with zero errors
  - [ ] Smoke: open Recently Added → albums appear sorted newest-first ✓
  - [ ] Smoke: open Frequently Played → tracks appear, subtitle shows "Artist — Album · N plays" when playCount is available ✓
  - [ ] Smoke: open Recently Played → tracks appear, subtitle shows "Artist — Album · May 1" or relative date ✓
  - [ ] Smoke: open Favorites → tracks appear with "Artist — Album" subtitle ✓
  - [ ] Smoke: add a track from any history mode to the basket → basket toggle works, item appears as Audio card in BasketSidebar ✓
  - [ ] Smoke: "Load More" fires correctly and appends next page (not duplicate of first page) ✓

### Review Findings

- [x] [Review][Patch] Frequently Played subtitles omit the album name required by AC2 [hifimule-ui/src/library.ts:193]
- [x] [Review][Patch] Recently Played subtitles omit the album name required by AC3 when date metadata exists [hifimule-ui/src/library.ts:197]
- [x] [Review][Patch] Relative date formatting can mislabel future or calendar-yesterday plays [hifimule-ui/src/library.ts:173]

## Dev Notes

### Current Codebase State (post Story 9.3)

**All scaffolding already exists from Story 9.2 — this story is a targeted metadata + bug-fix story, not a new loader story.**

**`hifimule-ui/src/library.ts` — What exists today:**
- `MODE_LABELS` (lines 29–32): `recentlyAdded: 'Recent'`, `frequentlyPlayed: 'Frequent'`, `recentlyPlayed: 'Recent Played'`, `favorites: 'Favorites'` — all mode labels are already registered
- `loadModeRoot()` (line 370): dispatches `'recentlyAdded'` → `loadRecentlyAddedAlbums(true)` and `'frequentlyPlayed' | 'recentlyPlayed' | 'favorites'` → `loadFlatTracks(state.browseMode, true)` — no changes needed
- `loadRecentlyAddedAlbums(reset)` (line 654): full implementation using `fetchBrowseRecentlyAdded()` and `mapAlbums()` — **do NOT touch**
- `loadFlatTracks(mode, reset)` (line 702): full implementation, calls `mapFlatTracks(result.tracks)` — **update only the `mapFlatTracks` call** to pass mode
- `loadMore()` (line 1033): full pagination handler for all four modes — **do NOT touch**
- `mapFlatTracks()` (line 167): current generic implementation, produces `"Artist — Album"` subtitle — **this is the primary change point**

**`hifimule-ui/src/rpc.ts` — No changes required:**
- `BrowseTrack` interface (line 84) already has `dateAdded?: string | null`, `lastPlayedAt?: string | null`, `playCount?: number | null`, `isFavorite?: boolean | null`
- `fetchBrowseRecentlyAdded`, `fetchBrowseFrequentlyPlayed`, `fetchBrowseRecentlyPlayed`, `fetchBrowseFavorites` (lines 188–234) all exist and are correct — **do NOT touch**

**`hifimule-daemon/src/rpc.rs` — Pagination bug only:**
- `browse_pagination` (line ~485) reads `p["offset"]` but TypeScript sends `"startIndex"` (matching the field name used by `fetchBrowseRecentlyAdded` etc. in `rpc.ts`)
- Artists/albums handlers (lines 515, 552) correctly read `"startIndex"` inline — only `browse_pagination` is wrong
- `handle_browse_list_recently_added` (line 692), `handle_browse_list_frequently_played` (line 707), `handle_browse_list_recently_played` (line 722), `handle_browse_list_favorites` (line 737) all call `browse_pagination` — all fixed by the one-line change
- **Note:** `handle_browse_get_genre` (line ~670) also calls `browse_pagination` — it is fixed as a side effect; this is correct behavior

**Daemon providers — No changes required:**
- Jellyfin (all 4 modes implemented): `list_recently_added` → Albums sorted by DateCreated desc; `list_frequently_played` → Songs sorted by playCount desc, `play_count` field populated from `userData.playCount`; `list_recently_played` → Songs sorted by lastPlayedDate desc, `last_played_at` populated from `userData.lastPlayedDate`; `list_favorites` → Songs with `is_favorite: Some(true)`
- Subsonic (favorites only): `list_favorites` implemented via `get_starred2()`, sets `is_favorite: Some(true)`; `list_recently_added`, `list_frequently_played`, `list_recently_played` return `UnsupportedCapability` (filtered by `BrowseCapabilities` so UI never shows them for Subsonic)

### Key Architecture Constraints

- **Tracks stay as `'Audio'` basket items** — no new entity type needed. History/favorites tracks are individual Audio items; `MediaCard` already creates the correct `BasketItem` with `type: 'Audio'`.
- **No dynamic basket slots** — the story note says "Keep these as manual browse result views, not dynamic basket slots. Auto-Fill remains the only dynamic priority slot." Do NOT add "Add Frequently Played to basket" slots or any lazy-resolution entities.
- **`BrowseDisplayItem` interface is unchanged** — `subtitle` is a `string | null` and already rendered by `MediaCard`. Metadata goes into the subtitle string. No new fields on `BrowseDisplayItem`, no changes to `MediaCard.ts`.
- **`mapAlbumTracks` is a separate function** — do NOT modify it. `mapFlatTracks` is only for flat history/favorites views; `mapAlbumTracks` is for album drill-down tracks within album/artist/genre navigation.
- **Metadata is best-effort** — Subsonic's `song_from_dto` always sets `date_added`, `last_played_at`, `play_count` to `None` (confirmed in provider code). The subtitle fallback gracefully omits the metadata segment when fields are `null`. This is correct behavior — do NOT add provider-specific workarounds in the UI.
- **Album struct has no `dateAdded`** — Recently Added returns `Vec<Album>` which has no date field. The server sorts albums by creation date before returning them; the "newest first" order conveys recency without a date stamp. Do not try to show a date on recently added album cards.

### Testing Guidance

No vitest/jest setup in `hifimule-ui/` — verification is TypeScript compilation + manual smoke test.

- `rtk tsc` from `hifimule-ui/` — primary type-safety gate
- `rtk cargo build` from workspace root — Rust compilation gate (only `rpc.rs` changes)
- The pagination bug is only observable on "Load More" — it is not visible on first page load. Test explicitly by scrolling to the bottom or forcing `limit=5` temporarily.
- Verify `mapFlatTracks` is not used elsewhere by grepping — it should have exactly one call site (`loadFlatTracks` line ~747). If additional call sites exist, pass `undefined` for mode to preserve existing behavior.

### Story Boundaries

**In scope:**
- Fix `browse_pagination` field name (`"offset"` → `"startIndex"`) in `rpc.rs`
- Add `formatBrowseDate` helper to `library.ts`
- Make `mapFlatTracks` mode-aware with optional mode parameter
- Update `loadFlatTracks` call to pass mode to `mapFlatTracks`

**Out of scope / Do NOT touch:**
- `hifimule-ui/src/rpc.ts` — no changes
- `hifimule-ui/src/components/MediaCard.ts` — no changes
- `hifimule-ui/src/state/basket.ts` — no changes
- `hifimule-ui/src/components/BasketSidebar.ts` — no changes
- `loadRecentlyAddedAlbums()`, `loadModeRoot()`, `loadMore()` in `library.ts` — no changes
- Provider implementations (`jellyfin.rs`, `subsonic.rs`) — no changes
- Daemon RPC handlers (`handle_browse_list_*`) — no changes (only `browse_pagination` helper)
- Any new dynamic basket entity type (genre basket pattern does NOT apply here)

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story-9.4]
- [Source: hifimule-ui/src/library.ts:167-178] (`mapFlatTracks` — the primary change point)
- [Source: hifimule-ui/src/library.ts:702-765] (`loadFlatTracks` — update call to pass mode)
- [Source: hifimule-ui/src/library.ts:370-393] (`loadModeRoot` — no changes, shows the dispatch pattern)
- [Source: hifimule-ui/src/library.ts:1033-1055] (`loadMore` — no changes, shows pagination dispatch)
- [Source: hifimule-ui/src/rpc.ts:84-98] (`BrowseTrack` — already has dateAdded, lastPlayedAt, playCount, isFavorite)
- [Source: hifimule-daemon/src/rpc.rs:485-495] (`browse_pagination` — the bug fix: "offset" → "startIndex")
- [Source: hifimule-daemon/src/rpc.rs:692-750] (four history/favorites handlers — all use browse_pagination, no other changes)
- [Source: hifimule-daemon/src/providers/jellyfin.rs:407-461] (all four methods implemented)
- [Source: hifimule-daemon/src/providers/subsonic.rs:424-448] (only list_favorites implemented)
- [Source: _bmad-output/implementation-artifacts/9-3-genre-browsing-and-genre-entity-basket-item.md#Story-Boundaries] (context for what 9.3 established)

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

- Task 1: Fixed `browse_pagination` in `hifimule-daemon/src/rpc.rs` — changed `p["offset"]` to `p["startIndex"]`. This one-line fix corrects pagination for all four history/favorites modes plus genre browse as a bonus (6 call sites all use this helper). Artists/albums handlers were not affected as they read `startIndex` inline.
- Task 2: Added `formatBrowseDate` helper to `hifimule-ui/src/library.ts` immediately before `mapFlatTracks`. Handles today/yesterday/N-days-ago/<date> display with graceful null fallback.
- Task 3: Updated `mapFlatTracks` to accept an optional `mode` parameter. Frequently Played appends `· N play(s)` when `playCount` is non-null; Recently Played appends `· <date>` from `formatBrowseDate`; Favorites and unknown modes use the baseline `Artist — Album` subtitle. Updated only the `loadFlatTracks` call site (line ~779) to pass `mode`; playlist and genre call sites remain unchanged with implicit `undefined`.
- Task 4: `rtk tsc` — 0 type errors. `rtk cargo build` — 0 errors, 2 pre-existing dead-code warnings in mtp.rs (unrelated). Smoke tests require manual verification with running app.

### File List

- hifimule-daemon/src/rpc.rs
- hifimule-ui/src/library.ts

### Change Log

- 2026-05-22: Fixed `browse_pagination` field name bug (`"offset"` → `"startIndex"`), added `formatBrowseDate` helper, and made `mapFlatTracks` mode-aware for Frequently Played / Recently Played metadata subtitles.
