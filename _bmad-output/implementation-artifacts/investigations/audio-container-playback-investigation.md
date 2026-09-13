# Investigation: Audio container playback failures after 15.4 development

## Hand-off Brief

1. **What happened.** User reports deterministic playback failure for two M4A files and premature stop after about one second for one MP3; FLAC and most MP3 files play normally.
2. **Where the case stands.** The MP3 root cause is confirmed in HifiMule's bounded reader. Both M4As pass the production decoder; their live failure is blocked on error detail that the current pipeline discards.
3. **What's needed next.** Fix the bounded random-access contract and preserve typed worker errors, then reproduce one M4A live to identify its provider/output boundary.

## Case Info

| Field            | Value |
| ---------------- | ----- |
| Ticket           | N/A |
| Date opened      | 2026-09-13 |
| Status           | Concluded |
| System           | Windows; HifiMule after 15.4 development |
| Evidence sources | Three user-supplied audio files, source code, version control, application/test output |

## Problem Statement

User report: "After 15.4 development, I'm testing audio playback. I have a issue playing m4a files : I get audio output unavailable on the 2 m4a files. On flac it works, on mp3 most work but I have one that only play for 1s and stop."

## Evidence Inventory

| Source | Status | Notes |
| ------ | ------ | ----- |
| `C:\Users\alexi\Downloads\11 Let It Ride.m4a` | Available | FFprobe identifies 243.043 s AAC stereo at 44.1 kHz plus an MJPEG artwork stream. |
| `C:\Users\alexi\Downloads\1-01 L'invitation.m4a` | Available | FFprobe identifies 224.212 s AAC stereo at 44.1 kHz plus an MJPEG artwork stream. |
| `C:\Users\alexi\Downloads\03 That Time Of The Night (The Short.mp3` | Available | FFprobe identifies 360.568 s MP3 stereo at 44.1 kHz plus an MJPEG cover-art stream; full FFmpeg decode completes without warnings. |
| HifiMule playback source | Available | Playback implementation is concentrated in `hifimule-daemon/src/playback/`, including `audio.rs`, `decoder.rs`, `session.rs`, and `streaming.rs`. |
| Git history | Available | Relevant commits include `4dae294` (Story 15.4), `1ac5a98` (Dev 15.4 - OSX), and `6f7a32b` (Fix playback control on windows). |
| Playback tests | Partial | Unit/integration tests and a dedicated `experiments/playback-probe` exist; sample-specific coverage has not been found yet. |
| Static analysis | Missing | Not yet run; lower value until the failing path is traced. |
| Diagnostic archive / issue ticket | Missing | No archive or ticket supplied. |
| Application runtime logs | Missing | No playback log or stack trace supplied yet. |

## Investigation Backlog

| # | Path to Explore | Priority | Status | Notes |
| - | --------------- | -------- | ------ | ----- |
| 1 | Probe MP3 structure and decode integrity | High | Done | Valid 360.568 s MP3; FFmpeg decoded the complete audio stream without warnings. |
| 2 | Locate exact `audio output unavailable` origin | High | In Progress | UI string maps from `OUTPUT_UNAVAILABLE`; source producer/callers still need tracing. |
| 3 | Compare working FLAC/MP3 path with M4A/AAC path | High | Done | Both M4As pass the production decoder; the oversized MP3 reproduces at 53,968 frames. |
| 4 | Review recent 15.4-related changes | Medium | Done | Story introduced playback pipeline; latest Windows change is UI-only. |
| 5 | Reproduce through app/tests with diagnostics | High | Partial | Decoder-level reproduction is complete; live M4A error detail is not retained by the application. |
| 6 | Inspect existing AAC continuity evidence | High | Done | Prior issue is Linux FFmpeg 8 frame excess; not the current Windows/FFmpeg 9 failure. |

## Timeline of Events

| Time | Event | Source | Confidence |
| ---- | ----- | ------ | ---------- |
| 2026-09-13 | User reports failures during post-15.4 playback testing. | User report | Confirmed |
| 2026-09-13 | Both M4A samples successfully parsed by FFprobe. | FFprobe output | Confirmed |

## Confirmed Findings

### Finding 1: Both M4A samples contain parseable AAC audio

