# Sprint Change Proposal - Rich Media Library Navigation

**Project:** HifiMule / JellyfinSync  
**Date:** 2026-05-22  
**Workflow:** bmad-correct-course  
**Prepared for:** Alexis  
**Mode:** Batch review assumed; user did not specify Incremental vs Batch.

## 1. Issue Summary

The current media library navigation is too narrow for real curation work. The implemented and documented flow primarily supports hierarchical browsing through libraries, artists, albums, playlists, and tracks. Alexis requested Jellyfin-like navigation surfaces such as genre, recently added, frequently played/read, recently played/read, and favorites.

This is a user-facing product capability gap rather than a technical failure. It matters because HifiMule's value proposition is metadata-aware curation for dedicated audio players. Limiting navigation to artist/album makes the app feel less like a physical extension of the media server and more like a basic folder browser.

Evidence from current artifacts:

- PRD FR8 currently says users can browse server Playlists, Artists, and Albums.
- The original epic inventory still says FR8 covers Playlists, Genres, and Artists, creating a planning mismatch.
- Epic 3 Story 3.1 focuses on playlists/albums and a Library -> Artist -> Album -> Tracks hierarchy.
- Architecture currently documents browse RPCs for libraries, artists, albums, and playlists only.
- Current UI state in `hifimule-ui/src/library.ts` is built around `view: 'libraries' | 'items'`, one `parentId`, one breadcrumb stack, and artist-specific quick-nav.
- Auto-Fill already relies on favorites, play count, and creation date, but those server-side concepts are not exposed as manual browse modes.

Terminology note: the user said "frequently read" and "recently read." For music-domain UI and provider contracts, this proposal maps those to "Frequently Played" and "Recently Played" while keeping the broader intent: server history-based navigation.

## 2. Impact Analysis

### Checklist Progress

| Item | Status | Notes |
| --- | --- | --- |
| 1.1 Triggering story | [N/A] | No active triggering story. Sprint status marks Epic 3 and Epic 8 complete; this is a post-implementation navigation expansion. |
| 1.2 Core problem | [x] | New stakeholder/product requirement: browse modes are too limited for Jellyfin-like curation. |
| 1.3 Evidence | [x] | PRD, Epic 3, Architecture, UX, current UI/RPC implementation all show a narrower navigation surface. |
| 2.1 Current epic impact | [x] | Epic 3 remains valid but incomplete relative to the new navigation expectation. |
| 2.2 Epic-level changes | [!] | Add a new backlog epic rather than reopen done Epic 3. |
| 2.3 Remaining epics | [x] | Epic 8 provider abstraction is directly affected; sync, basket, and device epics stay structurally valid. |
| 2.4 New epic needed | [x] | New Epic 9: Rich Library Navigation is recommended. |
| 2.5 Priority/order | [x] | Implement after provider abstraction is stable; no rollback required. |
| 3.1 PRD conflict | [x] | FR8 must expand beyond Playlists, Artists, Albums. |
| 3.2 Architecture conflict | [x] | MediaProvider trait, provider adapters, RPC contract, and UI state model need updates. |
| 3.3 UI/UX conflict | [x] | UX needs a browse-mode navigation pattern, not only hierarchical breadcrumb navigation. |
| 3.4 Other artifacts | [!] | Sprint status should add the new epic after approval. Generated docs should be refreshed after implementation. |
| 4.1 Direct adjustment | [x] Viable | Add a new epic and targeted stories. Effort: Medium. Risk: Medium. |
| 4.2 Rollback | [x] Not viable | No completed work needs reverting. Existing artist/album navigation remains useful. |
| 4.3 PRD MVP review | [x] Viable but not required | MVP can remain achievable if rich navigation is treated as a backlog expansion or next release scope. |
| 4.4 Recommended path | [x] | Direct adjustment plus backlog addition. |
| 5.1-5.5 Proposal components | [x] | Included below. |
| 6.1-6.5 Final review/handoff | [!] | Requires user approval before sprint-status.yaml is updated or implementation begins. |

### Epic Impact

