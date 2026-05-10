---
title: 'Fix Jellyfin Playlist M3U Not Created'
type: 'bugfix'
created: '2026-05-10'
status: 'done'
baseline_commit: 'NO_VCS'
context:
  - '{project-root}/_bmad-output/planning-artifacts/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/4-7-playlist-m3u-file-generation.md'
---

<frozen-after-approval reason="human-owned intent - do not modify unless human renegotiates">

## Intent

**Problem:** When a Jellyfin playlist is synchronized, the tracks can land on the device while the native `.m3u` playlist file is absent. This leaves Rockbox/DAP users with synced audio but no playable device playlist.

**Approach:** Harden the Jellyfin playlist sync path so playlist metadata is preserved from `sync_calculate_delta` through `sync_execute`, and make M3U generation rewrite an unchanged playlist when the manifest claims it exists but the device file is missing.

## Boundaries & Constraints

**Always:** Keep `.m3u` files in the managed sync folder using the existing `generate_m3u_files` path and `DeviceIO` abstraction. Preserve current filename sanitization, relative track paths, manifest `playlists` tracking, and Jellyfin item expansion behavior.

**Ask First:** Ask before changing the visible UI sync flow, moving `.m3u` files to device root, changing the manifest schema, or altering playlist behavior for non-Jellyfin providers beyond shared helper correctness.

**Never:** Do not bypass `DeviceIO` with direct filesystem writes in sync code. Do not add new Jellyfin API calls during `sync_execute`; playlist track order and metadata must come from the delta produced by `sync_calculate_delta`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|----------------------------|----------------|
| Jellyfin playlist delta | Basket contains a Jellyfin item with `Type: "Playlist"` and child audio tracks | `sync_calculate_delta` returns track adds and a `playlists` entry with playlist ID, name, ordered track IDs, artist, and durations | Existing RPC failure behavior remains unchanged if Jellyfin item lookup or child expansion fails |
| Missing unchanged M3U | Manifest has a matching playlist entry and track IDs are unchanged, but the `.m3u` file is not present on the device | `generate_m3u_files` writes the missing `.m3u` and keeps/refreshes the manifest playlist entry | If the presence check fails because device listing fails, treat it as needing a write and surface any write failure as the existing M3U warning |
| Existing unchanged M3U | Manifest has matching playlist entry and the `.m3u` file exists | Sync does not rewrite the file | No warning |
| Normal first playlist sync | Playlist is new to manifest and tracks are synced | `.m3u` is written to the managed folder after track writes complete | Missing track IDs are omitted with existing warning behavior |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/rpc.rs` -- `handle_sync_calculate_delta` expands Jellyfin playlist basket items and attaches `SyncDelta.playlists`; existing test verifies track adds but not playlist metadata.
- `hifimule-daemon/src/sync.rs` -- `execute_sync` and `execute_provider_sync` call `generate_m3u_files`; `generate_m3u_files` currently skips unchanged manifest entries without checking device file presence.
- `hifimule-daemon/src/device_io.rs` -- `DeviceIO::list_files` and `read_file` are the available device-safe ways to check whether a managed playlist file exists.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/rpc.rs` -- Extend the Jellyfin playlist delta regression test to assert `delta.playlists` contains the selected playlist and ordered child track metadata -- proves the execution input can create M3U files.
- [x] `hifimule-daemon/src/sync.rs` -- Add a small device-file presence check for the target `.m3u` path before skipping an unchanged playlist -- prevents a stale manifest from suppressing a missing file.
- [x] `hifimule-daemon/src/sync.rs` -- Add a regression test where `manifest.playlists` already matches the playlist but the file is absent, and assert `generate_m3u_files` writes it -- locks the reported failure mode.
- [x] `hifimule-daemon/src/sync.rs` -- Keep the existing no-rewrite test passing when the `.m3u` file exists -- preserves differential sync behavior.

**Acceptance Criteria:**
- Given a Jellyfin playlist is selected for sync, when `sync_calculate_delta` completes, then the returned delta includes a non-empty `playlists` array for that playlist.
- Given a playlist manifest entry says a playlist is unchanged but the `.m3u` file is missing from the managed folder, when sync runs, then the `.m3u` file is created.
- Given a playlist manifest entry is unchanged and its `.m3u` file exists, when sync runs, then the file is not rewritten.

## Spec Change Log

## Design Notes

Use the existing relative path already passed to `DeviceIO::write_with_verify` as the presence-check target. Prefer `list_files(managed_subfolder)` over direct filesystem inspection so MSC and MTP backends behave consistently; if listing is unavailable or fails, force regeneration rather than silently skipping.

## Verification

**Commands:**
- `rtk cargo test -p hifimule-daemon test_rpc_sync_calculate_delta_expands_playlist_to_tracks --no-fail-fast` -- expected: Jellyfin delta includes playlist metadata and track adds.
- `rtk cargo test -p hifimule-daemon test_generate_m3u --no-fail-fast` -- expected: M3U generation regressions pass.
- `rtk cargo test -p hifimule-daemon` -- expected: daemon suite passes.

## Suggested Review Order

**M3U Repair Path**

- Presence check stays behind the device abstraction.
  [`sync.rs:1312`](../../hifimule-daemon/src/sync.rs#L1312)

- Unchanged playlists now rewrite only when the file is absent.
  [`sync.rs:1476`](../../hifimule-daemon/src/sync.rs#L1476)

**Delta Contract**

- Jellyfin playlist delta now proves ordered playlist metadata survives.
  [`rpc.rs:4771`](../../hifimule-daemon/src/rpc.rs#L4771)

**Regression Coverage**

- Existing unchanged files still avoid rewrites.
  [`sync.rs:2449`](../../hifimule-daemon/src/sync.rs#L2449)

- Missing unchanged files are recreated.
  [`sync.rs:2516`](../../hifimule-daemon/src/sync.rs#L2516)