**Evidence:** FFprobe output captured on 2026-09-13 for the two referenced files.

**Detail:** Each file has 44.1 kHz stereo AAC audio of several minutes' duration and a secondary MJPEG artwork stream. This rules out an absent audio stream or a wholly unreadable container.

### Finding 2: The affected MP3 is structurally valid and decodes completely with FFmpeg

**Evidence:** FFprobe and FFmpeg null-output decode executed on 2026-09-13 against the supplied MP3.

**Detail:** The file contains a 360.568-second, 44.1 kHz stereo MP3 stream plus embedded MJPEG cover art. FFmpeg decoded the full audio stream without warning or error, so the one-second stop is not explained by an ordinary corrupt/truncated bitstream.

### Finding 3: The repository retains earlier evidence of an AAC continuity problem

**Evidence:** Git commit `ed9d110` is titled `docs: record Linux PipeWire validation and AAC continuity failure`.

**Detail:** This is a high-value historical lead, but its mechanism and applicability to the current Windows symptoms remain unconfirmed until the commit is inspected.

### Finding 4: `OUTPUT_UNAVAILABLE` is assigned by substring, not failure type

**Evidence:** `hifimule-daemon/src/playback/audio.rs:255-264`.

**Detail:** Any worker error whose rendered message contains `output` becomes `OUTPUT_UNAVAILABLE`. This includes decoder-side `unsupported output layout` from `hifimule-daemon/src/playback/decoder.rs:50`, so the UI code does not prove the physical output device failed.

### Finding 5: The production decoder selects audio explicitly

**Evidence:** `hifimule-daemon/src/playback/decoder.rs:42`.

**Detail:** FFmpeg streams are filtered by `Type::Audio`; MJPEG cover art cannot be selected as the decoded track.

### Finding 6: The affected MP3 exceeds the bounded seek window

**Evidence:** The sample is 9,027,323 bytes; `hifimule-daemon/src/playback/streaming.rs:7` limits retained compressed data to 8 MiB, and `streaming.rs:125-145` implements end-relative seeking by consuming to EOF before rejecting targets outside the retained window.

**Detail:** Unlike FLAC, all non-FLAC inputs are advertised as seekable (`hifimule-daemon/src/playback/decoder.rs:22-34`). An MP3 demuxer seeking to end for metadata can therefore evict the beginning and cannot return to it.

### Finding 7: Production decoding reproduces the MP3 at approximately 1.12 seconds

**Evidence:** Ignored production decoder test `diagnostic_external_flac`, run through the validated FFmpeg 9 build wrapper on 2026-09-13.

**Detail:** Both supplied M4As passed. The supplied MP3 produced only 53,968 output frames at 48 kHz (1.124 seconds), versus its declared 360.568-second duration and complete decode through ordinary FFmpeg.

### Finding 8: The original worker error is discarded

**Evidence:** `hifimule-daemon/src/playback/audio.rs:251-265` publishes only a broad code selected from message substrings; `hifimule-daemon/src/playback/session.rs:651-654` stores that code and retry flag. No worker error is written to `daemon.log`.

**Detail:** Existing user logs therefore cannot reveal why the live M4A pipeline emitted `OUTPUT_UNAVAILABLE`.

## Deduced Conclusions

### Deduction 1: The M4A symptom is downstream of basic container discovery

**Based on:** Finding 1.

**Reasoning:** Independent media probing reads the containers and identifies ordinary AAC audio streams with valid durations.

**Conclusion:** The application error likely occurs during backend selection, stream selection, decode, resampling, or output initialization rather than because the files have no audio.

### Deduction 2: The MP3 stop is caused by an invalid bounded-seek contract

**Based on:** Findings 2 and 6.

**Reasoning:** The MP3 decodes completely from a normal file, but is larger than the production reader's retained seek window. Production advertises random access while being unable to seek back to evicted bytes; the MP3 demuxer may inspect end metadata and strand itself near EOF.

**Conclusion:** The one-second application playback is caused by the streaming adapter, not MP3 corruption or the cover-art stream itself. Production reproduced 53,968 frames, matching the reported duration.

## Hypothesized Paths

### Hypothesis 1: A shared playback-path regression affects these format variants

**Status:** Refuted