Epic 3, "The Curation Hub," is the directly impacted area. Its completed stories remain useful, especially Story 3.1, Story 3.7, Story 3.8, and Story 3.9. The requested change does not invalidate those stories. It adds a richer root navigation model above them.

Epic 8, "Multi-Provider Media Server Support," is also affected. The provider abstraction already makes this change feasible, but the trait currently exposes artists, albums, playlists, search, downloads, artwork, changes, and scrobbling. It needs explicit browse-mode methods and capability reporting for provider-specific support.

No sync engine rollback is needed. Basket behavior can continue to rely on item IDs and container expansion. A new genre entity type is optional but recommended so genre browsing can produce a first-class syncable entity, matching existing artist behavior.

### Story Impact

Existing stories to amend:

- Story 3.1: broaden from artist/album/playlist hierarchy to rich browse modes.
- Story 3.7: update cache and quick-nav assumptions so caches are keyed by browse mode and parent, not only parent.
- Story 3.9: use as the model for a new Genre Entity Basket Item story if genre-level sync is included.
- Story 8.1: extend `MediaProvider` domain models and provider methods.
- Story 8.2 and 8.3: add Jellyfin/Subsonic implementations for supported browse modes.

New stories recommended:

- Story 9.1: Provider Browse Modes and Capability Contract.
- Story 9.2: Browse Mode Navigation UI.
- Story 9.3: Genre Browsing and Genre Entity Basket Item.
- Story 9.4: History and Favorites Browse Modes.

### Artifact Conflicts

PRD:

- FR8 is too narrow.
- The user journey references "Recently Added" as a Jellyfin playlist, but the requested behavior is a first-class browse mode.
- FR29 already depends on favorites, play count, and creation date. That logic should be reused conceptually but not coupled to manual browsing.

Architecture:

- `MediaProvider` needs provider-level methods for rich browse modes.
- The current architecture says browse methods map to exact provider calls and no generic dispatch exists. Preserve this by adding explicit methods instead of one loose `browse(mode)` endpoint.
- `jellyfin_get_views` and `jellyfin_get_items` are legacy RPC names and can remain temporarily, but the richer contract should be documented under `browse.*`.
- Provider capability reporting must decide which tabs are shown. Unsupported modes should be hidden or disabled by capability, not allowed to fail at click time.

UX:

- The Library Browser needs browse-mode navigation using tabs or a compact segmented control.
- Breadcrumbs still apply inside hierarchical modes such as Artists, Albums, Playlists, and Genres.
- Smart/history modes should use sorted list/grid views with metadata cues such as date added, play count, last played, or favorite state.
- The existing device-locked state must continue to disable add buttons across all modes.

Technical/code:

- `hifimule-daemon/src/providers/mod.rs`
- `hifimule-daemon/src/providers/jellyfin.rs`
- `hifimule-daemon/src/providers/subsonic.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-ui/src/library.ts`
- `hifimule-ui/src/components/MediaCard.ts`
- `hifimule-ui/src/state/basket.ts`
- Generated docs under `docs/` after implementation.

## 3. Recommended Approach

Recommended path: Direct Adjustment with a new backlog epic.

Do not reopen the completed Epic 3 as if the earlier work failed. Instead, add Epic 9: Rich Library Navigation. This preserves existing velocity and makes the change easy to route as a coherent feature set.

Effort estimate: Medium.

Risk level: Medium.

Primary risks:

- Provider parity: Jellyfin and Subsonic/OpenSubsonic do not expose all smart/history modes in exactly the same shape.
- UI complexity: adding tabs, smart lists, breadcrumbs, caching, and basket selection can make `library.ts` hard to maintain if done as a patchwork.
- Dynamic entity semantics: adding genres as basket entities raises the same "resolve at sync time" question already solved for artists.

Risk reducers:

- Add provider capability reporting and hide unsupported modes.
- Preserve explicit provider methods instead of generic stringly typed browse dispatch.
- Reuse existing basket metadata fetch and artist entity patterns.
- Keep Auto-Fill separate from manual browse modes; do not make favorites/recently-added modes dynamic basket slots in this change.

## 4. Detailed Change Proposals

