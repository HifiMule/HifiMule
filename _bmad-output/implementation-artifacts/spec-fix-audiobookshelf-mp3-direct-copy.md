---
title: 'Copy compatible Audiobookshelf MP3 files with the 192 kbps profile'
type: 'bugfix'
created: '2026-10-22'
status: 'done'
baseline_commit: 'cadc7fe40a29a05a02bc7d1111447bbeccecc0d3'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Selecting the shipped Rockbox / iPod MP3 192 kbps profile blocks an Audiobookshelf podcast containing MP3 at 128 kbps. Its empty direct-play list rejects every original, while Audiobookshelf has no verified downloadable server transcode fallback.

**Approach:** Allow original MP3 within the 192 kbps ceiling in that preset, including existing installations using the unchanged shipped preset. Carry available Audiobookshelf source properties into one shared direct-copy decision used by preview and transfer, so admitting MP3 does not accidentally admit a known bitrate above the ceiling.

## Boundaries & Constraints

**Always:** Preserve authentication, session cleanup, identity checks, managed ownership and atomic publication. Treat 192 kbps as a ceiling; honor known audio constraints and bitrate overrides. Preserve customer edits and other presets. Missing properties retain the existing unknown-value policy without invented evidence.

**Ask First:** Extending this change to other preset policies, changing unknown-property admission, or adding Audiobookshelf server transcoding support.

**Never:** Treat every transcode target as permission for direct copying; disable compatibility checks globally; infer bitrate from an episode's capacity estimate; modify playback/progress behavior; add dependencies or a new settings flow.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Reported case | Verified MP3, 128000 bps, shipped 192 preset | Preview admits original; transfer copies original audio without encoding | Existing download errors apply |
| Boundary | Verified MP3, 192000 bps | Direct copy | Existing download errors apply |
| Above ceiling | Verified MP3, 192001 or 320000 bps | Direct-copy decision rejects; existing conversion/block policy applies | No incompatible original published |
| Unsupported format | Verified AAC/M4A against MP3-only preset | Remains incompatible for direct copy | Existing blocked reason remains truthful |
| Missing properties | Verified MP3; bitrate absent, null or zero | Retain existing unknown-property policy; do not fabricate bitrate | Malformed optional evidence cannot become a large or wrapped value |
| Existing default | Old, unchanged shipped 192 payload, including pre-bit-depth variant | Narrow migration adds MP3 direct support; rerun is stable | Invalid JSON retains existing error handling |
| Customer profile | Edited payload, custom force-transcode profile or different preset | No migration or implicit new direct formats | Preserve all user values |
| Changed source | Properties change between preview and execution | Transfer reevaluates current verified representation | Reject incompatible original before publication |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/assets/device-profiles.json` — 192 preset forbids direct play.
- `hifimule-daemon/src/transcoding.rs` — default merging preserves existing arrays.
- `hifimule-daemon/src/providers/audiobookshelf.rs` — file DTOs discard source properties.
- `hifimule-daemon/src/providers/mod.rs` — private sync representation drops numerical properties.
- `hifimule-daemon/src/sync.rs` — preview checks format only; execution checks planned bitrate/properties.
- `hifimule-daemon/src/rpc.rs` — both preflight routes share `blocked_provider_media_add`; podcast desired items have unknown bitrate.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/assets/device-profiles.json` — add MP3 direct play to the 192 preset and describe copying MP3 within its ceiling.
- [x] `hifimule-daemon/src/transcoding.rs` — migrate recognized unchanged legacy 192 payloads before defaults merging. Test seeding, both legacy variants, idempotence and custom-profile preservation.
- [x] `hifimule-daemon/src/providers/audiobookshelf.rs`, `hifimule-daemon/src/providers/mod.rs` — retain positive, bounded file `bitRate` in bps and `channels` in private sync properties. Reuse identity-verified podcast details and book details already fetched for tags. Keep absent values optional; update constructors and fixtures without public catalog changes.
- [x] `hifimule-daemon/src/sync.rs`, `hifimule-daemon/src/rpc.rs` — share format/ceiling admission between preview and transfer. Prefer current resolved properties over stale planned evidence; preserve overrides and existing conversion/error policies.
- [x] Tests in those modules and `hifimule-daemon/src/rpc/mp3_direct_copy_tests.rs` — write failing regressions first, covering the matrix with the actual embedded preset. Exercise an Audiobookshelf provider mock through podcast preview and transfer; assert no encoding request, correct MP3 path/manifest and unchanged audio. Retain incompatible-media and music coverage.

**Acceptance Criteria:**
- Given an existing installation with the unchanged shipped 192 preset, when it loads profiles and syncs the reported podcast, then preview admits it and transfer publishes the original MP3 audio successfully.
- Given current verified media properties, when preview and execution evaluate direct-copy eligibility, then both enforce the same codec and numerical ceilings.
- Given customer-edited profiles or an incompatible source, when this fix is installed, then profile edits survive and incompatible originals remain unpublished.

