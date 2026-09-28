# Story 16.5: Match Radio track loudness using available metadata

Status: ready-for-dev

## Story

As a HifiMule user,
I want Radio to use available track loudness metadata,
so that music from different albums has more consistent listening levels without compressing its dynamics.

**Requirements:** Radio portion of FR74; P-NFR1–2 and P-NFR4; loudness portion of P-AR7. **Dependencies:** Epic 15 playback foundation, especially Story 15.10, and Stories 16.1–16.4. This story adds a Radio policy to the existing gain path. It does not add loudness analysis, dynamic compression, crossfade, a gain settings UI, source reporting, or adaptive quality.

## Acceptance Criteria

1. **Track adjustment.** Given a Radio occurrence with valid track gain and peak evidence for its chosen source and decoded representation, preparation derives one track-level adjustment from the documented ReplayGain reference and static peak rule. It applies exactly once to that occurrence's decoded audio, including decoder drain and resampler tail.
2. **Peak protection.** If requested gain would exceed the supported sample-peak ceiling, the existing static rule reduces the scalar. The guarantee is limited to the declared decoded sample-peak reference and qualified signal path; it is not a true-peak, resampled-output, device-volume, or analog-output guarantee.
3. **Safe fallback.** Missing, partial, malformed, conflicting, unsupported, or representation-incompatible evidence leaves gain exactly at unity without rejecting an otherwise playable track. Album gain/peak, `fallbackGain`, embedded tags, unknown loudness units, and guessed loudness are not substituted for missing track evidence.
4. **Occurrence boundaries.** Consecutive Radio tracks can have different frozen gains. Each applies only to its own samples, with no extra silence, overlap, duplicate/drop, or double scaling at prepared handoff, replay, seek, and decoder flush. Different source copies of one recording use the chosen occurrence's source evidence.
5. **Mode separation.** Explicit album sessions retain Story 15.10's common album gain; standalone manual track playback and Preview use their existing unity policy for this story. Radio context does not leak into an audition, and Preview/Return restores the preserved Radio occurrence and its gain without applying it twice. The session mode, not the browsed server/album, selects policy.
6. **Lifecycle fencing.** Replacement, queue edit, seek, Pause/Resume, Back, Retry, output recreation, Preview/Return, and restart keep or recompute only the active occurrence's validated policy according to a versioned, documented freeze rule. Stale provider results or prepared audio cannot alter a newer generation. Restoration remains paused until an explicit Resume.
7. **Bounded implementation.** Metadata resolution is limited to the chosen track/source and existing bounded preparation path. No library scan/download or extra callback work occurs. Audio callbacks perform no allocation, blocking I/O, database/provider call, or sync-held lock because of this feature.
8. **Evidence.** Deterministic valid/invalid/missing metadata, distinct source copies, boundary, album, Preview, and restore fixtures verify scalar math and production audio on Windows, macOS, and Linux. Record tested runtime/architecture and numerical tolerance; distinguish digital fixture results from installed and physical output evidence. Do not claim equal perceived loudness for inaccurate or unavailable metadata.

## Tasks / Subtasks

- [ ] Lock the track policy contract before wiring playback (AC: 1–3, 5).
  - [ ] Define the supported OpenSubsonic `Child.replayGain.trackGain` dB adjustment and `trackPeak` linear sample peak pair, conventional ReplayGain reference (89 dB SPL), zero preamp, accepted provider capability and original representation baseline. Use the supplied adjustment; do not treat it as raw measured LUFS or convert it between reference scales.
  - [ ] Reuse Story 15.10's accepted bounds (`gain ∈ [-60,+30]` dB, `peak ∈ (0,64]`), `SAMPLE_PEAK_CEILING = 10^(-1/20)`, `min(10^(gain/20), ceiling/peak)`, finite checks, and downward f32 rounding. Factor shared math/format qualification instead of cloning it. Keep album-specific consistency checks album-only.
  - [ ] Specify frozen-policy version, provenance/reason, source and occurrence identity, and the safe unity result for every unsupported or invalid case. Decide and document exactly when a policy is frozen and how old Radio sessions without it restore.