### PRD Modification - FR8

Section: Functional Requirements -> 3. Content Selection & Browsing

OLD:

```markdown
- **FR8:** Users can browse server Playlists, Artists, and Albums from the connected media server, regardless of server type. The provider abstraction normalizes the library tree across Jellyfin (unified `/Items` query) and Subsonic (method-per-level: `getArtists` -> `getArtist` -> `getAlbum`).
```

NEW:

```markdown
- **FR8:** Users can browse music from the connected media server through server-supported navigation modes: Playlists, Artists, Albums, Genres, Recently Added, Frequently Played, Recently Played, and Favorites. The provider abstraction normalizes these browse modes across Jellyfin, Navidrome, Subsonic, and OpenSubsonic-compatible servers. Unsupported modes are hidden or clearly unavailable based on provider capabilities.
```

Rationale:

This updates the product requirement to match the requested Jellyfin-like library navigation while protecting multi-provider scope through capability-based behavior.

### PRD Modification - User Journey

Section: User Journeys -> Arthur's Weekly Ritual

OLD:

```markdown
Arthur opens the UI, which automatically highlights 50 new tracks in his "Recently Added" Jellyfin playlist. He clicks "Sync".
```

NEW:

```markdown
Arthur opens the UI, switches to Recently Added, reviews the newest music from his server, and adds selected albums or tracks to the basket. He clicks "Sync".
```

Rationale:

This avoids treating Recently Added as a playlist-only concept and aligns the journey with first-class navigation modes.

### Epic Modification - Story 3.1

Story: 3.1 Immersive Media Browser (Multi-Server Integration)  
Section: User Story and Acceptance Criteria

OLD:

```markdown
As a Ritualist (Arthur),
I want to browse my Jellyfin playlists and albums with high-quality artwork,
So that I can enjoy the curation process as I do on the server.

...

**And** the browse hierarchy (Library -> Artist -> Album -> Tracks) works identically regardless of server type.
```

NEW:

```markdown
As a Ritualist (Arthur),
I want to browse my media server library through familiar music views such as Artists, Albums, Playlists, Genres, Recently Added, Frequently Played, Recently Played, and Favorites,
So that HifiMule feels like a curation surface for my server rather than only an artist/album tree.

...

**And** the Library Browser exposes provider-supported browse modes in a stable navigation control.
**And** hierarchical modes preserve breadcrumb navigation, synced badges, artwork, pagination, and add-to-basket behavior.
**And** history and favorites modes display music-only results sorted by the matching server metadata.
**And** unsupported modes are hidden or marked unavailable based on provider capabilities.
```

Rationale:

This reframes the original media browser story around the broader navigation expectation without removing existing behavior.

### Epic Addition - Epic 9: Rich Library Navigation

NEW:

```markdown
## Epic 9: Rich Library Navigation

Expand the Library Browser from artist/album hierarchy into a Jellyfin-like curation surface with provider-supported navigation modes for genres, recently added, frequently played, recently played, and favorites.
```

Rationale:

Epic 3 is already done in sprint status. A new epic is cleaner than reopening completed work and gives Product/Dev a focused backlog unit.

### New Story - 9.1 Provider Browse Modes and Capability Contract

NEW:

```markdown
### Story 9.1: Provider Browse Modes and Capability Contract

As a System Admin (Alexis),
I want the daemon provider layer to expose supported browse modes explicitly,
So that the UI can show Jellyfin-like navigation without hardcoding server-specific API behavior.

Acceptance Criteria:

Given a provider is connected
When the UI requests available browse modes
Then the daemon returns the modes supported by that provider: artists, albums, playlists, genres, recentlyAdded, frequentlyPlayed, recentlyPlayed, favorites.

Given a browse mode is unsupported by the active provider
Then the UI does not offer it as an active navigation path.

Given browse data is requested
Then all calls go through `Arc<dyn MediaProvider>` and no UI or RPC handler constructs server-specific URLs directly.

Technical Notes:
- Extend domain models with `Genre` and optional browse metadata fields such as `dateAdded`, `lastPlayedAt`, `playCount`, and `isFavorite`.
- Extend `Capabilities` or add `BrowseCapabilities` with boolean support per mode.
- Add explicit trait methods rather than a generic string dispatch:
  - `list_genres(library_id: Option<&str>)`
  - `get_genre_tracks(genre_id_or_name: &str)`
  - `list_recently_added(library_id: Option<&str>, limit: u32, offset: u32)`
  - `list_frequently_played(library_id: Option<&str>, limit: u32, offset: u32)`
  - `list_recently_played(library_id: Option<&str>, limit: u32, offset: u32)`
  - `list_favorites(library_id: Option<&str>, limit: u32, offset: u32)`
- Preserve the architecture rule that server API details stay inside `providers/`.
```

