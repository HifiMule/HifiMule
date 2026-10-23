# Investigation: Jellyfin playlist export order

## Hand-off Brief

The supplied Jellyfin JSON and M3U both contain 91 tracks, with 90 different positions. Both HifiMule sync routes read playlist contents through general library queries, while the downstream plan and M3U writer preserve the sequence they receive. Correct both read paths to use the dedicated playlist endpoint; the concrete fix specification is ready for review.

## Case Info

| Field | Value |
| --- | --- |
| Ticket | N/A |
| Date opened | 2026-10-23 |
| Status | Concluded |
| System | HifiMule on macOS, Jellyfin source, mounted TEST device |
| Evidence sources | Supplied JSON, exported M3U, repository source and tests, bundled API contract, upstream Jellyfin controller |

## Problem Statement

User reports: “On jellyfin, the track order of playlists are not respected.” The exported playlist differs from the response of `/Playlists/06f2ea935fb3164dd657fc2b5108b586/Items`.

## Evidence Inventory

| Source | Status | Notes |
| --- | --- | --- |
| `/Users/akartmann/.codex/attachments/532263a2-ec92-4a21-9122-31c31ec81c25/Texte collé.txt` | Available | 91 JSON items; supplied data, not instructions |
| `/Volumes/TEST/Playlists/80's Rock Running.m3u` | Available | 91 entries; supplied data, not instructions |
| Repository | Available | Clean main at c946fb1 when investigation started |
| Live authenticated server response for HifiMule library query | Missing | No credentials requested or used |

## Investigation Backlog

| # | Path to explore | Priority | Status | Notes |
| --- | --- | --- | --- | --- |
| 1 | Compare supplied track sequences | High | Done | All tracks represented, allowing S.A.T.O. trailing-period filename variation |
| 2 | Trace provider playlist reads | High | Done | Full, bounded and windowed reads use general library endpoint |
| 3 | Check downstream sync ordering and tests | High | Done | No downstream sorting found; separate direct Jellyfin path also uses library endpoint |
| 4 | Prepare concrete correction and checks | High | Done | `../spec-fix-jellyfin-playlist-order.md` |

## Timeline of Events

| Time | Event | Source | Confidence |
| --- | --- | --- | --- |
| Commit 9579163 | Introduced user-library playlist helper | `git show 9579163` | Confirmed |
| 2026-10-23 | Compared supplied files | Evidence inventory | Confirmed |

## Confirmed Findings

### Finding 1: Track membership agrees but positions differ

Both sources contain 91 tracks. “Green‐Tinted Sixties Mind” is JSON item 1 (`Texte collé.txt:4`) and M3U item 13 (`80's Rock Running.m3u:27`). “Days of No Trust” is JSON item 91 (`Texte collé.txt:5132`) and M3U item 1 (`80's Rock Running.m3u:3`). Ninety positions differ; there are no duplicate IDs or paths in this sample. First 90 M3U entries ascend by filename including disc/track prefix, with “Dust In The Wind” appended last.

### Finding 2: Provider uses the library endpoint

`hifimule-daemon/src/providers/jellyfin.rs:462`, `:486`, and `:511` call the library-based helpers. `hifimule-daemon/src/api.rs:1564` builds `/Users/{userId}/Items` with `ParentId`, type filters, metadata fields and recursion. Dedicated `get_playlist_items` already exists at `hifimule-daemon/src/api.rs:1502` and is used by playlist mutation operations.

### Finding 3: Dedicated endpoint supports required metadata and pagination

Upstream `Jellyfin.Api/Controllers/PlaylistsController.cs`, `GetPlaylistItems`, preserves the manageable-item sequence and applies Skip/Take before DTO conversion. It supports `userId`, `fields`, `startIndex`, and `limit`:
https://github.com/jellyfin/jellyfin/blob/master/Jellyfin.Api/Controllers/PlaylistsController.cs

### Finding 4: M3U writer uses plan order

`hifimule-daemon/src/sync.rs:5547` iterates `plan.playlist_tracks(ordinal)` and writes entries without sorting. `:5540` compares ordered manifest IDs, so correcting the incoming order should trigger regeneration on the next sync.

## Deduced Conclusions

