---
title: 'Fix local encoding duration validation for estimated MP3 inputs'
type: 'bugfix'
created: '2026-10-22'
status: 'done'
baseline_commit: '6781d78c92206097d99add7e89510611a27d4046'
context: ['_bmad-output/implementation-artifacts/investigations/local-mp3-duration-dmv-investigation.md']
---

<frozen-after-approval reason="human-owned intent — implement the fix authorized after the DMV diagnosis">

## Intent

**Problem:** Local encoding rejects both supplied DMV MP3s because trailing APEv2 metadata inflates a bitrate-based duration estimate by over three seconds. A correct re-encode preserves all decoded audio but fails the existing two-second input/output comparison.

**Approach:** Distinguish estimated input duration from reliable duration metadata. For bitrate-derived or unavailable durations, derive expected duration from the samples actually decoded before resampling, then compare the completed output against that expectation. Retain existing reliable-header checks, cleanup, recovery limits, and tolerance.

## Boundaries & Constraints

**Always:** Fix the shared native file encoder used by all local sync targets. Count decoded samples before conversion so loss in resampling, FIFO handling, encoding or output finalization is still detectable. Keep the current two-second tolerance for codec delay/padding. Use the existing FFmpeg bindings and native test infrastructure. Keep errors actionable by including expected/output duration values. Retain per-job memory bounds and streaming operation. Reopen the completed output before acceptance, and remove failed output through the existing wrapper. Preserve source audio files and supported tags/artwork.

**Ask First:** Expanding this task into server identity changes, new dependencies, changing the device profile contract, or release/publishing work.

**Never:** Increase the tolerance to conceal metadata inflation, disable all duration checks, strip metadata from user originals, add a second full decode pass to production encoding, change playback behavior, or route local failures to server encoding.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
| --- | --- | --- | --- |
| Tagged MP3 estimate | MP3 lacks a trustworthy frame-count header and ends with a large APEv2 tag | Expected duration follows decoded samples rather than file-size estimate; valid output accepted | Genuine conversion failure still returned |
| Plain estimated input | Same MP3 audio without trailing APE metadata | Duration and decoded output remain equivalent to tagged input | Normal validation applies |
| Resampling | Estimated input converted at a lower output sample rate | Sample duration uses the source rate; valid shorter-rate output accepted | Rate/profile constraints remain enforced |
| Reliable metadata | WAV or MP3 with reliable duration header | Existing expected duration protection remains active | Material duration mismatch fails |
| Missing duration | Input has no positive probed duration but decodes audio | Decoded sample count supplies an expectation | Invalid/no decoded audio remains an error |
| Truncated output | Output duration falls short of known expected audio length by more than two seconds | Validation fails | Existing wrapper removes failed conversion output |
| Unsupported/corrupt data | Existing recovery policy rejects corruption or output is invalid | Existing failures remain failures | No change to recovery budgets or output admission |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/transcoding/local.rs` — `encode_inner` probes input, receives decoded frames, resamples and encodes, then validates reopened output; native tests and fixture generation live in the same module.
- `hifimule-daemon/src/sync.rs:2949` — caller passes a staged input and profile to the shared encoder; no caller change required.
- `scripts/build-daemon.mjs` — mandatory controlled-runtime verification and environment setup for daemon tests.
- `target/audio-runtime/ffmpeg-9.0.2-aarch64-apple-darwin` — existing local controlled FFmpeg runtime; no runtime modification required.
- `_bmad-output/implementation-artifacts/investigations/local-mp3-duration-dmv-evidence.json` — measured original duration, decoded duration, APE sizes, and hashes from diagnosis.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/transcoding/local.rs` — read FFmpeg’s duration estimation method and count successfully decoded samples before conversion; select decoded duration for bitrate estimates or missing input duration, retaining reliable input duration otherwise.
- [x] `hifimule-daemon/src/transcoding/local.rs` — retain two-second output comparison and improve mismatch details; keep existing error prefix and cleanup semantics.
- [x] `hifimule-daemon/src/transcoding/local.rs` — generate a synthetic MP3 without a frame-count duration header, add a valid large APEv2 tag, and demonstrate that the old estimate differs by more than two seconds. Verify native conversions preserve the expected decoded audio and source bytes, including resampling.
- [x] `hifimule-daemon/src/transcoding/local.rs` — cover true short output and reliable-header source truncation so the fix cannot silently turn duration validation off.
- [x] `_bmad-output/implementation-artifacts/spec-fix-local-encoding-estimated-duration.md` — record test commands/results and review disposition after implementation.