Rationale:

This creates a stable backend contract before changing the UI.

### New Story - 9.2 Browse Mode Navigation UI

NEW:

```markdown
### Story 9.2: Browse Mode Navigation UI

As a Ritualist (Arthur),
I want a clear browse-mode control in the Library Browser,
So that I can switch between Artists, Albums, Playlists, Genres, Recently Added, Frequently Played, Recently Played, and Favorites without losing basket context.

Acceptance Criteria:

Given the main UI is open and a server is connected
When supported browse modes are returned by the daemon
Then the Library Browser renders them as a compact tab or segmented navigation control.

Given I switch browse modes
Then the current basket remains unchanged and the library content refreshes to the selected mode.

Given I browse into a hierarchical item
Then breadcrumbs continue to work within that mode.

Given I return to a previous browse mode
Then scroll and page cache restore for that mode when valid.

Given no device is selected
Then add buttons are disabled in every browse mode.

Technical Notes:
- Refactor `library.ts` state to include `browseMode`.
- Cache key should include both browse mode and parent ID, e.g. `${browseMode}:${parentId ?? 'root'}`.
- Keep artist quick-nav, and consider applying the same pattern to genres if result count is large.
- Preserve existing `MediaCard` selection overlay and synced badge behavior.
```

Rationale:

This updates the UX without disrupting the basket-centric layout.

### New Story - 9.3 Genre Browsing and Genre Entity Basket Item

NEW:

```markdown
### Story 9.3: Genre Browsing and Genre Entity Basket Item

As a Ritualist (Arthur),
I want to browse by genre and add a genre to the basket as a single entity,
So that my device can receive a dynamic genre-based selection without manually picking every album.

Acceptance Criteria:

Given the active provider supports genres
When I open the Genres browse mode
Then I see a music-only list/grid of genres.

Given I click a genre
Then I can view the tracks or albums associated with that genre.

Given I click (+) on a genre
Then a single Genre card is added to the basket.

Given sync starts with a Genre card in the basket
Then the daemon resolves the current track list for that genre at sync time.

Given artist, album, playlist, track, and genre selections overlap
Then duplicates are removed during sync planning.

Technical Notes:
- Add `BasketItem.type: 'MusicGenre'`.
- Use the existing Artist entity pattern as the model.
- Genre entity size and track count may be estimates at add time, then resolved exactly at sync time.
```

Rationale:

Genre is not just a view; it is a natural sync intent. Treating it as an entity keeps it consistent with Artist basket behavior.

### New Story - 9.4 History and Favorites Browse Modes

NEW:

```markdown
### Story 9.4: History and Favorites Browse Modes

As a Convenience Seeker (Sarah),
I want quick access to Recently Added, Frequently Played, Recently Played, and Favorites,
So that I can build a device basket from the music I am most likely to want offline.

Acceptance Criteria:

Given the active provider supports Recently Added
When I open Recently Added
Then the newest music items are shown first.

Given the active provider supports Frequently Played
When I open Frequently Played
Then items are sorted by server play count descending.

Given the active provider supports Recently Played
When I open Recently Played
Then items are sorted by last played date descending.

Given the active provider supports Favorites
When I open Favorites
Then favorited music items are shown.

Given a mode returns tracks directly
Then track cards can be added to the basket with existing metadata and size calculation behavior.

Technical Notes:
- Keep these as manual browse result views, not dynamic basket slots.
- Auto-Fill remains the only dynamic priority slot for this change.
- Display relevant metadata when available: play count, last played date, date added, favorite state.
```