- [ ] Retain and validate optional track evidence through the existing provider boundary (AC: 1–3, 4, 7).
  - [ ] Add an equality-safe, bounded, daemon-private track evidence type beside `AlbumLoudnessEvidence` on `Song`; keep public `Song` JSON and provider credentials unchanged. Parse `trackGain`/`trackPeak` independently of album fields from tolerant raw OpenSubsonic `replayGain` JSON. A malformed optional field must not invalidate the song DTO or its album evidence.
  - [ ] Apply the same verified OpenSubsonic capability gate to both `get_song`/`resolve_playback` and album/list paths. Classic Subsonic, Jellyfin, Audiobookshelf and embedded-only values remain unity until an equivalent gain/peak/reference/decoder-baseline contract is separately established. Nonzero or invalid `baseGain` is not silently combined; preserve the codec's mandatory output gain in normal decoding.
  - [ ] Bind evidence to the source copy returned by `resolve_playback`, not recording identity or currently browsed server. The selected original representation must match the qualified suffix/container/codec; a later contradiction yields recoverable preparation failure for frozen non-unity policy, not unsafe gain or automatic source replacement.
- [ ] Integrate a frozen Radio policy with session ownership and audio preparation (AC: 1, 4–7).
  - [ ] Reuse `playback/loudness.rs` math, `audio.rs::qualified_gain_suffix`, `decoder.rs::decode_stream_with_seek_and_gain`, and both CPAL/Pulse preparation paths. Scale packed f32 after swresample and before PCM enqueue, including flush; preserve unity's bit-exact bypass and submitted-tail replay without rescaling.
  - [ ] Dispatch by queue/session mode and occurrence: album member → common album policy; Radio occurrence → its own track policy; manual/Preview/nonmember → unity. Pass the selected policy and admitted representation through initial, successor, seek, Back, Retry, output recreation and Preview return. Keep the existing generation/control-epoch/occurrence checks before publishing audio or persisting a policy.
  - [ ] Persist only the minimal versioned, validated frozen policy needed for accepted Radio occurrences. If the SQLite schema changes, migrate schema 10 transactionally, preserve queue, source references, recording associations, heard/excluded state, album context and command outcomes, and reject corrupt/future policy safely. A metadata fetch failure must not erase an accepted occurrence or its recoverable session.
  - [ ] Preserve bounded five-upcoming Radio queue, two audio slots, existing compressed/PCM caps and preparation deadlines. No new global cache, provider-side library analysis, callback DSP, limiter or crossfade.
- [ ] Verify source-to-sample behavior and regression gates (AC: 1–8).
  - [ ] Test raw OpenSubsonic metadata parsing: valid zero/positive/negative gain, peak >1, cap reduction, partial/malformed/overflow/non-finite values, nonzero `baseGain`, absent capability, album-only values and conflicting track tags. Confirm malformed track evidence leaves album behavior intact and vice versa.
  - [ ] Test owner/admission and SQLite migration/reopen: source-copy-specific policy, identical track IDs on different servers, manual repetitions, new Radio, album/standalone/Preview dispatch, paused restoration, stale resolve after replacement/seek/queue edit, and corrupted/future policy. Exercise CPAL and Pulse call sites.
  - [ ] Use two known-level Radio tracks with distinct gain/peak tags through provider → prepared decoder → boundary consumer; compare independent expected scalars and packed-f32 samples, including tail, seek and Pause replay. Assert exact frame/boundary markers and no accidental gain-square. Test album session on the same fixtures to prove common gain persists.
  - [ ] Run focused and full affected daemon tests via the controlled `npm run build:daemon -- test -p hifimule-daemon ...` wrapper, formatting, Clippy, and four shipping-platform source/fixture jobs. If UI/wire contracts change, run TypeScript/build and localization parity. Record actual installed/physical-output tests separately and leave unrun rows unverified.

