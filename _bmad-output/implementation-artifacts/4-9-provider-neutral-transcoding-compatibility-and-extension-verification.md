# Story 4.9: Provider-Neutral Transcoding, Compatibility, and Extension Verification

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a user syncing to a device that only supports MP3,
I want incompatible source formats to be truly transcoded before being written, and skipped when compatible transcoding is unavailable,
so that the device never receives unplayable media.

## Acceptance Criteria

1. **Given** the target device/profile requires MP3 and the source item is FLAC, **When** sync starts against a Subsonic/Navidrome provider, **Then** the daemon requests a provider stream URL with `format=mp3` and `maxBitRate` in kbps.

2. **Given** the target device/profile requires MP3 and the provider cannot transcode the track to MP3, **When** sync plans or executes that item, **Then** sync skips that track with a clear warning/result entry instead of copying the incompatible source file. **And** the skipped track is not written to the device manifest.

3. **Given** transcoding is requested but cannot be negotiated or confirmed, **When** sync executes the item, **Then** sync skips that item with a clear warning/result entry instead of writing source bytes to a target-extension path.

4. **Given** the provider returns direct/passthrough content, **When** the source suffix/content type is compatible with the active device profile, **Then** the output filename uses the original source suffix, not the requested target extension.

5. **Given** the provider returns direct/passthrough content whose source suffix/content type is not compatible with the active device profile, **When** sync executes the item, **Then** sync skips the item and keeps it out of the manifest.

6. **Given** the active provider is Jellyfin, **When** a non-passthrough device profile is selected, **Then** existing PlaybackInfo transcoding behavior remains intact.

7. **Given** the active provider is Subsonic/OpenSubsonic, **When** sync needs a stream or download URL, **Then** no code outside `providers/subsonic.rs` constructs Subsonic stream URLs directly.

## Tasks / Subtasks

- [x] Task 1: Define profile compatibility checks in the sync path (AC: 2, 4, 5)
  - [x] Derive the target device-compatible container/codec set from the active transcoding profile or device-preferred audio container.
  - [x] Derive the source container/content type from provider metadata (`provider_suffix`, `provider_content_type`) before writing.
  - [x] Treat profile compatibility as a hard device constraint, not a best-effort preference.

- [x] Task 2: Enforce skip-on-unsupported-transcode behavior (AC: 2, 3, 5)
  - [x] If required transcoding cannot be negotiated, record a warning/result entry and continue with the next item.
  - [x] Do not write the file to the device.
  - [x] Do not append skipped items to the manifest.
  - [x] Ensure the UI-visible sync operation status can surface skipped/incompatible items clearly.

- [x] Task 3: Preserve direct/passthrough correctness (AC: 4, 5)
  - [x] If passthrough is compatible, keep the source suffix as the filename extension.
  - [x] If passthrough is incompatible with the device profile, skip instead of writing.
  - [x] Ensure filename extension is chosen from actual confirmed output content, not just the requested profile.

- [x] Task 4: Keep provider boundaries intact (AC: 1, 6, 7)
  - [x] For Subsonic/OpenSubsonic, call `provider.download_url(track_id, profile)` and keep stream URL construction inside `providers/subsonic.rs`.
  - [x] For Jellyfin, preserve PlaybackInfo-based stream resolution and direct-play detection.
  - [x] Avoid UI-specific or provider-specific branching in the generic sync engine except through provider/domain contracts.

- [x] Task 5: Verification (AC: 1-7)
  - [x] Add tests for Subsonic FLAC-to-MP3 stream URL generation with `maxBitRate` in kbps.
  - [x] Add tests for compatible direct-download fallback preserving `.flac` or the actual source suffix.
  - [x] Add tests for skipped incompatible direct downloads.
  - [x] Add tests for skipped tracks when required transcoding cannot be honored.
  - [x] Add tests proving skipped items are excluded from the manifest.
  - [x] Run `rtk cargo test -p hifimule-daemon`.

### Review Findings

- [x] [Review][Patch] Real provider sync drops Subsonic source format metadata, so compatible passthrough cannot be detected [hifimule-daemon/src/rpc.rs:1462]
- [x] [Review][Patch] Missing or unreadable selected transcoding profile falls back to unconstrained provider sync [hifimule-daemon/src/rpc.rs:2875]
- [x] [Review][Patch] Direct compatibility flattens container and codec aliases, allowing unconfirmed codecs to pass through [hifimule-daemon/src/sync.rs:477]
- [x] [Review][Patch] Generic direct-download response content types can cause compatible passthrough tracks to be skipped [hifimule-daemon/src/sync.rs:1572]
- [x] [Review][Patch] Skipped incompatible items leave byte progress and ETA totals inconsistent [hifimule-daemon/src/sync.rs:1426]

