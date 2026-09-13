---
title: 'Add M4R playback compatibility'
type: 'feature'
created: '2026-09-13'
status: 'done'
baseline_commit: '18a811046a7f4d41a402b2d8e60b5aca8e921948'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Provider playback rejects tracks labeled `m4r`, even though the supplied M4R sample is an MP4/MOV container with AAC audio and the controlled FFmpeg runtime already enables the required MOV demuxer and AAC decoder.

**Approach:** Recognize `m4r` as a supported original playback representation and pass the `.m4r` container hint through the existing FFmpeg decoding pipeline. Prove compatibility with focused provider-selection and production-decoder regressions using the supplied sample.

## Boundaries & Constraints

**Always:** Preserve current representation ordering and fallback behavior; treat matching case-insensitively like existing formats; retain the `.m4r` hint for FFmpeg; keep all existing codec, memory, and error contracts unchanged.

**Ask First:** Any newly discovered need to change FFmpeg dependencies, runtime build flags, streaming/seek behavior, provider APIs, or packaging.

**Never:** Transcode or rename M4R files, identify arbitrary MP4 video as playable audio, add support for OGG/WMA/AIF in this change, commit the supplied copyrighted sample, or modify library versions.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Provider selection | Original representation labeled `m4r` | Representation is selected through the existing original-source priority | No `no supported playback representation` error |
| Case normalization | Original representation labeled `M4R` | Same result as lowercase `m4r` | Existing case-insensitive matching is preserved |
| Decoder hint | Container is `m4r` with AAC audio | Decoder receives `stream.m4r` and produces PCM through the MOV/AAC path | Existing typed decode failures remain unchanged |
| Unsupported format | Representation uses an unrelated unknown label | It remains rejected | Existing `UnsupportedCapability` behavior is preserved |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/providers/mod.rs` -- playback representation allowlist and selection regressions.
- `hifimule-daemon/src/playback/audio.rs` -- normalization of the container into the decoder filename hint.
- `hifimule-daemon/src/playback/decoder.rs` -- production FFmpeg decoder and environment-gated supplied-file coverage.
- `hifimule-daemon/audio-runtime.json` -- evidence that MOV demuxing and AAC decoding are already enabled; no planned change.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/providers/mod.rs` -- first add failing lowercase and mixed-case M4R selection coverage, then minimally allow `m4r`.
- [x] `hifimule-daemon/src/playback/audio.rs` -- extend decoder-hint coverage to prove `.m4r` is retained without codec/container confusion.
- [x] `hifimule-daemon/src/playback/decoder.rs` -- exercise the supplied M4R through the production decoder without committing the media.

**Acceptance Criteria:**
- Given a provider advertises an original M4R representation, when playback selects a source, then that representation is accepted and retains original-source priority.
- Given the supplied M4R file, when it is decoded using the controlled production path, then decoding yields non-empty PCM without a dependency or runtime configuration change.
- Given existing supported and unsupported representations, when the provider tests run, then their behavior remains unchanged.

## Spec Change Log

## Verification

**Commands:**
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon m4r_original_representation_is_supported` -- expected: red before implementation, green afterward.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon decoder_hint_normalizes_containers_without_treating_codec_as_container` -- expected: M4R hint assertion passes.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon diagnostic_external_m4r -- --ignored --nocapture` with `HIFIMULE_DIAGNOSTIC_M4R` set to the supplied file -- expected: decoded frames and PCM are non-zero.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon` -- expected: complete daemon suite passes.
- `rtk cargo fmt --all -- --check` -- expected: formatting passes.

**Executed verification:**
- Supplied network-share M4R production decode: 1 passed, with non-zero frames and PCM.
- Complete daemon suite: 731 passed, 0 failed, 5 ignored; 7 pre-existing dead-code warnings.
- Workspace formatting check: passed.
- Independent review: no reachable edge-case findings; the acceptance evidence gap was closed by the supplied-file run.

## Suggested Review Order

**Compatibility boundary**

- Admit M4R labels through the existing case-insensitive playback capability gate.
  [`mod.rs:115`](../../hifimule-daemon/src/providers/mod.rs#L115)

- Preserve M4R as a normalized filename hint for FFmpeg container probing.
  [`audio.rs:946`](../../hifimule-daemon/src/playback/audio.rs#L946)

**Regression evidence**

- Verify lowercase, mixed-case, and original-source priority at provider selection.
  [`mod.rs:1477`](../../hifimule-daemon/src/providers/mod.rs#L1477)

- Decode the supplied external sample through the production bounded-stream path.
  [`decoder.rs:342`](../../hifimule-daemon/src/playback/decoder.rs#L342)