**Theory:** Development associated with 15.4 changed media initialization or end-of-stream handling, exposing M4A/AAC initialization failure and premature MP3 termination.

**Supporting indicators:** The user observed the symptoms specifically after 15.4 development, while FLAC and most MP3 files still work.

**Would confirm:** A relevant recent code change plus deterministic reproduction through the changed path.

**Would refute:** The same failures on an earlier build, or evidence that both failures originate solely in malformed/unsupported sample data.

**Resolution:** Evidence now indicates at least two mechanisms: the MP3 exceeds the bounded window, while both M4As fit wholly inside it. A single format/cover-art selection bug does not explain all three.

### Hypothesis 2: Embedded cover art is selected instead of audio

**Status:** Refuted

**Theory:** The common MJPEG stream is accidentally selected for playback.

**Supporting indicators:** All three failing samples contain MJPEG artwork.

**Would confirm:** Decoder choosing the MJPEG stream.

**Would refute:** Explicit audio-medium filtering.

**Resolution:** Refuted by `hifimule-daemon/src/playback/decoder.rs:42`.

### Hypothesis 3: M4A failure occurs during seek/probe or resampler setup and is mislabeled as output failure

**Status:** Open

**Theory:** The decoder or resampler fails before producing PCM, while broad string matching maps its error to `OUTPUT_UNAVAILABLE`.

**Supporting indicators:** Both files are valid AAC; FLAC works; the displayed error category is not type-safe.

**Would confirm:** Production-path error chain from either supplied M4A.

**Would refute:** Evidence of an actual CPAL device/open failure for these starts.

**Resolution:** Both supplied M4As pass controlled-runtime production decoding. The remaining live failure is outside standalone decoding; provider response and CPAL setup remain distinguishable only after error-detail instrumentation.

## Missing Evidence

| Gap | Impact | How to Obtain |
| --- | ------ | ------------- |
| Exact application error/log context | Prevents locating the failing component boundary | Reproduce in the app with logs or add targeted diagnostics. |
| A known-working control file's metadata | Limits direct working-vs-failing comparison | Probe a FLAC or MP3 that plays normally. |
| Application-level reproduction trace | Needed to distinguish decoder, buffer, and output-state failures | Run the playback probe or daemon path with the supplied samples and capture diagnostics. |

## Source Code Trace

| Element | Detail |
| ------- | ------ |
| Error origin | MP3: invalid random-access contract in `hifimule-daemon/src/playback/streaming.rs:125-145`; misleading UI code mapping in `hifimule-daemon/src/playback/audio.rs:251-265`. |
| Trigger | MP3 demuxer end-relative seek on a source larger than the 8 MiB retained window; M4A live trigger remains hidden. |
| Condition | Non-FLAC streams are advertised seekable even when prior bytes can be evicted (`decoder.rs:22-34`). |
| Related files | `playback/decoder.rs`, `playback/streaming.rs`, `playback/audio.rs`, `playback/session.rs`. |

## Conclusion

**Confidence:** Medium overall; High for MP3, Low for the remaining M4A live-path cause

The MP3 root cause is confirmed by deterministic production-path reproduction: the bounded reader decodes only 1.124 seconds of this otherwise valid six-minute file. Both M4As decode successfully through HifiMule's production decoder, refuting file corruption, AAC support, and cover-art stream selection; their live provider/output failure remains open because the implementation discards the underlying worker error and exposes only a substring-derived code.

## Recommended Next Steps

### Fix direction

1. Replace the false bounded-seek contract. Use genuine HTTP range-backed seeking for random-access containers, or expose affected sequential formats as non-seekable when safe. Merely increasing the 8 MiB window would postpone the same failure for larger files.
2. Replace message-substring classification with typed errors from source fetch, demux/decode/resample, and CPAL output stages.
3. Log a sanitized native error chain and relevant representation metadata (`container`, `codec`, HTTP status/content type, decoder hint, endpoint), without URLs or credentials.
4. Add a seek-triggering MP3 larger than the compressed window as a regression fixture and require decoding to its declared duration.

### Diagnostic

After error preservation is implemented, replay either supplied M4A through the live provider path. The captured typed stage will determine whether the remaining change belongs in provider representation/fetch handling or CPAL output initialization.

