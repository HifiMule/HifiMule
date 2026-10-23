---
title: Consistent disc and track numbers in music filenames
type: feature
created: 2026-10-23
status: done
baseline_commit: 11a801f3b3aa639c235a3bd9b6afaf9c05420de6
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Music filenames currently contain only the track number, so tracks from different discs interleave when a multi-disc album is sorted by filename. Single-disc and missing-disc metadata also need a consistent convention.

**Approach:** Generate every music filename as `{disc}-{track:02} {title}.{extension}`, using the server's disc number and defaulting to 1 when it is missing or zero. Keep disc numbers unpadded, as explicitly requested; alphabetical disc ordering beyond nine discs is an accepted limitation.

## Boundaries & Constraints

**Always:** Apply to manual selection, auto-fill, automatic sync, recovery, and force sync. Preserve server metadata, missing-track fallback `00`, minimum two-digit track padding, extensions, sanitization, and path limits. Support new and legacy filenames in Rockbox playback logs.

**Ask First:** Adding a bulk migration of existing device filenames, or changing naming rules for other media types.

**Never:** Pad/truncate disc numbers, add disc folders/preferences, change other media naming or track identity. Retain existing manifest paths without naming-only re-downloads; new writes and force sync use the new convention.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| First disc | Disc 1, track 1, title Intro | `1-01 Intro.flac` | Existing write errors apply |
| Later disc | Disc 2, track 3, title Finale | `2-03 Finale.flac` | Existing write errors apply |
| Missing/invalid disc | Disc absent or zero, track 2 | `1-02 Title.flac` | Default to 1 |
| Missing track | Disc 2, track absent | `2-00 Title.flac` | Existing track fallback |
| Large numbers | Disc 10, track 123 | `10-123 Title.flac` | Accept alphabetical disc misordering |
| Repeated track/title | Disc 1 and 2 both contain track 1 titled Intro | Two distinct output paths | Preserve both tracks |
| Playback logs | `2-03 Finale.flac` or legacy `03 - Finale.flac` | Extract title `Finale` | Preserve existing malformed-entry handling |
| Long/unsafe title | Invalid characters or excessive path length | Sanitized, bounded path retaining numbering and extension | Existing path-construction failure when necessary |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/sync.rs` -- shared path builder, disc propagation, path/provider-write tests.
- `hifimule-daemon/src/providers/jellyfin.rs` -- maps `ParentIndexNumber`; existing test asserts disc 2.
- `hifimule-daemon/src/providers/subsonic.rs` -- maps `discNumber`; fixture has disc 2 without an assertion.
- `hifimule-daemon/src/rpc.rs` -- manual/auto-fill adapters and force-sync source refresh retain disc metadata.
- `hifimule-daemon/src/rpc/sync_plan_tests.rs` -- metadata propagation tests.
- `hifimule-daemon/src/auto_fill/mod.rs` and `hifimule-daemon/src/auto_fill/pipeline.rs` -- both routes retain disc numbers.
- `hifimule-daemon/src/scrobbler.rs` -- prefix parser would incorrectly extract `01 Title` from `2-01 Title`.
- `hifimule-daemon/src/device_music_scan.rs` -- import and pre-sync reconciliation match audio tags, including disc/track, independently of filenames.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/sync.rs` -- test the matrix, change Music-only filename construction, update affected expectations; cover distinct paths for repeated titles across discs and correct playlist references.
- [x] `hifimule-daemon/src/providers/subsonic.rs` and `hifimule-daemon/src/rpc/sync_plan_tests.rs` -- verify provider conversion, manual/auto-fill propagation, serialization, and force-sync source refresh using existing fixtures.
- [x] `hifimule-daemon/src/scrobbler.rs` -- recognize the complete anchored disc-track prefix; test new/legacy prefixes and preserve numeric-leading and unnumbered titles.
- [x] `hifimule-daemon/src/device_music_scan.rs` -- cover import/reconciliation under legacy and disc-prefixed filenames, retain the adopted path and bytes, and reject a different disc with matching title/track.

