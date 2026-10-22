# Investigation: Local MP3 duration mismatch for DMV

## Hand-off Brief

1. **What happened.** Both supplied MP3s have trailing APEv2 tags that inflate FFmpeg’s estimated input duration by over three seconds and cause the current two-second validation rule to reject a duration-preserving re-encode.
2. **Where the case stands.** The file-level cause is confirmed with HifiMule’s controlled FFmpeg 9.0.2 runtime; matching titles do not explain the failing validator in the inspected source.
3. **What's needed next.** Correct duration validation to account for estimated input durations while retaining detection of genuine encoding truncation, then verify the actual daemon encoder with a regression fixture.

## Case Info

| Field | Value |
| --- | --- |
| Ticket | N/A |
| Date opened | 2026-10-22 |
| Status | Concluded — diagnosis |
| System | HifiMule current checkout on macOS; controlled FFmpeg 9.0.2 aarch64-apple-darwin |
| Evidence sources | Supplied MP3s, source trace, full audio decode, controlled-runtime CLI re-encode, temporary metadata-removal experiment |
| Machine-readable evidence | `local-mp3-duration-dmv-evidence.json` beside this report |
| Source control context | Working tree initially clean; recent local encoder change `eee05521507aebaad3403acd162c5bd368c24c9e` (2026-10-21, “Fix mp3 encoding”) |

## Problem Statement

During the last sync using local encoding, many tracks failed with `Local encoding failed: Local mp3 encoding failed: Encoded duration does not match original`. The user associates daemon item ID `279f4f0882aae34670de98411079f426` with `05 05 - DMV.mp3` and notes another server track named `09 09 - DMV`.

## Evidence Inventory

| Source | Status | Notes |
| --- | --- | --- |
| `/Users/akartmann/Downloads/05 05 - DMV.mp3` | Available | SHA-256 `0e0ed981ef4ead97d4cd9df6f38474a21202312925e2135ba7904acf7491014a` |
| `/Users/akartmann/Downloads/09 09 - DMV.mp3` | Available | SHA-256 `de67eb8266ccff4eb793a66056f776c589f61d019452b63cdc1b8da946d5a647` |
| Local encoder and sync/provider source | Available | Traced validator, input selection, staging, and ID routing |
| Controlled FFmpeg runtime | Available | Probed, decoded, and re-encoded both inputs; see reproduction details |
| Full daemon log and affected executable version/profile | Missing | Exact error and item ID supplied in the user message only |
| Server response mapping the item ID | Missing | Cannot independently establish which original belongs to the logged ID |
| Other failed files | Missing | Cannot attribute every reported failure to this mechanism |

## Investigation Backlog

| # | Path to Explore | Priority | Status | Notes |
| --- | --- | --- | --- | --- |
| 1 | Compare both files’ duration, tags, and full decoded sample counts | High | Done | Both estimates exceed decoded length by over three seconds |
| 2 | Isolate the cause of the inaccurate estimate | High | Done | Removing APEv2 metadata in temporary copies corrects estimates without changing decoded PCM |
| 3 | Trace source selection and expected duration | High | Done | ID-based download; validator probes the staged input directly |
| 4 | Re-encode both files with the controlled runtime | High | Done | MP3 192 kbps outputs match full decoded source duration |
| 5 | Reproduce through the exact daemon encoder and affected settings | Medium | Open — implementation verification | CLI reproduction is recorded; no temporary Rust code or production code was added |
| 6 | Confirm the supplied ID mapping and remaining failed items | Medium | Blocked on evidence | Requires sanitized server item metadata and affected run logs/files |

## Timeline of Events

| Time | Event | Source | Confidence |
| --- | --- | --- | --- |
| Latest sync (time unavailable) | Multiple local encoding duration errors reported | User message | User-reported |
| 2026-10-22 | Exact validation located | `hifimule-daemon/src/transcoding/local.rs:1156` | Confirmed |
| 2026-10-22 | Both files decoded completely and re-encoded using controlled FFmpeg 9.0.2 | `local-mp3-duration-dmv-evidence.json` | Confirmed |
| 2026-10-22 | APE tag removal isolated inflated input estimates; decoded PCM hashes remained identical | Same evidence artifact | Confirmed |

