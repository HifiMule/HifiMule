---
title: 'Support AC3 sources in playback, encoding, and mediacheck'
type: 'feature'
created: '2026-10-24'
status: 'done'
baseline_commit: 'ca32adafa791bcb58b11cbe3ccf1a12851331973'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** HifiMule cannot play or convert `.ac3` originals, and mediacheck ignores them. The supplied example is 96.096 seconds, 48 kHz, 512 kbps, six channels. Playback also rejects multichannel input.

**Approach:** Enable AC3 in the controlled FFmpeg runtime and playback selection. Downmix known layouts through the existing resampler; reuse shared source decoding for encoding and runtime-derived extensions for mediacheck.

## Boundaries & Constraints

**Always:** Preserve provider abstraction, authenticated streaming, bounded buffers, cancellation, validation and failed-sync cleanup. Keep originals unchanged. Support the supplied 5.1 sample. Match AC3 extensions case-insensitively. Use the controlled runtime wrapper.

**Ask First:** Adding AC3 as an output encoding profile or extending scope to E-AC3, Dolby passthrough, or native surround output.

**Never:** Add another decoder/dependency, require system FFmpeg, commit the recording, weaken validation, or certify AC3 seeking/gapless behavior. Retain mono/stereo output.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|---------------------------|----------------|
| Playback | Original mono/stereo or known-layout 5.1 AC3 | Finite PCM at selected mono/stereo output; center audio survives downmix | Existing playback failure/retry policy |
| Local conversion | Valid AC3 bytes, including extensionless staging and audio/ac3, audio/x-ac3, generic or missing MIME | Verified output in an existing selected format; original unchanged | Existing cancellation, capacity limits and staging cleanup |
| Library audit | Healthy `.ac3` and `.AC3` files | Checked and reported healthy rather than ignored | Normal report and exit status |
| Corrupt source | Invalid bytes under `.ac3` or audio/ac3 | Audit reports bad; sync rejects output and avoids manifest commit | Existing diagnostics and cleanup |
| Unknown layout | Multichannel frame without usable channel mapping | No guessed speaker placement | Explicit unsupported-layout error |

</frozen-after-approval>

## Code Map

