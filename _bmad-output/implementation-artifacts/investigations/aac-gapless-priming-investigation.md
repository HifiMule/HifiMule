# Investigation: AAC gapless priming at prepared boundaries

## Hand-off Brief

1. **What happened.** Confirmed: the supplied M4A successor decodes with about 51 ms of leading near-zero AAC priming, so exact decoded concatenation creates an audible interruption after a live predecessor tail.
2. **Where the case stands.** The continuous output stream is working; FFmpeg reports no start skip for either file, while it reports exact end discard (51 and 448 input samples).
3. **What's needed next.** Add a narrowly validated legacy-AAC priming classification or retain the files as explicitly unsupported; a blanket 2112-sample trim would corrupt files from other AAC encoders.

## Case Info

| Field | Value |
| --- | --- |
| Ticket | Story 15.9 field follow-up |
| Date opened | 2026-09-18 |
| Status | Concluded |
| System | User reports Linux and macOS; local analysis on macOS ARM64 with FFmpeg 9.0.1 |
| Evidence sources | Supplied M4A files, FFprobe/FFmpeg decode, production decoder and boundary consumer |

## Problem Statement

The user reports that ordinary separated tracks transition comparably to VLC, but continuous MP3 and M4A album tracks contain a short audible silence. The supplied `08-Brain Damage.m4a` and `09-Eclipse.m4a` should form a continuous transition.

## Evidence Inventory

| Source | Status | Notes |
| --- | --- | --- |
| `/Users/akartmann/Downloads/08-Brain Damage.m4a` | Available | SHA-256 `6f542fd4274251d2655bd4ae22d6a42e23b586a39cd8a7f2630fc8c47efb5e22` |
| `/Users/akartmann/Downloads/09-Eclipse.m4a` | Available | SHA-256 `114ef725b3e3e1cb083b72aa99fbd113624cea0229cc681fbd48fad70ac9d0c1` |
| Production decoder/output source | Available | `hifimule-daemon/src/playback/decoder.rs`, `output.rs`, `audio.rs` |
| Native physical capture | Missing | User listening result is available; no raw capture artifact |

## Investigation Backlog

| # | Path to Explore | Priority | Status | Notes |
| - | --- | --- | --- | --- |
| 1 | Establish exact leading priming classification | High | Done | Four independent markers required; no silence detection |
| 2 | Add production regression | High | Done | Unit classifier plus opt-in diagnostic against both supplied files |
| 3 | Verify MP3 report separately | Medium | Done | Two supplied pairs distinguish encoded silence from a clean, active gapless boundary |

## Timeline of Events

| Time | Event | Source | Confidence |
| --- | --- | --- | --- |
| 2026-09-18 | Linux and macOS continuous-album silence reported | User report | Confirmed |
| 2026-09-18 | Both supplied streams identified as AAC-LC, 44.1 kHz stereo | FFprobe | Confirmed |
| 2026-09-18 | Decoded successor begins with about 51 ms below -100 dBFS | independent FFmpeg f32 decode | Confirmed |

## Confirmed Findings

### Finding 1: The successor file contains decoded priming before audible program audio

**Evidence:** `/tmp/eclipse.f32` analysis from supplied SHA-256 fixture.

**Detail:** Independent FFmpeg conversion to 48 kHz stereo produces 2,455 leading frames at or below -100 dBFS (51.15 ms). `Brain Damage` ends with non-silent audio.

### Finding 2: Container timing does not authorize a start trim

**Evidence:** FFmpeg debug output for both supplied files.

**Detail:** Each MOV edit list starts at media time zero. FFmpeg injects `skip 0`; it does inject exact end discard of 51 samples for track 08 and 448 for track 09.

### Finding 3: Output handoff is not adding its former refill gap

**Evidence:** `hifimule-daemon/src/playback/output.rs` boundary consumer and passing same-callback boundary tests.

**Detail:** A ready successor is consumed in the same callback/write span. The observed duration matches decoded successor priming rather than the 100 ms refill threshold.

## Deduced Conclusions

### Deduction 1: Exact decoded concatenation is insufficient for legacy AAC without start-padding metadata

**Based on:** Findings 1–3.

**Reasoning:** The output layer faithfully concatenates samples, but the decoded successor begins with encoder priming that the demuxer does not mark for removal.

**Conclusion:** The defect is in representation-specific padding interpretation, before the output boundary.

