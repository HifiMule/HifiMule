# Story 10.1: Device Manifest Editing - Identity and Folder Settings

Status: ready-for-dev

## Story

As a System Admin (Alexis),
I want to edit an existing managed device manifest,
So that I can correct device identity and folder layout without reinitializing the device.

## Acceptance Criteria

1. Given a managed device is selected, when I open Device Settings, then I can edit name, icon, music folder, and playlist folder.
2. Given I clear playlist folder, when I save, then the daemon stores it as null/omitted and resolves it to the music folder.
3. Given I change only name or icon, when I save, then no sync relocation is required and the device hub refreshes without requiring reconnect.
4. Given I change music folder or playlist folder, when I save, then the daemon returns `relocationRequired = true` and the UI marks the next sync preview as requiring cleanup/resync.
5. Given an invalid folder path is entered, when I save, then the daemon rejects absolute paths, parent traversal, root-only unsafe values, and empty path components.
6. Given an MTP device uses cached folder IDs, when a folder path changes, then stale folder ID cache entries for affected paths are cleared or recomputed.

## Tasks / Subtasks

- [ ] Add `playlist_path: Option<String>` to `DeviceManifest` with `#[serde(default)]` and camelCase JSON compatibility.
- [ ] Extend device initialization to accept optional `playlistFolderPath`, defaulting to the selected music folder.
- [ ] Add `device.update_manifest` RPC for selected device metadata and folder updates.
- [ ] Validate editable folder paths as device-relative paths: no absolute path, parent traversal, empty component, or unsafe root-only value.
- [ ] Persist name/icon-only updates without marking relocation required.
- [ ] Detect music or playlist folder changes and return `relocationRequired = true` plus cleanup preview fields.
- [ ] Clear or recompute affected MTP `folder_ids` cache entries when folder paths change.
- [ ] Add Device Settings UI entry from the selected device hub card.
- [ ] Add UI fields for name, icon, music folder, and playlist folder.
- [ ] Add tests for manifest backward compatibility, path validation, metadata-only updates, folder-change relocation flag, and MTP folder ID invalidation.

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

TBD

### Debug Log References

TBD

### Completion Notes List

TBD

### File List

TBD

## Change Log

- 2026-05-24: Created from approved Correct Course proposal for device configuration improvements.
