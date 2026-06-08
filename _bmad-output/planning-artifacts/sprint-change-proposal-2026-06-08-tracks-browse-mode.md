# Sprint Change Proposal: Tracks Browse Mode — Dual-Panel Artist/Album Filters with Auto-Pagination

**Date:** 2026-06-08
**Author:** Alexis
**Scope:** Moderate (new daemon RPC + new top-level browse mode + new UI surface)
**Handoff:** Product Owner + Developer agent

---

## 1. Issue Summary

After completing Epic 11 (Playlist curation) and Story 9.8 (extending the grid/list toggle), the project has a rich Playlist Curation View that browses tracks via a dual-panel artist/album filter. Users now want the *same* affordance applied to the entire library — not just to a single playlist's content.

**Problem statement:** There is no flat "all tracks" surface in HifiMule. Tracks are currently only reachable by drilling into an album (Artists → Artist → Album → Tracks) or by opening a playlist for curation. A user who wants to browse the library at the track grain — e.g. to find a specific track they don't know the album of, or to add individual tracks to the basket or to a playlist — must guess the album. This is a gap relative to mainstream music apps (Jellyfin, Navidrome, iTunes-style "Songs" view), and it leaves the dual-panel filter pattern (built for Story 11.6) under-exploited.

**Trigger:** Direct user request to add a Tracks/Songs view that:
1. Displays **all** tracks in the library.
2. Provides **artist/album filters** in the dual-panel layout used by the Playlist Curation View.
3. Differs from the curation view in that the filter panels (and the track list) must **auto-paginate** like the existing artist/album root views (Story 9.7), because the full library can contain thousands of artists/albums/tracks.
4. Track rows must expose the standard **Add (+) / Remove (-) basket** actions and the **"Add to playlist…"** context menu introduced in Story 11.7.

**Issue type:** New requirement emerged from stakeholder (user) feedback after observing the curation view's ergonomics.

---

## 2. Impact Analysis

### Epic Impact

- **Epic 9 — Rich Library Navigation:** Natural home for this work. Epic 9 is explicitly about expanding the browser into a "Jellyfin-like curation surface with provider-supported navigation modes". A `tracks` mode is the missing entry in the FR8 navigation list. **Two new stories proposed:** 9.9 (daemon contract) and 9.10 (UI).
- **Epic 11 — Selection-as-Playlist & Curation:** No direct changes, but Story 11.7's "Add to playlist…" context menu must apply to track rows on the new view. This is already specified in 11.7 ("Track context menus apply wherever track rows are rendered") so no story edit is required, only verification.
- **Other epics:** Not affected. No basket model changes (tracks are already first-class basket entities). No sync engine, no provider write-back changes.

### Story Impact

- **Story 9.1 (Provider Browse Modes & Capability Contract):** Extend `BrowseMode` enum to include `Tracks`. Capability gate: a provider that does not implement the new `list_tracks` method is hidden from the UI's available modes list.
- **Story 9.2 (Browse Mode Navigation UI):** Add `tracks` to the browse-mode segmented control. The view-mode toggle (grid/list) does **not** apply to the tracks mode — the tracks mode uses its own dual-panel + virtualized track-list layout, not the grid/list duality. (Aligns with how `playlists` and `genres` differ from artists/albums root.)
- **Story 9.7 (Virtualized List/Table):** Reuses the same virtualized renderer concept for the three panels of the tracks mode, but does not require code changes — Story 9.10 will share rendering helpers.
- **Story 11.7 (Add to Playlist context menu):** No spec change; the new tracks-mode track rows must satisfy the existing AC ("right-click an individual track row in any browse view → context menu appears"). Verification only.

### Artifact Conflicts

| Artifact | Section | Change |
|---|---|---|
| **PRD** | FR8 | Add "Tracks/Songs" to the supported navigation modes. |
| **PRD** | New FR41 | Define the Tracks browse mode and its dual-panel/auto-pagination contract. |
| **Architecture** | §`browse.*` RPC table | Add `browse.listTracks` with paginated, artistId/albumId/letter-filtered semantics. |
| **Architecture** | MediaProvider trait section | Add `list_tracks(filter)` method; document Jellyfin and Subsonic mappings. |
| **UX Design Spec** | §5.1 Navigation | Add `Tracks` to the list of supported browse modes. |
| **UX Design Spec** | §5.2 Custom Components | New entry: "Tracks Browse View (dual-panel, paginated)" specifying layout, pagination, and actions; cross-reference Playlist Curation View as the visual sibling. |
| **Epics** | FR Coverage Map | Add `FR41: Epic 9 — Tracks Browse Mode (Stories 9.9–9.10)`. |
| **Epics** | Epic 9 | Add **Story 9.9** (daemon contract) and **Story 9.10** (UI). |

