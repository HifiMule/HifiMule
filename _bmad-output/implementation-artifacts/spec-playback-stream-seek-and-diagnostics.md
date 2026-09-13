---
title: 'Fix bounded MP3 playback and expose actionable playback diagnostics'
type: 'bugfix'
created: '2026-09-13'
status: 'done'
baseline_commit: '6f7a32b02c45e28819a48688423d5afeb3e0089d'
context:
  - '{project-root}/_bmad-output/planning-artifacts/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/investigations/audio-container-playback-investigation.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The production bounded reader advertises incomplete random access to MP3 demuxing, causing a valid 360-second MP3 larger than its 8 MiB window to decode only 1.124 seconds. Live M4A failures are reported as `OUTPUT_UNAVAILABLE` using message substring matching, while the underlying native error is discarded, preventing diagnosis.

**Approach:** Treat reliably identified MP3 streams as sequential inputs while retaining bounded memory and existing decode-and-discard resume behavior. Replace substring-based failure classification with typed pipeline stages and sanitized diagnostic logging, without changing the public playback failure schema.

## Boundaries & Constraints

**Always:** Keep compressed/PCM memory bounded; retain M4A/MP4 seekability because tail-`moov` inputs require random access; preserve cancellation and generation fencing; use the controlled FFmpeg runtime; log only sanitized stage, stable code, representation, and error chain; keep public `{ code, retryable }` compatibility.

**Ask First:** Any public RPC/schema change, persistent diagnostic storage, authenticated HTTP range implementation, or change to resume semantics.

**Never:** Increase the 8 MiB cap as the fix; make all containers sequential; log URLs, headers, credentials, response bodies, titles, or track/server IDs; commit the supplied audio files; claim the M4A cause before a live typed diagnostic identifies it.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|---------------------------|----------------|
| Oversized MP3 | MP3 exceeds 8 MiB and contains end metadata/artwork | Sequential production decode reaches the complete stream duration with bounded memory | Decode failure remains `DECODE_FAILED` with sanitized `decode` stage |
| Ordinary MP3 | ID3-tagged, raw MPEG-frame, or `.mp3`-hinted stream | Recognized as sequential without misclassifying ADTS AAC | Unknown input retains existing container behavior |
| M4A playback failure | Live provider or CPAL path fails | Public failure stays stable; daemon log identifies sanitized stage and chain | Provider, timeout, decode, output-open, and output-lost remain distinct |
| Misleading text | Decode error text contains `output` | Classified as `DECODE_FAILED`, not `OUTPUT_UNAVAILABLE` | No substring-based routing |
| Cancellation | Stop/replacement supersedes worker | No stale or duplicate failure diagnostic is published | Existing generation fence remains authoritative |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/playback/decoder.rs` -- Chooses seekable versus sequential FFmpeg I/O and contains real decode regressions.
- `hifimule-daemon/src/playback/audio.rs` -- Owns fetch, decoder worker, CPAL output, failure classification, and diagnostic emission.
- `hifimule-daemon/src/playback/streaming.rs` -- Defines the bounded 8 MiB sliding window; behavior is preserved and reused.
- `hifimule-daemon/src/playback/session.rs` -- Provides the authoritative generation lock used to fence diagnostic emission.
- `hifimule-daemon/src/rpc.rs` -- Resolves/starts playback and contains a second substring-based classifier.
- `hifimule-daemon/src/providers/mod.rs` -- Contains the existing credential-safe message sanitizer.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/playback/decoder.rs` -- add failing oversized-MP3 regression coverage, robust MP3 recognition, and sequential FFmpeg input selection.
- [x] `hifimule-daemon/src/playback/audio.rs` -- add typed pipeline errors, contextual stage mapping, sanitized logging, and classification tests.
- [x] `hifimule-daemon/src/rpc.rs` -- remove resolution/start substring routing and consume stable typed failure classification.
- [x] `hifimule-daemon/src/providers/mod.rs` -- expose and reuse the existing sanitizer within the crate, retaining redaction tests.
- [x] `_bmad-output/implementation-artifacts/investigations/audio-container-playback-investigation.md` -- record implementation and verification evidence.

**Acceptance Criteria:**
- Given a valid MP3 larger than 8 MiB, when decoded through `BoundedHttpReader`, then output is complete rather than approximately one second and retained compressed memory remains capped.
- Given a decode error containing `output`, when surfaced, then its public code is `DECODE_FAILED` and its sanitized log identifies the decode stage.
- Given an actual output-open or output-loss error, when surfaced, then it receives the correct stable output code without inspecting message text.
- Given either supplied M4A, when passed through production `decode_stream`, then it continues to decode successfully; live errors become attributable on the next application reproduction.

## Spec Change Log

## Design Notes

MP3 detection should combine normalized filename/container hints with byte signatures: ID3 and a validated MPEG audio frame header. A lone `0xff` is insufficient because it can collide with ADTS AAC. Keep the public session model unchanged; diagnostics belong at internal catch points and must reuse the provider sanitizer.

