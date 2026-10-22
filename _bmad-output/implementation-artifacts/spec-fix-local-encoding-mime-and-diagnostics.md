---
title: 'Fix local original MIME validation and encoding diagnostics'
type: 'bugfix'
created: '2026-10-22'
status: 'done'
baseline_commit: 'f28a15aed717a7738814d62ec3eb28dab5779daa'
context: ['_bmad-output/implementation-artifacts/investigations/local-mp3-duration-dmv-investigation.md']
---

<frozen-after-approval reason="human-owned intent — user selected the proposed MIME validation and encoding diagnostics fix">

## Intent

**Problem:** Local sync rejects valid AIFF/WMA originals because their MIME types lack device-format mappings. Encoding failures omit track identity, and duration failures omit the metadata method and independently decoded duration, preventing diagnosis of parallel jobs.

**Approach:** Separate local-original MIME admission from strict output compatibility. Let native probing validate admitted audio bytes. Add identity to encoding errors and source/decoded/output durations plus estimation method to mismatches, preserving validation protection.

## Boundaries & Constraints

**Always:** Admit normalized nonempty audio media types including AIFF/WMA aliases, mixed case and parameters. Preserve missing MIME, generic binary and recognized application/ogg admission. Reject HTML, JSON, XML and other unrecognized non-audio types before decoding. Require a decodable audio stream and validated profile-compatible output before device writing. Preserve manual cancellation versus Auto-Fill skip/continue policy, temporary-file cleanup, trusted-header truncation protection, two-second tolerance and decode recovery limits. Include track name and ID in every native encoder failure. Use existing dependencies and the controlled runtime; preserve original files.

**Ask First:** Runtime codec expansion, changing profile meaning, retaining downloaded music after failure, or release/publishing.

**Never:** Add AC3 codec support, loosen direct-play/server-output compatibility, admit definite non-audio based on catalog metadata, fall back to server encoding, raise recovery budgets, disable duration validation or claim the unidentified duration failures are solved.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Behavior | Failure Handling |
| --- | --- | --- | --- |
| Supported original | AIFF/WMA under audio MIME aliases | Native conversion commits verified MP3 | Real decode failure retains policy |
| Header variation | Case or semicolon parameters | Normalize media type for local admission | Probe actual bytes |
| Generic original | Missing MIME or either existing generic binary type | Native probe decides validity | Non-audio cannot pass |
| OGG | application/ogg | Existing admission preserved | Output validation retained |
| Non-audio | HTML/JSON/XML despite audio catalog entry | Reject before decoding/writing | Include rejected MIME; preserve both operation modes |
| Unsupported audio | audio/ac3 or unknown subtype | Permit probing, never unchecked output | Native failure includes track identity |
| Encoder failure | Invalid file or native error | Surface name and ID in manual error/Auto-Fill warning | Preserve cleanup and cancellation/continuation |
| Duration mismatch | Difference exceeds two seconds | Existing failure remains | Show expected, source, decoded, output seconds and method |
| Valid conversion | Output duration within tolerance | Existing success/recovery behavior | No new warning |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/sync.rs:2044`, `:3827` — generic MIME normalization and faulty local-original gate; keep shared format mapper strict.
- `hifimule-daemon/src/sync.rs:2927` — encode-and-enqueue has track identity but surfaces only bare error text.
- `hifimule-daemon/src/sync.rs:7832`, `:8050` — native sync success/failure integration fixtures and policies.
- `hifimule-daemon/src/transcoding/local.rs:689`, `:991`, `:1168` — source duration/method, independent decoded samples, expectation selection and validation.
- `hifimule-daemon/src/playback/decoder.rs:1168` — existing generated AIFF/WMA fixture approach.
- `scripts/build-daemon.mjs` — controlled runtime and localhost mock-server test environment.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/sync.rs` — add local-only audio MIME admission and report rejected MIME; preserve shared compatibility logic.
- [x] `hifimule-daemon/src/sync.rs` — include item name/ID in native errors for both sync modes without changing operation policy.
- [x] `hifimule-daemon/src/transcoding/local.rs` — enrich mismatches with probed, independently decoded and output durations, expectation and FFmpeg estimation method; retain existing validation logic.
- [x] Test modules in these files — verify real AIFF/WMA-to-MP3 sync; MIME aliases/case/parameters, missing/generic/OGG headers, non-audio rejection, fake audio bodies, failure identity and duration provenance.
- [x] This spec and investigation — record verification and final scope.