Rationale:

This captures the Jellyfin-style smart views without increasing sync semantics beyond the requested navigation improvement.

### Architecture Modification - MediaProvider Contract

Section: Media Provider Layer

OLD:

```rust
pub trait MediaProvider: Send + Sync {
    async fn list_libraries(&self) -> Result<Vec<Library>, ProviderError>;
    async fn list_artists(&self, library_id: Option<&str>) -> Result<Vec<Artist>, ProviderError>;
    async fn get_artist(&self, id: &str) -> Result<ArtistWithAlbums, ProviderError>;
    async fn get_album(&self, id: &str) -> Result<AlbumWithTracks, ProviderError>;
    async fn search(&self, query: &str) -> Result<SearchResult, ProviderError>;
    fn download_url(&self, track_id: &str, profile: &TranscodeProfile) -> Result<Url, ProviderError>;
    fn cover_art_url(&self, item_id: &str, size: u32) -> Result<Url, ProviderError>;
    async fn list_playlists(&self) -> Result<Vec<Playlist>, ProviderError>;
    async fn get_playlist(&self, id: &str) -> Result<PlaylistWithTracks, ProviderError>;
    async fn changes_since(&self, since: SystemTime) -> Result<Vec<ChangeEvent>, ProviderError>;
    fn server_type(&self) -> ServerType;
    fn capabilities(&self) -> &Capabilities;
}
```

NEW:

```rust
pub trait MediaProvider: Send + Sync {
    async fn list_libraries(&self) -> Result<Vec<Library>, ProviderError>;
    async fn list_artists(&self, library_id: Option<&str>) -> Result<Vec<Artist>, ProviderError>;
    async fn get_artist(&self, artist_id: &str) -> Result<ArtistWithAlbums, ProviderError>;
    async fn list_albums(&self, library_id: Option<&str>) -> Result<Vec<Album>, ProviderError>;
    async fn get_album(&self, album_id: &str) -> Result<AlbumWithTracks, ProviderError>;
    async fn list_playlists(&self) -> Result<Vec<Playlist>, ProviderError>;
    async fn get_playlist(&self, playlist_id: &str) -> Result<PlaylistWithTracks, ProviderError>;
    async fn list_genres(&self, library_id: Option<&str>) -> Result<Vec<Genre>, ProviderError>;
    async fn get_genre_tracks(&self, genre_id_or_name: &str) -> Result<Vec<Song>, ProviderError>;
    async fn list_recently_added(&self, library_id: Option<&str>, limit: u32, offset: u32) -> Result<Vec<Song>, ProviderError>;
    async fn list_frequently_played(&self, library_id: Option<&str>, limit: u32, offset: u32) -> Result<Vec<Song>, ProviderError>;
    async fn list_recently_played(&self, library_id: Option<&str>, limit: u32, offset: u32) -> Result<Vec<Song>, ProviderError>;
    async fn list_favorites(&self, library_id: Option<&str>, limit: u32, offset: u32) -> Result<Vec<Song>, ProviderError>;
    async fn search(&self, query: &str) -> Result<SearchResult, ProviderError>;
    async fn download_url(&self, song_id: &str, profile: Option<&TranscodeProfile>) -> Result<String, ProviderError>;
    async fn cover_art_url(&self, cover_art_id: &str) -> Result<String, ProviderError>;
    async fn changes_since(&self, token: Option<&str>) -> Result<Vec<ChangeEvent>, ProviderError>;
    async fn scrobble(&self, request: ScrobbleRequest) -> Result<(), ProviderError>;
    fn server_type(&self) -> ServerType;
    fn capabilities(&self) -> &Capabilities;
}
```

Rationale:

This keeps provider behavior explicit and testable. The exact current trait already differs from the architecture document, so the architecture should be amended to reflect the current async URL and token-based `changes_since` signatures while adding the new methods.

### Architecture Modification - RPC Contract

Section: Library Browsing - Multi-Provider RPC Contract