**Acceptance Criteria:**
- Given an inflated bitrate-based duration estimate, when the actual native encoder converts the full stream, then completed output is admitted based on decoded audio length.
- Given the existing supported-codec and recovery tests, when the shared duration validation changes, then their supported output and failure/cleanup behavior still pass.
- Given the two user-supplied originals, when local native conversion is checked if accessible, then the false duration failure is resolved without modifying either original.

## Spec Change Log

## Design Notes

The encoder already counts samples sent to the output codec, but that counter occurs after conversion and therefore cannot independently detect samples lost during resampling. Count original decoded sample frames in the receive loop, normalize by the original sample rate, and reuse the completed-output probe. FFmpeg exposes its duration estimation method through the existing format-context bindings. For a trustworthy source duration, preserve the pre-existing comparison to catch early EOF despite retained duration headers. The synthetic fixture should contain actual native-generated MPEG packets without the muxer’s Xing/Info frame and a valid APEv2 trailer; do not commit user audio.

## Verification

- `rtk npm run build:daemon -- test -p hifimule-daemon transcoding::local::tests -- --nocapture` — native regressions and existing codec/recovery tests pass.
- `rtk npm run build:daemon -- test -p hifimule-daemon` — daemon regression suite passes, except explicitly identified unrelated environment failures if any.
- `rtk cargo fmt --all -- --check` and `rtk git diff --check` — touched Rust code formatted; no whitespace defects.
- Native reproduction against the supplied originals is supplemental evidence and must leave their recorded SHA-256 hashes unchanged.

## Implementation evidence

- Native encoder selects decoded duration only for bitrate-estimated or unavailable source duration; reliable headers and the two-second tolerance remain in force.
- All 20 local encoder tests passed, including large-APE/no-Xing, 24 kHz resampling, reliable-header early EOF cleanup, and real short-output rejection.
- Temporary native 192 kbps MP3 harness (removed after running): 05 completed at 298.840816 seconds, 7,257,564 bytes; 09 completed at 296.855510 seconds, 7,201,325 bytes. Both reported zero invalid decode events and zero rejected bytes. Both original SHA-256 hashes remain unchanged and match the investigation.
- Local rustfmt (edition 2024) and `rtk git diff --check` passed. Broader daemon verification and independent review follow.

## Final verification and review

- `rtk npm run build:daemon -- test -p hifimule-daemon` passed outside the sandbox: 1,661 unit tests and 5 integration tests passed, 14 ignored, zero failures. Initial sandbox run prevented Mockito localhost listeners (`Operation not permitted`); permitting local sockets resolved those environment failures.
- All three independent review roles (blind adversarial, edge-case, acceptance audit) returned no actionable findings.
- `rtk proxy rustfmt --edition 2024 --check hifimule-daemon/src/transcoding/local.rs` and `rtk git diff --check` passed.
- `rtk cargo fmt --all -- --check` reports pre-existing formatting differences in unchanged repository files. No unrelated formatting changes were applied.

## Suggested Review Order

**Expected audio duration**

- Choose decoded duration for estimates while retaining reliable-header protection.
  [local.rs:1168](../../hifimule-daemon/src/transcoding/local.rs#L1168)

- Count source samples before resampling to detect conversion loss.
  [local.rs:1012](../../hifimule-daemon/src/transcoding/local.rs#L1012)

- Identify FFmpeg bitrate estimates without parsing or modifying metadata.
  [local.rs:690](../../hifimule-daemon/src/transcoding/local.rs#L690)

- Retain tolerance and report expected and actual output durations.
  [local.rs:1180](../../hifimule-daemon/src/transcoding/local.rs#L1180)

**Regression coverage**

- Preserve complete audio despite large APE tags, including resampling.
  [local.rs:1231](../../hifimule-daemon/src/transcoding/local.rs#L1231)

- Reject early EOF with retained reliable duration headers and remove failed output.
  [local.rs:1309](../../hifimule-daemon/src/transcoding/local.rs#L1309)

- Reject a genuinely short output while preserving the existing tolerance.
  [local.rs:1349](../../hifimule-daemon/src/transcoding/local.rs#L1349)

