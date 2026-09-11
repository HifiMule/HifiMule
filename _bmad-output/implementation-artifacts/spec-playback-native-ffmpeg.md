---
title: 'Prove native Rust FFmpeg decoding into the playback callback'
type: 'chore'
created: '2026-09-11'
status: 'done'
baseline_commit: '2f272f5'
context: []
---

<frozen-after-approval reason="user approved the next isolated Rust/native playback proof">

## Intent

**Problem:** FFmpeg CLI passed six formats, but HifiMule cannot base a native Rust playback decision on a subprocess comparison. Native library decoding must retain the same track boundaries while feeding the existing bounded audio path.

**Approach:** Add an optional, pinned FFmpeg Rust backend to the standalone experiment. Compare decoded PCM with the existing fixtures and run a silent native callback test on this Mac. Preserve the Symphonia baseline. Document exactly which library versions were linked and which platform and physical-output claims remain untested.

## Boundaries & Constraints

**Always:** Keep dependencies and implementation inside `experiments/playback-probe`; use the existing bounded callback queue and finite-file deadlines. Keep FFmpeg contexts local to the decoding thread, transfer only owned PCM, reject unsupported formats explicitly, preserve original standard-stereo sample rate, and use the same verifier thresholds. All Windows, macOS and Linux remain targets. Record local results as macOS evidence only.

**Ask First:** Live servers or unavailable external hardware require user-provided access. The already approved synthetic silent MacBook endpoint tests require no additional confirmation.

**Never:** Change production playback, startup lifecycle, system audio settings, or distribution packaging. Do not use fixture manifest lengths to trim decoded output. No audible automatic tests, music uploads, codec fallback hidden from reports, or claim of physical gapless output from callback counters. Do not ship the test machine's FFmpeg build as a production decision.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Decode | Generated six-format stereo sequence | Native libraries produce f32 PCM and actual per-track metadata | Verifier reports observed mismatch honestly |
| Native play | Matching endpoint and finite files | Worker fills same bounded queue; callback consumes sequence | Report underruns and stop state |
| Unsupported | No feature, unsupported layout or rate change | Explicit refusal | No silent resampling or backend fallback |
| Bad input | Invalid/truncated local file or decode error | Nonzero result | Do not treat arbitrary errors as EOF |
| Existing output | File, symlink, hard link | Existing bytes preserved | Exclusive creation fails |
| Output failure | Device disappears or no progress | Stop without device fallback | Existing deadlines apply |
| End of stream | Decoder has buffered output | Drain decoder/converter correctly | No invented silence or dropped buffered samples |

</frozen-after-approval>

## Code Map

- `experiments/playback-probe/src/main.rs`: Symphonia decode, PCM output contract, fixed queue and native CPAL callback; introduce explicit backend dispatch.
- `experiments/playback-probe/Cargo.toml`, `Cargo.lock`: independent workspace, optional native dependencies only.
- `experiments/playback-probe/verify.py`: common fixture comparison and CLI decoder baseline.
- `experiments/playback-probe/README.md`: local build and cross-platform test guidance.
- `_bmad-output/implementation-artifacts/playback-feasibility-results.md`: measured results and remaining risks.

## Tasks & Acceptance

**Execution:**
- [x] `src/ffmpeg_backend.rs`, `src/main.rs` under the probe: implement explicit native backend, strict decoding, sample conversion, draining and common sink integration with meaningful edge-case tests.
- [x] Probe `Cargo.toml`, `Cargo.lock`: pin optional binding; establish local linked versions and inspect relevant wrapper limitations.
- [x] Probe `verify.py`, `test_tools.py`: select native backend distinctly, preserve comparison semantics and report linked library identity.
- [x] Probe `README.md`: document feature build, decode/play commands, build prerequisites, local evidence and limitations.
- [x] Feasibility results and this spec: record codec matrix, native silent run, relevant performance observations, review outcomes and remaining OS/hardware work.
- [x] Run native and default builds/tests, Python tests, unchanged codec matrix and a silent callback run; review and fix material findings.

