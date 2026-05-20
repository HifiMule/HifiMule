# HifiMule 0.5.6

Release date: 2026-05-20

## Highlights

- Rockbox playback history is now read from `.rockbox/playback.log`, matching the current Rockbox playback-log format.
- Legacy `.scrobbler.log` files are still supported as a fallback for older setups.
- Scrobbles are now submitted to Jellyfin through playback-session reporting, which better reflects real playback progress and play counts.
- HifiMule now prefers the local sync manifest when matching a Rockbox play back to its Jellyfin item, reducing mismatches when filenames, casing, or search results are imperfect.

## Changes Since 0.5.5

### Rockbox Playback Log Support

- Added support for Rockbox playback log rows in the form `timestamp:elapsed_ms:duration_ms:path`.
- Extracted artist, album, and title from synced device paths.
- Normalized Rockbox paths so storage prefixes such as `/<microSD0>/` do not prevent matching.
- Converted Rockbox elapsed playback time into Jellyfin ticks for playback-session submission.
- Kept the old tab-separated `.scrobbler.log` parser for compatibility.

### Jellyfin Scrobbling

- Changed Jellyfin submission from a simple "mark played" call to a playback start/stop session flow.
- Added playback position reporting when ending a Jellyfin playback session.
- Added support for sending an explicit `datePlayed` value when using the played-item endpoint.
- Added a short registration delay for playback reporting, with a faster test-only delay.

### Matching And Deduplication

- Passed the synced device manifest into scrobble processing.
- Matched playback-log entries against manifest `local_path` values before falling back to Jellyfin search.
- Preserved duplicate detection before submission so already-recorded scrobbles are skipped.
- Kept title and album/album-artist search as a fallback when the manifest cannot identify a track.

### Reliability

- Treats missing playback logs as an empty successful scrobble pass rather than an error.
- Prefers `.rockbox/playback.log` when both new and legacy logs are present.
- Improved test coverage for playback-log parsing, manifest matching, missing-log handling, and Jellyfin playback-session submission.

## Commits Included

- `eb47f89` - New log format for rockbox scrobbler
- `275278c` - Fix scrobble matching
- `a8675a2` - Fix scrobble to jellyfin
