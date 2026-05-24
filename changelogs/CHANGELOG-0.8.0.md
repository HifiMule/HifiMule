# HifiMule 0.8.0

Release date: 2026-05-24

## Highlights

- **Device Settings can now edit existing managed devices**: rename a device, change its icon, switch transcoding profiles, and update music or playlist folders without reinitializing the device.
- **Playlists can live outside the music folder**: Rockbox-style layouts can write tracks to `Music` and playlists to `Playlists`, with `.m3u` entries generated relative to the playlist folder.
- **Folder changes are handled safely**: when a music or playlist folder changes, HifiMule previews cleanup/resync work, only removes manifest-owned files, and asks before large cleanup operations.
- **Device profiles now provide smarter defaults**: Rockbox, Garmin, generic MP3, modern DAP, Sony Walkman, car USB, and spoken-word profiles can prefill recommended music and playlist folders.
- **More built-in device profiles**: new presets cover modern lossless DAPs, Sony Walkman players, car USB sticks, and compact audiobook/podcast sync.
- **UI polish and Windows debug fixes**: the device hub and setup flow received polish, and Windows debug startup behavior was corrected.

---

## New Features

### Editable Device Settings (Story 10.1)

Managed devices now expose a Device Settings action from the selected device hub card.

Editable fields include:
- device name
- device icon
- transcoding profile
- music folder
- playlist folder

Metadata-only changes, such as name, icon, or transcoding profile, refresh the selected device without requiring a reconnect. Folder layout changes return a relocation signal so the UI can clearly mark the next sync preview as cleanup/resync work.

The daemon now provides a `device.update_manifest` RPC for these edits and preserves backward compatibility with older manifests.

### Separate Playlist Folder Support (Story 10.2)

Device manifests can now store an optional `playlistPath`.

If `playlistPath` is absent, HifiMule keeps the previous behavior and writes playlists into the music folder. If it is set, `.m3u` playlist files are written to that folder instead.

This enables common device layouts such as:
- `Music/...` for tracks
- `Playlists/*.m3u` for playlist files

Generated playlist entries are relative from the playlist folder to the synced tracks and always use forward slashes for device compatibility.

### Profile-Based Folder Defaults (Story 10.3)

Device profiles can now include:
- `defaultMusicFolder`
- `defaultPlaylistFolder`

The initialize-device flow and Device Settings dialog use these values as prefills while the folder fields are untouched. If the user edits either folder field manually, HifiMule preserves the user's values when switching profiles.

The profile selector now appears before folder inputs in both setup and settings so the recommended folder layout is visible before manual edits.

### Expanded Built-In Device Profiles

The bundled `device-profiles.json` now includes additional device-oriented presets:
- Rockbox / iPod - MP3 320 kbps
- Rockbox / iPod - MP3 192 kbps
- Generic MP3 Player
- Garmin Music Watch
- Modern DAP - Lossless Friendly
- Sony Walkman - AAC / FLAC
- Car Stereo USB - MP3 256 kbps
- Audiobooks / Podcasts - MP3 96 kbps

Each profile can define both codec/transcoding behavior and recommended folder defaults.

---

## Improvements

### Safer Folder Relocation

Sync delta calculation now treats manifest-owned tracks outside the configured music folder as relocation cleanup plus rewrite work.

Playlist relocation is handled separately: manifest-owned playlist files outside the configured playlist folder are cleaned up before new playlist files are written.

Unmanaged files in old or new folders are left untouched.

### Destructive Cleanup Confirmation

Large cleanup operations now require explicit confirmation before sync proceeds.

The daemon includes cleanup metadata in sync delta responses:
- destructive cleanup count
- destructive cleanup threshold

Manual sync prompts the user when the threshold is exceeded. Auto-sync does not silently run threshold-exceeding relocation cleanup.

### Transcoding Profile Changes Resync Existing Tracks

Changing a device transcoding profile now marks existing synced tracks for rewrite when needed. On the next sync, tracks are regenerated under the active profile instead of leaving older transcoded files in place.

The manifest tracks the last synced transcoding profile and clears the dirty marker only after a successful sync.

### Safer Path Validation

Editable device folder paths are validated as device-relative paths.

Rejected values include:
- absolute paths
- Windows drive-prefixed paths
- parent traversal
- current-directory components
- empty path components
- root-only unsafe values

This validation is used during initialization, manifest updates, playlist folder creation, and cleanup.

### MTP Folder Cache Handling

When music or playlist folders change on MTP devices, stale cached folder IDs for affected paths are cleared or recomputed so future writes target the correct folders.

### Device Hub State

The daemon state now includes connected-device summaries with managed paths, playlist path, and transcoding profile ID so the UI can open Device Settings with accurate current values.

---

## Bug Fixes

### Windows Debug Startup

Fixed Windows debug UI startup behavior in the daemon entrypoint.

### Auto-Fill Slider

Fixed auto-fill slider handling in the basket sidebar so the selected size value is read correctly.

### Root-Managed Device Settings

Fixed Device Settings behavior for devices managed at the device root.

### Safer Profile Persistence

Profile-change persistence now avoids manifest/SQLite divergence on partial failure and avoids deleting existing files before a replacement succeeds.

### Relocation Cleanup Safety

Review fixes tightened several cleanup paths:
- track relocation cleanup validates old track paths correctly
- relocation cannot be collapsed into an ID change that preserves the old folder path
- playlist relocation cleanup counts toward destructive cleanup confirmation
- same-name legacy playlist files are not deleted unless proven manifest-owned
- removed playlist manifest entries are kept if the real delete fails
- stored playlist paths are revalidated before MSC directory creation
- M3U relative path generation respects case-sensitive folder differences

---

## Documentation and Planning Updates

Planning and architecture artifacts were updated for the device configuration work, including PRD, architecture, UX, epics, sprint status, and sprint change proposals for:
- device manifest editing, identity, and folder settings
- separate playlist folder and relocation cleanup
- profile-based default folders and setup ordering

The daemon and UI architecture docs were refreshed where the new RPC contracts, manifest fields, and component behavior changed.

---

## Validation

Automated verification recorded during implementation included:
- `rtk cargo test -p hifimule-daemon`
- `rtk cargo test`
- focused daemon tests for manifest compatibility, folder path validation, metadata-only updates, folder relocation behavior, MTP folder ID invalidation, playlist generation, relocation cleanup, destructive cleanup threshold handling, and settings preview behavior
- TypeScript checks using the repo-local TypeScript compiler
- Vite build verification during Story 10.1

---

## Commits Included

- `7e858e4` - UI polish pass
- `9bf619b` - Added new profile, and implement merge
- `18f6a90` - Dev 10.3
- `b02a483` - Review 10.2
- `ffca43d` - Dev 10.2
- `08db012` - Review 10.1
- `23d830f` - Dev 10.2
- `f646d6c` - Correct course
- `6157fd7` - Fix debug ui on windows
- `839aad1` - Fix aytofill slider