**Acceptance Criteria:**
- Given the feature is disabled, when baseline commands run, then Symphonia remains usable without FFmpeg system libraries.
- Given the feature is enabled, when native FFmpeg is selected, then the process decodes through linked libraries and the same verifier evaluates all six generated formats without fixture-aware trimming.
- Given the native decoder feeds playback, when the silent sequence completes, then actual consumed frames and starvation counters are recorded with endpoint and backend identity.
- Given errors or format changes, when decoding cannot continue safely, then the command reports failure rather than substituting another backend or claiming successful completion.
- Given the results document, when choosing a next step, then native Mac evidence is distinguished from packaging, physical output, real streaming and Windows/Linux validation.

## Spec Change Log

## Design Notes

Use an optional `native-ffmpeg` feature and a `--decoder` selector shared by decode/play commands. Existing invocations remain Symphonia. FFmpeg creates and drops all demux/decoder/converter contexts inside each decode invocation; neither preflight nor worker sends those contexts between threads. Avoid broad dependence on an upstream Send promise. The pinned wrapper must still be evaluated before production adoption.

Container/codec skip metadata and decoder draining may differ from CLI behavior. First measure direct library output; any required timeline handling must derive from actual media metadata and be independently justified. Preserve a failed codec result rather than forcing it to pass. Standard stereo and exact source/output sample rate intentionally bound this experiment; integer device output, multichannel remapping and quality adaptation are later work.

## Verification

- `rtk cargo test --manifest-path experiments/playback-probe/Cargo.toml` and with `--features native-ffmpeg`.
- Clippy for the native feature; Python tool tests; verifier `--decoder native-ffmpeg --require wav,flac,alac,mp3,aac,opus`.
- Silent native play using explicitly enumerated MacBook Pro speakers; record decoded/consumed frame count, underruns and library identity.
- Inspect production diff boundary and generated exclusions before local commit. No push.

### Executed verification

Native 14 Rust tests and default 9 Rust tests passed; native all-targets Clippy is clean. Both Python suites pass, nine tests each. Native six-format matrix passes with exact lengths; default Symphonia outcomes unchanged. Silent AAC and Opus CoreAudio runs each consumed all 288,041 frames with zero reported underruns. `otool -L` confirms FFmpeg linkage only in the feature-enabled build. Remaining platform/physical-output limits are documented in the feasibility report.

### Review outcome

Three independent reviews identified and corrected nested protocol access, inconsistent audio-stream selection, and clean-packet WAV truncation. The last was reproduced before adding RIFF/chunk-bound checks and a regression covering both valid and truncated payloads. The full codec matrix still passes. Two-stream selection and rejected nested HTTP were also exercised. Follow-up edge review found no remaining defects.

## Suggested Review Order

- Decoder selection keeps default builds independent.
  [main.rs:25](../../experiments/playback-probe/src/main.rs#L25)

- Validate local input, then drain native decoding into the shared sink.
  [ffmpeg_backend.rs:103](../../experiments/playback-probe/src/ffmpeg_backend.rs#L103)

- Reject RIFF truncation that FFmpeg can otherwise accept.
  [ffmpeg_backend.rs:71](../../experiments/playback-probe/src/ffmpeg_backend.rs#L71)

- Preserve stereo ordering while converting sample representation.
  [ffmpeg_backend.rs:30](../../experiments/playback-probe/src/ffmpeg_backend.rs#L30)

- Use identical comparison rules and retain loaded-library identity.
  [verify.py:45](../../experiments/playback-probe/verify.py#L45)

- Reproduce complete-packet truncation and confirm a valid counterpart.
  [main.rs:571](../../experiments/playback-probe/src/main.rs#L571)

- Keep native dependencies optional in the standalone workspace.
  [Cargo.toml:9](../../experiments/playback-probe/Cargo.toml#L9)