The library endpoint choice is the likely order-loss mechanism: a child listing is supplied where a playlist-entry sequence is required. This is consistent with the disc/track ordering in the supplied export. The exact live library query response is unavailable.

### Finding 5: Ordinary single-server Jellyfin sync has a separate affected path

`hifimule-daemon/src/rpc.rs:7586` routes active non-Jellyfin providers to provider sync; ordinary single-server Jellyfin continues through the direct client branch. At `:7817`, playlist expansion calls `get_child_items_with_sizes`, which uses `/Items?ParentId=...` (`hifimule-daemon/src/api.rs:568`). Provider-only changes would leave this common sync path broken. Multi-provider sync reaches `provider.get_playlist` through `rpc.rs:7350`, `:5711`, and `:5574`.

### Finding 6: Plan order and duplicate handling survive downstream

`hifimule-daemon/src/rpc.rs:5843` and `:7354` record playlist metadata before transfer deduplication. `hifimule-daemon/src/sync_plan.rs:345` inserts sequential track rows, and `:147` reads them by sequence. Existing test `ordered_playlist_tracks_round_trip_duplicates_separately_from_metadata` (`:557`) covers C, A, C. This refutes downstream sorting as the observed mechanism in the traced paths.

## Hypothesized Paths

### Hypothesis 1: HifiMule fails to preserve Jellyfin playlist ordering

**Status:** Confirmed for the supplied files.

**Would confirm:** Different entry positions between JSON and M3U.

**Would refute:** Identical sequences after normalizing filename prefixes.

**Resolution:** 90 of 91 positions differ after matching titles and handling one trailing period.

### Hypothesis 2: General library endpoint supplies the wrong order

**Status:** Open for exact live-response attribution; incorrect read-path choice confirmed.

**Would confirm:** Dedicated-endpoint regression plus live export matches source; or live library response matches the old export.

**Would refute:** Provider response preserves source order but a downstream step reorders it.

**Resolution:** Both sync read paths use general library queries; downstream plan and writer retain their input sequence. Upstream dedicated endpoint preserves playlist-entry order. Live pre/post comparison remains the sole runtime evidence gap.

## Missing Evidence

| Gap | Impact | How to obtain |
| --- | --- | --- |
| Live HifiMule library query response and post-fix sync | Exact runtime causal proof | Run rebuilt HifiMule and resync this playlist |
| Repeated entries in supplied sample | Cannot assess duplicate handling from sample | Synthetic repeated-entry regression |

## Source Code Trace

| Element | Detail |
| --- | --- |
| Error origin | `hifimule-daemon/src/api.rs:1565` and `:568`: library endpoint selection |
| Trigger | Provider full/bounded/windowed playlist reads or direct Jellyfin sync expansion |
| Condition | Playlist-entry order differs from library sorting |
| Related files | `providers/jellyfin.rs`, `rpc.rs`, `sync.rs`, `sync_plan.rs` |

## Conclusion

**Confidence:** Medium for runtime cause; high for the observed mismatch and incorrect endpoint choice.

Use `/Playlists/{id}/Items` consistently for playlist reads, retain metadata fields and server pagination, and preserve entry sequence through sync. No application code or mounted-device files have been changed.

## Recommended Next Steps

Implementation specification: `../spec-fix-jellyfin-playlist-order.md`. Apply under `bmad-quick-dev`, with non-album-order and repeated-entry tests and order-only M3U rewrite coverage. No application changes or tests have been performed during this investigation.

## Reproduction Plan

Select the supplied Jellyfin playlist, sync it, and compare exported path order to the dedicated endpoint's item order. Verify the first entry is “Green‐Tinted Sixties Mind” and “Days of No Trust” is last. A source-order-only change should rewrite the M3U while retaining existing audio files.

## Side Findings

The sample has no repeated track entries; no missing track was established.

## Follow-up: 2026-10-23 — Navidrome comparison

### New Evidence