## Reproduction Plan

1. Serve the oversized MP3 through the same chunked HTTP path as production and assert decoded/consumed duration is approximately 360.568 seconds, not 1.124 seconds.
2. Run both M4As through standalone `decode_stream`; retain the current passing result.
3. Run one M4A through authenticated playback RPC and record the typed failing stage plus sanitized error chain.
4. Verify working FLAC and ordinary MP3 controls against the same endpoint and representation selection.

## Side Findings

- All three files include embedded JPEG artwork, but production explicitly selects an audio-medium stream; artwork selection is refuted as the cause.
- Commit `ed9d110` concerns AAC length excess with Linux FFmpeg 8.0.1. The controlled Windows FFmpeg 9 decoder passes both current M4As, so that historical issue is separate.

## Follow-up: 2026-09-13

### New Evidence

- The supplied oversized MP3 now passes two production decoder regressions: filename-hinted and extensionless ID3 detection both produce more than 17 million 48 kHz frames while compressed high-water remains at or below 8 MiB.
- Both supplied M4As still pass the production decoder after the change.
- The generated six-format production decoder matrix passes.
- The complete daemon suite passes 727 tests with 3 environment-gated tests ignored after review fixes.
- Exact supplied-file verification: both oversized-MP3 ignored tests passed with `HIFIMULE_DIAGNOSTIC_MP3` set to the supplied MP3; `diagnostic_external_flac -- --ignored --nocapture` passed twice with `HIFIMULE_DIAGNOSTIC_FLAC` set to each supplied M4A. The exact controlled-wrapper invocations are recorded in the approved spec.

### Additional Findings

- MP3 detection now accepts normalized `.mp3` hints, ID3 followed by a valid MPEG-audio header, and raw valid MPEG-audio headers while rejecting an ADTS AAC header.
- Pipeline failures are classified by typed stage rather than message substring. Source timeout/detail survives FFmpeg's errno conversion via shared reader failure state.
- Diagnostics log only stable stage/code, an allowlisted representation, and a sanitized chain. URLs, credentials, titles, track IDs, and server IDs are redacted or omitted.
- Resume failures retain the public `RESUME_UNAVAILABLE` code while emitting the same typed, sanitized diagnostic; diagnostic logging is fenced under the authoritative generation lock.
- Early output-start, snapshot, and output-loss exits cancel and join the decoder worker, avoiding detached blocked workers.
- Mixed-case URL schemes are redacted, mixed-case JSON/XML response types are rejected consistently, `.mpeg` is no longer treated as definitive MP3, and extensionless ID3 tags are probed up to the unchanged 8 MiB bound.

### Updated Hypotheses

- The MP3 bounded-seek hypothesis is resolved and fixed.
- The live M4A cause remains open; the next reproduction will now identify provider, timeout, decode, output-open, or output-loss stage in `%APPDATA%/HifiMule/daemon.log`.

### Backlog Changes

- Oversized MP3 decoding and error-attribution implementation are complete.
- Live M4A application reproduction remains for user validation.

### Updated Conclusion

The confirmed MP3 defect is addressed without increasing the bounded compressed-memory cap or changing M4A seekability. The M4A files remain decoder-valid; the new typed diagnostic path supplies the previously missing evidence needed to isolate their live failure.

## Follow-up: 2026-09-13 #2

### Confirmed Live M4A Cause

The new live diagnostic reported `stage=provider code=PLAYBACK_UNSUPPORTED representation=unknown` for every M4A. Provider resolution itself implements playback for both Jellyfin and Subsonic; the rejection occurred afterward in `select_playback_representation`.

Both providers use the known file suffix as the original representation's codec/container label. The filter accepted codec names such as `aac` and `alac`, but rejected container labels `m4a` and `mp4`. Consequently MP3 passed (`mp3` was allowlisted) while every M4A was rejected before HTTP fetch or decoding.

### Resolution and Verification

The supported-representation filter now accepts `m4a` and `mp4`, matching formats already handled by the production FFmpeg decoder. A regression test first reproduced `UnsupportedCapability("no supported playback representation")`, then passed after the filter change. The provider suite passes 23 tests and the complete daemon suite passes 727 tests with 3 environment-gated tests ignored.