## Dev Notes

### Current code, changes and preservation map

| UPDATE file/boundary | Current behavior | Story 16.5 change and preservation requirement |
| --- | --- | --- |
| `hifimule-daemon/src/domain/models.rs` | `Song` has private `AlbumLoudnessEvidence`; public wire excludes it and structs retain `Eq`. | Add small private track evidence with absent/rejected distinction and equality-safe finite values; keep old JSON and all constructors compatible. |
| `hifimule-daemon/src/providers/subsonic.rs` | `SongDto.replay_gain` is tolerant raw JSON; `parse_album_loudness` checks `baseGain`, `albumGain` and `albumPeak`. `get_album` gates OpenSubsonic metadata, while `resolve_playback` obtains `get_song`. | Parse independent track pair, gate every exposed path, retain chosen song/source. Keep album parsing, auth, ordinary song playback and classic-server fallback. |
| `hifimule-daemon/src/playback/loudness.rs` | Versioned album policy validates evidence and a reproducible f32 scalar; static sample-peak rule and original-format qualification exist. | Extract shared bounded scalar math and add a distinct track policy/reason. Do not reuse album-wide completeness/tolerance as a track rule or change album results. |
| `hifimule-daemon/src/playback/{model,session,persistence}.rs` | `Occurrence` has source/occurrence identity but no Song metadata. Owner picks album scalar for album members and unity otherwise; SQLite schema 10 persists album context and Radio identity/membership. | Store minimal per-occurrence frozen Radio policy privately; dispatch by mode, validate on restore and preserve accepted queue/source/history. Version any durable change and fence all owner mutations. Avoid changing public snapshot schema unless required. |
| `hifimule-daemon/src/playback/{commands,audio}.rs` and `audio/pulse_output.rs` | Initial and successor paths resolve chosen source; `qualified_gain_suffix` checks original representation for non-unity; generation and control epoch fence preparation; CPAL/Pulse use candidate gain. | Supply correct frozen Radio gain/suffix at all preparation sites. Preserve failure/retry semantics, selected endpoint, output-loss inhibition, source routing and existing deadlines. |
| `hifimule-daemon/src/playback/decoder.rs` | Single worker applies scalar once to converted frames and resampler flush; unity bypasses multiplication. | Reuse unchanged if possible. Preserve frame count, padding/seek behavior, PCM bounds and callback-free gain computation. |