## Spec Change Log

## Design Notes

Empty direct-play lists intentionally force conversion: repair this preset without weakening arbitrary profiles. Reuse shared ceiling/unknown-value helpers. Audiobookshelf v2.37.1 exposes `bitRate` and `channels` in its [AudioFile implementation](https://github.com/advplyr/audiobookshelf/blob/v2.37.1/server/objects/files/AudioFile.js); do not invent sample-rate/depth fields.

## Verification

- Test-first evidence: `rtk node scripts/build-daemon.mjs test -p hifimule-daemon mp3_direct_copy -- --nocapture` initially ran 3 regressions: 1 passed, 2 failed on the empty embedded direct-play list and the reported 128000 bps podcast block, before production edits.
- Final targeted run: `rtk node scripts/build-daemon.mjs test -p hifimule-daemon mp3_direct_copy` — 4 passed, 0 failed. Log: `target/mp3-direct-copy-targeted.log`.
- Final full run: `rtk node scripts/build-daemon.mjs test -p hifimule-daemon` — 1657 passed, 2 failed, 12 ignored (18.62 seconds). Both failures repeat in unchanged Windows path-policy tests: `sync::tests::music_destination_policy_preserves_other_media_and_inspection_errors` and `sync::tests::selected_music_collisions_keep_one_owner_across_retry_force_and_missing_files`. The expanded shared direct-media compatibility test passed. Log: `target/mp3-direct-copy-tests-final.log`.
- The direct Node command uses the same controlled audio runtime wrapper as `npm run build:daemon`; npm is unavailable in this host's PATH. Wrapper child processes required approved sandbox escalation.
- The real Audiobookshelf provider mock covers bps boundary/over-ceiling/unknown/malformed evidence and bitrate overrides in preview; transfer copies byte-identical original MP3, records current source bitrate despite stale planned evidence, and refuses changed over-ceiling media or a lower override before publication. Book/podcast DTO tests cover missing/null/zero/negative/out-of-range/fractional/malformed properties. The shared helper covers channels, sample rate, bit depth and AAC rejection.
- `rtk git diff --check` — no whitespace errors.
- Baseline confirmation: an unchanged archive of `cadc7fe40a29a05a02bc7d1111447bbeccecc0d3` ran 34 music tests: 32 passed and the same two Windows path tests failed. Log: `target/mp3-direct-copy-baseline.log`. Both are recorded in `deferred-work.md`.
- Independent blind, edge-case and acceptance reviews completed. The migration's later-edit ambiguity was patched by matching the old shipped description as well as its payload. A new regression failed before that patch; acceptance re-review found no actionable deviation. Speculative coverage/style observations were rejected as non-defects or already covered by the shared helpers and existing provider tests.
- Live-device validation unavailable.
- Final reviewed unit run: `rtk node scripts/build-daemon.mjs test -p hifimule-daemon --bin hifimule-daemon` — 1658 passed, the same 2 baseline failures, 12 ignored. All five new regressions passed, including preservation of a later direct-play edit. Log: `target/mp3-direct-copy-reviewed-tests.log`.
- Final Audiobookshelf contract run: `rtk node scripts/build-daemon.mjs test -p hifimule-daemon --test audiobookshelf_contract` — 5 passed, 0 failed. Log: `target/mp3-direct-copy-contract.log`.
- Local commit attempted but Git could not sign with configured key `6F368F9DB9D4B8AE` (`No secret key`). Changes remain staged; signing configuration was preserved. No push performed.

## Suggested Review Order

**Direct-copy eligibility**

- Preview and transfer reuse the existing codec and numeric ceiling rules.
  [sync.rs:1861](../../hifimule-daemon/src/sync.rs#L1861)
- Transfer records current verified properties instead of stale planning data.
  [sync.rs:3579](../../hifimule-daemon/src/sync.rs#L3579)

**Provider evidence**

- Preserve bounded optional bitrate and channels without changing public catalog metadata.
  [audiobookshelf.rs:865](../../hifimule-daemon/src/providers/audiobookshelf.rs#L865)
- Obtain sync evidence from the same identity-verified podcast file.
  [audiobookshelf.rs:1112](../../hifimule-daemon/src/providers/audiobookshelf.rs#L1112)

**Preset upgrade and regressions**

- Upgrade recognized legacy defaults while preserving later direct-play edits.
  [transcoding.rs:111](../../hifimule-daemon/src/transcoding.rs#L111)
- Permit MP3 direct copies within the shipped 192 kbps preset.
  [device-profiles.json:39](../../hifimule-daemon/assets/device-profiles.json#L39)
- Exercise actual provider admission, unchanged transfer, stale properties and bitrate overrides.
  [mp3_direct_copy_tests.rs:3](../../hifimule-daemon/src/rpc/mp3_direct_copy_tests.rs#L3)
- Ensure a user can disable direct play after the upgrade.
  [transcoding.rs:205](../../hifimule-daemon/src/transcoding.rs#L205)