**Acceptance Criteria:**
- Given Jellyfin or Navidrome album metadata, when music is synced manually or through either auto-fill route, then written filenames retain the supplied disc and track numbers using the new convention.
- Given source-backed force sync or missing-file recovery, when replacement music is written, then it uses the same convention and any playlist references resolve to the written paths.
- Given an existing device manifest, when ordinary incremental sync retains music, then its recorded paths remain usable without a naming-only re-download.
- Given audiobook and podcast selections, when paths are constructed, then their existing naming behavior remains unchanged.
- Given a Rockbox playback log referencing new or legacy music filenames, when it is parsed, then the extracted title excludes the complete numbering prefix.
- Given compatible music with old or new filenames, when imported or reconciled, then tag-based matching preserves the actual path without renaming or rewriting it and does not confuse distinct discs.

## Spec Change Log

- 2026-10-23: User explicitly requested checking song reconciliation during implementation. Added regression coverage for import and pre-sync reconciliation; matching already uses embedded tags, so filename-derived identity remains out of scope. Keep existing path-preservation and disc-disambiguation behavior.

## Design Notes

Metadata already reaches the shared builder; no API calls, schema changes, or dependencies are needed. Apply fallback at filename construction. Manifest-only force adds without source disc metadata use 1. Preserve provider/DeviceIO boundaries and manifest ownership.

## Verification

**Commands:**
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon construct_desired_file_path` -- filename, fallback, extension, and path-limit cases pass.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon disc` -- disc conversion and propagation cases pass.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon scrobbler` -- new and legacy title parsing passes.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon device_music_scan` -- import/reconciliation preserves old/new paths and disc separation.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon` -- daemon suite passes, including sync, playlists, and other media. Report audio-runtime blockers accurately.

**Execution evidence (2026-10-23):**
- Before production edits, the filename regression failed with `01 - Intro.flac` versus `1-01 Intro.flac`; the playback parser regression failed with `01 Intro` versus `Intro`.
- After implementation, the `disc` filter passed 45 tests, including filename defaults, metadata mapping, Rockbox parsing, distinct provider writes and playlist references.
- Final `rtk node scripts/build-daemon.mjs test -p hifimule-daemon -- --quiet`: **1672 passed, 0 failed, 12 ignored**; integration contract tests: **5 passed**. Includes tagged-audio reconciliation for legacy/new filenames and disc disambiguation, read-only adoption, retained files, source-backed force refresh, and other media.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon construct_desired_file_path -- --quiet`: **7 passed**. `rtk git diff --check` passed.
- Bundled audio-runtime capabilities verified; subprocess sandbox restrictions were resolved using approved elevated test execution. No outstanding verification blocker.
- After review patches: `rtk node scripts/build-daemon.mjs test -p hifimule-daemon scrobbler -- --quiet` **14 passed**; `rtk node scripts/build-daemon.mjs test -p hifimule-daemon multi_disc_same_title -- --quiet` **1 passed**. Final full daemon rerun: **1672 passed, 0 failed, 12 ignored** (21.82s); integration contracts **5 passed**.

## Review Notes

- P2 (deduplicated parser finding): added a failing regression for compact legacy `03-12 Monkeys` before patching. Recognize the new format only when the disc is positive and unpadded, preserving the legacy title `12 Monkeys` while retaining support for disc 10 and larger.
- P3: strengthened the multi-disc provider integration test to require each source ID's exact disc path in the manifest and literal playlist references in source order.

## Suggested Review Order

**Music filenames**

- Always include the source disc, defaulting to one when metadata is missing.
  [sync.rs:1356](../../hifimule-daemon/src/sync.rs#L1356)

**Playback-log compatibility**

- Recognize unpadded disc prefixes while preserving legacy prefixes and numeric titles.
  [scrobbler.rs:173](../../hifimule-daemon/src/scrobbler.rs#L173)

**Regression coverage**

- Verify separate files, correct source ownership, and playlist order across discs.
  [sync.rs:12875](../../hifimule-daemon/src/sync.rs#L12875)

- Reuse tagged audio under either naming format without confusing discs or rewriting files.
  [device_music_scan.rs:1052](../../hifimule-daemon/src/device_music_scan.rs#L1052)

- Preserve provider disc numbers through adapters and serialized sync plans.
  [sync_plan_tests.rs:1104](../../hifimule-daemon/src/rpc/sync_plan_tests.rs#L1104)
