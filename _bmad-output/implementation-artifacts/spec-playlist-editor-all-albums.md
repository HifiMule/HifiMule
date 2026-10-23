---
title: 'Show playlist albums before selecting an artist'
type: 'bugfix'
created: '2026-10-23'
status: 'done'
baseline_commit: 'f78916740b0c8db5f7744e06537eba9badbe7627'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The playlist editor replaces its album list with “Select an artist to see albums” while “All artists” is selected. Unlike the Tracks view, users cannot browse or filter by an album directly after opening a playlist.

**Approach:** Show “All albums” and the playlist’s albums immediately. Selecting an artist narrows the album list; selecting an album filters the tracks independently of whether an artist is selected. Reuse the editor’s existing rows, localized labels, and playlist mutation operations.

## Boundaries & Constraints

**Always:** Derive albums exclusively from the loaded playlist. Preserve playlist track order, absolute positions, statistics, escaping, native selection buttons, and pressed states. Retain the editor’s existing artist-name/album-name grouping: album rows must carry both values, including empty strings for missing metadata. Distinguish same-title albums by artist in the global list. Clear the album filter when switching artists or selecting “All artists,” matching the Tracks view’s reset behavior. Preserve existing mutation guards and error handling.

**Ask First:** Changes to playlist grouping semantics beyond the existing artist/album pair, provider contracts, or backend playlist operations.

**Never:** Populate the pane with library albums absent from the playlist; add RPCs, dependencies, pagination, A–Z controls, or a shared browser framework; redesign the layout; change the Tracks view or unrelated playlist features.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|----------------------------|----------------|
| Open playlist | Tracks from multiple artists and albums | “All artists” and “All albums” selected; sorted album rows shown; all tracks retain playlist order | Existing load error display remains |
| Direct album selection | “All artists” selected | Only tracks matching the row’s artist/album pair appear; artist selection stays global | No RPC for filtering |
| Duplicate album title | Two artists have “Greatest Hits” | Separate distinguishable rows; selection/removal targets only the chosen artist’s pair | Existing remove failure rollback remains |
| Change artist | An album filter is active | New artist’s albums appear and album selection resets to “All albums” | No stale selection retained |
| Clear filters | Album selected, then “All albums” or “All artists” clicked | “All albums” retains the artist scope; “All artists” restores every playlist album and track | No playlist mutation |
| Missing metadata | Empty artist/album values and real names resembling fallback labels | Separate groups with localized fallback labels; selection/removal uses raw values | Metadata never becomes executable markup |
| Playlist changes | Add/remove causes an album or artist to appear/disappear | Both panels refresh; invalid selections clear; surviving selections remain coherent | Existing RPC failure restores tracks and a valid filter state |
| Empty playlist | Zero tracks | “All albums” remains available with no album rows; existing artist/track empty states remain | No selection crash |

</frozen-after-approval>

## Code Map

- `hifimule-ui/src/components/PlaylistCurationView.ts` — full/partial rendering, filters, row events, removal. Both render paths hide global albums; `getTracksForPanel()` bypasses global album filtering.
- `hifimule-ui/src/components/TracksBrowseView.ts` — reference: `renderAlbumPanel()` always adds “All albums”; `selectArtist()` clears album selection.
- `hifimule-ui/tests/playlistCurationHardening.test.mjs` — existing Node/TypeScript VM regression harness.
- `hifimule-ui/tests/playlist-accessibility.html` — browser fixture with mocked RPCs, multiple artists/albums, and missing metadata.
- `hifimule-ui/src/styles.css` and `hifimule-i18n/catalog.json` — reuse existing curation styles and localized labels.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-ui/src/components/PlaylistCurationView.ts` — derive sorted artist/album rows for the selected artist or the whole playlist and use the same derivation in full and partial renders; remove the select-artist placeholder gate.
- [x] `hifimule-ui/src/components/PlaylistCurationView.ts` — preserve pair identity in selected album state, rendering, and event bindings; apply album filtering globally; clear selection on artist changes; keep selected states and album removal targets consistent after mutations.
- [x] `hifimule-ui/tests/playlistCurationHardening.test.mjs` — cover matrix cases through the existing harness, including row events, same-title removal targets, full/partial rendering, missing metadata, and empty/mutated playlists. Mock mutations so checks cannot edit a real playlist.

**Acceptance Criteria:**
- Given the playlist editor is opened, when the playlist loads, then the album pane offers the same immediate album navigation pattern as Tracks, scoped to playlist contents.
- Given keyboard navigation, when a user activates an album or reset button, then the visible tracks and button pressed states agree with the resulting filters.
- Given an album filter, when tracks are displayed or reordered, then their order and displayed positions continue to refer to the complete playlist.

## Spec Change Log

## Verification

**Commands:**
- `rtk node --test hifimule-ui/tests/playlistCurationHardening.test.mjs` — existing and new behavioral regressions pass.
- `rtk npm --prefix hifimule-ui run build` — TypeScript and Vite production build pass.
- `rtk git diff --check` — no whitespace errors.

**Browser check:** Use `hifimule-ui/tests/playlist-accessibility.html` with the local Vite server to verify the actual component’s initial album list, artist/album selection and reset, and keyboard pressed states. Use the fixture’s mocked RPCs for removal checks.

**Implementation checks (2026-10-23):** All 11 Node regression tests pass, including global album navigation, pair identity, pressed states, mutation rollback, missing metadata, and absolute reorder positions. TypeScript/Vite production build and `git diff --check` pass. Browser fixture verified initial global albums, keyboard Enter selection and Space reset, artist scope/reset, pressed states, absolute track positions, and mocked album removal. Three independent reviews completed; one accessibility patch added the artist to global album removal labels and regression coverage. All checks pass after the patch.

## Suggested Review Order

**Album lists and filter identity**

- Build global or artist-scoped pairs once, preserving valid selections.
  [PlaylistCurationView.ts:123](../../hifimule-ui/src/components/PlaylistCurationView.ts#L123)

- Expose every playlist album with unambiguous visible and accessible labels.
  [PlaylistCurationView.ts:167](../../hifimule-ui/src/components/PlaylistCurationView.ts#L167)

**Selection events**

- Apply pair selection without requiring an artist and keep pressed states synchronized.
  [PlaylistCurationView.ts:456](../../hifimule-ui/src/components/PlaylistCurationView.ts#L456)

**Regression coverage**

- Verify immediate albums, resets, duplicate titles, rollback, missing metadata, and absolute ordering.
  [playlistCurationHardening.test.mjs:96](../../hifimule-ui/tests/playlistCurationHardening.test.mjs#L96)