**Signal contract.** The supported v1 track evidence is the OpenSubsonic numeric track ReplayGain pair for the original qualified WAV/FLAC/M4A/MP3 representation, with `baseGain` absent or zero under the established album policy. ReplayGain is a supplied relative adjustment against its reference; no EBU `loudnorm` filter or extra reference conversion is needed. Opus, transcodes, unsupported suffix/content type, and provider values with unverified peak conventions use unity. `qualified_gain_suffix` and decoder container/codec checks already guard non-unity album playback; extend their use to track policy. Sample peak metadata cannot prove intersample true peak or universal converted-output safety. [Source: `15-10-preserve-an-album-s-relative-loudness-with-consistent-gain.md` §Implementation contract; `playback/loudness.rs`; `playback/audio.rs`; `playback/decoder.rs`; [OpenSubsonic ReplayGain](https://opensubsonic.netlify.app/docs/responses/replaygain/)]

**Freeze and restore.** Existing `Occurrence` is a wire-level source/ordinal record, whereas the prepared gain is currently projected from album context into `current_gain_bits`. The implementation must bind a Radio policy to the specific accepted occurrence and source before its audio can publish, then persist enough to restore that decision paused. A later metadata response cannot retroactively change an already prepared/played occurrence. Failed fetches use a validated unity fallback only when no frozen non-unity policy exists; a frozen non-unity occurrence with an incompatible later representation must fail recoverably rather than silently change its baseline. Keep the original server/track reference authoritative even when 16.4 groups duplicate recordings. [Source: `playback/model.rs` §Occurrence/PersistedSession; `playback/session.rs` §set_current_policy; `playback/commands.rs` preparation; Story 16.4 §Review Resolution]

**Previous-story intelligence.** Story 16.4 added private versioned recording evidence, deterministic source-copy selection and SQLite schema 10; six review fixes tightened conflict handling, source ranking and artist anchoring. Its last source CI predates those fixes, so rerun affected four-platform fixtures on this story's final commit. Story 15.10's code provides the gain primitive, but its narrative still leaves installed/native platform evidence open; do not inherit it as a verified result. Current latest commits: `7aa1ab8 Review 16.4`, `3988baa Implement recording-aware Radio selection`, `0421475 Story 16.4`, `fb762b6 Review 16.3`, `6722df2 Record story 16.3 CI evidence and review status`. [Source: Story 16.4 §§Review Findings, Dev Agent Record; Story 15.10 §Review Resolution; Git history]

**Libraries and current external contract (checked 2026-09-28).** Use repository pins and controlled runtime: Rust edition 2024/MSRV 1.93.0, FFmpeg 9.0.1 via `ffmpeg-next/sys` 9.0.0, patched CPAL 0.18.2 and existing Pulse backend. The OpenSubsonic [ReplayGain response](https://opensubsonic.netlify.app/docs/responses/replaygain/) defines optional `trackGain` in dB, positive `trackPeak`, and separate `albumGain`, `albumPeak`, `baseGain`, `fallbackGain`; [Child](https://opensubsonic.netlify.app/docs/responses/child/) carries optional `replayGain`. This confirms field shape, not installed-server coverage or signal baseline. Consume a qualified provider's supplied dB adjustment directly rather than launching a new analyzer or converting between loudness references. No dependency upgrade is needed for this story.

### Project Structure Notes

Keep provider parsing in `providers/`, metadata and shared gain math in `domain/models.rs` and `playback/loudness.rs`, session ownership and durability in `playback/session.rs` and `playback/persistence.rs`, and native preparation in `playback/{commands,audio}.rs`. Rust/SQL use snake_case; JSON uses camelCase. Keep credentials and stream URLs daemon-side. The base UX specification predates Playback and does not require a new control for this policy. [Source: `_bmad-output/planning-artifacts/architecture.md` §§Playback Audio Pipeline, Provider Integration, Radio Selection; `_bmad-output/implementation-artifacts/epic-16-context.md`]

### References

- `_bmad-output/planning-artifacts/epics.md` — Epic 16, Story 16.5 acceptance criteria and implementation gate; P-AR7.
- `_bmad-output/planning-artifacts/prd.md` — FR74, P-NFR1–2 and P-NFR4.
- `_bmad-output/planning-artifacts/architecture.md` — Playback Audio Pipeline, Provider Integration, Radio Selection and Validation Refinements.
- `_bmad-output/implementation-artifacts/epic-16-context.md` — sequence, source identity and bounded state.
- `_bmad-output/implementation-artifacts/15-10-preserve-an-album-s-relative-loudness-with-consistent-gain.md` — reusable signal/metadata contract and evidence limits.
- `_bmad-output/implementation-artifacts/16-4-avoid-duplicate-radio-recordings-across-configured-servers.md` — current Radio state, persistence and review intelligence.
- `docs/api-contracts-hifimule-daemon.md`, `docs/playback-installed-test-checklist.md` — update implementation contract/evidence where applicable.

## Dev Agent Record

### Agent Model Used

GPT-6 Codex (story preparation)

### Debug Log References

- Story creation inspected planning artifacts, current gain/provider/session code and recent Git history; no implementation or platform test was run.

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.

### File List

- `_bmad-output/implementation-artifacts/16-5-match-radio-track-loudness-using-available-metadata.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
