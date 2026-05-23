---
title: 'Fix OpenSubsonic Single Track Sync'
type: 'bugfix'
created: '2026-05-23'
status: 'done'
baseline_commit: '72731729dd068d1045cb0c83751f87ffe4d280e0'
context:
  - '{project-root}/_bmad-output/planning-artifacts/project-context.md'
---

<frozen-after-approval reason="human-owned intent - do not modify unless human renegotiates">

## Intent

**Problem:** Syncing a basket item whose OpenSubsonic type is `Audio` does not produce a downloadable desired item. Album basket items work because provider-neutral sync expands album IDs through `get_album`, but a plain track ID currently falls through album, playlist, artist, and genre lookup and is treated as not found.

**Approach:** Add an explicit provider-level single-song resolution path and use it during provider-neutral sync expansion before falling back to genre lookup. Implement it for OpenSubsonic via the server's song endpoint and keep unsupported providers safely on the existing fallback path.

## Boundaries & Constraints

**Always:** Keep all server-specific OpenSubsonic HTTP details inside `hifimule-daemon/src/providers/subsonic.rs`; route sync expansion through `MediaProvider`; preserve existing album, playlist, artist, favorite, and genre sync behavior; deduplicate selected album plus selected track by existing item ID logic.

**Ask First:** If the OpenSubsonic API/client in this repo lacks a direct song endpoint and the only viable implementation requires broad search-index crawling or persistent cache changes.

**Never:** Do not special-case OpenSubsonic IDs in UI basket code; do not call Subsonic HTTP APIs directly from `rpc.rs`; do not change Jellyfin sync semantics or manifest schema.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Single OpenSubsonic track | Basket contains one `Audio` item with ID `NPMDWJzSQSe4SP4KYfLNeZ` | Delta contains exactly that track as a desired item and sync can download using the same song ID | If server says not found, continue existing fallback checks and finally return the current not-found RPC error |
| Album plus one track from same album | Basket contains album `4t2Kbd7gEZsfF9s1DvcSTH` and track `NPMDWJzSQSe4SP4KYfLNeZ` | Album expansion still contributes the album tracks, and the duplicate selected track is not duplicated | Existing `seen_ids` dedupe remains authoritative |
| Provider without single-song support | Provider returns `UnsupportedCapability` for single-song lookup | Existing album/playlist/artist/genre behavior remains unchanged | Unsupported single-song lookup must not abort sync |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/providers/mod.rs` -- `MediaProvider` trait; add a default unsupported single-song method so non-Subsonic providers do not need behavioral changes.
- `hifimule-daemon/src/providers/subsonic.rs` -- OpenSubsonic provider/client and DTO mapping; add `getSong` client call and provider implementation returning `Song`.
- `hifimule-daemon/src/rpc.rs` -- provider-neutral basket item expansion; add song lookup into `provider_sync_items_for_id` and unit-test the expansion path.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/providers/mod.rs` -- add `get_song(song_id)` with default `UnsupportedCapability` -- allows sync expansion to ask providers for single tracks without breaking existing provider implementations.
- [x] `hifimule-daemon/src/providers/subsonic.rs` -- add `SubsonicClient::get_song`, DTO body type, `MediaProvider::get_song` implementation, and focused unit coverage -- makes OpenSubsonic track IDs resolvable.
- [x] `hifimule-daemon/src/rpc.rs` -- try `provider.get_song(item_id)` in `provider_sync_items_for_id` after album/playlist/artist and before genre -- maps single tracks into `DesiredItem`s while preserving fallbacks.
- [x] `hifimule-daemon/src/rpc.rs` -- add/update provider sync tests for single-song resolution and unsupported-song fallback -- covers the I/O matrix edge cases.

**Acceptance Criteria:**
- Given an OpenSubsonic `Audio` basket item, when sync delta is calculated, then the track ID resolves to one desired item with title, artist, album, duration, and provider album ID populated from the provider.
- Given an album and one of its tracks are both selected, when sync delta is calculated, then the track is present once in the desired set.
- Given a provider does not implement single-song lookup, when a non-song item is synced, then existing album, playlist, artist, and genre resolution still works.

## Verification

**Commands:**
- `rtk cargo test -p hifimule-daemon provider_sync_items_for_id` -- passed: 2 passed, 339 filtered out.
- `rtk cargo test -p hifimule-daemon provider_sync_items_for_id` -- passed after review patch: 3 passed, 340 filtered out.
- `rtk cargo test -p hifimule-daemon provider_calculate_delta_dedupes_album_and_selected_song` -- passed: 1 passed, 342 filtered out.
- `rtk cargo test -p hifimule-daemon subsonic` -- passed after review patch: 62 passed, 281 filtered out.
- `rtk cargo test -p hifimule-daemon changes_since_album_fallback` -- passed after oscillation fix: 3 passed, 341 filtered out.
- `rtk cargo test -p hifimule-daemon provider_sync_items_for_id` -- passed after oscillation fix: 3 passed, 341 filtered out.
- `rtk cargo test -p hifimule-daemon provider_calculate_delta_dedupes_album_and_selected_song` -- passed after oscillation fix: 1 passed, 343 filtered out.
- `rtk cargo test -p hifimule-daemon subsonic` -- passed after oscillation fix: 63 passed, 281 filtered out.

## Suggested Review Order

**Sync Expansion**

- Single-track IDs now resolve through the provider before genre fallback.
  [`rpc.rs:1582`](../../hifimule-daemon/src/rpc.rs#L1582)

- Non-song lookup failures are propagated instead of hidden as not found.
  [`rpc.rs:1637`](../../hifimule-daemon/src/rpc.rs#L1637)

- Album fallback only creates sibling tracks for selected album containers.
  [`subsonic.rs:106`](../../hifimule-daemon/src/providers/subsonic.rs#L106)

**Provider Contract**

- Providers can opt into direct song lookup without breaking unsupported implementations.
  [`mod.rs:76`](../../hifimule-daemon/src/providers/mod.rs#L76)

- Provider change context distinguishes album selections from song album metadata.
  [`device/mod.rs:101`](../../hifimule-daemon/src/device/mod.rs#L101)

- OpenSubsonic maps getSong responses into the shared Song domain model.
  [`subsonic.rs:318`](../../hifimule-daemon/src/providers/subsonic.rs#L318)

- Client HTTP details stay inside the Subsonic provider boundary.
  [`subsonic.rs:742`](../../hifimule-daemon/src/providers/subsonic.rs#L742)

**Tests**

- Provider expansion covers single-song success and error propagation.
  [`rpc.rs:6923`](../../hifimule-daemon/src/rpc.rs#L6923)

- Album plus selected track dedupe is pinned at delta level.
  [`rpc.rs:6969`](../../hifimule-daemon/src/rpc.rs#L6969)

- Subsonic getSong response mapping is covered with mock HTTP.
  [`subsonic.rs:1775`](../../hifimule-daemon/src/providers/subsonic.rs#L1775)

- Second-sync single-track oscillation is covered by album fallback regression.
  [`subsonic.rs:2307`](../../hifimule-daemon/src/providers/subsonic.rs#L2307)
