---
title: 'Seek within Audiobookshelf audiobooks and podcasts'
type: 'feature'
created: '2026-09-25'
status: 'done'
baseline_commit: '6f33d9461b9857753726e4d9c607d8431d5fcfc8'
context:
  - '_bmad-output/implementation-artifacts/15-7-seek-within-a-track-and-see-the-actual-playback-position.md'
  - 'docs/audiobookshelf-integration-contract.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Audiobooks and podcast episodes play through HifiMule, but their timeline remains unavailable for seeking even when the direct Audiobookshelf stream supports byte ranges. This makes long-form listening difficult to navigate.

**Approach:** Qualify supported direct audiobook parts and podcast episodes for the existing playback seek pipeline. Enable the existing timeline and native seek controls only after transport, decoder format, and track duration have been verified at runtime.

## Boundaries & Constraints

**Always:** Preserve separate Books and Podcasts library scope and stable source identity. Seek in the current physical audio part or episode; keep book-wide progress and chapter display consistent with the committed position. Retain private authentication, bounded refresh, verified byte ranges, playback-session cleanup, generation fencing, pause/stop behavior, and actual landing validation. A failed or superseded seek must keep the last committed position and expose an actionable state.

**Ask First:** Ask if implementation needs to add an unverified codec, HLS/transcode seeking, or a new public playback contract beyond the existing seek capability and state fields.

**Never:** Claim all Audiobookshelf streams are seekable from a filename or server declaration alone. Turn chapter markers into independent tracks, change music-provider seek behavior, expose authenticated URLs or session IDs to UI/RPC/logs, or enable download/sync seeking.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Qualified direct media | Book part or episode with positive duration, verified range response, and supported opened format | Timeline/native seek becomes available; forward and backward requests land within the existing tolerance and resume in the same part or episode | Failure leaves committed position unchanged |
| Multi-part book | Seek while a later physical part is playing | Position changes within that part and whole-book progress derives from its offset plus the new committed local position | No accidental part switch or duplicate progress report |
| Unqualified media | Unknown/contradictory duration, changed range representation, unsupported codec/container, or HLS/transcode | Ordinary playback continues with seek unavailable and a reason | Explicit seek request is rejected without playback mutation |
| Race | Seek overlaps pause, stop, replacement, output loss, or another seek | Existing owner fencing chooses the current operation; no stale audio or position commits | Superseded work cleans up and cannot alter the new session |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/providers/audiobookshelf.rs` -- role-scoped direct book and episode representations currently set `seek_mechanism: None`.
- `hifimule-daemon/src/providers/mod.rs` -- provider-qualified seek mechanism types.
- `hifimule-daemon/src/playback/decoder.rs` -- FFmpeg format, duration, landing, and pre-roll validation.
- `hifimule-daemon/src/playback/audio.rs` -- candidate admission and runtime capability publication.
- `hifimule-daemon/src/playback/session.rs` -- owner seek fencing, committed position, and continuity.
- `hifimule-ui/src/components/PlaybackControls.ts` -- existing seek timeline shared by all media roles.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/providers/mod.rs`, `hifimule-daemon/src/providers/audiobookshelf.rs` -- define role-safe seek candidates only for verified direct original formats and range-capable requests.
- [x] `hifimule-daemon/src/playback/decoder.rs` -- match Audiobookshelf candidates against opened container/codec and reconcile duration; retain measured landing and compressed pre-roll behavior.
- [x] `hifimule-daemon/src/playback/audio.rs`, `hifimule-daemon/src/playback/session.rs` -- publish qualified capability and preserve book/episode identity, position, lifecycle, and cleanup through seeks. Existing shared owner code required no change after the provider qualified the candidate.
- [x] Co-located provider, decoder, playback, RPC, and UI tests -- cover the matrix, including book progress, episode scope, rejected candidates, repeated/backward seek, and races. Added candidate assertions and decoder matrix rows; existing owner, RPC, and UI regressions cover shared paths.
- [x] `docs/audiobookshelf-integration-contract.md` -- record the supported seek formats and evidence limits without promoting offline tests to installed evidence.

**Acceptance Criteria:**
- Given a qualified audiobook part or podcast episode, when the listener seeks in the existing timeline or native controls, then playback resumes at the validated position in that same item and the displayed position updates.
- Given a multi-part audiobook, when a seek commits, then local and whole-book progress remain consistent without changing parts.
- Given an unqualified or changed stream, when playback starts or seek is requested, then ordinary listening remains available and seeking is unavailable or rejected without a false commit.
- Given overlapping seek and lifecycle commands, when stale work completes, then it cannot alter the current session or leak its upstream playback session.

## Spec Change Log

- Review found that Audiobookshelf MP3 inherited a provider-only duration fallback. Removed that fallback and added absent/contradictory-duration tests. Keep the direct-format candidate and measured decoder landing checks.

## Design Notes

Use the existing seek mechanism as the qualification boundary: the provider offers a candidate after direct stream validation; FFmpeg confirms the opened media and duration before the owner exposes seeking. Audiobookshelf uses its own mechanism variants so a format match alone cannot imply Jellyfin or Navidrome transport guarantees.

The shared owner already fences seek against session, generation, and occurrence changes; book progress uses the committed local position plus the part offset and pauses reporting while seek is pending. The shared UI reads only the published seek capability, so no new UI contract or component change was needed.

## Verification

**Commands:**
- `rtk proxy node scripts/build-daemon.mjs test -p hifimule-daemon playback -- --test-threads=1` -- focused playback tests pass.
- `rtk proxy node scripts/build-daemon.mjs test -p hifimule-daemon providers -- --test-threads=1` -- provider tests pass.
- `rtk proxy node --test scripts/tests/playback-ui.test.mjs` -- shared timeline regression passes.
- `rtk npm --prefix hifimule-ui run build` -- UI compiles.
- `rtk git -c safe.directory=C:/Workspaces/HifiMule diff --check` -- no whitespace errors.

## Suggested Review Order

**Seek admission**

- Book parts offer a candidate only after direct playback and authenticated range validation.
  [audiobookshelf.rs:2775](../../hifimule-daemon/src/providers/audiobookshelf.rs#L2775)

- Podcast episodes use the same qualification while retaining their episode identity.
  [audiobookshelf.rs:740](../../hifimule-daemon/src/providers/audiobookshelf.rs#L740)

**Decoder boundary**

- FFmpeg must confirm Audiobookshelf's actual AAC or MP3 format.
  [decoder.rs:554](../../hifimule-daemon/src/playback/decoder.rs#L554)

- Missing or contradictory Audiobookshelf duration keeps seeking unavailable.
  [decoder.rs:566](../../hifimule-daemon/src/playback/decoder.rs#L566)

**Evidence and supporting checks**

- Independent audio fixtures verify forward, backward, and repeated landing.
  [decoder.rs:965](../../hifimule-daemon/src/playback/decoder.rs#L965)

- The contract distinguishes offline checks from installed playback evidence.
  [audiobookshelf-integration-contract.md:88](../../docs/audiobookshelf-integration-contract.md#L88)

- New mechanism variants keep provider transport guarantees separate.
  [mod.rs:142](../../hifimule-daemon/src/providers/mod.rs#L142)
