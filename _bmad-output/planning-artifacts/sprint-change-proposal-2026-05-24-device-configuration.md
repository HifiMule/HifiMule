# Sprint Change Proposal: Device Configuration Improvements

**Date:** 2026-05-24  
**Project:** HifiMule  
**Requested by:** Alexis  
**Status:** Approved - routed for implementation

## 1. Issue Summary

Some target devices, especially Rockbox-based players, expect playlist files in a specific folder that is not always the same folder used for music files. HifiMule currently treats `manifest.managed_paths[0]` as the music folder and writes `.m3u` playlist files into that same folder.

The device manifest also supports name and icon at initialization, but users cannot edit an existing managed device manifest from the UI. That blocks basic corrections like renaming a device, changing its icon, or moving the managed music/playlist folders after setup.

Evidence from current artifacts and code:

- Story 2.9 implemented name/icon capture during initialization only.
- Story 4.7 writes `.m3u` files into the managed music folder.
- `DeviceManifest` currently stores `managed_paths`, `name`, `icon`, and playlist entries, but has no dedicated playlist folder field.
- Folder changes need cleanup semantics so managed tracks/playlists from old locations are removed before syncing into the new target folders, while unmanaged files remain protected.

## 2. Impact Analysis

### Epic Impact

This is a post-completion correction touching completed Epic 2 and Epic 4 behavior.

- **Epic 2: Connection & Verification** needs a follow-up story for editing existing device manifests after initialization.
- **Epic 4: Synchronization Engine** needs a follow-up story for separate playlist folder support and relocation cleanup during sync planning/execution.
- Existing epics remain valid. No epic rollback or MVP reduction is required.

### Story Impact

Affected completed stories:

- **Story 2.6: Initialize New Device Manifest** - initialization should capture a playlist folder in addition to the music folder. Default playlist folder is the chosen music folder.
- **Story 2.9: Device Identity - Name & Icon** - extend from initialize-only to editable name/icon.
- **Story 4.7: Playlist M3U File Generation** - playlist output location should come from a new playlist folder setting, defaulting to the music folder for backward compatibility.
- **Story 4.10: Idempotent Managed File Deletion on USB** - relocation cleanup should reuse the same safe managed deletion behavior.

New proposed stories:

- **Story 10.1: Device Manifest Editing - Identity and Folder Settings**
- **Story 10.2: Separate Playlist Folder and Relocation Cleanup**

### Artifact Conflicts

PRD needs small requirement additions:

- FR26 currently says initialization captures a designated sync folder, name, and icon. It should include a playlist folder path defaulting to the music folder.
- FR11/FR12 preview/differential sync should account for relocation cleanup when managed folders change.
- Device Identity requirement should include editing existing devices, not only setup.

Architecture needs additions:

- `DeviceManifest` extension: add `playlist_path: Option<String>` or a structured `folders` object.
- IPC: add a device update RPC for editable manifest fields, including the selected transcoding profile.
- Sync: treat folder changes as a planned relocation cleanup before writes.
- Device IO rule remains unchanged: all device file operations use `DeviceIO`.

UX spec needs additions:

- Device hub or device settings panel gets an edit action.
- Edit form supports name, icon, transcoding profile, music folder, and playlist folder.
- Folder change confirmation must preview cleanup/resync impact before applying.

### Technical Impact

Recommended manifest shape:

```rust
pub struct DeviceManifest {
    pub managed_paths: Vec<String>,
    #[serde(default)]
    pub playlist_path: Option<String>,
    // existing fields unchanged
}
```

Compatibility rule:

- If `playlist_path` is missing or `None`, playlist output uses `managed_paths[0]`.
- New manifests write `playlistPath` equal to the chosen music folder unless the user selects a different playlist folder.

Folder update behavior:

- Editing name or icon is metadata-only and should update the manifest immediately.
- Editing music folder and/or playlist folder creates a relocation-required state.
- The next sync preview must show managed files/playlists that will be removed from old folder locations and rewritten under the new locations.
- On sync start, HifiMule deletes only manifest-owned tracks and playlist files from old managed locations, then writes new tracks/playlists to the configured locations.
- If cleanup exceeds the existing destructive safety threshold, require the same manual confirmation protocol used for managed cleanup.

## 3. Recommended Approach

**Recommended path:** Direct Adjustment.

