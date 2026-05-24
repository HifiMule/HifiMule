# Story 10.3: Profile-Based Default Folders and Setup Ordering

Status: done

## Story

As a System Admin (Alexis),
I want device profiles to prefill recommended music and playlist folders before I edit folder paths,
So that Rockbox and Garmin-style devices start with sensible folder layouts without manual typing.

## Acceptance Criteria

1. Given `device-profiles.json` contains `defaultMusicFolder` and `defaultPlaylistFolder`, when the UI lists device profiles, then `device_profiles.list` returns those default folder fields along with id, name, and description.
2. Given a Rockbox profile is selected during device initialization, when folder fields have not been manually edited, then music folder defaults to `Music` and playlist folder defaults to `Playlists`.
3. Given the Garmin Music Watch profile is selected during device initialization, when folder fields have not been manually edited, then music folder defaults to `Music` and playlist folder defaults to `Music`.
4. Given Device Settings is opened for an existing managed device, then the profile selector appears before music and playlist folder inputs.
5. Given the user changes profile in Device Settings without editing folder fields, then the UI applies the selected profile's folder defaults.
6. Given the user has edited either folder field, when the selected profile changes, then the UI preserves the user's folder edits.

## Tasks / Subtasks

- [x] Add optional profile folder default fields to daemon profile parsing.
- [x] Add built-in default folders for Rockbox, Garmin Music Watch, and generic MP3 player profiles.
- [x] Return default folder fields from `device_profiles.list`.
- [x] Move profile selection before folder fields in Initialize Device.
- [x] Move profile selection before folder fields in Device Settings.
- [x] Apply profile folder defaults only while folder fields are still untouched.
- [x] Keep missing default fields backward compatible for user-edited `device-profiles.json` files.

## Dev Notes

- Existing profile JSON files without default fields continue to parse.
- Profile folder defaults are UI prefills only; the manifest still stores explicit folder settings after initialization or save.
- User folder edits take priority over profile changes.

## References

- Proposal: `_bmad-output/planning-artifacts/sprint-change-proposal-2026-05-24-device-profile-folder-defaults.md`
- PRD: Device Configuration, FR26, Transcoding Handshake
- Architecture: `device-profiles.json`, `device_profiles.list`
- UX: Device Profile Settings

## Dev Agent Record

### Agent Model Used

GPT-5 Codex

### Completion Notes List

- Added optional `defaultMusicFolder` and `defaultPlaylistFolder` metadata to device profiles.
- Rockbox profiles now default to `Music` and `Playlists`; Garmin Music Watch and Generic MP3 Player default to `Music` and `Music`.
- Initialization and settings dialogs now place profile selection before folder inputs and use defaults while preserving manual folder edits.

### File List

- `_bmad-output/implementation-artifacts/10-3-profile-based-default-folders-and-setup-ordering.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `_bmad-output/planning-artifacts/architecture.md`
- `_bmad-output/planning-artifacts/epics.md`
- `_bmad-output/planning-artifacts/prd.md`
- `_bmad-output/planning-artifacts/ux-design-specification.md`
- `hifimule-daemon/assets/device-profiles.json`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-daemon/src/transcoding.rs`
- `hifimule-ui/src/components/BasketSidebar.ts`
- `hifimule-ui/src/components/InitDeviceModal.ts`

## Change Log

- 2026-05-24: Implemented profile-based default folders and setup/settings ordering.