## Dev Notes

### Current Code Context

- `hifimule-daemon/src/sync.rs` has `forced_audio_profile(container)` and currently applies `preferred_audio_container` before stream resolution. Review this carefully; the previous behavior can choose an output extension even when actual content compatibility is not confirmed.
- `execute_sync()` currently determines `extension_override` after stream resolution. The safe rule is: requested target extension applies only when transcoding is confirmed; incompatible passthrough must be skipped.
- `SyncOperation` already has `warnings: Vec<String>` and `errors: Vec<SyncFileError>`. Prefer a visible warning/result for skipped incompatible tracks unless implementation already has a more precise skipped-item structure.
- `SyncAddItem` includes provider metadata fields: `provider_album_id`, `provider_content_type`, and `provider_suffix`. Use these rather than probing filenames where possible.
- `hifimule-daemon/src/providers/subsonic.rs` implements `download_url(song_id, Some(profile))` using `/rest/stream.view` with `format` and `maxBitRate`; keep this provider-local.
- `hifimule-daemon/src/providers/jellyfin.rs` and `hifimule-daemon/src/api.rs` own Jellyfin PlaybackInfo behavior; preserve existing tests and behavior.

### Safety Rule

The fallback for unsupported or unconfirmed transcoding is omission, not passthrough. HifiMule should never copy a source file that the selected device profile says is incompatible.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story-4.9-Provider-Neutral-Transcoding-Compatibility-and-Extension-Verification]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-05-23-navidrome-subsonic-parity.md]
- [Source: hifimule-daemon/src/sync.rs] (`execute_sync`, `forced_audio_profile`, extension selection)
- [Source: hifimule-daemon/src/providers/subsonic.rs] (`download_url`, `stream_url`)
- [Source: hifimule-daemon/src/providers/mod.rs] (`MediaProvider`, `TranscodeProfile`)

## Dev Agent Record

### Agent Model Used

GPT-5 Codex

### Debug Log References

- `rtk cargo test -p hifimule-daemon execute_provider_sync_` - red phase failed before implementation because `execute_provider_sync` lacked active profile input; green phase passed 4 provider-sync tests.
- `rtk cargo test -p hifimule-daemon test_rpc_sync_detect_changes_returns_stable_wire_fields_and_metadata` - passed after aligning the test fixture with album-managed change detection.
- `rtk cargo test -p hifimule-daemon` - passed, 348 tests.
- `rtk cargo clippy -p hifimule-daemon --all-targets` - exited successfully; repo still reports existing clippy warning categories.
- `rtk cargo test -p hifimule-daemon` - passed after review fixes, 349 tests.
- `rtk cargo clippy -p hifimule-daemon --all-targets` - exited successfully after review fixes; repo still reports existing clippy warning categories.

### Completion Notes List

- Added provider-neutral audio compatibility derivation from active device profiles and device-preferred containers, using provider suffix/content-type metadata before writing.
- Updated Subsonic/OpenSubsonic provider sync to request transcoding only when source metadata is not direct-compatible, confirm returned content before choosing the output extension, and skip incompatible or unconfirmed output with operation warnings.
- Preserved compatible passthrough suffixes and kept skipped tracks out of both device writes and manifest updates.
- Loaded active transcoding profiles for non-Jellyfin provider execution while keeping Subsonic URL construction behind `MediaProvider::download_url`.
- Kept Jellyfin PlaybackInfo execution behavior unchanged.
- Resolved review findings by carrying provider format metadata through provider songs, failing invalid selected profile loads, separating container and codec compatibility checks, allowing generic binary direct-download responses when source metadata is compatible, and adjusting byte totals for skipped items.

### File List

- `_bmad-output/implementation-artifacts/4-9-provider-neutral-transcoding-compatibility-and-extension-verification.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `hifimule-daemon/src/domain/models.rs`
- `hifimule-daemon/src/providers/jellyfin.rs`
- `hifimule-daemon/src/providers/subsonic.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-daemon/src/sync.rs`

## Change Log

- 2026-05-23: Created story from approved Correct Course proposal for Navidrome/Subsonic parity and sync correctness.
- 2026-05-23: Implemented provider-neutral transcoding compatibility enforcement, skip warnings, passthrough extension preservation, and provider-sync verification tests.
- 2026-05-23: Resolved code-review findings and marked story done.
