---
title: 'Preserve Jellyfin playlist order in device exports'
type: 'bugfix'
created: '2026-10-23'
status: 'done'
baseline_commit: 'c946fb158641eae72b199efb18aa6f07e7edc1b4'
context:
  - '{project-root}/_bmad-output/planning-artifacts/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/investigations/jellyfin-playlist-order-investigation.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The supplied Jellyfin playlist response and exported “80's Rock Running.m3u” each contain 91 entries, but 90 positions differ. HifiMule reads playlist tracks through general library child queries, whose order does not represent the playlist entry sequence.

**Approach:** Retrieve Jellyfin playlist entries through `/Playlists/{playlistId}/Items`, retaining required sync metadata and server-side pagination. Preserve that sequence through existing provider, sync-plan, and M3U mechanisms, so the next normal sync repairs an existing export when only its order changes.

## Boundaries & Constraints

**Always:** Keep MediaSources and ProviderIds fields available for size, format, and identity conversion. Preserve authentication, user scoping, base-path-safe URL construction, error propagation, bounded/windowed retrieval limits, and downloadable-track filtering. Preserve repeated entries in playlist output while allowing shared track transfers to remain deduplicated. Keep existing device paths, atomic writes, and manifest order comparison.

**Ask First:** Expansion to unrelated provider behavior, playlist editing semantics, UI changes, manifest schema changes, or direct modifications to the user's mounted device/server.

**Never:** Sort playlist entries by album, title, filename, disc, or track number. Fetch playlist state during sync execution; resolve it during preparation through existing boundaries. Add dependencies or make a library-query fallback silently restore incorrect ordering. Change album/artist expansion to use playlist APIs.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
| --- | --- | --- | --- |
| Full playlist | Entry sequence C, A, B with conflicting album track numbers | Provider and exported playlist retain C, A, B | Existing provider/RPC errors propagate |
| Repeated entry | C, A, C with separate playlist entry IDs | Both C occurrences remain in output | Missing audio uses existing warning/omission behavior |
| Bounded/windowed read | Nonzero offset and finite limit | Dedicated endpoint receives StartIndex/Limit and result preserves returned order | Keep existing bounded result handling |
| Empty playlist | Empty Items array | Empty provider result and existing empty-export behavior | No fabricated entries |
| Metadata | Entries include MediaSources and ProviderIds | Existing Song and DesiredItem conversion retain required values | Existing conversion defaults remain |
| Order-only change | Existing manifest/export C, A, B; source B, C, A | Next normal sync rewrites M3U in new order without unnecessary audio transfer | Existing write verification/warnings remain |
| Source failure | Dedicated endpoint returns non-success | Retrieval fails visibly | No unordered fallback |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/api.rs:1502` — existing dedicated playlist read used by mutation code; library-based full/page/window helpers follow it.
- `hifimule-daemon/src/providers/jellyfin.rs:448` — full playlist read; bounded and windowed implementations follow. Existing provider tests at approximately 1814–1910 mock library endpoints.
- `hifimule-daemon/src/rpc.rs:7815` — legacy sync expansion uses generic child lookup even when `is_playlist`; playlist metadata copies that result sequence.
- `hifimule-daemon/src/sync_plan.rs:345` — persists ordered playlist track rows; repeated-entry round-trip regression already exists.
- `hifimule-daemon/src/sync.rs:5484` — M3U generation writes plan order and compares ordered track IDs to detect regeneration needs.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/api.rs` — consolidate playlist reads around the dedicated endpoint, with full and optional offset/limit support and required metadata fields; remove obsolete library helpers once callers migrate.
- [x] `hifimule-daemon/src/providers/jellyfin.rs` — route full, bounded and windowed reads through ordered retrieval; update mocks and assert non-album ordering, repeats, pagination, metadata, empty result, and failure behavior with focused cases.
- [x] `hifimule-daemon/src/rpc.rs` — select dedicated retrieval for playlist expansion in the legacy sync branch; preserve generic child expansion for albums/artists; update affected playlist mocks and assert delta playlist sequence.
- [x] `hifimule-daemon/src/sync.rs` — extend focused M3U regression coverage for out-of-album-order entries, duplicates, and order-only rewrites using existing device abstractions.
- [x] Investigation/spec artifacts — record implementation, review findings, verification results, and remaining live-server evidence gap.

**Acceptance Criteria:**
- Given a Jellyfin playlist is synchronized by either existing sync preparation path, when its playlist delta is built, then track IDs retain the dedicated endpoint's sequence.
- Given playlist playback or radio requests a bounded window, when retrieval runs, then pagination applies to the playlist entry sequence and remains server-side.
- Given an existing export contains the old sequence and audio already exists, when the next sync receives corrected order, then the M3U is regenerated and audio is not needlessly transferred.
- Given the supplied source playlist, when rebuilt HifiMule is used for a normal sync, then the exported first entry is “Green‐Tinted Sixties Mind” and the last is “Days of No Trust”.

## Spec Change Log

## Design Notes

The dedicated endpoint supports Fields, StartIndex, and Limit in the bundled OpenAPI contract and upstream controller. Reuse the existing playlist-item request logic rather than adding a second ordering lookup. Playlist mutation callers require PlaylistItemId and must keep their entry semantics. Downstream sync sorting/dedup must remain separate from playlist entry vectors; current plan and writer already retain sequence.

## Verification

