---
title: Podcast device paths and filenames
type: bugfix
created: 2026-09-25
status: done
baseline_commit: ab7e062
context: []
---

<frozen-after-approval reason="user-owned intent">

## Intent

**Problem:** Podcast episodes currently sync beneath an `Unknown Artist` folder and use a `00` track prefix, making device browsing and chronological sorting poor.

**Approach:** Put each episode directly in its podcast show folder and prefix its filename with its publication date in `YYYYMMDD` form.

## Boundaries & Constraints

**Always:** Apply the same naming to manually selected and auto-filled podcast episodes. Preserve path sanitization and length limits. Leave music and audiobook naming unchanged.

**Ask First:** No pending design decisions.

**Never:** Use podcast title, date, or ordinal as episode identity. Change auto-fill selection policy in this fix.
Do not add migration for files created by unreleased code.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Published episode | Show `Talks`, title `News`, publication `2026-09-24T23:30:00-04:00` | `Podcasts/Talks/20260924 - News.<ext>` | N/A |
| Missing or malformed date | Podcast episode with no valid publication date | File still gets a deterministic name under the show folder | Do not invent a publication date |
| Other media | Music or audiobook item | Existing directory and filename convention | N/A |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/rpc.rs` -- builds desired podcast episodes and auto-fill items from provider metadata.
- `hifimule-daemon/src/sync.rs` -- builds final device paths and calculates sync delta.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/rpc.rs` -- propagate the episode publication date into sync metadata for both selection routes.
- [x] `hifimule-daemon/src/sync.rs` -- use podcast-specific folder and date-prefixed filename, preserving path limits.
- [x] `hifimule-daemon/src/sync.rs` and `hifimule-daemon/src/rpc.rs` -- test dated, missing-date, and other-media cases.

**Acceptance Criteria:**
- Given a published podcast episode, when synced manually or by auto-fill, then its device path contains only the show folder and a `YYYYMMDD`-prefixed filename.

## Spec Change Log

## Design Notes

Publication dates come from the podcast provider's RFC 3339 timestamp. Preserve the calendar date in that timestamp, including its offset, since it is the publication date shown by the source.

## Verification

**Commands:**
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon podcast` -- podcast cases pass with the bundled FFmpeg runtime.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon music_path_keeps_artist_album_and_track_number` -- music path remains unchanged.

## Suggested Review Order

**Podcast filename data**

- Carry each episode's publication date into manual and auto-fill selections.
  [rpc.rs:4511](../../hifimule-daemon/src/rpc.rs#L4511)

- Preserve the source's calendar day when formatting the date.
  [rpc.rs:4534](../../hifimule-daemon/src/rpc.rs#L4534)

**Device path**

- Put podcasts directly under their show and prefix filenames with the date.
  [sync.rs:1298](../../hifimule-daemon/src/sync.rs#L1298)

**Verification**

- Check dated, undated, and music paths.
  [sync.rs:6937](../../hifimule-daemon/src/sync.rs#L6937)
