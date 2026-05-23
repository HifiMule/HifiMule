# Story 9.6: Navidrome/Subsonic Browse Parity Hardening

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a Navidrome/Subsonic user,
I want the browse surface to expose every history/navigation mode the active server can support,
so that switching from Jellyfin does not remove core curation workflows.

## Acceptance Criteria

1. **Given** a Navidrome/OpenSubsonic server exposes a reliable Recently Added album endpoint, **When** `browse.listModes` is called, **Then** `recentlyAdded` is included. **And** `browse.listRecentlyAdded` returns newest albums first.

2. **Given** a Navidrome/OpenSubsonic server exposes reliable frequent listening data, **When** `browse.listModes` is called, **Then** `frequentlyPlayed` is included. **And** `browse.listFrequentlyPlayed` returns tracks sorted by server play count descending.

3. **Given** a Navidrome/OpenSubsonic server exposes reliable recent listening data, **When** `browse.listModes` is called, **Then** `recentlyPlayed` is included. **And** `browse.listRecentlyPlayed` returns tracks sorted by last played date descending.

4. **Given** classic Subsonic cannot support a mode reliably, **When** capabilities are calculated, **Then** the mode remains hidden. **And** the daemon returns `UnsupportedCapability` if the mode is called directly.

5. **Given** Albums mode is open and the result count warrants quick navigation, **When** the active provider is Subsonic/Navidrome, **Then** alphabetic filtering works consistently with Jellyfin album quick navigation.

6. **Given** provider support differs by server, **When** the UI renders browse modes, **Then** all capability decisions are made in `SubsonicProvider`, not in the UI.

## Tasks / Subtasks

- [ ] Task 1: Refine Subsonic/OpenSubsonic browse capabilities (AC: 1-4, 6)
  - [ ] Detect whether the connected server can reliably support recently added, frequently played, and recently played modes.
  - [ ] Include supported modes in `SubsonicProvider.capabilities().browse.list_modes`.
  - [ ] Keep unsupported modes hidden and returning `UnsupportedCapability` when called directly.
  - [ ] Keep capability decisions inside `SubsonicProvider`.

- [ ] Task 2: Implement supported history navigation methods (AC: 1-3)
  - [ ] Implement `list_recently_added` for servers with reliable newest-album support.
  - [ ] Implement `list_frequently_played` for servers with reliable play-count sorting.
  - [ ] Implement `list_recently_played` for servers with reliable last-played sorting.
  - [ ] Preserve provider-domain normalization for IDs, duration, bitrate, cover art, dates, and play counts.

- [ ] Task 3: Harden album letter filtering for Subsonic/Navidrome (AC: 5)
  - [ ] Verify `browse.listAlbums` forwards `letter` to `MediaProvider::list_albums`.
  - [ ] Ensure Subsonic/Navidrome album letter filtering matches Jellyfin quick-nav expectations.
  - [ ] Include `#`/non-alpha behavior if the UI sends that value.

- [ ] Task 4: Keep the UI provider-neutral (AC: 6)
  - [ ] Do not add Navidrome/Subsonic-specific mode branching in `hifimule-ui/src/library.ts`.
  - [ ] Continue rendering modes from `browse.listModes`.
  - [ ] Ensure unsupported modes do not show as broken buttons.

- [ ] Task 5: Verification (AC: 1-6)
  - [ ] Add provider tests for OpenSubsonic/Navidrome capability lists.
  - [ ] Add provider tests for classic Subsonic capability lists.
  - [ ] Add sorting tests for recently added, frequently played, and recently played where implemented.
  - [ ] Add album letter filtering tests for Subsonic/Navidrome.
  - [ ] Run `rtk cargo test -p hifimule-daemon`.
  - [ ] Run `rtk tsc` from `hifimule-ui`.
  - [ ] Manually smoke test against a Navidrome server.

## Dev Notes

### Current Code Context

- `hifimule-daemon/src/providers/subsonic.rs` currently advertises only Artists, Albums, Playlists, Genres, and Favorites in `capabilities().browse.list_modes`.
- `SubsonicProvider` currently implements Favorites via `getStarred2`, genres via `getGenres`/`getSongsByGenre`, and album filtering using `getAlbumList2` plus client-side filtering.
- A current test asserts `list_recently_added` is unsupported for Subsonic. This story should replace that expectation for servers where reliable support exists while preserving unsupported behavior for classic/limited servers.
- `hifimule-ui/src/library.ts` already supports `recentlyAdded`, `frequentlyPlayed`, and `recentlyPlayed` generically. Avoid UI provider-specific changes unless a genuine existing bug blocks provider-neutral behavior.
- `hifimule-daemon/src/rpc.rs` already has `browse.listRecentlyAdded`, `browse.listFrequentlyPlayed`, and `browse.listRecentlyPlayed` handlers that call the provider methods.

### Capability Rule

Prefer honest capability exposure. If a Subsonic-compatible server cannot provide a correct mode, hide it rather than synthesizing misleading data.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story-9.6-Navidrome-Subsonic-Browse-Parity-Hardening]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-05-23-navidrome-subsonic-parity.md]
- [Source: hifimule-daemon/src/providers/subsonic.rs] (`capabilities`, browse methods, Subsonic client methods)
- [Source: hifimule-daemon/src/providers/mod.rs] (`BrowseMode`, `BrowseCapabilities`, `MediaProvider`)
- [Source: hifimule-daemon/src/rpc.rs] (`browse.listModes`, history browse handlers)
- [Source: hifimule-ui/src/library.ts] (provider-neutral mode rendering)

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List

## Change Log

- 2026-05-23: Created story from approved Correct Course proposal for Navidrome/Subsonic browse parity.
