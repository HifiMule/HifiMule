# HifiMule 0.7.0

Release date: 2026-05-24

## Highlights

- **Favorites now browse like the rest of your library**: Favorites are organized as Artists -> Albums -> Tracks instead of one flat song list. You can sync a favorite artist, a favorite album, or just the favorite tracks inside an album without losing that context.
- **Better Navidrome/OpenSubsonic browsing**: OpenSubsonic servers can now expose Recently Added, Frequently Played, and Recently Played modes when the server supports them. Album quick navigation is also more consistent for Subsonic-compatible libraries.
- **Single-track OpenSubsonic sync works correctly**: adding one track from a Subsonic/Navidrome library now resolves that exact song during sync instead of being treated as an unknown item.
- **Safer device-format handling**: when a device requires a compatible format such as MP3, HifiMule now confirms the actual output format before writing. Incompatible or unconfirmed tracks are skipped with warnings instead of copying files the device cannot play.
- **Quieter USB cleanup**: missing files that were already deleted from a managed USB device no longer fail the whole cleanup. Real delete errors are still reported.

---

## New Features

### Hierarchical Favorites Navigation (Story 9.5)

Favorites now use a provider-neutral favorite tree instead of the previous flat favorite-track result.

At the Favorites root, HifiMule shows artists from three sources:
- artists directly marked as favorite
- artists with favorite albums
- artists with favorite tracks

Drilling into a favorite artist or album preserves the meaning of the selection:
- A directly favorite artist shows all of that artist's albums.
- An inferred artist, shown because it contains favorite albums or tracks, shows only those favorite albums or albums containing favorite tracks.
- A directly favorite album shows all album tracks.
- An inferred album shows only its favorite tracks.

This keeps Favorites useful both for broad sync choices and for precise "only my starred tracks" workflows.

### Scoped Favorites Basket Items

HifiMule now distinguishes full artist/album selections from inferred Favorites selections.

New scoped basket item types preserve favorite-only sync behavior:
- `FavoriteArtist`
- `FavoriteAlbum`

This prevents a favorite track inside a non-favorite album from accidentally expanding into the full album or full artist during sync. Directly favorite artists and albums still behave as full container selections.

### Navidrome/OpenSubsonic History Modes (Story 9.6)

For OpenSubsonic-compatible servers, the browse-mode capability list can now include:
- Recently Added
- Frequently Played
- Recently Played

The Subsonic provider owns these capability decisions, so the UI remains provider-neutral and only renders modes the active server advertises.

Implemented behavior:
- Recently Added uses newest album ordering.
- Frequently Played returns tracks sorted by play count.
- Recently Played returns tracks sorted by last-played date.
- Classic or limited Subsonic servers keep unsupported history modes hidden and return the existing unsupported-capability error if called directly.

### Direct OpenSubsonic Song Lookup

Provider-neutral sync expansion can now ask a provider for a single song by ID. OpenSubsonic implements this through its song endpoint, which fixes single `Audio` basket items.

This means:
- selecting one Subsonic/Navidrome track syncs that track
- selecting an album and one track from the same album does not duplicate the track
- providers without direct song lookup continue using the existing album, playlist, artist, and genre fallback path

---

## Improvements

### Provider-Neutral Transcoding and Compatibility (Story 4.9)

The sync engine now treats device compatibility as a hard constraint.

For Subsonic/OpenSubsonic:
- FLAC-to-MP3 and similar conversions request provider stream URLs with the requested format and bitrate.
- Stream URL construction stays inside the Subsonic provider.
- Provider suffix and content-type metadata are carried through sync planning.

For all provider-sync paths:
- compatible passthrough keeps the source file extension
- incompatible passthrough is skipped instead of written under a misleading target extension
- unconfirmed transcoding is skipped instead of copied
- skipped items are left out of the device manifest
- operation warnings and progress totals account for skipped tracks

Jellyfin PlaybackInfo-based transcoding remains intact.

### Album Quick Navigation for Subsonic/Navidrome

Subsonic-compatible album browsing now handles letter filtering more consistently with Jellyfin, including `#` behavior for non-alphabetic album names.

### Faster Subsonic History Retrieval

Frequently Played and Recently Played retrieval for OpenSubsonic was tightened to use server frequent/recent album lists plus targeted album fetches instead of a broad song dump.

### Documentation Refresh

Project documentation was refreshed and renamed from JellyfinSync-oriented filenames to HifiMule-oriented filenames, including architecture, API contracts, data models, component inventory, project overview, development guide, and documentation index updates.

---

## Bug Fixes

### Single Track Sync for OpenSubsonic

Fixed an issue where a basket item representing one OpenSubsonic track could fail to produce a downloadable desired item. Provider-neutral sync expansion now tries direct song lookup before falling back to genre resolution.

Regression coverage verifies:
- single-song expansion succeeds
- album plus selected track deduplicates correctly
- unsupported single-song lookup does not break other providers
- second syncs do not oscillate because of album fallback behavior

### Managed File Deletion on USB (Story 4.10)

Cleanup now treats already-missing managed files as successfully deleted.

Covered cases:
- MSC/USB files missing with `NotFound` or OS error 2
- distinguishable MTP missing-object errors
- stale manifest entries for missing managed files

Safety checks remain in place:
- permission errors, read-only media, disconnects, and generic backend failures still surface as errors
- unmanaged paths, absolute paths, parent traversal, rooted paths, and unsafe directory-level delete paths are rejected
- provider-sync deletes are validated before calling `DeviceIO`

### Favorites Sync Semantics

Fixed review issues where inferred Favorites basket entries could expand too broadly or duplicate tracks during auto-sync and delta calculation.

### Active Profile Handling

Missing or unreadable selected transcoding profiles now fail safely instead of falling back to unconstrained provider sync.

---

## Validation

Automated verification recorded during implementation included:
- `rtk cargo test -p hifimule-daemon` passing with the expanded daemon test suite
- focused provider tests for Subsonic/OpenSubsonic history capabilities and sorting
- focused sync tests for single-song expansion, album plus selected-track dedupe, transcoding compatibility, skipped item manifest behavior, and idempotent delete cleanup
- TypeScript checks for the UI using the repo-local TypeScript compiler

Manual verification recorded during implementation:
- Favorites root -> artist -> album -> tracks smoke test confirmed
- Navidrome/OpenSubsonic browse smoke test confirmed

---

## Commits Included

- `e31225f` - Updated documentation
- `aa24131` - Bump cargo version:
- `fbb9863` - Review 4.9
- `29272cf` - Dev 4.9
- `8910d78` - Fix single track management in subsonic
- `7273172` - Format
- `6e2dc2a` - Review 4.10
- `4cf0ab2` - Dev 4.10
- `4e8bf52` - Review 9.6
- `7648ca5` - Dev 9.6
- `a6e58c4` - Correct course
- `4bf148c` - Review 9.5
- `27f387c` - Story and dev 9.5
