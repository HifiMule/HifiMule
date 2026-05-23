# Sprint Change Proposal - 2026-05-23 - Navidrome/Subsonic Parity and Sync Correctness

## 1. Issue Summary

Navidrome/Subsonic behavior is not currently on par with Jellyfin in three user-visible areas, and one USB deletion error is surfacing during sync cleanup:

- Navidrome/Subsonic browsing does not expose Recently Added, Frequently Played, or Recently Played navigation.
- Albums do not have alphabetic quick filtering comparable to Jellyfin.
- Device-profile-incompatible files can be written with a target extension without confirmed transcoding, for example a FLAC payload saved as `.mp3`.
- On USB mass-storage deletes, missing managed files can produce `Failed to delete file: Le fichier spécifié est introuvable. (os error 2)`.

Evidence from implementation:

- `SubsonicProvider.capabilities()` currently advertises only Artists, Albums, Playlists, Genres, and Favorites; it omits Recently Added, Frequently Played, and Recently Played. See `hifimule-daemon/src/providers/subsonic.rs:396`.
- `SubsonicProvider` has an explicit test asserting `list_recently_added` is unsupported. See `hifimule-daemon/src/providers/subsonic.rs:2508`.
- Subsonic stream URL construction exists and can request `/rest/stream.view` with `format` and `maxBitRate`. See `hifimule-daemon/src/providers/subsonic.rs:683`.
- Sync extension selection lets a device-preferred audio container override the source extension even when actual transcoding is not confirmed. See `hifimule-daemon/src/sync.rs:631` and `hifimule-daemon/src/sync.rs:664`.
- MSC delete uses `tokio::fs::remove_file` directly and propagates missing-file errors. See `hifimule-daemon/src/device_io.rs:156`.

## 2. Impact Analysis

**Epic Impact:** Epic 9 and Epic 4 are affected. Epic 9 remains feature-complete for Jellyfin but needs provider-parity correction for OpenSubsonic/Navidrome. Epic 4 needs a sync correctness hardening story because filename extension and actual content format must never diverge.

**Story Impact:** Story 9.4 was scoped around capability-driven history modes but accepted Subsonic unsupported behavior. That is now insufficient for Navidrome parity. Story 8.3 already required Subsonic streaming URL support, but sync correctness needs follow-through in the active sync path. Story 4.8 covered transcoding profiles, but the observed FLAC-as-MP3 case shows the acceptance criteria need an explicit content/extension guard.

**Artifact Conflicts:** PRD FR8 already requires provider-normalized navigation modes where supported. PRD FR31 describes Jellyfin transcoding specifically, but current product scope now includes Subsonic/OpenSubsonic; architecture already states Subsonic transcoding is provider-specific via `stream?format=mp3&maxBitRate=192`. No PRD reduction is needed, but the epics/stories must be amended.

**Technical Impact:** The provider abstraction remains valid. The fix should not add server-specific logic in the UI. It should extend Subsonic provider capabilities and methods, route sync through provider-neutral download/stream behavior where possible, and make missing managed-file deletes idempotent or manifest-aware.

## 3. Recommended Approach

Use **Direct Adjustment**.

Add focused corrective stories rather than replan the product:

- **Story 9.6: Navidrome/Subsonic Browse Parity Hardening**
- **Story 4.9: Provider-Neutral Transcoding and Extension Verification**
- **Story 4.10: Idempotent Managed File Deletion on USB**

Effort: Medium. Risk: Medium. The browse changes depend on exact Subsonic/OpenSubsonic endpoint support and must hide unsupported modes honestly. The sync changes are correctness-critical because silent format mismatch can produce unusable files on target devices.

Rollback is not recommended; the existing provider and UI architecture is the right shape. MVP review is not required; these are parity/correctness defects against already-approved scope.

## 4. Detailed Change Proposals

### Story: 9.6 Navidrome/Subsonic Browse Parity Hardening

Section: New story under Epic 9.

OLD:

Story 9.4 notes Subsonic history modes as unsupported and hides them via `BrowseCapabilities`.

NEW:

As a Navidrome/Subsonic user, I want the browse surface to expose every history/navigation mode the active server can support, so that switching from Jellyfin does not remove core curation workflows.

Acceptance criteria:

- Given a Navidrome/OpenSubsonic server exposes a reliable Recently Added album endpoint, when `browse.listModes` is called, then `recentlyAdded` is included and `browse.listRecentlyAdded` returns newest albums first.
- Given a Navidrome/OpenSubsonic server exposes reliable frequent/recent listening data, when `browse.listModes` is called, then `frequentlyPlayed` and `recentlyPlayed` are included and sorted correctly.
- Given classic Subsonic cannot support a mode reliably, then the mode remains hidden and the daemon returns `UnsupportedCapability` if called directly.
- Given Albums mode is open and the result count warrants quick navigation, then alphabetic filtering works for Subsonic/Navidrome albums as it does for Jellyfin.
- Given provider support differs by server, then all capability decisions are made in `SubsonicProvider`, not in the UI.

Rationale: This preserves the provider-neutral architecture while making Navidrome parity explicit instead of accidentally accepting a smaller feature set.

### Story: 4.9 Provider-Neutral Transcoding, Compatibility, and Extension Verification

Section: New story under Epic 4.

OLD:

Story 4.8 requires device transcoding profiles and says Subsonic `download_url(song_id, Some(profile))` returns `/rest/stream.view`, but sync correctness does not explicitly require content format confirmation before assigning the target extension.