**Commands:**
- `rtk npm run build:daemon -- test -p hifimule-daemon providers::jellyfin::tests` — provider read and mutation tests pass against dedicated endpoint mocks.
- `rtk npm run build:daemon -- test -p hifimule-daemon test_rpc_sync_calculate_delta_expands_playlist_to_tracks` — sync preparation retains ordered playlist metadata.
- `rtk npm run build:daemon -- test -p hifimule-daemon ordered_playlist_tracks_round_trip_duplicates_separately_from_metadata` — plan persistence preserves sequence and repeated entries.
- `rtk npm run build:daemon -- test -p hifimule-daemon test_generate_m3u` — playlist output and rewrite regressions pass.
- Run focused new API regressions and any additional affected endpoint-mock tests found during implementation.

**Manual validation:** After implementation and rebuilding, resync the source playlist in the user's app and compare all 91 entries with the supplied JSON. This requires a live user session and is not yet performed. No permission to write mounted-device files or edit server playlists is inferred from this bug report.


### Implementation verification — 2026-10-23

- Implemented full and optional window reads through `/Playlists/{id}/Items`, with `MediaSources,ProviderIds`, user scoping, existing authentication, base-path construction and HTTP status errors. Removed the library-based playlist helpers.
- Provider reads retain only Audio/MusicVideo entries after applying the returned server window; raw API reads retain all entry types and PlaylistItemId for mutation callers. No change to mutation semantics.
- Direct single-server Jellyfin sync selects dedicated retrieval for playlists while keeping library child retrieval for other containers. Repeated playlist entries remain separate from deduplicated audio transfers.
- `rtk npm run build:daemon -- test -p hifimule-daemon playlist` — **65 passed**, including API base path/auth/metadata/entry-ID coverage, provider reads and mutations, direct RPC delta order/duplicates, shared plan round-trip and unrelated playlist consumers.
- `rtk npm run build:daemon -- test -p hifimule-daemon test_generate_m3u` — **11 passed**, including the new non-album-order, repeated-entry and order-only rewrite regression. Existing audio remains unchanged, with no audio adds/deletes in the calculated delta.
- `rtk git diff --check` — passed. Searches found no remaining old playlist-library helper references or affected library-endpoint playlist mocks.
- The initial provider-module run passed 49/50 tests; the new empty-result fixture needed an explicit query matcher. That fixture was corrected and passes in the 65-test playlist run. Compiler warnings are existing deprecation/dead-code/unused-result warnings in unrelated code.
- Live Jellyfin and mounted-device validation remains unperformed; normal resync with the rebuilt app is still required to compare the supplied 91-entry playlist. No mounted-device or server data was modified. Review remains pending.


### Review correction verification — 2026-10-23

- Resolved mixed-entry paging: the provider's offset/limit count playable Audio/MusicVideo entries, while Jellyfin's StartIndex counts raw entries. Bounded/windowed reads now share a scan over dedicated endpoint pages, filter raw entries and skip the requested playable prefix. Raw pages are capped at 200 entries; zero limits return without HTTP and checked raw offset progression prevents overflow.
- This supersedes the initial assumption that filtering a single returned raw window was sufficient. A non-audio prefix no longer yields an empty playback prefix, and successive playable windows no longer repeat or skip tracks after filtered entries.
- Added a full 200-entry first-page regression with a filtered entry, a second raw page, a window spanning both pages, and a subsequent playable window. Added a one-entry raw-page non-audio prefix regression plus zero-limit/no-HTTP checks.
- Preserved non-success HTTP response bodies in the consolidated raw helper; an existing failure fixture now asserts the diagnostic text survives mapping.
- `rtk npm run build:daemon -- test -p hifimule-daemon playlist` — **66 passed** after review fixes. `rtk git diff --check` — passed.
- Deliberate limitation: windows rescan from raw offset zero to translate playable offsets. The adjacent ponytail note identifies future raw-cursor metadata as the way to avoid this O(offset) scan without adding state now. Live Jellyfin/device comparison remains unperformed.

### Review outcome

Three independent reviews covered the code diff, edge cases, and acceptance criteria. Duplicated findings about mixed-entry prefix/window paging were classified as implementation patches and resolved by the shared playable-window scanner. The diagnostic-body regression was also patched. The edge-case reviewer rechecked the scanner and returned no remaining findings. Acceptance review found no spec violations; live export validation remains explicitly outstanding. No deferred work or unresolved implementation findings were accepted.

## Suggested Review Order

**Ordered retrieval**

- Read playlist entries directly, retaining metadata and mutation entry identities.
  [api.rs:1514](../../hifimule-daemon/src/api.rs#L1514)
- Preserve filtered playlist order in full provider retrieval.
  [jellyfin.rs:498](../../hifimule-daemon/src/providers/jellyfin.rs#L498)
- Translate playable offsets across bounded raw pages without losing non-audio gaps.
  [jellyfin.rs:147](../../hifimule-daemon/src/providers/jellyfin.rs#L147)
- Correct the separate single-server sync path while retaining album expansion.
  [rpc.rs:7815](../../hifimule-daemon/src/rpc.rs#L7815)

**Regression coverage**

- Verify authentication, base paths, metadata, raw entries, and entry IDs.
  [api.rs:2240](../../hifimule-daemon/src/api.rs#L2240)
- Check consecutive playable windows across filtered raw pages.
  [jellyfin.rs:1920](../../hifimule-daemon/src/providers/jellyfin.rs#L1920)
- Keep playlist repeats while deduplicating audio transfers.
  [rpc.rs:15250](../../hifimule-daemon/src/rpc.rs#L15250)
- Rewrite order-only M3U changes without transferring or altering existing audio.
  [sync.rs:12205](../../hifimule-daemon/src/sync.rs#L12205)