### Technical Impact

**Daemon (Rust):**
- New `BrowseMode::Tracks` variant.
- New `MediaProvider::list_tracks(filter: TrackListFilter)` trait method with paginated response. Default impl returns `NotSupported`.
- `TrackListFilter` carries optional `library_id`, `artist_id`, `album_id`, `letter`, `start_index`, `limit`.
- JellyfinProvider: maps to `GET /Users/{uid}/Items?IncludeItemTypes=Audio&Recursive=true&SortBy=Name&StartIndex&Limit[&ArtistIds][&AlbumIds][&NameStartsWith]`.
- SubsonicProvider: composes from `search3?query=&songCount&songOffset` for the "all tracks" case; when `artist_id` is set, uses `getArtist`→album list→`getAlbum` for tracks; when `album_id` is set, uses `getAlbum` directly. (May need a `not_supported` flag for classic Subsonic without song search.)
- New `browse.listTracks` RPC handler dispatches to `provider.list_tracks`.
- `BrowseCapabilities` extended with `supports_tracks_mode: bool` (or this is derived from list_modes containing `Tracks`).
- A–Z letter filter on tracks (optional in v1 — can be deferred).

**Frontend (TypeScript):**
- `BrowseMode` union string adds `"tracks"`.
- New `fetchBrowseTracks(filter)` RPC helper.
- New `TracksBrowseView.ts` component (sibling to `PlaylistCurationView.ts`) implementing the dual-panel + bottom track-list layout with **three independently paginated regions**:
  - Left panel: paginated artists list (reuses the artist-root pagination + autoload-on-scroll logic).
  - Right panel: paginated albums list, filtered by the selected artist (reuses the album-root pagination logic and `browse.listAlbums?artistId=…` if it exists, or `browse.getArtist`'s albums when artist is selected).
  - Bottom panel: paginated tracks list via `browse.listTracks`, filtered by selected artist/album.
- "All artists" and "All albums" entries (mirrors the Story 11.10 pattern in the curation view) — clearing the filter shows the full paginated lower content of the next layer.
- Track row actions:
  - **(+) Add to basket / (-) Remove from basket** — reuses the existing `MediaCard` track-row affordance, including the disabled state when no device is selected.
  - **Context menu "Add to playlist…"** — wires into existing Story 11.7 flow; capability-gated on `supports_playlist_write`.
  - **Per-row "Send to playlist…" submenu** — explicit submenu rendering of the same flow for keyboard/touch users (additional surface, same RPC).
- View-toggle (grid/list) is **suppressed** in tracks mode (the dual-panel layout is the only rendering).
- Breadcrumb behavior: tracks mode is flat — no drill-down breadcrumbs; selecting an artist or album updates the panel filter state rather than pushing a breadcrumb.
- Basket integration: track rows respect the existing "no device selected → disabled add buttons" rule (per Story 9.2 AC).
- A–Z letter control on artist and album panels (reuses existing letter strip from artists-root view).

**i18n keys (new):**
- `library.mode.tracks` → "Tracks" / "Pistes" / "Pistas"
- `tracks.view.all_artists`, `tracks.view.all_albums`
- `tracks.view.no_tracks`, `tracks.view.loading`
- `tracks.view.add_to_playlist`, `tracks.view.send_to_playlist`

**Tests:**
- Unit: `list_tracks` filter combinations on both adapters.
- UI: pagination triggers on all three panels; "All artists"/"All albums" works; context menu shows/hides correctly.
- Integration smoke: large library (>5000 tracks) renders responsively.

### Secondary Artifacts

- **No changes** to deployment scripts, IaC, CI/CD, or monitoring.
- **Localization files** (en/fr/es) need the new keys above.
- **Component inventory** (`docs/component-inventory-hifimule-ui.md`) needs a new entry for `TracksBrowseView.ts`.

---

## 3. Recommended Approach

**Direct Adjustment (within Epic 9)** — add two new stories to Epic 9, update PRD/Architecture/UX-spec sections noted above. No rollback, no MVP re-scope.

| Path | Effort | Risk | Verdict |
|---|---|---|---|
| **Option 1 — Direct Adjustment (recommended)** | Medium | Low–Medium | ✓ Selected |
| Option 2 — Rollback | N/A | — | Not viable; no completed work to revert. |
| Option 3 — MVP Review | N/A | — | Not viable; this is an additive feature, not a constraint discovery. |

**Justification:**
- Epic 9's stated scope ("expand the Library Browser… into a Jellyfin-like curation surface") *already* envisions track-grain navigation; adding the Tracks mode is filling a gap rather than introducing a new direction.
- The required daemon work is small and pattern-matches Story 9.1/9.2 conventions (capability-gated, paginated, provider-trait-driven).
- The UI work pattern-matches the existing `PlaylistCurationView` — same dual-panel skeleton, with the per-panel pagination machinery already shipped for artist/album root views (Story 9.7). The integration surface is well-understood.
- Track-row actions reuse existing infrastructure (basket add/remove, "Add to playlist" context menu from Story 11.7) — no new contracts needed for actions.
- Risk concentration: SubsonicProvider's "all tracks" enumeration is the only non-trivial unknown. Classic Subsonic predates `search3`; mitigate by capability-gating the mode (a provider that cannot enumerate tracks does not advertise `Tracks` in `list_modes`).

**Timeline impact:** ~2 development stories. No blocking on other epics. Recommend implementing in sequence: 9.9 first (daemon contract), then 9.10 (UI).

---

## 4. Detailed Change Proposals

### 4.1 PRD — FR8

**OLD:**
> FR8: Browse server-supported music navigation modes within the UI: Playlists, Artists, Albums, Genres, Recently Added, Frequently Played, Recently Played, and Favorites.

**NEW:**
> FR8: Browse server-supported music navigation modes within the UI: Playlists, Artists, Albums, **Tracks**, Genres, Recently Added, Frequently Played, Recently Played, and Favorites.

---

### 4.2 PRD — New FR41

**ADD:**
> FR41: The system can present the entire library as a flat Tracks browse mode with a dual-panel artist/album filter layout. The artist filter panel, album filter panel, and track list are each independently paginated with autoload-on-scroll, so libraries with thousands of artists/albums/tracks remain responsive. "All artists" and "All albums" filter entries are provided so the unfiltered global track list is reachable. Track rows expose the standard basket add/remove actions and the "Add to playlist…" context menu (capability-gated on `supports_playlist_write`). The mode is gated by provider capability: providers that cannot enumerate library-wide tracks (e.g., classic Subsonic without `search3`) do not advertise this mode.

---

### 4.3 Architecture — `browse.*` RPC Table

**ADD:**

| RPC | Params | Result |
|---|---|---|
| `browse.listTracks` | `{ libraryId?: string, artistId?: string, albumId?: string, letter?: string, startIndex?: number, limit?: number }` | `{ tracks: Track[], total: number, startIndex: number, limit: number }` |

**Notes:**
- Filters compose: `artistId` narrows to that artist; combined with `albumId` narrows to that album. `letter` is a server-side prefix filter on track title (optional in v1).
- `total` reflects the filtered count.
- Pagination semantics mirror `browse.listAlbums`.

---

### 4.4 Architecture — MediaProvider Trait

**ADD method:**
```rust
async fn list_tracks(&self, filter: TrackListFilter) -> Result<TrackListPage, ProviderError>;
// default impl: Err(ProviderError::NotSupported)
```

**Adapter mappings:**
- **Jellyfin:** `GET /Users/{uid}/Items?IncludeItemTypes=Audio&Recursive=true&SortBy=Name,Album&StartIndex&Limit[&ArtistIds][&AlbumIds][&NameStartsWith]`.
- **Subsonic/OpenSubsonic:** `getSong`/`search3?query=&songCount&songOffset` for unfiltered enumeration; `getArtist`+`getAlbum` aggregation when `artistId` or `albumId` is set. Classic Subsonic without `search3` returns `NotSupported`; OpenSubsonic and Navidrome (which supports `search3`) return tracks.

---

### 4.5 UX Design Spec — §5.1 Navigation

**OLD:**
> a compact browse-mode control for server-supported views: Artists, Albums, Playlists, Genres, Recently Added, Frequently Played, Recently Played, and Favorites.

**NEW:**
> a compact browse-mode control for server-supported views: Artists, Albums, Playlists, **Tracks**, Genres, Recently Added, Frequently Played, Recently Played, and Favorites.

---

### 4.6 UX Design Spec — §5.2 Custom Components

**ADD entry:**
> **Tracks Browse View (dual-panel, paginated):** A library-wide track-browsing surface laid out like the Playlist Curation View — left panel lists artists, right panel lists albums (filtered by the selected artist), and a bottom panel lists tracks (filtered by the selected artist/album). The key difference from curation: each panel is **auto-paginated** against the entire library via the existing `browse.list*` RPC layer (autoload-on-scroll, identical to Artists/Albums root views in Story 9.7). The artist panel offers an "All artists" entry; the album panel an "All albums" entry. Each track row exposes (+) add / (-) remove basket affordances (disabled when no device is selected) and a right-click "Add to playlist…" context menu and a per-row "Send to playlist…" submenu (both gated on `supports_playlist_write`). The grid/list toggle does not apply to this mode — the dual-panel layout is the sole rendering. A–Z filter controls remain available on the artist and album panels.

---

### 4.7 Epics — FR Coverage Map

**ADD:** `FR41: Epic 9 — Tracks Browse Mode (Stories 9.9–9.10)`

---

### 4.8 Epics — Epic 9 New Stories

#### New Story 9.9: Tracks Browse Mode — Provider Contract & Daemon RPC

As a System Admin (Alexis),
I want the daemon to expose a flat, paginated, filterable track listing,
So that the UI can present a library-wide Tracks browse mode for both Jellyfin and OpenSubsonic-class servers.

**Acceptance Criteria:**

**Given** a provider that implements `list_tracks`
**When** `browse.listTracks({ startIndex: 0, limit: 200 })` is called
**Then** the daemon returns the first page of library tracks along with `total`.

**Given** `browse.listTracks` is called with `artistId`
**Then** the response is filtered to tracks whose artist matches.

**Given** `browse.listTracks` is called with `albumId`
**Then** the response is filtered to tracks within that album.

**Given** both `artistId` and `albumId` are provided
**Then** the album filter takes precedence (album implies its artist).

**Given** a Subsonic provider without `search3` support
**When** `browse.listModes` is called
**Then** `Tracks` is not present in the returned list.

**Given** a provider that does not advertise `Tracks`
**When** `browse.listTracks` is called anyway
**Then** an RPC error indicating unsupported capability is returned.

**Given** A–Z letter filtering is implemented (optional v1)
**When** `letter` is provided
**Then** only tracks whose title starts with that letter are returned.

**Technical Notes:**
- `BrowseMode::Tracks` added to `providers/mod.rs`.
- New `TrackListFilter` and `TrackListPage` types.
- Jellyfin: `Items?IncludeItemTypes=Audio&Recursive=true&SortBy=Name,Album&StartIndex&Limit[&ArtistIds][&AlbumIds][&NameStartsWith]`.
- Subsonic/OpenSubsonic: `search3?query=&songCount&songOffset` for unfiltered; aggregate from `getArtist`/`getAlbum` for filtered cases.
- Default trait impl returns `NotSupported`; classic Subsonic provider omits `Tracks` from `list_modes`.
- All Subsonic URL auth sanitization rules apply.
- Tests: unfiltered page, artist filter, album filter, letter filter, `NotSupported` path.

---

#### New Story 9.10: Tracks Browse Mode — Dual-Panel UI with Auto-Pagination & Track Actions

As a Ritualist (Arthur),
I want to browse my entire library at the track grain with artist and album filters,
So that I can quickly find and queue individual songs without drilling through albums.

**Acceptance Criteria:**

**Given** the active provider advertises the Tracks mode
**When** the Library Browser renders the browse-mode bar
**Then** a "Tracks" mode is shown alongside the existing modes.

**Given** I select the Tracks mode
**Then** the view renders three panels: an artists panel on the left, an albums panel on the right, and a track list panel below.

**Given** the Tracks view is rendering
**Then** the artist panel auto-paginates the full library artist list via `browse.listArtists` with autoload-on-scroll.
**And** the album panel auto-paginates albums (filtered by the selected artist if any) via `browse.listAlbums` with autoload-on-scroll.
**And** the track list auto-paginates via `browse.listTracks` with the active artist/album filters.

**Given** the artist panel shows an "All artists" entry at the top
**When** I select it
**Then** the album panel shows all library albums (paginated) and the track panel shows all library tracks (paginated).

**Given** I select an artist in the left panel
**Then** the album panel filters to that artist's albums (paginated).
**And** the track panel filters to that artist's tracks (paginated).

**Given** I select an album in the right panel
**Then** the track panel filters to that album's tracks (paginated).

**Given** the album panel shows an "All albums" entry at the top
**When** I select it
**Then** the track panel filter clears its album constraint (artist constraint, if any, remains).

**Given** a device is selected
**When** a track row renders
**Then** a (+) "Add to basket" control is shown; if the track is already in the basket, a (-) "Remove from basket" control is shown instead.

**Given** no device is selected
**Then** all (+) controls render disabled.

**Given** the active provider supports playlist write
**When** I right-click a track row
**Then** an "Add to playlist…" context menu appears (per Story 11.7).
**And** the track row also renders a visible "Send to playlist…" affordance opening the same flow.

**Given** the active provider does not support playlist write
**Then** both the context menu and the "Send to playlist…" affordance are hidden.

**Given** I am in Tracks mode
**Then** the grid/list view toggle is not displayed (the dual-panel layout is the sole rendering).

**Given** an A–Z letter strip is available on the artist or album panel
**When** I select a letter
**Then** the corresponding panel filters its list and pagination resets.

**Given** I switch away from Tracks mode and back
**Then** the panel selections and scroll positions are restored from the page cache (consistent with other browse modes).

**Technical Notes:**
- New `TracksBrowseView.ts` modeled on `PlaylistCurationView.ts`, but each panel manages its own paginated list state (re-using the autoload-on-scroll logic from `library.ts` artist/album root path).
- `BrowseMode` TS union extended with `"tracks"`.
- New `fetchBrowseTracks(filter)` helper in `rpc.ts`.
- Per-panel state lives in the component, not in the global `library.ts` state, since this view's pagination is multi-axis. Cross-mode page cache key uses `tracks:${artistId ?? '*'}:${albumId ?? '*'}:${letter ?? '*'}`.
- The grid/list global toggle is read but ignored in this view's renderer; the toggle button is hidden when `browseMode === 'tracks'` (similar to how Story 9.8's renderer suppresses behavior in non-applicable modes).
- Track-row context menu re-uses the dispatcher already wired in Story 11.7. No new RPC.
- Per-row "Send to playlist…" is a visible button/icon that calls the same dispatcher — implementation reuses 11.7's dialog.
- i18n keys added (en/fr/es): `library.mode.tracks`, `tracks.view.all_artists`, `tracks.view.all_albums`, `tracks.view.no_tracks`, `tracks.view.loading`, `tracks.view.send_to_playlist`.
- Manual test matrix:
  - Library size: small (<100 tracks), medium (~5k), and large (~50k if available).
  - Filter chains: no filter, artist only, artist+album, A–Z letter.
  - Capabilities: Jellyfin, Navidrome (OpenSubsonic), classic Subsonic (mode hidden).
  - Device selection: no device (controls disabled), with device.
  - Playlist write capability: on (context menu visible) / off (hidden).