**Acceptance Criteria:**
- Given supported AIFF/WMA originals, when local sync downloads their audio MIME responses, then verified MP3s are committed through the original-download route.
- Given non-audio bytes or unsupported codecs, when sync attempts preparation, then failed output never enters the manifest and existing operation policy remains.
- Given parallel Auto-Fill encoding errors, when warnings surface, then each identifies its track name and ID.
- Given a duration failure, when reported, then its diagnostic distinguishes probed metadata, decoded length, output length and expectation origin without weakening validation.

## Spec Change Log

## Design Notes

Audio MIME authorizes probing, not successful conversion. Keep admission local so direct-play/server-output format checks remain strict. Reuse the independent pre-resampling decoded count; avoid a second production decode or retained source files. Generate fixtures rather than commit user music. AC3 remains unsupported and must not be silently claimed as supported. Existing investigation notes are this chat’s own uncommitted work and must be preserved.

## Verification

- `rtk npm run build:daemon -- test -p hifimule-daemon local_encoding -- --nocapture` plus native duration tests — focused regressions pass.
- `rtk npm run build:daemon -- test -p hifimule-daemon` — full suite passes with localhost sockets permitted.
- Check touched Rust formatting without rewriting unrelated existing formatting; `rtk git diff --check` passes.
- Independent blind, edge-case and acceptance reviews have no unresolved actionable findings.

## Implementation Evidence

- Local-only admission handles normalized audio MIME types and preserved missing/generic/application-ogg cases; shared output compatibility is unchanged.
- Native failures include track name/ID. Rejected MIME is shown. Duration mismatch errors show all duration values, the FFmpeg method and selected expectation.
- Focused HTTP/native suite: 10 passed, 1 ignored; native encoder suite: 21 passed. HTTP fixtures are tracked generated media. Cases exercise both operation modes and actual AIFF/WMA conversion, plus continuing to a good track after an Auto-Fill failure.
- Native truncation assertions allow actual codec delay in decoded length while checking diagnostic provenance and cleanup. No tolerance/recovery budgets changed.
- Changed local.rs formatting and whitespace checks pass. Broader tests and independent review follow.

## Final Verification and Review

- Controlled-runtime full daemon suite passed with localhost mock servers permitted: 1,665 unit tests and 5 integration tests passed, 14 ignored, zero failures. Unit suite completed in 64.55 seconds.
- Focused sync suite: 10 passed, 1 ignored. Native encoder suite: 21 passed. New HTTP matrices cover 11 successful MIME/body variants and 8 rejection/native-failure variants in both operation modes, including Auto-Fill continuation to a valid track.
- Blind adversarial review, edge-case review and acceptance audit each returned no actionable findings. No deferred or unresolved review findings.
- `rtk proxy rustfmt --edition 2024 --check hifimule-daemon/src/transcoding/local.rs` and `rtk git diff --check` pass. New sync code follows local formatting; unrelated existing whole-file formatting was not rewritten.
- AC3 runtime support, log retention, and the causes of the four unidentified duration mismatches are unchanged and were not claimed resolved.

## Suggested Review Order

**Local input admission**

- Permit probing of audio inputs independently of device compatibility.
  [sync.rs:2059](../../hifimule-daemon/src/sync.rs#L2059)

- Apply the policy only to originals; report rejected response MIME.
  [sync.rs:3852](../../hifimule-daemon/src/sync.rs#L3852)

**Failure diagnostics**

- Preserve track name and ID when native errors become Auto-Fill warnings.
  [sync.rs:3004](../../hifimule-daemon/src/sync.rs#L3004)

- Expose source, decoded and output durations while preserving expectation and tolerance.
  [local.rs:1179](../../hifimule-daemon/src/transcoding/local.rs#L1179)

**Regression coverage**

- Verify real AIFF/WMA originals reach validated MP3 output in both modes.
  [sync.rs:7985](../../hifimule-daemon/src/sync.rs#L7985)

- Reject non-audio and mislabeled bodies; preserve cleanup, identity and Auto-Fill continuation.
  [sync.rs:8093](../../hifimule-daemon/src/sync.rs#L8093)

- Distinguish trusted, bitrate and missing duration provenance in failure messages.
  [local.rs:1390](../../hifimule-daemon/src/transcoding/local.rs#L1390)

