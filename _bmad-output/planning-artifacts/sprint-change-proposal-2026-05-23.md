# Sprint Change Proposal - 2026-05-23

## 1. Issue Summary

Story 9.4 specified Favorites too narrowly as "favorited music items" and its implementation reasonably produced a flat favorited-track view. During follow-up implementation work, the intended product behavior was clarified: Favorites should preserve the normal Artists -> Albums -> Tracks navigation and include favorite artists and favorite albums, not only favorite songs.

Evidence from the current implementation shows a partial hierarchical approach already started: the UI has a `FavoriteTree`, a `fetchBrowseFavoriteItems()` RPC wrapper, Favorites-specific loaders, and provider-side `list_favorite_items()` implementations. The planning spec did not yet describe that behavior.

## 2. Impact Analysis

**Epic Impact:** Epic 9 remains the impacted area. It moves from `done` back to `in-progress` until hierarchical Favorites is implemented and verified.

**Story Impact:** Story 9.4 remains done for history modes and flat metadata behavior. A new Story 9.5, "Hierarchical Favorites Navigation", captures the corrected Favorites scope.

**Artifact Conflicts:** The Epic 9 section in `epics.md` needed a new story because the old 9.4 acceptance criteria did not cover favorite artists, favorite albums, or hierarchy-dependent track expansion.

**Technical Impact:** No new basket entity type is required. The existing provider abstraction gains or keeps `list_favorite_items()` returning favorite artists, albums, and songs. The UI derives a cached favorites tree and routes Favorites navigation through existing artist/album/track card behavior.

## 3. Recommended Approach

Use a direct adjustment: add Story 9.5 to Epic 9 and update sprint status. This is a moderate correction to a completed epic, but it does not require a PRD rewrite or architecture replan because the partial implementation already follows the provider-neutral browse model.

Risk is mostly in provider parity and edge cases: Jellyfin and Subsonic/OpenSubsonic may expose favorite entity data differently, and the UI must avoid falling back to a broken flat root.

## 4. Detailed Change Proposals

**Stories**

Add Story 9.5: Hierarchical Favorites Navigation.

Key acceptance change:
- Favorites root shows favorite artists plus artists inferred from favorite albums/tracks.
- Favorite artist selection shows all artist albums.
- Non-favorite artist selection shows only favorite albums or albums containing favorite tracks.
- Favorite album or album under favorite artist shows all tracks.
- Non-favorite album shows only favorite tracks.

**Epics**

Append Story 9.5 under Epic 9 to correct the Favorites spec while preserving Story 9.4 as completed history/favorites foundation work.

**Sprint Status**

Set `epic-9` back to `in-progress` and add `9-5-hierarchical-favorites-navigation: ready-for-dev`.

## 5. Implementation Handoff

Scope classification: Moderate.

Route to Developer agent for implementation of Story 9.5, using the partial favorite-tree work already present in the working tree. Success criteria are the Story 9.5 acceptance criteria plus `rtk tsc`, `rtk cargo test -p hifimule-daemon`, and manual smoke tests for favorite artist, favorite album, and isolated favorite track cases.