---

## 5. Implementation Handoff

**Scope classification:** **Moderate** — new daemon RPC + new top-level browse mode + new UI surface. Two stories, sequenced.

**Routing:**
1. **Product Owner** — add Stories 9.9 and 9.10 to Epic 9 in `epics.md`; update PRD FR8 + FR41; update Architecture `browse.*` table and MediaProvider trait section; update UX Spec §5.1 and §5.2; update FR Coverage Map; update `sprint-status.yaml` with the two new story entries (status: backlog).
2. **Developer agent (Story 9.9)** — implement provider trait method, Jellyfin and Subsonic adapters, `browse.listTracks` RPC handler, capability gating, and tests.
3. **Developer agent (Story 9.10, blocked by 9.9)** — implement `TracksBrowseView.ts`, browse-mode bar wiring, panel pagination, A–Z, track-row actions (basket + playlist), and tests. Manually verify against Jellyfin and Navidrome on a non-trivial library.

**Success criteria:**
- Tracks mode appears in the browse-mode bar on Jellyfin and Navidrome connections.
- Tracks mode is absent on a classic Subsonic connection lacking `search3`.
- Selecting an artist and then an album narrows the track list correctly.
- Each of the three panels auto-paginates without blocking the UI on a library of thousands of items.
- Track rows correctly toggle (+)/(-) basket state and respect the no-device-selected disabled rule.
- "Add to playlist…" context menu and per-row "Send to playlist…" affordance both work and are hidden when `supports_playlist_write` is false.
- Navigating away from Tracks mode and back restores the panel selections and scroll positions.

---

## 6. Open Questions / Defer-to-Implementation

1. **A–Z filter on tracks:** Specified as optional in v1 — implementer may defer to a follow-up if upstream `NameStartsWith` semantics differ between Jellyfin and Subsonic.
2. **Sort order:** Default is title-ascending; whether to expose a sort selector (by Date Added, Play Count, Artist) is **out of scope for this story** — propose as a follow-up if user demand emerges.
3. **Server-side track search:** This view does *not* embed a free-text search box; that remains the role of the existing `browse.search` RPC. Combining tracks-mode filters with a free-text query is a possible follow-up but is not required by this proposal.
