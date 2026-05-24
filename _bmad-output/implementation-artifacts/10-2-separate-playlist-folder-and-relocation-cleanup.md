# Story 10.2: Separate Playlist Folder and Relocation Cleanup

Status: review

## Story

As a Rockbox/DAP user,
I want playlist files to be written to the folder my device expects,
So that native playlist browsing works while HifiMule still manages music safely.

## Acceptance Criteria

1. Given a manifest has no `playlistPath`, when playlist sync runs, then playlist files are written to `managed_paths[0]` as before.
2. Given `playlistPath` is set, when playlist sync runs, then `.m3u` files are written to that folder.
3. Given `playlistPath` differs from the music folder, when playlist content is generated, then M3U track entries are relative from the playlist folder to the track files, using forward slashes.
4. Given the music folder changes, when sync preview is calculated, then existing manifest-owned tracks outside the new music folder are shown as cleanup/removal before rewrite.
5. Given the playlist folder changes, when sync preview is calculated, then existing manifest-owned playlist files outside the new playlist folder are shown as cleanup/removal before rewrite.
6. Given cleanup deletes a manifest-owned file that is already missing, when sync cleanup runs, then deletion is treated as successful and the stale manifest entry is removed.
7. Given cleanup would delete more than the configured destructive safety threshold, when sync starts, then the UI requires explicit confirmation before sync proceeds.
8. Given unmanaged files exist in old or new folders, when relocation cleanup runs, then they are not deleted or modified.

## Tasks / Subtasks

- [x] Resolve playlist output folder from `manifest.playlist_path`, falling back to `manifest.managed_paths[0]`.
- [x] Update M3U generation to write to the resolved playlist folder.
- [x] Calculate M3U track paths relative from the playlist folder to each track `local_path`.
- [x] Update sync preview/delta behavior so track relocation cleanup appears when manifest-owned track paths are outside the configured music folder.
- [x] Update playlist cleanup so stale manifest-owned playlist files outside the configured playlist folder are removed before writing new playlist files.
- [x] Reuse Story 4.10 idempotent deletion handling for already-missing managed files.
- [x] Enforce destructive cleanup threshold before relocation cleanup proceeds.
- [x] Ensure auto-sync does not silently perform threshold-exceeding relocation cleanup.
- [x] Add tests for legacy fallback, custom playlist folder writes, relative path generation between sibling folders, music folder relocation cleanup, playlist folder relocation cleanup, threshold confirmation, and unmanaged file protection.

## Dev Notes

- Do not delete files that are not represented by manifest-owned `synced_items` or `playlists` entries.
- Do not assume playlist files live under the same folder as tracks.
- Preserve forward slash path normalization in `.m3u` files.
- Keep all device operations behind `DeviceIO`.
- MTP implementations may need folder creation and folder ID cache refresh for the new playlist folder.

## References

- Proposal: `_bmad-output/planning-artifacts/sprint-change-proposal-2026-05-24-device-configuration.md`
- PRD: FR11, FR12, FR36
- Architecture: Manifest Extension, Device IO Abstraction, Multi-Device IPC
- Prior stories: 4.7, 4.10, 7.1

## Dev Agent Record

### Agent Model Used

GPT-5 Codex

### Debug Log References

- `rtk cargo test -p hifimule-daemon generate_m3u`
- `rtk cargo test -p hifimule-daemon relocation`
- `rtk cargo test -p hifimule-daemon destructive_cleanup_threshold`
- `rtk cargo test -p hifimule-daemon`
- `rtk powershell -NoProfile -Command "& 'C:\Users\alexi\.cache\codex-runtimes\codex-primary-runtime\dependencies\node\bin\node.exe' .\node_modules\typescript\bin\tsc --noEmit"`

### Completion Notes List

- Playlist generation now resolves output from `manifest.playlist_path`, falling back to the music folder, and ensures the playlist folder exists through `DeviceIO`.
- M3U entries are generated relative from the playlist folder to each synced track path with forward slash normalization.
- Sync delta now treats manifest-owned tracks outside the configured music folder as relocation cleanup plus rewrite work while leaving unmanaged files untouched.
- Playlist relocation cleanup removes manifest-owned stale playlist files from previous music/custom playlist locations, including already-missing files as successful cleanup.
- Large destructive cleanup jobs require explicit `confirmDestructiveCleanup` on manual sync; auto-sync skips threshold-exceeding cleanup instead of running silently.
- Added daemon tests for fallback/custom playlist writes, sibling-folder relative paths, track and playlist relocation cleanup, settings preview, and destructive confirmation.

### File List

- `_bmad-output/implementation-artifacts/10-2-separate-playlist-folder-and-relocation-cleanup.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `hifimule-daemon/src/main.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-daemon/src/sync.rs`
- `hifimule-ui/src/components/BasketSidebar.ts`

## Change Log

- 2026-05-24: Created from approved Correct Course proposal for device configuration improvements.
- 2026-05-24: Implemented separate playlist folder output, relocation cleanup, destructive cleanup confirmation, and tests.
