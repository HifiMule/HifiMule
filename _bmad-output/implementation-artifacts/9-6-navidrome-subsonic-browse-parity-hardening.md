# Story 9.6: Navidrome/Subsonic Browse Parity Hardening

Status: done

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

- [x] Task 1: Refine Subsonic/OpenSubsonic browse capabilities (AC: 1-4, 6)
  - [x] Detect whether the connected server can reliably support recently added, frequently played, and recently played modes.
  - [x] Include supported modes in `SubsonicProvider.capabilities().browse.list_modes`.
  - [x] Keep unsupported modes hidden and returning `UnsupportedCapability` when called directly.
  - [x] Keep capability decisions inside `SubsonicProvider`.

- [x] Task 2: Implement supported history navigation methods (AC: 1-3)
  - [x] Implement `list_recently_added` for servers with reliable newest-album support.
  - [x] Implement `list_frequently_played` for servers with reliable play-count sorting.
  - [x] Implement `list_recently_played` for servers with reliable last-played sorting.
  - [x] Preserve provider-domain normalization for IDs, duration, bitrate, cover art, dates, and play counts.

- [x] Task 3: Harden album letter filtering for Subsonic/Navidrome (AC: 5)
  - [x] Verify `browse.listAlbums` forwards `letter` to `MediaProvider::list_albums`.
  - [x] Ensure Subsonic/Navidrome album letter filtering matches Jellyfin quick-nav expectations.
  - [x] Include `#`/non-alpha behavior if the UI sends that value.

- [x] Task 4: Keep the UI provider-neutral (AC: 6)
  - [x] Do not add Navidrome/Subsonic-specific mode branching in `hifimule-ui/src/library.ts`.
  - [x] Continue rendering modes from `browse.listModes`.
  - [x] Ensure unsupported modes do not show as broken buttons.

- [x] Task 5: Verification (AC: 1-6)
  - [x] Add provider tests for OpenSubsonic/Navidrome capability lists.
  - [x] Add provider tests for classic Subsonic capability lists.
  - [x] Add sorting tests for recently added, frequently played, and recently played where implemented.
  - [x] Add album letter filtering tests for Subsonic/Navidrome.
  - [x] Run `rtk cargo test -p hifimule-daemon`.
  - [x] Run `rtk tsc` from `hifimule-ui`.
  - [x] Manually smoke test against a Navidrome server.

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

GPT-5 Codex

### Debug Log References

- 2026-05-23: `rtk cargo test -p hifimule-daemon providers::subsonic` red phase failed on missing OpenSubsonic history methods/modes and `#` album filtering.
- 2026-05-23: `rtk cargo test -p hifimule-daemon providers::subsonic` passed: 42 passed.
- 2026-05-23: `rtk cargo test -p hifimule-daemon` passed: 327 passed.
- 2026-05-23: `rtk tsc` from `hifimule-ui` could not start because `npx` is not on PATH; repo-local `.\node_modules\.bin\tsc.cmd` passed after approval.
- 2026-05-23: Manual Navidrome smoke test validated by Alexis.
- 2026-05-23: Review patch verified with `rtk cargo test -p hifimule-daemon providers::subsonic` (43 passed) and `rtk cargo test -p hifimule-daemon` (328 passed).
- 2026-05-23: Follow-up performance patch changed frequent/recent played retrieval from full `search3` song dumps to `getAlbumList2(type=frequent|recent)` plus targeted `getAlbum` calls; verified with `rtk cargo test -p hifimule-daemon providers::subsonic` (43 passed) and `rtk cargo test -p hifimule-daemon` (328 passed).

### Completion Notes List

- Implemented OpenSubsonic-only exposure for `recentlyAdded`, `frequentlyPlayed`, and `recentlyPlayed` browse modes in `SubsonicProvider.capabilities()`.
- Implemented Subsonic/OpenSubsonic history browse methods: newest albums through `getAlbumList2(type=newest)`, frequently played tracks sorted by `playCount` descending, and recently played tracks sorted by `played` descending.
- Preserved classic Subsonic unsupported behavior for direct history method calls.
- Normalized Subsonic song `created`, `played`, and `playCount` fields into provider-domain `Song` metadata.
- Hardened Subsonic album quick-nav filtering, including `#` for non-alpha album names, without adding UI provider-specific branching.
- Manual Navidrome smoke testing was confirmed valid by Alexis.
- Optimized frequently/recently played retrieval to narrow via server frequent/recent album lists before fetching tracks.

### File List

- hifimule-daemon/src/providers/subsonic.rs
- _bmad-output/implementation-artifacts/9-6-navidrome-subsonic-browse-parity-hardening.md
- _bmad-output/implementation-artifacts/sprint-status.yaml

## Change Log

- 2026-05-23: Created story from approved Correct Course proposal for Navidrome/Subsonic browse parity.
- 2026-05-23: Implemented OpenSubsonic history browse capabilities/methods and Subsonic album quick-nav hardening; automated and manual validation passed.

### Review Findings

- [x] [Review][Decision][Dismissed] History modes are exposed for every OpenSubsonic server without proving per-mode reliability — Alexis chose to treat `open_subsonic` as sufficient for Story 9.6.
- [x] [Review][Patch] `list_recently_added` returns the current page length as `total` and bypasses `offset` when `limit == 0` [hifimule-daemon/src/providers/subsonic.rs:452]