## Hypothesized Paths

### Hypothesis 1: A validated legacy Apple AAC signature can authorize a 2112-input-sample trim

**Status:** Confirmed

**Theory:** Both files begin with the same seven-byte AAC filler packet and carry legacy Apple-style MP4 metadata. Apple AAC commonly uses 2112 priming samples, but FFmpeg explicitly warns that 2112 is not universal across AAC encoders.

**Supporting indicators:** Identical first packet `20 00 20 00 00 80 0e`; decoded leading interval; legacy iTunes metadata.

**Would confirm:** A deterministic classifier plus boundary comparison showing the trim preserves the intended waveform for independently sourced examples.

**Would refute:** A file with the same signature whose program audio begins before sample 2112.

**Resolution:** The production decoder classified both supplied files and discarded exactly 2112 input frames (2299 frames after 44.1→48 kHz conversion). Removing any one marker is rejected by the unit test.

## Missing Evidence

| Gap | Impact | How to Obtain |
| --- | --- | --- |
| Failing MP3 pair | Cannot determine whether MP3 metadata is ignored or absent | Supply a reproducible adjacent MP3 pair |
| Raw native capture | Cannot measure residual boundary samples | Capture installed output around the boundary |

## Source Code Trace

| Element | Detail |
| --- | --- |
| Error origin | `hifimule-daemon/src/playback/decoder.rs`, decoded frames are resampled and queued without a representation padding policy |
| Trigger | AAC/M4A successor whose container reports zero start skip despite encoder priming |
| Condition | Active tail is non-silent and prepared successor starts with untrimmed priming |
| Related files | `playback/output.rs`, `playback/audio.rs`, FFmpeg MOV/AAC demuxing |

## Conclusion

**Confidence:** High

The supplied M4A interruption is confirmed as unmarked legacy Apple AAC priming in the decoded successor, not a native-stream reopen or the former refill path. The implemented rule requires the MOV family, AAC, `iTunNORM`, and the exact filler packet before discarding 2112 input frames; generic AAC remains untouched. The root cause and production behavior are confirmed with high confidence. The two MP3 pairs are classified separately below.

## Recommended Next Steps

### Fix direction

Add a representation-specific padding decision before resampling and test that intentional leading silence after the authorized priming interval remains intact. Keep unmatched AAC files untrimmed and explicitly uncertified.

### Diagnostic

Record the selected padding rule and trimmed input-frame count in private diagnostics, then compare the concatenated boundary window against VLC or a captured reference.

## Reproduction Plan

Decode the supplied pair to the fixed output format, concatenate at the production boundary, and measure the successor leading interval. After a qualified trim, verify no added/missing/duplicated program frames and repeat on Linux and macOS.

## Side Findings

- The supplied files contain exact trailing discard metadata, and FFmpeg applies it.
- FFmpeg's own gapless discussion states that 2112 samples is specific to Apple encoders and must not be assumed for all AAC.

## Follow-up: 2026-09-18

### New Evidence

- `/Users/akartmann/Downloads/01-Anytime.mp3`, SHA-256 `1ffcb7b4e1e0d1469456755d6e357e4c28d8769b61430939bb6401cd414a4fe3`.
- `/Users/akartmann/Downloads/02-We Believe in Love.mp3`, SHA-256 `ec1736936b43743c9221389183fb55b67790740571c892c3d3deabc35db5473e`.
- Both are 44.1 kHz stereo MP3 encoded by LAME 3.97 with Xing metadata.
- FFmpeg injects and applies a 1105-sample initial skip to both files. It applies terminal discards of 647 samples to track 01 and 515 plus a complete 1152-sample frame to track 02.
- After those declared trims and conversion to 48 kHz stereo, track 01 still contains 96,228 consecutive terminal frames at or below -100 dBFS: 2004.75 ms. Track 02 begins with nonzero audio at its first output frame.

### Additional Findings

The MP3 transition silence is encoded program content after valid LAME/Xing padding has already been removed. It is not the unmarked encoder priming defect found in the legacy AAC pair and is not introduced by HifiMule's boundary consumer.

### Updated Hypotheses

- **MP3 metadata is ignored by the production decoder — Refuted.** FFmpeg logs show the exact skip/discard side data being consumed by the MP3 decoder.
- **A generic MP3 compatibility trim is required — Refuted for this pair.** The remaining interval is approximately two seconds of source silence, far beyond the declared padding, and removing it would violate intentional-silence preservation.

