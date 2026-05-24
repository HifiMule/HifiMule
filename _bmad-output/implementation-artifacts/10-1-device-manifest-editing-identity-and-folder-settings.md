# Story 10.1: Device Manifest Editing - Identity and Folder Settings

Status: review

## Story

As a System Admin (Alexis),
I want to edit an existing managed device manifest,
So that I can correct device identity and folder layout without reinitializing the device.

## Acceptance Criteria

1. Given a managed device is selected, when I open Device Settings, then I can edit name, icon, transcoding profile, music folder, and playlist folder.
2. Given I clear playlist folder, when I save, then the daemon stores it as null/omitted and resolves it to the music folder.
3. Given I change only name, icon, or transcoding profile, when I save, then no sync relocation is required and the device hub refreshes without requiring reconnect.
4. Given I change music folder or playlist folder, when I save, then the daemon returns `relocationRequired = true` and the UI marks the next sync preview as requiring cleanup/resync.
5. Given an invalid folder path is entered, when I save, then the daemon rejects absolute paths, parent traversal, root-only unsafe values, and empty path components.
6. Given an MTP device uses cached folder IDs, when a folder path changes, then stale folder ID cache entries for affected paths are cleared or recomputed.

## Tasks / Subtasks

- [x] Add `playlist_path: Option<String>` to `DeviceManifest` with `#[serde(default)]` and camelCase JSON compatibility.
- [x] Extend device initialization to accept optional `playlistFolderPath`, defaulting to the selected music folder.
- [x] Add `device.update_manifest` RPC for selected device metadata, transcoding profile, and folder updates.
- [x] Validate editable folder paths as device-relative paths: no absolute path, parent traversal, empty component, or unsafe root-only value.
- [x] Persist name/icon/profile-only updates without marking relocation required.
- [x] Detect music or playlist folder changes and return `relocationRequired = true` plus cleanup preview fields.
- [x] Clear or recompute affected MTP `folder_ids` cache entries when folder paths change.
- [x] Add Device Settings UI entry from the selected device hub card.
- [x] Add UI fields for name, icon, transcoding profile, music folder, and playlist folder.
- [x] Add tests for manifest backward compatibility, path validation, metadata-only updates, folder-change relocation flag, and MTP folder ID invalidation.

## Dev Notes

- Existing manifests must continue to deserialize and sync unchanged.
- Keep using `DeviceIO` and `device::write_manifest()` for device manifest writes.
- Do not write directly to device paths from RPC handlers.
- Name/icon behavior should align with Story 2.9 validation and icon whitelist.
- `playlistFolderPath` should behave like "inherit music folder" when omitted, null, or empty.

## References

- Proposal: `_bmad-output/planning-artifacts/sprint-change-proposal-2026-05-24-device-configuration.md`
- PRD: FR26, FR36
- Architecture: Data Architecture / Manifest Extension, Multi-Device IPC
- UX: Device Profile Settings, Device Hub
- Prior stories: 2.6, 2.9, 7.3

## Dev Agent Record

### Agent Model Used

GPT-5 Codex

### Debug Log References

- `rtk cargo test -p hifimule-daemon`
- `rtk cargo test`
- `rtk C:\Users\alexi\.cache\codex-runtimes\codex-primary-runtime\dependencies\node\bin\node.exe node_modules/typescript/bin/tsc`
- `rtk C:\Users\alexi\.cache\codex-runtimes\codex-primary-runtime\dependencies\node\bin\node.exe node_modules/vite/bin/vite.js build`

### Completion Notes List

- Added backward-compatible `playlistPath` manifest support with music-folder fallback resolution.
- Extended device initialization and the UI initialize flow to carry an optional playlist folder.
- Added `device.update_manifest` for selected-device identity, transcoding profile, and folder edits, including validation, relocation signaling, cleanup preview counts, and affected MTP `folder_ids` cache invalidation.
- Added a Device Settings dialog from the selected device hub card and a cleanup/resync marker after folder-layout edits.
- Updated Device Settings to reuse the tile-based creation icon selector and edit the selected device transcoding profile.
- Profile changes now mark existing synced tracks for rewrite on the next sync so already-transcoded files are regenerated under the active profile.
- Added daemon coverage for manifest compatibility, path validation, metadata-only updates, folder relocation behavior, cache invalidation, and RPC persistence.

### File List

- `_bmad-output/implementation-artifacts/10-1-device-manifest-editing-identity-and-folder-settings.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `hifimule-daemon/src/device/mod.rs`
- `hifimule-daemon/src/device/tests.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-ui/src/components/BasketSidebar.ts`
- `hifimule-ui/src/components/InitDeviceModal.ts`
- `hifimule-ui/src/styles.css`

## Change Log

- 2026-05-24: Created from approved Correct Course proposal for device configuration improvements.
- 2026-05-24: Implemented device manifest editing for identity and folder settings; story moved to review.
- 2026-05-24: Adjusted editor UX to match creation icon selection and added profile editing.
- 2026-05-24: Added profile-change dirty tracking so existing synced tracks are retranscoded on the next sync.