All task paths below are repository-relative. Shared decoding lives in `hifimule-daemon/src/source_audio.rs`; mediacheck already uses its extensions case-insensitively. Playback has a separate representation gate and channel guard. Sync already admits audio/* MIME and probes extensionless staging. Platform receipts include runtime flags, automatically rejecting stale builds.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/audio-runtime.json` — enable AC3 demuxer, decoder and parser; retain outputs and runtime pin.
- [x] `hifimule-daemon/tests/fixtures/` — add `generated-ac3-mono.ac3`, `generated-ac3-stereo.ac3`, `generated-ac3-5point1.ac3`; document generation in `generated-audio.source.md`. Use a center-only 5.1 signal to detect dropped dialogue.
- [x] `hifimule-daemon/src/providers/mod.rs` — admit AC3 and test mixed-case original representation selection.
- [x] `hifimule-daemon/src/playback/decoder.rs` — allow known multichannel layouts through resampling, retaining rejection of unknown layouts. Test full/finite PCM, channel counts, center contribution, resume discard and streaming beyond the compressed window. Add an ignored external-sample diagnostic.
- [x] `hifimule-daemon/src/source_audio.rs` — expose `.ac3`, adding a runtime-gated alias if demuxer metadata omits it. Replace the negative extension assertion and test decoding.
- [x] `hifimule-daemon/src/transcoding/local.rs`, `hifimule-daemon/src/sync.rs` — test extensionless conversion, stereo downmix and existing targets; extend manual/Auto-Fill MIME tests with AC3 metadata. Retain invalid-source cleanup coverage.
- [x] `hifimule-daemon/tests/mediacheck_cli.rs` — test lowercase/uppercase and corrupt AC3: classification, counts, exit codes, unchanged originals.
- [x] `docs/mediacheck.md`, `docs/api-contracts-hifimule-daemon.md` — document AC3/downmix and remove obsolete unsupported claims.

**Acceptance Criteria:**
- Given the supplied sample, when decoded through the production playback path, then its complete audio produces stereo PCM with no unsupported-layout failure.
- Given an AC3 original selected for local encoding in manual or Auto-Fill sync, when synchronization finishes, then the verified target file is committed and temporary staging is removed.
- Given a folder containing AC3 files, when mediacheck audits it, then valid files count as checked and invalid files count as bad.
- Given existing formats and runtime receipts, when regressions run, then formats still work and stale runtimes are rejected.

## Spec Change Log

## Verification

**Commands:**
- `rtk node --test scripts/tests/*audio-runtime.test.mjs` — runtime checks pass.
- `rtk npm run build:daemon -- test -p hifimule-daemon --lib` — daemon regressions pass with rebuilt runtime.
- `rtk npm run build:daemon -- test -p hifimule-daemon --test mediacheck_cli` — audit classifications pass.
- `rtk cargo fmt --all -- --check` — formatting passes.

Run the decoder diagnostic against `/Users/akartmann/Downloads/sample-ac3-1.ac3`; audit a temporary copy and convert through the production encoder. Record duration, channels and rate. Audible output and installed Windows/Linux checks require their respective environments.

### Implementation verification — 2026-10-24

All execution tasks above are complete. The approved intent block is unchanged.

- Rebuilt the controlled FFmpeg 9.0.2 macOS ARM64 runtime with AC3 demuxer,
  decoder and parser. Existing encoders, runtime pin and output profiles are unchanged.
- Runtime tests: `rtk node --test scripts/tests/*audio-runtime.test.mjs` —
  90 passed, 1 skipped. Includes stale/partial receipt rejection.
- Source/library tests: `rtk npm run build:daemon -- test -p hifimule-daemon --lib` — 14 passed.
- Full daemon regression: `rtk npm run build:daemon -- test -p hifimule-daemon --bin hifimule-daemon` —
  1,691 passed, 16 ignored, zero failures. These binary tests cover playback,
  provider selection, production encoding to all 11 existing codec/container
  targets, manual/Auto-Fill AC3 MIME variants, and invalid-source cleanup.
  The sandbox initially denied localhost mock-server bindings; the same suite
  passes outside the sandbox after automatic approval.
- Audit CLI: `rtk npm run build:daemon -- test -p hifimule-daemon --test mediacheck_cli` —
  6 passed. Lowercase/uppercase AC3 are checked; corrupt AC3 is bad; sources remain unchanged.
- Supplied recording: set `HIFIMULE_DIAGNOSTIC_AC3` to the external sample and run
  `rtk npm run build:daemon -- test -p hifimule-daemon --bin hifimule-daemon diagnostic_external_ac3 -- --ignored --nocapture` —
  both diagnostics passed. Production playback emitted 4,612,608 stereo frames
  at 48 kHz, exactly 96.096 seconds. Production encoding decoded the 6-channel,
  48 kHz source and produced a verified stereo MP3 of 2,307,500 bytes.
- Production mediacheck audited a temporary uppercase-extension sample copy:
  checked 1, ignored 0, recoverable 0, bad 0, operational errors 0; exit 0.
  Original and audit copy SHA-256 stayed
  `c08c716898a77819b306b94fadbdce29f1b7ca127895ca2c5c2d9fdad97f3076`.
  The recording is not part of the repository.
- `rtk cargo fmt --all -- --check` reports existing baseline formatting drift,
  beginning at `hifimule-daemon/src/api.rs:145`. Formatting-only changes outside
  this feature were restored; all added standalone code was rustfmt formatted.

Limits: no audible device-output check or installed Windows/Linux qualification
was possible in this environment. AC3 seeking and gapless behavior remain
unqualified; output remains mono/stereo. No external sample was committed.

### Review patch verification — AC3 concealed corruption

Review reproduced a defect inside the approved invalid-source and bounded
recovery requirements: native AC3 decoding could return successful frames with
`decode_error_flags` after CRC or audio-block errors. Those concealed frames
were previously accepted as clean by shared source validation.

Patched `hifimule-daemon/src/source_audio.rs` to enable native CRC checking for
AC3 only and route flagged frames through the existing `DecodeRecovery` limits
before resetting consecutive errors, counting source duration, or invoking the
encoding callback. Other codecs retain their existing configuration. Native
concealed PCM is excluded from conversion. No approved intent changed.

New tests cover sparse recoverable damage across 189 frames, sustained errors,
a high error fraction, CRC corruption throughout the file, partial final-frame
truncation, and malformed audio blocks with a deliberately restored matching
CRC. Production encoding tests verify recovery evidence, packet release,
unchanged originals and removal of failed outputs. CLI tests check classification,
counts, unchanged sources and exit 1 for real damaged AC3 payloads.

Final checks after the patch:

- `rtk npm run build:daemon -- test -p hifimule-daemon --lib` — 16 passed.
- `rtk npm run build:daemon -- test -p hifimule-daemon --bin hifimule-daemon ac3 -- --nocapture` —
  4 passed, 2 external diagnostics ignored in the ordinary run.
- `rtk npm run build:daemon -- test -p hifimule-daemon --test mediacheck_cli` — 7 passed.
- Full daemon regression with localhost mock servers enabled — 1,692 passed,
  zero failures, 16 ignored; includes manual/Auto-Fill MIME and cleanup tests.
- The review's exact CRC-every-frame, overwritten-middle and truncated-tail
  audit inputs each now report checked 1, bad 1, ignored 0, exit 1.
- Both supplied-recording diagnostics pass again: 4,612,608 stereo playback
  frames at 48 kHz (96.096 seconds), and verified stereo MP3 encoding from the
  6-channel source (2,307,500 bytes). Production mediacheck still reports healthy,
  checked 1, bad 0, exit 0. All audit inputs and the original remain unchanged;
  original SHA-256 remains
  `c08c716898a77819b306b94fadbdce29f1b7ca127895ca2c5c2d9fdad97f3076`.
- Targeted rustfmt checks pass for the three Rust files changed by this patch.
  No broad formatting changes were introduced.

Partial final AC3 frames now produce recovery evidence and follow existing
limits; a single partial frame can remain recoverable when its error share is
within those limits. Removal of whole valid frames from raw AC3 cannot be
inferred from its bitrate-estimated duration. These checks detect reported
integrity errors and do not claim to detect every possible source alteration.

### Independent review outcome

Blind review and acceptance audit found no actionable deviations. The edge-case
review found one implementation patch: FFmpeg concealed damaged AC3 frames without
raising decode errors. Native CRC checking and frame error-flag handling now feed
the existing recovery limits. The reviewer independently reran all three original
corruption reproductions and confirmed BAD reports with exit 1, with no residual
finding. No intent or spec-design changes were needed; no work was deferred.

## Suggested Review Order

**Source decoding and validation**

- Open AC3 with CRC checks; reuse the shared audit and encoding pipeline.
  [source_audio.rs:192](../../hifimule-daemon/src/source_audio.rs#L192)

- Count concealed frames against recovery limits before accepting their PCM.
  [source_audio.rs:272](../../hifimule-daemon/src/source_audio.rs#L272)

**Playback admission and downmix**

- Admit original AC3 representations through the existing provider selection.
  [mod.rs:341](../../hifimule-daemon/src/providers/mod.rs#L341)

- Downmix explicit speaker layouts while rejecting unknown multichannel mappings.
  [decoder.rs:519](../../hifimule-daemon/src/playback/decoder.rs#L519)

**Runtime and regression evidence**

- Package the AC3 demuxer, decoder and parser on every supported platform.
  [audio-runtime.json:13](../../hifimule-daemon/audio-runtime.json#L13)

- Exercise extensionless conversion across existing target formats.
  [local.rs:1700](../../hifimule-daemon/src/transcoding/local.rs#L1700)

- Verify MIME variants and committed outputs in both sync modes.
  [sync.rs:8181](../../hifimule-daemon/src/sync.rs#L8181)

- Check case-insensitive scanning, corruption classification and unchanged originals.
  [mediacheck_cli.rs:73](../../hifimule-daemon/tests/mediacheck_cli.rs#L73)