Implementation retains source read detail in a shared typed failure state because FFmpeg reduces custom-AVIO failures to a generic errno. This state is read only after the decoder completes and does not enter the audio callback.

Verification note: the strict `-D warnings` clippy gate is blocked by 100+ pre-existing repository warnings outside this change. Clippy without warning promotion exits successfully and reports no warning in the changed playback/provider code; full details remain in the task transcript.

## Verification

**Commands:**
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon playback::decoder::tests -- --nocapture` -- decoder regressions pass under controlled FFmpeg.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon playback::audio::tests` -- typed mapping and sanitization tests pass.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon` -- complete daemon suite passes: 727 passed, 0 failed, 3 environment-gated tests ignored.
- PowerShell with `HIFIMULE_DIAGNOSTIC_MP3=C:\Users\alexi\Downloads\03 That Time Of The Night (The Short.mp3`, then `rtk node scripts/build-daemon.mjs test -p hifimule-daemon oversized_mp3_decodes_complete_stream_with_bounded_memory -- --ignored --nocapture` -- 1 passed, including the 8 MiB high-water assertion.
- The same environment value, then `rtk node scripts/build-daemon.mjs test -p hifimule-daemon oversized_id3_mp3_without_filename_hint_decodes_complete_stream -- --ignored --nocapture` -- 1 passed without a filename hint.
- PowerShell with `HIFIMULE_DIAGNOSTIC_FLAC` set in turn to `C:\Users\alexi\Downloads\11 Let It Ride.m4a` and `C:\Users\alexi\Downloads\1-01 L'invitation.m4a`, then `rtk node scripts/build-daemon.mjs test -p hifimule-daemon diagnostic_external_flac -- --ignored --nocapture` -- both production decoder runs passed.
- `rtk node scripts/build-daemon.mjs clippy -p hifimule-daemon --all-targets` -- exits 0; strict warning promotion remains blocked by 100+ pre-existing repository warnings.
- `rtk cargo fmt --all -- --check` -- formatting passes.
- Environment-gated production decoder test against the supplied MP3 -- reports full-duration frames after the fix; both M4As remain passing.

## Review Resolution

- Three independent reviews completed: blind adversarial diff review, exhaustive edge-case review, and acceptance audit.
- Fixed review findings: generation-atomic diagnostic logging, typed resume diagnostics while preserving `RESUME_UNAVAILABLE`, decoder cancellation/join on early output errors, mixed-case URL and content-type handling, ambiguous `.mpeg` hints, and large ID3 probing within the unchanged bounded window.
- Rejected as outside or contrary to the frozen contract: changing established public retry semantics, introducing new public failure codes, and classifying the controlled FFmpeg runtime outside the required stage set.

## Suggested Review Order

**Playback failure attribution**

- Start here: typed logging and publication preserve stable public failures without substring routing.
  [`audio.rs:229`](../../hifimule-daemon/src/playback/audio.rs#L229)

- Generation locking prevents stale diagnostics during rapid playback replacement.
  [`session.rs:328`](../../hifimule-daemon/src/playback/session.rs#L328)

- Resume keeps `RESUME_UNAVAILABLE` while retaining the underlying typed diagnostic.
  [`rpc.rs:704`](../../hifimule-daemon/src/rpc.rs#L704)

**Bounded container handling**

- MP3 recognition combines unambiguous hints with bounded byte-signature validation.
  [`decoder.rs:60`](../../hifimule-daemon/src/playback/decoder.rs#L60)

- Shared typed reader state survives FFmpeg's custom-I/O errno reduction.
  [`streaming.rs:54`](../../hifimule-daemon/src/playback/streaming.rs#L54)

- Output lifecycle exits cancel and join decoder workers before returning failures.
  [`audio.rs:625`](../../hifimule-daemon/src/playback/audio.rs#L625)

**Sanitization and regression evidence**

- Diagnostic sanitization handles URLs, credentials, titles, and playback identifiers.
  [`mod.rs:657`](../../hifimule-daemon/src/providers/mod.rs#L657)

- Supplied oversized MP3 validates complete decoding under the unchanged memory cap.
  [`decoder.rs:341`](../../hifimule-daemon/src/playback/decoder.rs#L341)

- Large extensionless ID3 inputs remain detectable within the bounded window.
  [`decoder.rs:379`](../../hifimule-daemon/src/playback/decoder.rs#L379)

## Live M4A Follow-up

The typed diagnostic isolated the live failure to representation selection: providers label original M4A files with the `m4a` container suffix, while the supported-representation filter previously accepted only underlying codec names such as AAC/ALAC. The filter now accepts `m4a` and `mp4`; its regression test failed with `no supported playback representation` before the fix and passes afterward. Provider tests are 23/23 green and the full daemon suite remains 727 passed, 0 failed, 3 ignored.
