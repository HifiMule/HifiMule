---
title: 'Compare FFmpeg decoding against the native playback baseline'
type: 'chore'
created: '2026-09-11'
status: 'done'
baseline_commit: 'cacda5b'
context: []
---

<frozen-after-approval reason="user approved continuing the feasibility work">

## Intent

**Problem:** The isolated Symphonia 0.5.5 experiment preserves the tested WAV/FLAC/ALAC/MP3 sequences but adds AAC padding and cannot decode Opus. We need a measured alternative before choosing the production playback stack across Windows, macOS, and Linux.

**Approach:** Add an optional FFmpeg command-line decoder adapter to the same fixture verifier. Compare actual frame counts, full-signal error, and track boundaries without using expected fixture counts to trim or otherwise repair the decoder output. Document viable Rust integration options and the limitations of a local command-line proof.

## Boundaries & Constraints

**Always:** Keep this within `experiments/playback-probe`. Preserve the existing Symphonia baseline and normal application behavior. Identify the exact external binary version and configuration. Use generated original audio, bounded subprocess deadlines, finite regular input files, explicit failures, and exclusive output creation. All three OSes remain targets; executed evidence is specific to this Mac.

**Ask First:** Live server/hardware testing needs user-provided access when unavailable. This comparison requires no credentials or changes to system audio settings.

**Never:** Add production FFmpeg dependencies or release packaging yet, upload music, rewrite input files, silently resample/remix, infer physical gapless output from PCM comparisons, or claim this proves older FFmpeg versions behave identically. Python is a test harness only, not a proposed production runtime. Do not use the verifier manifest's expected lengths to modify audio during decoding.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Baseline | Existing synthetic files | Same Symphonia results remain reproducible | Required failures exit nonzero |
| Alternative | FFmpeg/ffprobe installed | Concatenated f32 PCM plus per-track rate/channel/frame metadata | Failed decoder exits nonzero |
| Missing executable | No FFmpeg or ffprobe | Explicit failed variant/report | Never reuse stale success |
| Output exists | Ordinary file, symlink, or hard link | Refuse overwrite | Original bytes unchanged |
| Bad audio | Truncated or invalid input | Strict decode error or explicit measured mismatch | No success for zero frames |
| Format change | Different rates/layouts between tracks | Reject unsupported combination | Never silently convert |
| Timeout | Tool stalls | Kill/wait for subprocess and return failure | Temporary data cleaned |

</frozen-after-approval>

## Code Map

- `experiments/playback-probe/src/main.rs` — existing bounded native/Symphonia probe; preserve implementation.
- `experiments/playback-probe/verify.py` — shared reference and boundary comparison, currently invokes one binary.
- `experiments/playback-probe/generate-fixtures.py` — generated original fixtures and optional encoders.
- `experiments/playback-probe/test_tools.py` — failure-report regression checks.
- `_bmad-output/implementation-artifacts/playback-feasibility-results.md` — measured baseline and remaining platform/provider work.

## Tasks & Acceptance

**Execution:**
- [x] `experiments/playback-probe/ffmpeg_decode.py` — implement finite-file decoder adapter with matching JSONL/PCM contract and subprocess cleanup.
- [x] `experiments/playback-probe/verify.py` — add explicit backend selection and backend/tool-version identity in reports without changing tolerances.
- [x] `experiments/playback-probe/test_ffmpeg_decode.py`, `test_tools.py` — test overwrite safety, malformed metadata/input, subprocess errors/timeouts, and backend dispatch/reporting.
- [x] `experiments/playback-probe/README.md` — document repeatable comparison commands and external tool requirements.
- [x] `_bmad-output/implementation-artifacts/playback-feasibility-results.md` — append measurements, candidate integration assessment, and pending runtime/provider checks.
- [x] Run regression tests, both decoder comparisons, and focused review; correct actionable defects.

**Acceptance Criteria:**
- Given the same fixture manifest, when selecting either decoder, then identical comparison rules and required-format semantics apply.
- Given AAC and Opus fixtures, when FFmpeg finishes successfully, then observed length and error metrics determine their result without fixture-aware trimming.
- Given unavailable or failing tools, when a comparison runs, then fresh results explicitly contain failure and required failures return nonzero.
- Given existing output or aliased input/output paths, when the adapter starts, then no existing file is truncated.
- Given the report, when assessing feasibility, then it distinguishes CLI decoding from Rust linkage, native output, platform portability, and actual streaming adaptation.

## Spec Change Log

## Design Notes

Use the CLI as an independent decoder baseline because FFmpeg already exists in this environment. Production could bind libavformat/libavcodec or host a managed decoder process; this experiment makes neither decision. Probe audio metadata from the input itself, select the first audio stream explicitly, preserve standard stereo and sample rate, and count actual output bytes. Require complete frames. Avoid collecting decoded PCM in subprocess capture buffers; use temporary files and bounded copies. Serialize only successful per-track results.

The installed FFmpeg 9.0.1 may behave differently from older releases. The result must retain its tool version rather than convert this observation into a generic FFmpeg guarantee. Codec/package availability and shared-library versions require a platform-specific assessment. Existing known missing Vorbis encoding remains visible as not run.

## Verification

- `rtk proxy python3 experiments/playback-probe/test_tools.py`
- `rtk proxy python3 experiments/playback-probe/test_ffmpeg_decode.py`
- `rtk proxy python3 experiments/playback-probe/verify.py --decoder symphonia --output experiments/playback-probe/results/symphonia-comparison.json`
- `rtk proxy python3 experiments/playback-probe/verify.py --decoder ffmpeg --require wav,flac,alac,mp3,aac,opus --output experiments/playback-probe/results/ffmpeg-comparison.json`
- `rtk cargo test --manifest-path experiments/playback-probe/Cargo.toml`

Review production diff boundaries and generated-file exclusions before local commit. No live media server, external audio interface, Windows machine, or Linux desktop is presumed available.

### Completed validation

FFmpeg 9.0.1 passed all six required codecs with exact per-track lengths. Symphonia retained its documented AAC and Opus limitations. Nine adapter tests, six verifier/tool tests, and nine Rust tests passed. General correctness and edge-case reviews found no actionable defects; a subsequent acceptance audit found no material unmet criteria. A shared 45-second subprocess budget keeps adapter children inside the verifier timeout, and the deadline regression test confirms an expired budget launches no child. No production files or dependencies changed.