NEW:

As a user syncing to a device that only supports MP3, I want incompatible source formats to be truly transcoded before being written, and skipped when compatible transcoding is unavailable, so that the device never receives unplayable media.

Acceptance criteria:

- Given the target device/profile requires MP3 and the source item is FLAC, when sync starts against a Subsonic/Navidrome provider, then the daemon requests a provider stream URL with `format=mp3` and `maxBitRate` in kbps.
- Given the target device/profile requires MP3 and the provider cannot transcode the track to MP3, then sync skips that track with a clear warning/result entry instead of copying the incompatible source file.
- Given transcoding is requested but cannot be negotiated or confirmed, then sync skips that item with a clear warning/result entry instead of writing FLAC bytes to an `.mp3` path.
- Given the provider returns direct/passthrough content, then the output filename uses the original source suffix, not the requested target extension.
- Given the provider returns direct/passthrough content whose source suffix/content type is not compatible with the active device profile, then sync skips the item and keeps it out of the manifest.
- Given the active provider is Jellyfin, existing PlaybackInfo transcoding behavior remains intact.
- Given the active provider is Subsonic/OpenSubsonic, no code outside `providers/subsonic.rs` constructs Subsonic stream URLs directly.
- Tests cover FLAC-to-MP3 success, direct-download fallback preserving `.flac` only when compatible, skipped incompatible direct downloads, skipped tracks when required transcoding cannot be honored, and manifest exclusion for skipped items.

Rationale: The observed FLAC-as-MP3 case is a data correctness bug, not just a UI/profile issue. The safe fallback is omission, not passthrough, when a device profile says the source format is unsupported.

### Story: 4.10 Idempotent Managed File Deletion on USB

Section: New story under Epic 4.

OLD:

MSC deletion propagates `remove_file` not-found errors directly during managed cleanup.

NEW:

As a user syncing a USB drive, I want cleanup to tolerate files that are already missing, so that stale manifest entries do not turn into noisy sync failures.

Acceptance criteria:

- Given a managed manifest entry points to a file that is already absent on an MSC device, when sync cleanup deletes it, then the delete is treated as successful and the manifest entry is removed.
- Given deletion fails for a real permission, readonly, or IO error, then sync reports the error and does not silently drop the manifest entry.
- Given an MTP backend reports item missing during delete, then the sync layer treats that missing-object case equivalently where the backend can distinguish it.
- Tests cover MSC missing-file deletion, a real deletion error, and sync cleanup removing stale manifest entries without failing the operation.

Rationale: Managed sync should be resilient to manual deletion or prior partial cleanup while still protecting against genuine device errors.

## 5. Checklist Status

- [x] 1.1 Triggering story identified: Epic 9 history navigation and Epic 4 sync/transcoding behavior.
- [x] 1.2 Core problem defined: Provider parity gaps plus sync correctness bug plus stale USB delete handling.
- [x] 1.3 Evidence gathered from implementation and user-reported error.
- [x] 2.1 Current epics can still be completed with amendments.
- [x] 2.2 Required epic-level changes: add three corrective stories.
- [x] 2.3 Remaining epics reviewed: no packaging, scrobble, or architecture replan required.
- [x] 2.4 No future epic is invalidated.
- [x] 2.5 Priority should move these stories ahead of any new polish work.
- [x] 3.1 PRD remains valid; no MVP reduction required.
- [x] 3.2 Architecture remains valid; provider/sync enforcement needs story-level tightening.
- [x] 3.3 UX remains valid; unsupported modes must still be hidden honestly.
- [x] 3.4 Testing artifacts need new provider and sync regression coverage.
- [x] 4.1 Direct Adjustment is viable.
- [N/A] 4.2 Rollback is not recommended.
- [N/A] 4.3 MVP review is not required.
- [x] 4.4 Recommended path selected: Direct Adjustment.
- [x] 5.1 Issue summary created.
- [x] 5.2 Epic/artifact impacts documented.
- [x] 5.3 Recommended path documented.
- [x] 5.4 MVP impact documented.
- [x] 5.5 Handoff plan defined.

## 6. Implementation Handoff

Scope classification: **Moderate**.

Route to Developer agent for story creation and implementation. Product Owner/backlog coordination is needed only to add the corrective stories and update `sprint-status.yaml` after approval.

Suggested sprint-status changes after approval:

- Keep `epic-9: in-progress`.
- Add `9-6-navidrome-subsonic-browse-parity-hardening: ready-for-dev`.
- Move `epic-4` from `done` to `in-progress`.
- Add `4-9-provider-neutral-transcoding-and-extension-verification: ready-for-dev`.
- Add `4-10-idempotent-managed-file-deletion-on-usb: ready-for-dev`.

Success criteria:

- Navidrome/OpenSubsonic shows all history modes it can support and keeps unsupported modes hidden.
- Albums quick-nav behaves consistently for Jellyfin and Subsonic/Navidrome.
- Required MP3 sync produces actual MP3 content or skips incompatible tracks with visible warnings; it never writes FLAC content with an `.mp3` extension and never copies incompatible direct-play files to the device.
- Missing managed files on USB deletion no longer fail sync with OS error 2.
- Verification includes `rtk cargo test -p hifimule-daemon`, `rtk tsc` from `hifimule-ui`, and manual smoke tests against a Navidrome server and a USB MSC device.