NEW methods:

```markdown
| Method | Params | Returns |
| --- | --- | --- |
| `browse.listModes` | - | `{ modes: BrowseMode[] }` |
| `browse.listAlbums` | `{ libraryId?: string, startIndex?: number, limit?: number }` | `{ albums: Album[], total: number }` |
| `browse.listGenres` | `{ libraryId?: string, startIndex?: number, limit?: number }` | `{ genres: Genre[], total: number }` |
| `browse.getGenre` | `{ genreIdOrName: string, startIndex?: number, limit?: number }` | `{ genre: Genre, tracks: Track[], total: number }` |
| `browse.listRecentlyAdded` | `{ libraryId?: string, startIndex?: number, limit?: number }` | `{ tracks: Track[], total: number }` |
| `browse.listFrequentlyPlayed` | `{ libraryId?: string, startIndex?: number, limit?: number }` | `{ tracks: Track[], total: number }` |
| `browse.listRecentlyPlayed` | `{ libraryId?: string, startIndex?: number, limit?: number }` | `{ tracks: Track[], total: number }` |
| `browse.listFavorites` | `{ libraryId?: string, startIndex?: number, limit?: number }` | `{ tracks: Track[], total: number }` |
```

Rationale:

This avoids overloading `jellyfin_get_items` further and gives the UI a stable multi-provider contract.

### UX Modification - Library Browser Navigation

Section: Component Strategy -> Library Grid and Navigation

OLD:

```markdown
*   **Navigation:** Vertical sidebar using `<sl-tree>` for folder exploration and `<sl-tab-group>` for views.
```

NEW:

```markdown
*   **Navigation:** The Library Browser includes a compact browse-mode control for server-supported views: Artists, Albums, Playlists, Genres, Recently Added, Frequently Played, Recently Played, and Favorites. Hierarchical modes use breadcrumbs; smart/history modes use sorted music grids or lists with relevant metadata. Unsupported provider modes are hidden or unavailable based on daemon capabilities.
```

Rationale:

The UX spec already hinted at tabs for views but does not define the actual Jellyfin-like navigation model.

## 5. Implementation Handoff

Scope classification: Moderate.

Route to:

- Product Owner / Developer for backlog reorganization.
- Developer agent for implementation after the new epic/stories are approved.
- Architect only if the team wants to revisit explicit RPCs vs a generic browse endpoint. This proposal recommends explicit methods to preserve current architecture rules.

Recommended sequence:

1. Approve proposal and add Epic 9 plus Stories 9.1-9.4 to `epics.md`.
2. Update `sprint-status.yaml` with `epic-9: backlog` and the four new stories as backlog.
3. Update PRD FR8 and the Arthur journey.
4. Update architecture browse contract and provider trait contract.
5. Update UX navigation section.
6. Implement Story 9.1 provider contract and tests.
7. Implement Story 9.2 UI navigation.
8. Implement Story 9.3 genre entity behavior.
9. Implement Story 9.4 history/favorites modes.
10. Regenerate or refresh generated project docs after code implementation.

Success criteria:

- Users can navigate by Artists, Albums, Playlists, Genres, Recently Added, Frequently Played, Recently Played, and Favorites when supported by the active provider.
- Unsupported provider modes do not produce broken UI states.
- Existing artist/album/playlist navigation and basket behavior continue to work.
- Genre can be added as a dynamic entity if Story 9.3 is accepted.
- History/favorites browse modes are manual selection surfaces, not hidden Auto-Fill side effects.
- All server API calls remain inside provider adapters.
- New behavior is covered by provider unit tests and UI interaction tests.

## 6. Approval State

Status: Approved by Alexis on 2026-05-22.

Approval granted for:

- Editing `prd.md`, `epics.md`, `architecture.md`, or `ux-design-specification.md`.
- Updating `_bmad-output/implementation-artifacts/sprint-status.yaml`.
- Routing the new Epic 9 backlog to implementation planning.

Recommended approval decision:

Approved: add Epic 9 as a new backlog epic and keep Auto-Fill separate from manual browse modes for this change.