### Backlog Changes

The failing-MP3 evidence gap is closed. No decoder change is authorized by this pair. An optional user-controlled remove-gaps or crossfade feature would be a separate product requirement and must not alter Story 15.9's exact-content mode.

### Updated Conclusion

The M4A and MP3 observations have different causes. The M4A pair needed the narrow legacy Apple priming correction now implemented. The MP3 pair is decoded according to valid Xing/LAME padding and contains about two seconds of actual terminal silence in track 01; HifiMule must preserve it under the current contract.

## Follow-up: Marillion MP3 pair, 2026-09-18

### New Evidence

- `/Users/akartmann/Downloads/01 Hotel Hobbies.mp3`, SHA-256 `35a467975ece1869b7cc2342010fc5a1c306dca28888598d46ddcf425ef7b5ba`.
- `/Users/akartmann/Downloads/02 Warm Wet Circles.mp3`, SHA-256 `7984df967d2cbd664cfb274a0fee658260ffc89e38a31dbfd2d18f0a09aa5834`.
- Both are 44.1 kHz stereo MP3 files with LAME 3.96/Xing gapless metadata. FFmpeg applies the 1105-sample initial skip to both, the declared terminal discard to track 01, and the declared terminal discard to track 02.
- After decoding to the production 48 kHz stereo format, track 01 has no terminal near-zero run at -100 through -40 dBFS. Track 02 has no leading near-zero run at those thresholds.
- The final frame of track 01 is `(-0.084870, -0.075148)` and the first frame of track 02 is `(-0.085296, -0.082323)`. Their channel deltas are `(-0.000427, -0.007174)`, consistent with a continuous waveform rather than a silent boundary.
- The 50 ms RMS levels are -24.92 dBFS at the predecessor tail and -24.77 dBFS at the successor head.

### Additional Finding

This pair is a clean positive MP3 continuity fixture: both sides of the decoded boundary contain active audio, the encoder delay and padding are declared, and the independent decoder applies them. Any audible silence when HifiMule plays this exact pair is therefore player-added and should be investigated in the prepared handoff or native output path; trimming source samples is neither necessary nor authorized.

### Updated Conclusion

MP3 does not need an AAC-style compatibility rule for this pair. Existing LAME/Xing metadata supplies the required padding information. The correct behavior is exact concatenation of the decoded predecessor tail and successor head through the continuous output stream.

## Follow-up: FLAC pair, 2026-09-18

### New Evidence

- `/Users/akartmann/Downloads/01-On s’aime pas.flac`, SHA-256 `5901db932c16f7807b53da3c7a831034226489bbd5f1fae3f89d2f74a2201426`.
- `/Users/akartmann/Downloads/02-Les Regrets.flac`, SHA-256 `63f3dff8a546f6526fb0379fcf0865cbb4c4f04df788fc8ed1d5774a1fb5eae9`.
- Both audio streams are lossless FLAC, 44.1 kHz stereo, and each container also carries an MJPEG cover-art stream.
- After reference conversion to the production 48 kHz stereo format, track 01 has only 0.042 ms of terminal samples at or below -100 dBFS; track 02 has only 0.021 ms at its head. Even at -40 dBFS, the respective runs are 0.896 ms and 0.479 ms. These sub-millisecond intervals are waveform crossings, not a pause.
- The 100 ms RMS levels are -21.66 dBFS at the predecessor tail and -22.49 dBFS at the successor head. Both sides contain active audio.
- The final predecessor frame is approximately `(0.00000025, -0.00000243)` and the first successor frame is `(0, 0)`, placing the file boundary at a natural near-zero crossing.
- Both supplied files passed HifiMule's opt-in production FLAC decoder diagnostic under the controlled FFmpeg 9.0.1 runtime.

### Additional Finding

This pair is a clean lossless continuity fixture. FLAC has no lossy encoder priming to infer or remove, and the attached cover art does not prevent the production decoder from selecting and completing the audio stream. HifiMule must preserve every decoded audio frame and join the tracks through the existing continuous output stream.

### Updated Conclusion

No format-specific correction is needed for these FLAC files. An audible pause between them would be player-added and would implicate preparation timing, handoff, or native output submission rather than file padding.