## Confirmed Findings

### Finding 1: Duration validation trusts the probed input estimate

**Evidence:** `hifimule-daemon/src/transcoding/local.rs:668`, `:689`, `:1104`, `:1156`.

The encoder reads `ictx.duration()` from the actual staged source. After completing the output, it reopens that output and fails when both durations are positive and differ by more than 2,000,000 microseconds. It does not check whether the input duration was estimated from bitrate. Server duration is not an input to this comparison.

### Finding 2: Both files independently exhibit inflated estimates

**Evidence:** `local-mp3-duration-dmv-evidence.json`; full decoded sample counts and controlled-runtime probes.

| File | Tagged album / track | Probed source duration | Decoded audio / re-encoded MP3 duration | Difference |
| --- | --- | ---: | ---: | ---: |
| `05 05 - DMV.mp3` | Primus — Pork Soda (1993), 5/15 | 301.962438 s | 298.840816 s | 3.121622 s |
| `09 09 - DMV.mp3` | Primus — They Can’t All Be Zingers (2006), 9/16 | 300.052625 s | 296.855510 s | 3.197115 s |

Both inputs are 44,100 Hz stereo MP3s with 128 kbps audio packets. FFmpeg explicitly warns: `Estimating duration from bitrate, this may be inaccurate`. Full decoding yields 13,178,880 and 13,091,328 audio sample frames respectively. The controlled-runtime 192 kbps re-encodes report the decoded source duration, rather than the inflated input estimate.

### Finding 3: Trailing APEv2 tags cause the estimate inflation

**Evidence:** Binary header/footer inspection and the metadata-removal experiment recorded in `local-mp3-duration-dmv-evidence.json`.

| File | First audio packet | End of final audio packet / APE header | APE tag bytes including header/footer | Following ID3v1 | Estimate after removing APE only |
| --- | ---: | ---: | ---: | ---: | ---: |
| 05 | 88,955 | 4,870,408 | 49,818 | 128 bytes | 298.848813 s |
| 09 | 80,269 | 4,829,957 | 51,026 | 128 bytes | 296.863500 s |

Each APEv2 footer has version 2000, 19 items, and a matching preceding header. Only this tag was omitted in temporary copies; leading ID3 tags, all MPEG audio frames, and the final ID3v1 tag were retained. The source estimate falls by exactly `APE_bytes / 16,000`: 3.113625 s and 3.189125 s. The remaining difference from decoded length is about 8 ms.

The original and tag-removed copy decode to byte-identical stereo PCM16, with identical sample counts. Their PCM SHA-256 hashes are:

- 05: `be3ec1f2a24ff267aae8d33dbcc403c75e6fc6299ded56d97ac794f2cfeaefc5`
- 09: `401ee5880c67e7911da150242ec3ff1600fad34bac5f87a659da26c8a90698b6`

This experiment refutes the theory that the extra three seconds represent playable audio lost during the CLI re-encode. Original files were never modified.

### Finding 4: Matching names do not select the encoding input

**Evidence:** Delegated source trace with citations below.

- Sync downloads by `item.id`; local encoding requests the original rather than a server transcode: `hifimule-daemon/src/sync.rs:3605`, `:3636`, `:3653`.
- Subsonic uses `download?id=<song_id>`: `hifimule-daemon/src/providers/subsonic.rs:711`, `:1604`. Jellyfin uses `/Items/<song_id>/Download`: `hifimule-daemon/src/providers/jellyfin.rs:759`, `:776`.
- Plan entries retain the item and server IDs; execution resolves the appropriate provider: `hifimule-daemon/src/sync_plan.rs:145`, `:185`, `:293`, `:461`; `hifimule-daemon/src/sync.rs:4364`, `:4383`.
- Staged filenames use a producer sequence index and sanitized item ID inside a fresh temporary directory: `hifimule-daemon/src/sync.rs:4085`, `:4118`, `:4277`, `:4293`.
- The owned staged path travels with its track into the encoding task: `hifimule-daemon/src/sync.rs:2937`, `:2949`, `:5266`.
- Server duration informs reservations/logging but is not passed to the encoder validator: `hifimule-daemon/src/sync.rs:1588`, `:1740`, `:2949`, `:2965`, `:3995`.

