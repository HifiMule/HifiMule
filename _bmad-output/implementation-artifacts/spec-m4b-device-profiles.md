---
title: Preserve M4B audio with explicit device profiles
type: feature
created: 2026-10-22
status: done
baseline_commit: 4b9b886
context:
  - docs/audiobookshelf-implementation.md
---

<frozen-after-approval reason="User approved the investigation and requested implementation, including the Modern DAP split">

## Intent

**Problem:** Bundled profiles cannot express reliable native M4B support, and Audiobookshelf's sync representation renames verified AAC/M4B sources to M4A. Users need a clear choice between preserving an existing M4B and converting to a target's supported format.

**Approach:** Preserve M4B source identity during sync, require explicit M4B direct-play capability, and add qualified audiobook presets alongside existing music profiles. Extend Garmin with a conservative migration; support M4B scanning and transfer using existing platform mechanisms.

## Boundaries & Constraints

**Always:** Preserve existing music profile IDs/settings and customer modifications. Use verified MIME/codec and scoped provider identities. Keep current playback admission unchanged. Keep chapter data on direct-copy tagging. Conversion for Audiobookshelf requires local encoding; otherwise incompatibility remains an explicit skip. Describe hardware support as qualified by device/firmware/AAC variant.

**Ask First:** Destructive changes to customer profiles, remote mutations, or a new dependency.