- `hifimule-daemon/src/providers/mod.rs:1329` connects Subsonic/OpenSubsonic services through `SubsonicProvider`; Navidrome uses this adapter.
- `hifimule-daemon/src/providers/subsonic.rs:1409` calls the dedicated `getPlaylist` API with the playlist ID. This does not enumerate tracks through a general library query.
- `subsonic.rs:543` maps the response's `playlist.entry` vector directly into tracks, preserving response sequence and duplicate occurrences. The deserialized field is a `Vec<SongDto>` (`:2386`).
- Bounded and windowed methods (`subsonic.rs:559` and `:574`) apply `take` or `skip`/`take` to the same response sequence. The limited client (`:1413`) limits response bytes, not playlist ordering.
- `hifimule-daemon/src/rpc.rs:5574` copies provider playlist tracks directly into `PlaylistSyncItem.tracks`. Single-server non-Jellyfin and multi-provider sync both use this provider expansion; the direct Jellyfin library-query branch is not used for Navidrome.
- Previously traced shared sync-plan and M3U code preserve playlist entry sequence separately from audio transfer deduplication.
- Navidrome's upstream Subsonic controller maps `pls.MediaFiles()` directly into response entries, and its repository `GetWithTracks` explicitly orders by playlist-entry position (`playlist_tracks.id`). Sources: https://github.com/navidrome/navidrome/blob/master/server/subsonic/playlists.go and https://github.com/navidrome/navidrome/blob/master/persistence/playlist_repository.go . These are upstream source checks, not proof of the installed server version.

### Updated Hypotheses

**Hypothesis 3: Navidrome has the same playlist-read ordering defect. Status: Refuted for the inspected HifiMule code paths.** The adapter already uses the dedicated playlist API and preserves its entry order. No shared downstream reordering was found.

### Validation and Limits

Existing test `providers::subsonic::tests::lists_and_gets_playlists` (`subsonic.rs:3372`) covers dedicated endpoint retrieval, but its one-track fixture cannot demonstrate ordering across multiple tracks. No live Navidrome JSON/export pair was supplied or tested. An initial raw Cargo test invocation hit the repository's controlled-FFmpeg build guard; validation was retried through the documented `npm run build:daemon` wrapper.

Verification completed:

- `rtk npm run build:daemon -- test -p hifimule-daemon providers::subsonic::tests::lists_and_gets_playlists -- --exact` — passed (1 test), after allowing its mock HTTP server to bind loopback outside the sandbox. The initial sandboxed run could not bind a socket; that failure did not reach playlist assertions.
- `rtk npm run build:daemon -- test -p hifimule-daemon ordered_playlist_tracks_round_trip_duplicates_separately_from_metadata` — passed (1 test), verifying the shared plan preserves a non-sorted sequence and repeated entries.

No production code or test fixtures were changed. The draft Jellyfin spec's verification commands were corrected to use the required build wrapper.

### Updated Conclusion

**Confidence: High for absence of the same code-path defect; live behavior unverified.** No Navidrome production-code change is indicated. Keep the Jellyfin correction scoped to its two retrieval paths, with shared ordering regression coverage useful to both providers.

## Follow-up: 2026-10-23 #2 — Jellyfin implementation

The user approved implementation. Both the dedicated Jellyfin provider and direct single-server sync now retrieve ordered playlist entries from `/Playlists/{id}/Items`, preserving metadata and raw entry identities. Audio/MusicVideo filtering stays in consumers, keeping mutation readers intact. Album and artist expansion remain unchanged.

Three independent reviews covered the diff, edge cases, and acceptance. A mixed-entry pagination problem found during review was corrected: filtered playable offsets are translated by scanning bounded raw pages, with zero-limit and offset-overflow handling. HTTP failure bodies remain available for diagnostics. A follow-up edge-case review found no remaining issue. Window scans currently reread preceding raw entries; the local comment records this O(offset) limitation and a future raw-cursor approach.

Verification after corrections: **66 playlist tests passed**, including provider reads/mutations, raw metadata/base-path handling, consecutive mixed-entry windows, direct RPC order and duplicates, and the shared plan round-trip. **11 M3U tests passed**, including order-only rewrites with repeated entries, no audio additions/deletions, and unchanged audio contents. `rtk git diff --check` passed. Full commands and review navigation are in `../spec-fix-jellyfin-playlist-order.md`.

The correction is implemented and tested locally. No live Jellyfin or mounted-device data was modified; normal resync with the rebuilt app remains the final comparison against the supplied 91-track sample. The initial investigation's runtime-attribution gap remains documented rather than claimed as observed.