## Deduced Conclusions

### Deduction 1: These inputs expose a false duration-mismatch rejection

**Based on:** Findings 1–3.

The source duration is an estimate that includes non-audio APE tag bytes. The output duration matches all decoded audio, but differs from that estimate by more than two seconds. Therefore the current validator can reject a correctly duration-preserving conversion for both supplied originals.

### Deduction 2: The second DMV track is independently affected

**Based on:** Findings 2–4.

The supplied files are separate album releases and have different audio lengths. Each produces its own misleading input estimate. An ambiguous title is unnecessary to explain either failure. Even an external server returning the wrong original would still have that returned original measured against its own re-encoding by this validator.

## Hypothesized Paths

### Hypothesis 1: Similar track names cause incorrect source selection

**Status:** Refuted as a direct explanation of this validator failure.

**Initial theory:** The other DMV track’s duration or bytes might be selected by title.

**Would confirm:** Ambiguous title-based source selection or cross-item expected duration.

**Would refute:** ID-based routing and a duration measured from the same file being encoded.

**Resolution:** Source trace confirms ID-based routing and direct probing. Both files also reproduce the metadata estimate problem independently. The actual ID-to-server-item mapping remains unverified.

### Hypothesis 2: Input duration metadata disagrees with decodable audio length

**Status:** Confirmed.

**Would confirm:** Full decoding and re-encoding preserve audio length while the input estimate differs by more than two seconds.

**Would refute:** Decoded source length agrees with the initial estimate.

**Resolution:** Confirmed for both files with full decoding, controlled-runtime re-encoding, APE removal, and identical decoded PCM hashes for original/copy.

### Hypothesis 3: Encoding discards the additional three seconds of real audio

**Status:** Refuted for the controlled-runtime CLI reproduction; exact daemon execution remains untested.

**Would confirm:** A decoded source longer than the encoded output.

**Would refute:** Full decoded source sample duration matches output duration and removal of metadata alone corrects input estimate.

**Resolution:** Both refutation criteria hold in the controlled-runtime experiment. No production encoder changes were made, and the exact failing daemon build/profile is unavailable.

### Hypothesis 4: Every reported sync failure has the same cause

**Status:** Open.

**Would confirm:** Other affected inputs exhibit estimated durations inflated by non-audio metadata and preserved decoded audio lengths.

**Would refute:** Other affected inputs have accurate durations and genuinely truncated encoded outputs, or different errors.

**Resolution:** Only the two supplied files were tested. Do not generalize to all failed tracks without sampling their evidence.

## Missing Evidence

| Gap | Impact | How to Obtain |
| --- | --- | --- |
| Full daemon log and executable version/profile | Exact runtime attribution and exact settings reproduction | Sanitized affected-run excerpt plus version and local profile |
| Server item response for `279f4f0882aae34670de98411079f426` | Independently map the ID to one supplied original | Relevant provider response including ID, album, track number, and duration |
| Other failed originals | Establish prevalence of this mechanism | Inspect representative failures and compare estimated/decoded durations |

## Source Code Trace

| Element | Detail |
| --- | --- |
| Error origin | `hifimule-daemon/src/transcoding/local.rs:1159` |
| Original selection | ID-based provider download in `hifimule-daemon/src/sync.rs:3605` |
| Input probe | `hifimule-daemon/src/transcoding/local.rs:668` |
| Expected duration | `ictx.duration()` at `hifimule-daemon/src/transcoding/local.rs:689` |
| Output probe | `hifimule-daemon/src/transcoding/local.rs:1104` |
| Failure condition | Positive source/output durations differ by more than two seconds, `hifimule-daemon/src/transcoding/local.rs:1156` |
| Cleanup | `encode_with_report` removes failed output at `hifimule-daemon/src/transcoding/local.rs:633` |
| Outer sync error | `hifimule-daemon/src/sync.rs:2988` |

## Conclusion

**Confidence:** High for the file-level cause and the current validator’s false-rejection mechanism. Exact incident attribution remains limited by missing daemon logs/build/profile and server ID mapping.

