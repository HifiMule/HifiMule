# Story 10.2: Separate Playlist Folder and Relocation Cleanup

Status: ready-for-dev

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

- [ ] Resolve playlist output folder from `manifest.playlist_path`, falling back to `manifest.managed_paths[0]`.
- [ ] Update M3U generation to write to the resolved playlist folder.
- [ ] Calculate M3U track paths relative from the playlist folder to each track `local_path`.
- [ ] Update sync preview/delta behavior so track relocation cleanup appears when manifest-owned track paths are outside the configured music folder.
- [ ] Update playlist cleanup so stale manifest-owned playlist files outside the configured playlist folder are removed before writing new playlist files.
- [ ] Reuse Story 4.10 idempotent deletion handling for already-missing managed files.
- [ ] Enforce destructive cleanup threshold before relocation cleanup proceeds.
- [ ] Ensure auto-sync does not silently perform threshold-exceeding relocation cleanup.
- [ ] Add tests for legacy fallback, custom playlist folder writes, relative path generation between sibling folders, music folder relocation cleanup, playlist folder relocation cleanup, threshold confirmation, and unmanaged file protection.

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

TBD

### Debug Log References

TBD

### Completion Notes List

TBD

### File List

TBD

## Change Log

- 2026-05-24: Created from approved Correct Course proposal for device configuration improvements.