**Never:** Globally alias M4B to M4A/MP4 and thereby grant native support to every AAC profile. Do not add M4B encoding/merging/splitting, ALAC source admission, DRM support, or progress synchronization. Do not promise LC/HE/xHE detection: ABS exposes only the codec family; variant enforcement requires a separate native-probing feature.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
| --- | --- | --- | --- |
| Native copy | Verified AAC/MP4 book or podcast with metadata.ext `.m4b` and an M4B-enabled profile | Preview allows and transfer writes `.m4b`; keep audio and embedded chapters | Existing provider validation/cleanup applies |
| No native support | Same source, Sony/base Modern DAP/MP3 profile | No false direct match from audio/mp4 MIME; convert via local encoding if enabled | Otherwise report incompatibility and skip |
| Metadata variation | Uppercase extension, missing/null/malformed metadata, MP3 with misleading extension | Normalize valid M4B evidence only for verified AAC; missing evidence retains existing M4A/MP3 behavior | Optional metadata does not break admission |
| Codec mismatch | M4B with ALAC or unverified codec | No new automatic admission | Preserve unsupported-source outcome |
| Existing installations | Legacy stock Garmin and custom profile file | Upgrade only identified legacy stock capability; append new IDs; retain names/folders/custom arrays | Idempotent merge, existing parse errors unchanged |
| Transport | Managed file named `.m4b` | Scanner recognizes audio; MTP uses existing appropriate audio mapping | No device certification inferred |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/assets/device-profiles.json` — embedded presets.
- `hifimule-daemon/src/providers/audiobookshelf.rs` — optional file metadata and verified sync media.
- `hifimule-daemon/src/sync.rs` — direct/output compatibility and managed destination selection.
- `hifimule-daemon/src/transcoding.rs` — seeding and migration, preserving customizations.
- `hifimule-daemon/src/device_music_scan.rs`, `src/device/mtp.rs`, Windows MTP implementation — recognition/transfer.
- `hifimule-daemon/src/rpc/*tests.rs` — real provider preview and managed transfer regression coverage.

## Tasks & Acceptance

**Execution:**
- [x] Add failing behavior tests before production changes for source preservation, MIME bypass, seeded profile decisions, migration and managed transfer.
- [x] Preserve `.m4b` from validated selected ABS book/podcast audio-file metadata during sync only; tolerate malformed optional metadata; do not trust arbitrary filename extensions as codec proof.
- [x] Require explicit M4B compatibility even when generic MP4 MIME is present. Preserve existing M4A compatibility behavior and codec restrictions.
- [x] Add five profiles: `modern-dap-lossless-m4b` (existing modern music formats plus AAC/M4B, same FLAC fallback/limits); `rockbox-audiobooks-m4b`; `audiobooks-m4b-mp3`; `pocketbook-audiobooks`; `android-voice-audiobooks`. The latter four use MP3 96 kbps fallback, 320 kbps direct ceiling, at most stereo/48 kHz. PocketBook adds Ogg/Vorbis, Voice adds Ogg/Vorbis and Opus; Rockbox/general preservation directly accept MP3 and AAC M4A/M4B. Set audiobook defaults to `Audiobooks` and `Playlists`. Retain existing nine profiles, add explicit AAC/M4B to Garmin and correct the original-download description.
- [x] Migrate unchanged legacy Garmin defaults, append new preset IDs, preserve custom values, and test idempotence. Keep prior MP3-192 migration behavior.
- [x] Recognize M4B in scanning/MTP with focused behavior checks; verify Windows transfer uses existing MP4 audio support if needed.
- [x] Document preset choices, local encoding requirements, AAC variant and chapter limitations in the existing development/ABS docs. Run focused and broader daemon checks; review the complete change and correct findings.

**Acceptance Criteria:**
- Given the shipped M4B presets, when compatible ABS AAC/M4B media is previewed and synced, then the original M4B extension survives and the managed manifest points to the written file.
- Given a profile without explicit native M4B support, when source MIME also says audio/mp4, then the original M4B is not incorrectly copied as device-compatible.
- Given an existing customized profile file, when defaults are merged repeatedly, then new IDs appear once and customized capability arrays remain unchanged.

## Spec Change Log

- 2026-10-23 review patch: scanned outputs that match only the conversion target must retain that target's bitrate ceiling; use the higher direct ceiling only for direct-compatible formats. Preserve the existing 5% measured-output tolerance. A custom profile's audio limits must still apply when it omits format lists; Download Original remains unrestricted. Both regressions were observed failing before their minimal fixes.

## Design Notes

The existing player can continue calling AAC-in-MP4 “M4A”; sync needs the file extension from the selected validated source. No new public source-property schema is required merely to retain this distinction. M4B input is separate from M4B encoding output. The original music profiles remain useful for devices without M4B library recognition; name the new profile's capability explicitly rather than saying the old one cannot play audiobooks.

Implementation separates the existing `MaxStreamingBitrate` direct-copy ceiling from `MusicStreamingTranscodingBitrate`; per-item overrides cap either value. Windows WPD/Shell already preserve generic-file names and need no new extension mapping. Unix/libmtp maps M4B to its existing M4A audio type. The exact legacy Jellyfin-only passthrough description is also migrated when `deviceProfile` is null, preserving custom descriptions and presentation settings.

## Verification

- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon m4b` — new regression checks pass after observed failures.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon` — daemon unit/integration suite, report every failure and ignored hardware check.
- Focused existing MP3 direct-copy, provider, profile merge, scan and local-encoding tests remain green.
- Inspect diff for accidental music-policy changes, custom-profile replacement, and chapter/progress claims.

Physical device compatibility and AAC-variant certification require hardware and remain unverified; this implementation must not describe them as tested.

### Verification evidence

- Initial feature regression cycle: five intended failures, then all passed. Final implementer cycle: six M4B tests and existing MP3 direct-copy tests passed.
- Initial full daemon run: 1,664 passed, two failed, 12 ignored. Both failing Windows music path-policy tests were reproduced individually on an untouched archive of baseline `4b9b8862ac31c7184c1326a057c46ccbf7de08bd`; see the existing deferred-work entry.
- Independent blind and acceptance reviews found no actionable issue. Edge review found the scanned conversion-ceiling regression above; root also caught the omitted-format-list audio-limit regression. With both tests added, red was confirmed: six passed, two failed before fixes.

- Final focused run: eight M4B tests passed after both review fixes; edge reviewer rechecked the fixes without additional findings.
- Final suite: 1,666 daemon tests passed, zero failed, 12 ignored, two baseline-confirmed Windows failures filtered out. All five `audiobookshelf_contract` integration tests passed. Full output: `target/m4b-daemon-regressions.log`.
- JSON validation: 14 unique profile IDs. `git diff --check` passed. No physical-device or Unix/libmtp runtime certification was performed on this Windows host.
- Local commit could not be created: configured GPG signing failed because keyboxd was unavailable; launching the existing daemon also failed. Signing settings were preserved, and the completed changes remain staged.

## Suggested Review Order

**Native sync and compatibility**

- Preserve M4B from validated source metadata during sync.
  [audiobookshelf.rs:3571](../../hifimule-daemon/src/providers/audiobookshelf.rs#L3571)

- Require explicit M4B support despite generic MP4 MIME.
  [sync.rs:1432](../../hifimule-daemon/src/sync.rs#L1432)

- Separate original-copy ceilings from fallback encoder bitrates.
  [sync.rs:1572](../../hifimule-daemon/src/sync.rs#L1572)

- Keep conversion-only scanned files within their encoder ceiling.
  [sync.rs:1801](../../hifimule-daemon/src/sync.rs#L1801)

**Existing installations and devices**

- Upgrade stock Garmin capabilities while preserving custom settings.
  [transcoding.rs:146](../../hifimule-daemon/src/transcoding.rs#L146)

- Recognize M4B during device scanning.
  [device_music_scan.rs:67](../../hifimule-daemon/src/device_music_scan.rs#L67)

- Use the existing MP4 audio transfer type for libmtp.
  [mtp.rs:1678](../../hifimule-daemon/src/device/mtp.rs#L1678)

**Presets and regression coverage**

- New qualified presets retain the existing music-only choices.
  [device-profiles.json:244](../../hifimule-daemon/assets/device-profiles.json#L244)

- Exercise preview, transfer, chapters, manifests and real MP3 conversion.
  [m4b_sync_tests.rs:3](../../hifimule-daemon/src/rpc/m4b_sync_tests.rs#L3)