Both DMV files carry roughly 50 KB trailing APEv2 tags that are counted in bitrate-based duration estimates. The current validator treats these estimates as exact audio lengths, so correct re-encodes exceed its two-second tolerance. The source trace and independent file experiments refute same-title confusion as the explanation for this comparison. This is a duration-validation defect exposed by these metadata layouts, rather than evidence that the CLI conversion lost audio.

## Recommended Next Steps

### Fix direction

Use `bmad-quick-dev` to correct duration validation. Establish expected audio length from decoded/resampled samples when the input duration is only an estimate, account for codec delay/padding, and retain a check that detects genuine truncated output. Keep decode-recovery limits distinct. Simply increasing the two-second threshold would leave the underlying estimate problem dependent on tag size. Do not make library-file mutation part of the solution.

### Diagnostic

Verify the fix through the actual daemon encoder with a deterministic synthetic MP3 fixture containing a sufficiently large APEv2 tail and no reliable frame-count duration header. Assert preservation of audio duration; separately verify that genuine truncation remains rejected. Add diagnostic detail to the duration error (expected duration, output duration, estimate method, difference) so future failures are attributable.

For incident-wide attribution, inspect representative additional failed items and the sanitized server mapping for the provided ID.

## Reproduction Plan

1. Confirm the two source SHA-256 hashes listed above.
2. Use the repository’s `target/audio-runtime/ffmpeg-9.0.2-aarch64-apple-darwin/bin/ffprobe` and `ffmpeg`. On this macOS build, set `DYLD_LIBRARY_PATH` to that runtime’s `lib` directory for child processes; direct execution otherwise references a stale build staging path.
3. Probe each original with `ffprobe -v warning -show_entries format=duration,size -of json INPUT`.
4. Decode all audio to PCM16 WAV with `ffmpeg -v error -i INPUT -map 0:a:0 -map_metadata -1 -c:a pcm_s16le OUTPUT.wav`; count WAV sample frames and divide by sample rate.
5. Re-encode with `ffmpeg -v warning -i INPUT -map 0:a:0 -map_metadata -1 -c:a libmp3lame -b:a 192k OUTPUT.mp3`, then probe it. The original/output differences exceed two seconds for both files. This reproduces the predicate through the same controlled FFmpeg runtime, not an invocation of the Rust daemon encoder itself.
6. In temporary copies only, omit the APE header/items/footer between the offsets in Finding 3, retaining all audio and both ID3 tag regions. Probe/decode again. The estimates now agree within about 8 ms, and decoded PCM sample counts/hashes match each original.
7. Temporary audio outputs/copies were automatically removed after the experiment. Only this report and numeric/hash evidence are retained in the repository.

## Side Findings

- The controlled runtime intentionally lacks the JPEG decoder; CLI probing warns about unspecified attached-artwork dimensions. The native encoder copies artwork and reads dimensions itself (`hifimule-daemon/src/transcoding/local.rs:317`, `:867`). Audio decoding and the duration experiment completed successfully; this warning does not explain the demonstrated three-second estimate inflation.
- The controlled-runtime CLI binaries require the same library-path environment used by the project build wrapper (`scripts/build-daemon.mjs:76`) in this local checkout. No build/runtime files were changed.

## Follow-up: 2026-10-22 — implementation verified

The user requested implementation after the diagnosis. `hifimule-daemon/src/transcoding/local.rs` now uses successfully decoded sample counts at the source rate for bitrate-estimated or unavailable input durations. It retains reliable-header duration checks, the two-second tolerance, and failed-output cleanup; mismatch errors include expected/output seconds.

Both supplied originals passed native 192 kbps MP3 conversion: 05 at 298.840816 seconds (7,257,564 bytes), 09 at 296.855510 seconds (7,201,325 bytes). Both reported zero invalid decode events/rejected bytes, and their original SHA-256 hashes remain unchanged. The temporary real-file test harness was removed. Permanent tests use synthetic audio only.

The controlled-runtime daemon suite passed with localhost mock servers permitted: 1,661 unit tests plus 5 integration tests, zero failures, 14 ignored. All three independent reviews returned no actionable findings. See `../spec-fix-local-encoding-estimated-duration.md` for implementation verification and suggested review order. The missing incident-wide ID mapping and other-file evidence are unchanged; they are unnecessary to verify the supplied-file fix.
