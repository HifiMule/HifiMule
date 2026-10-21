---
title: 'Use album artist for music folders'
type: 'bugfix'
created: '2026-10-21'
status: 'done'
baseline_commit: '9d6e434f3b1d9dd2bdb06baf087d88309b839b44'
context:
  - '{project-root}/_bmad-output/planning-artifacts/project-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Music synchronized through the provider-neutral path uses the track performer to create its artist directory. Guest credits can therefore split one album across directories. Jellyfin supplies an album artist that this path discards; the Subsonic adapter does not parse OpenSubsonic album-artist fields used by Navidrome.

**Approach:** Carry album artist separately through provider normalization, manual selection, auto-fill, and the sync plan. Prefer it when constructing music directories, falling back to the existing track artist when album artist is unavailable.

## Boundaries & Constraints

**Always:** Preserve track performer, artist identities, playlist labels, matching, and unmanaged-file protection. Keep path sanitization/length limits, provider abstraction, and DeviceIO. Optional fields must accept older serialized data.

**Ask First:** Reorganizing existing files, rewriting music tags, or changing artist display/matching.

**Never:** Infer album artist by stripping guest credits. Add per-track album lookups, dependencies, or manifest migration. Relocate existing files solely for metadata. Change audiobook/podcast layouts.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Behavior | Error Handling |
|----------|---------------|-------------------|----------------|
| Guest track | Album artist Main, performer Main feat. Guest | Main/Album folder; preserve performer | Existing transfer handling |
| Compilation | Various Artists album, differing performers | Shared Various Artists/Album folder | Existing collision handling |
| OpenSubsonic | displayAlbumArtist or albumArtists provided | Prefer nonblank display value, otherwise join nonblank names in source order | Missing extensions supported |
| Missing/blank metadata | No usable album artist | Performer fallback, then existing Unknown Artist | No empty folder component |
| Legacy plan | No albumArtist field | Deserialize and retain fallback | No schema failure |
| Resident file | Old performer directory in manifest | Retain existing path | No metadata-only relocation |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/domain/models.rs` — Song has only artist_name.
- `hifimule-daemon/src/providers/jellyfin.rs` — song_from_item discards AlbumArtist after resolving performer.
- `hifimule-daemon/src/providers/subsonic.rs` — SongDto/song_from_dto ignore displayAlbumArtist/albumArtists. Official contract: https://opensubsonic.netlify.app/docs/responses/child/.
- `hifimule-daemon/src/sync.rs` — DesiredItem, SyncAddItem, add_from_desired, and direct constructors carry metadata; construct_desired_file_path reads item.artist.
- `hifimule-daemon/src/rpc.rs` — provider_song_to_desired_item, auto_fill_to_desired_item, and legacy conversions produce desired items.
- `hifimule-daemon/src/auto_fill/mod.rs`, `hifimule-daemon/src/auto_fill/pipeline.rs` — AutoFillItem and both fill engines carry songs to sync.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/domain/models.rs` — add separate optional album_artist; default it for older serialized songs.
- [x] `hifimule-daemon/src/providers/jellyfin.rs`, `hifimule-daemon/src/providers/subsonic.rs` — normalize album artist independently; test real response shapes, multiple artists, and blanks.
- [x] `hifimule-daemon/src/auto_fill/mod.rs`, `hifimule-daemon/src/auto_fill/pipeline.rs` — propagate album artist through AutoFillItem and both fill routes.
- [x] `hifimule-daemon/src/rpc.rs`, `hifimule-daemon/src/sync.rs` — propagate through DesiredItem and SyncAddItem, including planned_song_bytes_with_preferred and all direct constructors; default serialized fields and prefer album artist only for music folders.
- [x] `hifimule-daemon/src/main.rs`, `hifimule-daemon/src/tests.rs` — update affected constructors/fixtures, plus compiler-identified literals elsewhere; use absent values for unrelated media.
- [x] Provider, RPC, auto-fill, and sync test modules in the files above — cover the matrix, manual/automatic propagation, plan round trips, sanitized paths, and unchanged residents.

**Acceptance Criteria:**
- Given tracks with one album artist and differing guest credits, when either provider prepares new manual or auto-fill transfers, then they share an artist/album directory while retaining performer metadata.
- Given older responses or saved plans omit album artist, when sync runs, then fallback behavior remains usable without extra requests.
- Given a resident managed file, when album artist becomes available, then normal sync does not relocate or redownload it solely for that metadata change.

## Spec Change Log

## Design Notes

Legacy Jellyfin RPC conversions already use AlbumArtist; shared conversion uses the performer. Preserve existing legacy behavior, including rank_and_truncate_with_profile, while carrying the new field independently. Do not change performer-based scan/rekey matching or playback identities.

Existing manifest paths stay authoritative; consolidation of already split albums is separate work.

## Verification

**Commands:**
- `rtk npm run build:daemon -- test -p hifimule-daemon` — daemon tests pass, including provider normalization, plan round trips, path construction, manual sync, auto-fill, and unchanged-resident regression coverage.
- `rtk git diff --check` — no whitespace errors.

Run the repository's established formatting check on modified Rust files; avoid unrelated formatting churn.

**Environment notes:** Raw Cargo is rejected by the existing build script because the controlled FFmpeg runtime must be verified; the supported build wrapper performs that verification. Tests using local mock HTTP servers require socket access outside the sandbox.

**Formatting:** `rtk rustfmt --edition 2024 --check --config skip_children=true <modified Rust files>` reports existing formatting differences on the unchanged HEAD baseline as well. Added regression test functions were formatted using rustfmt output; unrelated whole-file formatting was left unchanged.

**Result:** Supported wrapper completed successfully: 1,642 daemon unit tests passed, 14 intentionally ignored; 5 Audiobookshelf contract tests passed. Eight new regression tests cover provider metadata, both auto-fill routes, manual adapters, plan compatibility, path fallback/sanitization, and resident retention. `rtk git diff --check` passed. Full test output: `/tmp/hifimule-album-artist-tests.log`.

## Suggested Review Order

**Folder behavior**

- Choose album artist for music folders while retaining existing media layouts.
  [sync.rs:1313](../../hifimule-daemon/src/sync.rs#L1313)

**Provider normalization**

- Keep Jellyfin album credit separate from track performer.
  [jellyfin.rs:1265](../../hifimule-daemon/src/providers/jellyfin.rs#L1265)

- Read OpenSubsonic display credit, then ordered album-artist names.
  [subsonic.rs:1901](../../hifimule-daemon/src/providers/subsonic.rs#L1901)

**Metadata propagation**

- Preserve album artist across provider selection and sync planning.
  [rpc.rs:5000](../../hifimule-daemon/src/rpc.rs#L5000)

- Carry album credit through both auto-fill routes and serialized plans.
  [mod.rs:34](../../hifimule-daemon/src/auto_fill/mod.rs#L34)

**Supporting contract and tests**

- Add an optional field compatible with older serialized songs.
  [models.rs:103](../../hifimule-daemon/src/domain/models.rs#L103)

- Verify both providers, manual and automatic selection, plan compatibility, and shared paths.
  [sync.rs:5783](../../hifimule-daemon/src/sync.rs#L5783)

**Review result:** Blind correctness, edge-case, and acceptance reviews reported no actionable findings.