This change is moderate in behavior but narrow in architecture. It can be handled by adding two follow-up stories and updating PRD/architecture/UX sections. No completed epic should be rolled back.

Effort estimate: Medium.

Risk level: Medium.

Primary risks:

- Folder relocation must not delete unmanaged files.
- MTP folder handling may need extra care because folder IDs are cached in the manifest.
- Playlist relative paths must still be correct when `.m3u` files live outside the music folder.
- Auto-sync must not silently perform large cleanup without confirmation.

## 4. Detailed Change Proposals

### PRD Changes

Section: Functional Requirements / Device Connection & Discovery

OLD:

```markdown
FR26: The system can initialize a new `.hifimule.json` manifest on a connected device that has not previously been managed, capturing a hardware identifier, a designated sync folder path, an associated Jellyfin user profile, a user-provided display name, and an optional icon identifier selected from a built-in library.
```

NEW:

```markdown
FR26: The system can initialize a new `.hifimule.json` manifest on a connected device that has not previously been managed, capturing a hardware identifier, a designated music sync folder path, a playlist folder path that defaults to the music folder, an associated media-server user profile, a user-provided display name, and an optional icon identifier selected from a built-in library.
```

Add new FR:

```markdown
FR36: The system can edit an existing managed device manifest, allowing users to change device name, icon, transcoding profile, music folder, and playlist folder. Folder changes are reflected in the next sync preview and trigger managed relocation cleanup before new items are written.
```

### Architecture Changes

Section: Data Architecture / Manifest Extension

OLD:

```markdown
.hifimule.json includes `auto_sync_on_connect` (boolean), `auto_fill` block, `transcoding_profile_id`, `name`, `icon`, and `server_id`.
```

NEW:

```markdown
.hifimule.json includes `auto_sync_on_connect` (boolean), `auto_fill` block, `transcoding_profile_id`, `name`, `icon`, `playlist_path` (string | null, defaulting to the first managed music path), and `server_id`.
```

Section: Multi-Device IPC

Add:

```markdown
device.update_manifest(params: {
  deviceId: string,
  name?: string,
  icon?: string | null,
  transcodingProfileId?: string | null,
  musicFolderPath?: string,
  playlistFolderPath?: string | null
}) -> {
  ok: true,
  relocationRequired: boolean,
  cleanupPreview?: {
    tracksToRemove: number,
    playlistsToRemove: number,
    bytesToRemove: number
  }
}
```

Rules:

- Name, icon, and transcoding profile update the manifest immediately without sync relocation.
- `transcodingProfileId` is validated against `device-profiles.json`; null or passthrough clears the device transcoding profile.
- Folder fields are validated as device-relative paths with no absolute path, parent traversal, or empty component.
- `playlistFolderPath` defaults to `musicFolderPath` when omitted.
- Folder updates persist both the new field values and enough prior-location context for the next sync preview, or update manifest entries so the delta planner can detect old-path cleanup from current `local_path` and `playlists.filename` values.

### UX Changes

Section: Device Hub

Add:

```markdown
Each selected device exposes an edit action that opens Device Settings. The settings form allows changing device name, icon, and transcoding profile immediately. It also allows changing music folder and playlist folder, with playlist folder defaulting to the music folder. When a folder value changes, the UI presents a cleanup/resync preview before sync starts.
```

### Story Changes

Story: 2.9 Device Identity - Name & Icon  
Section: Acceptance Criteria

OLD:

```markdown
Given the "Initialize Device" dialog is open
Then a "Device Name" text input is shown...
```

NEW:

```markdown
Given a managed device is selected in the device hub
When I open Device Settings
Then I can edit the device name and icon.
And saving the change updates `.hifimule.json` via DeviceIO write_with_verify.
And the device hub refreshes without requiring reconnect.
```

Story: 4.7 Playlist M3U File Generation  
Section: Acceptance Criteria

OLD:

```markdown
When at least one basket item has `item_type = "Playlist"` and sync runs successfully, a `.m3u` file is written to the managed sync folder (`manifest.managed_paths[0]`, e.g. `device_path/Music`) for each playlist.
```

NEW:

```markdown
When at least one basket item has `item_type = "Playlist"` and sync runs successfully, a `.m3u` file is written to `manifest.playlistPath` for each playlist. If `playlistPath` is absent, null, or empty for an older manifest, the daemon writes playlists to `manifest.managed_paths[0]`.
```

Add:

