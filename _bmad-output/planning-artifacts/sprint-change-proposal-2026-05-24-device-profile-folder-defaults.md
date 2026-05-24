# Sprint Change Proposal: Device Profile Folder Defaults

**Date:** 2026-05-24  
**Project:** HifiMule  
**Requested by:** Alexis  
**Status:** Implemented direct adjustment

## 1. Issue Summary

Device folder configuration is currently generic even though specific device profiles imply known folder conventions. Rockbox devices commonly use `Music` for tracks and `Playlists` for `.m3u` files. Garmin music watches should use `Music` for both tracks and playlists. Because the UI asks for folders before profile selection, it cannot use profile selection to prefill sensible defaults.

## 2. Impact Analysis

Epic 10 remains the right home for this change. Stories 10.1 and 10.2 already added editable folders and separate playlist output; this proposal adds Story 10.3 to make profile metadata drive default folder values.

Artifact impacts:

- PRD: clarify that profile selection can provide folder defaults and that initialization uses those defaults before falling back to the music folder.
- Architecture: extend `device-profiles.json` entries with optional `defaultMusicFolder` and `defaultPlaylistFolder`; expose them through `device_profiles.list`.
- UX: move profile selection before folder inputs in initialization and settings; preserve user folder edits when profile changes.
- Implementation artifacts: add Story 10.3 and mark it done after implementation.

## 3. Recommended Approach

Recommended path: Direct Adjustment.

Effort estimate: Low. Risk level: Low.

The change is additive and backward compatible. Existing profile JSON files continue to parse when the new default fields are absent, and profile defaults are only UI prefills. The manifest still stores explicit folder values after initialization or settings save.

## 4. Detailed Change Proposals

Add profile metadata:

```json
{
  "id": "rockbox-mp3-320",
  "defaultMusicFolder": "Music",
  "defaultPlaylistFolder": "Playlists"
}
```

Built-in defaults:

- Rockbox / iPod MP3 320: `Music`, `Playlists`
- Rockbox / iPod MP3 192: `Music`, `Playlists`
- Garmin Music Watch: `Music`, `Music`
- Generic MP3 Player: `Music`, `Music`

UI behavior:

- Initialize Device shows transcoding profile before music and playlist folder fields.
- Device Settings shows transcoding profile before music and playlist folder fields.
- If the folder fields are untouched, changing profile applies that profile's defaults.
- If the user edited either folder, changing profile preserves the user's folder values.

## 5. Implementation Handoff

Scope classification: Minor.

Developer completed the change directly. Success criteria are profile default metadata, profile-first UI ordering, preservation of manual folder edits, and backward-compatible profile parsing.

## Checklist Status

- [x] 1.1 Triggering story identified: Epic 10 device configuration follow-up.
- [x] 1.2 Core problem defined: profile-specific folder conventions were not represented.
- [x] 1.3 Evidence captured from user examples: Rockbox and Garmin defaults.
- [x] 2.1 Epic impact evaluated.
- [x] 2.2 Story 10.3 added.
- [x] 2.3 Remaining epics unaffected.
- [x] 2.4 No new epic needed.
- [x] 2.5 Priority unchanged.
- [x] 3.1 PRD update identified.
- [x] 3.2 Architecture update identified.
- [x] 3.3 UX update identified.
- [x] 3.4 Sprint/status artifacts updated.
- [x] 4.1 Direct adjustment viable.
- [N/A] 4.2 Rollback not needed.
- [N/A] 4.3 MVP review not needed.
- [x] 4.4 Recommended path selected.
- [x] 5.1 Issue summary created.
- [x] 5.2 Impact documented.
- [x] 5.3 Recommendation documented.
- [x] 5.4 MVP impact documented: none.
- [x] 5.5 Handoff plan documented.
- [x] 6.1 Checklist reviewed.
- [x] 6.2 Proposal reviewed.
- [x] 6.3 User requested direct change on 2026-05-24.
- [x] 6.4 Sprint status updated.
- [x] 6.5 Next steps defined.