```markdown
Given the playlist folder differs from the music folder
When generating M3U entries
Then track paths are relative from the playlist file location to each synced track path, using forward slashes.
```

New Story 10.1: Device Manifest Editing - Identity and Folder Settings

Acceptance criteria:

1. Given a managed device is selected, when I open Device Settings, then I can edit name, icon, transcoding profile, music folder, and playlist folder.
2. Given I clear playlist folder, when I save, then the daemon stores it as null/omitted and resolves it to the music folder.
3. Given I change only name, icon, or transcoding profile, when I save, then no sync relocation is required.
4. Given I change music folder or playlist folder, when I save, then the daemon returns relocationRequired = true and the UI marks the next sync preview as requiring cleanup/resync.
5. Given an invalid folder path is entered, when I save, then the daemon rejects absolute paths, parent traversal, root-only unsafe values, and empty path components.
6. Given an MTP device uses cached folder IDs, when a folder path changes, then stale folder ID cache entries for affected paths are cleared or recomputed.

New Story 10.2: Separate Playlist Folder and Relocation Cleanup

Acceptance criteria:

1. Given a manifest has no `playlistPath`, when playlist sync runs, then playlist files are written to `managed_paths[0]` as before.
2. Given `playlistPath` is set, when playlist sync runs, then `.m3u` files are written to that folder.
3. Given `playlistPath` differs from the music folder, when playlist content is generated, then M3U track entries are relative from the playlist folder to the track files.
4. Given the music folder changes, when sync preview is calculated, then existing manifest-owned tracks outside the new music folder are shown as cleanup/removal before rewrite.
5. Given the playlist folder changes, when sync preview is calculated, then existing manifest-owned playlist files outside the new playlist folder are shown as cleanup/removal before rewrite.
6. Given cleanup deletes a manifest-owned file that is already missing, then deletion is treated as successful and the stale manifest entry is removed.
7. Given cleanup would delete more than the configured destructive safety threshold, then the UI requires explicit confirmation before sync proceeds.
8. Given unmanaged files exist in old or new folders, then they are not deleted or modified.

## 5. Implementation Handoff

Scope classification: **Moderate**.

Recommended handoff:

- Product Owner / Developer: add Epic 10 or equivalent follow-up stories to sprint status.
- Developer: implement manifest schema extension, update RPC, update UI settings, and harden sync relocation cleanup.
- Test Architect: verify relocation, MTP path changes, playlist-relative paths, and destructive cleanup confirmation.

Success criteria:

- Existing manifests continue to sync without migration work.
- New devices default playlist location to the music folder.
- Users can edit existing device name and icon.
- Users can edit music and playlist folders.
- Folder changes are visible in sync preview and safely cleaned before new writes.
- Rockbox-style playlist-folder requirements are supported without breaking current playlist behavior.

## Checklist Status

- [x] 1.1 Triggering story identified: Story 2.9 and Story 4.7 expose the gap.
- [x] 1.2 Core problem defined: device manifest configuration is too rigid after initialization.
- [x] 1.3 Evidence gathered from PRD, architecture, stories, and current source shape.
- [x] 2.1 Current epic impact evaluated.
- [x] 2.2 Epic-level changes identified.
- [x] 2.3 Remaining epics reviewed; no invalidation found.
- [x] 2.4 New epic/story need identified.
- [x] 2.5 Priority: direct follow-up after completed epics.
- [x] 3.1 PRD conflicts identified.
- [x] 3.2 Architecture conflicts identified.
- [x] 3.3 UX conflicts identified.
- [x] 3.4 Secondary artifacts: sprint status/stories need update after approval.
- [x] 4.1 Direct adjustment viable.
- [x] 4.2 Rollback not viable.
- [x] 4.3 MVP review not needed.
- [x] 4.4 Recommended path selected: Direct Adjustment.
- [x] 5.1 Issue summary created.
- [x] 5.2 Impact and artifact adjustments documented.
- [x] 5.3 Recommendation documented.
- [x] 5.4 MVP impact and action plan documented.
- [x] 5.5 Handoff plan documented.
- [x] 6.1 Checklist reviewed.
- [x] 6.2 Proposal reviewed.
- [x] 6.3 Approved by Alexis on 2026-05-24.
- [x] 6.4 Sprint status updated with Epic 10 and Stories 10.1-10.2.
- [x] 6.5 Next steps and handoff plan defined.
