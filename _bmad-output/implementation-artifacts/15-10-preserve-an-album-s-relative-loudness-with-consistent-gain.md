---
baseline_commit: 66c37bb1da77e2a7c0eff4ba284aba6e7c2ef404
---

# Story 15.10: Preserve an album's relative loudness with consistent gain

Status: in-progress

## Story

As a HifiMule user,
I want an album's tracks to share one appropriate loudness adjustment,
so that quiet and loud passages retain their intended relationship while usable metadata helps avoid excessive playback levels.

**Requirements:** Album portion of FR74; continuity regression for FR61; P-NFR1–2 and P-NFR4; loudness portion of P-AR7.

**Dependencies:** Stories 15.1–15.9. Prepared 2026-09-18 against the baseline above. Story 15.9 and sprint tracking are marked done, but its review notes and unchecked tasks still identify missing full Windows/Linux daemon builds, native race checks, successor-source cases and physical continuity captures. Its narrative still says in-progress. These are inherited evidence limitations, not proof of platform acceptance; do not change predecessor status in this story.

**Scope:** One daemon-owned, frozen album adjustment, optional provider metadata normalization, durable album membership/policy, static sample-peak protection and deterministic audio regression. Initial metadata support is the complete OpenSubsonic album ReplayGain pair described below. Jellyfin and embedded-only albums remain playable at unchanged gain when they lack that supported contract. No Radio track normalization (15.19), Preview (15.11), gain settings panel, metadata scanning service, DSP limiter, compression, crossfade, dependency upgrade or sync transcoding change.

## Acceptance Criteria

1. **Common album gain.** Given an album with valid, consistent supported album gain and peak evidence, resolve one adjustment before its first audio and apply it to every original album occurrence. Preserve relative quiet/loud levels; never substitute track gain.
2. **Static peak protection.** Given suggested gain exceeding the supported peak-reference ceiling, reduce the common scalar using the documented static rule. Use the same scalar for every track; no per-track peak correction, limiter or dynamic compression. State the signal domain of the protection claim.
3. **Safe unchanged fallback.** Missing, malformed, non-finite, contradictory, incomplete or unsupported evidence freezes unity for the album. Missing peak evidence cannot produce a metadata-based clipping-safety claim. Metadata failure alone does not reject an otherwise playable album.
4. **Deterministic normalization and precedence.** Document units, reference, supported source, validation tolerances and decoder baseline. Provider values and embedded tags never apply twice. No guessed conversion from unsupported loudness conventions or codec base gain.
5. **Stable session behavior.** Seek, Next, natural handoff, Pause/Resume, source Retry, output recreation and restart retain the same resolved policy for album members. Restart restores paused. Late metadata and stale preparations cannot change an accepted policy or a newer session. Prepared handoff adds no silence, missing samples, duplicate samples or unintended gain step.
6. **Correct conversion and bounded work.** Apply gain once in the worker at the documented point in the existing signal chain, including decoder drain and resampler tail. Callbacks perform no added allocation, blocking IO, database/provider work or sync locking. Metadata resolution does not download/decode the album.
7. **Deterministic evidence.** Known-level albums, silence, excessive gain, valid/invalid/conflicting metadata and lifecycle races verify the common scalar, relative levels, fallback and peak-reference limit against independent calculations. Retain Story 15.9 count/boundary assertions and prove replay is not scaled twice.
8. **Platform evidence.** Run the same production-pipeline fixtures on Windows, macOS and Linux, declaring numerical tolerance and runtime/architecture. Distinguish digital sample results from OS/interface volume or processing and physical-output continuity. Unrun platform checks remain open.

## Tasks / Subtasks

- [x] Lock the metadata/signal contract in executable tests and existing documentation (AC: 1–8).
  - [x] Implement the policy below as a small pure resolver; document supported provider/convention/representation and exact peak-domain limits.
  - [x] Add numerical, malformed-metadata and unchanged-fallback tests before wiring production playback.
- [x] Retain optional album evidence through the provider boundary (AC: 1, 3–4, 6).
  - [x] Parse OpenSubsonic `replayGain` tolerantly; normalize only the supported pair with provenance. Preserve absent versus rejected metadata and existing Song equality/serialization behavior.
  - [x] Resolve across the complete ordered album before RPC conversion discards Song metadata. Preserve source routing, cancellation/deadline, bounded album size and duplicate occurrences.
  - [x] Keep Jellyfin/classic Subsonic/embedded-only/unsupported formats at unity with a typed reason; do not infer metadata coverage from upstream DTOs.
- [x] Persist and fence album policy with its occurrence membership (AC: 1, 3, 5).
  - [x] Add a transactional playback schema migration and validation for policy version, source album identity, membership and frozen scalar/provenance.
  - [x] Commit queue and policy together through existing album reservation/owner flow; failed or superseded admission leaves the previous queue and policy intact.
  - [x] Preserve policy through both restoration paths, seek/retry/Next and successor preparation. Clear/replace it only on successful replacement; appended nonmembers use unity.
- [x] Thread immutable resolved gain through both native audio paths (AC: 2, 4–6).
  - [x] Pass the correct occurrence policy to initial and successor decoder calls in CPAL and Pulse, plus seek/restart preparation.
  - [x] Scale converted packed-f32 frames and flushed tail exactly once before queue insertion; unity takes an unchanged fast path.
  - [x] Preserve callback/replay, output pinning, pause authorization, presentation receipts, clean EOF/error distinction and aggregate buffer bounds.
- [ ] Add integration and regression evidence (AC: 1–8).
  - [x] Exercise provider → album admission → persisted policy → production decoder → boundary consumer, including partial tags, provider/tag duplication and independent source identities.
  - [x] Test migration/rollback/corruption/restart, stale resolution, queue membership and owner-side gain retention across transport paths; native worker recreation remains in the installed-platform gate below.
  - [x] Run controlled-runtime tests, actual platform builds and fixture runs; update installed checklist with gain-specific digital evidence and explicit physical evidence limitations. macOS ARM64 digital checks pass; Windows, Linux, macOS x64 and physical-output evidence remain open.

### Review Findings

Review of `66c37bb..0c18afe`, 2026-09-18. Blind Hunter, Edge Case Hunter and Acceptance Auditor completed; findings were deduplicated and checked against the implementation. All six patch items were applied after user approval, with no decisions or pre-existing deferrals. Two lower-confidence/low-impact candidates were dismissed. The controlled full daemon suite passed: 955 passed, 6 ignored. The initial sandboxed playback run had six local-socket permission failures; the full rerun with socket access passed. Existing unrun platform and physical-output evidence remains open.

- [x] [Review][Patch] **[P1] Retain admitted representation evidence for later preparation.** Qualification uses the newly fetched song suffix, while persisted membership retains only server/track IDs and the scalar. If an admitted FLAC member later resolves as MP3 with matching fresh metadata, preparation and decoder validation accept the new representation with the old peak/gain baseline. Persist sufficient per-member admission evidence and reject contradictory later representations before enqueueing audio; cover Retry and restart as well as initial/successor preparation. AC 2–5. [hifimule-daemon/src/playback/audio.rs:728]
- [x] [Review][Patch] **[P2] Validate returned album identity against the requested album.** The resolver only checks track album IDs against the returned album ID. A coherent response for B to a request for A receives non-unity gain and is persisted under A. Include the requested source identity in qualification and freeze unity on contradiction. AC 1, 3, 5. [hifimule-daemon/src/rpc.rs:923]
- [x] [Review][Patch] **[P2] Enforce the occurrence cap before loudness resolution.** The resolver runs before `order_album_tracks` rejects more than 10,000 occurrences, allocating two response-sized vectors and synchronously scanning otherwise qualifying rows first. Validate the size before resolution and use constant-space extrema accumulation. AC 6. [hifimule-daemon/src/rpc.rs:924]
- [x] [Review][Patch] **[P2] Honor the inclusive gain-consistency boundary.** Valid gains of 1.00 and 1.01 dB produce an f64 difference of `0.010000000000000009`, so the strict comparison incorrectly freezes unity. Account for floating-point subtraction error at the documented inclusive tolerance and test positive, negative and near-boundary values without widening the substantive tolerance. AC 1, 4, 7. [hifimule-daemon/src/playback/loudness.rs:119]
- [x] [Review][Patch] **[P2] Keep representation-qualification failures recoverable.** Non-original, missing/unsupported suffix and contradictory-container paths return `PLAYBACK_UNSUPPORTED` with `retryable: false`, hiding the UI Retry action even if the original representation becomes available again. Use a recoverable preparation failure while preserving the frozen policy and occurrence. The owner Retry RPC remains possible; the defect is the advertised/UI recovery behavior. AC 5 and the representation contract. [hifimule-daemon/src/playback/audio.rs:570]
- [x] [Review][Patch] **[P2] Add the gain integration and lifecycle evidence marked complete.** The added production decoder test uses one WAV and a manually supplied 0.5 scalar; RPC tests deliberately disable audio, and the non-unity owner test covers append/select. These do not exercise the required real adapter → admission → persisted policy → initial/successor decoder → boundary consumer path, embedded/provider duplication, gain during seek/retry/restart, or submitted-tail replay. Add these regressions, including the two-track relative-level and independent peak assertions, and align completed task claims with actual evidence. AC 5, 7. [hifimule-daemon/src/playback/decoder.rs:648]

### Review Resolution

- Persisted the canonical admission suffix for each non-unity member and propagated it through initial/successor, seek, Retry and restart preparation. Representation contradictions are retryable; earlier development v3 non-unity records missing this evidence retain their data and fail restoration rather than guessing a baseline. Legacy v1/v2 unity restore remains supported.
- Centralized ordered admission, checking size before metadata resolution and the returned album identity against the request. Resolution now uses constant-space extrema and a narrow f64 roundoff allowance for inclusive tolerances.
- Added real OpenSubsonic → admission → SQLite reopen → FLAC decode → boundary/replay evidence, independent gain/peak and relative-level checks, embedded-tag conflict checks, converted-tail fixtures and owner lifecycle tests.
- The new Next regression exposed a stale predecessor policy in the precommit terminal response. The response now projects both gain and format from its destination occurrence, including unity for appended nonmembers.
- Validation: 257 playback tests passed, 6 ignored; full controlled daemon suite 963 passed, 6 ignored; installed-evidence validator 33 passed; macOS ARM64 daemon build, formatting and diff checks passed; Clippy completed with existing repository warnings. Follow-up edge/acceptance review found no additional correctness bug. Actual native worker recreation and unrun platform/physical evidence remain open, so story and sprint status remain `in-progress` rather than claiming AC8 completion.

## Dev Notes

### Implementation contract: metadata and common gain

These are conservative Story 15.10 implementation choices, not claims that every server or file supplies ReplayGain. No new user preference is needed.

**Supported input:** OpenSubsonic `Child.replayGain.albumGain` (numeric dB adjustment in the provider's standard ReplayGain convention) plus `albumPeak` (positive linear sample amplitude, 1.0 = full scale), obtained through the existing `MediaProvider::get_album`. Use the conventional ReplayGain reference of 89 dB SPL with zero additional preamp; consume the supplied dB adjustment directly, without treating it as measured LUFS or recalculating loudness. The selected adapter/representation must establish that peak and gain refer to the same decoded baseline. Do not treat every server response containing similar field names as independently verified metadata coverage.

**Unsupported in v1:** trackGain/trackPeak substitution, fallbackGain, R128 tags, Sound Check/iTunNORM, explicit alternative reference levels, and nonzero/invalid baseGain. Ignore embedded ReplayGain as an independent adjustment or fallback. Opus's mandatory header/output gain is part of normal decoding and must remain intact, but exclude Opus from provider-gain qualification until its peak/gain baseline is verified; absent baseGain alone does not prove a zero codec output gain. Unqualified transcoded representations also use unchanged album gain. Jellyfin's upstream gain fields alone do not establish a complete peak/reference contract; retain unity without inventing peak values or new API fields.

**Parsing:** Missing metadata differs from malformed metadata. Invalid optional values must not make the entire album DTO fail deserialization. Accept finite JSON numbers only for the supported provider fields; strings, nulls, arrays, wrong shapes, NaN/infinity in direct internal tests and overflow are rejected. Validate gain within [-60, +30] dB and peak within (0, 64]; these are application acceptance bounds, not assertions about the standard. Zero gain is valid; zero peak is not. Bound retained normalized evidence to fixed-size fields and typed reasons; never persist raw JSON, URLs or tag dictionaries. Preserve `Song`/aggregate `Eq` contracts using an equality-safe validated representation rather than adding bare floats everywhere or removing equality broadly.

**Whole-album consistency:** Every returned occurrence must belong to the requested source album and contain a supported usable pair. If a declared album track count contradicts the complete response, or any member is missing/invalid/unsupported, freeze unity. Preserve duplicate track occurrences. Require max gain minus min gain <= 0.01 dB and max peak minus min peak <= max(1e-6, 1e-4 * max peak). Use the minimum accepted gain and maximum accepted peak, avoiding first-row/order-dependent results. Do not average incompatible values or form an album peak from track peaks. Missing album membership evidence is insufficient to assert a common adjustment.

**Precedence:** This provider pair is the sole v1 authority. Embedded gain tags never multiply it, override it on the next track, or activate after a unity fallback. Decode without optional replay-gain/loudnorm filters; retain codec-mandatory decoding behavior. Explicitly test that changing embedded album/track tags cannot change the frozen scalar. A future embedded-tag policy requires a versioned, bounded admission design, not opportunistic per-track overrides.

**Calculation:** For accepted common dB gain D and peak P, compute in f64:

```text
ceiling = 10^(-1 / 20)                 # -1 dBFS sample-reference margin
requested = 10^(D / 20)
effective = min(requested, ceiling / P)
```

Require finite positive results, convert once to the runtime f32 scalar, and round downward if needed so representational rounding cannot exceed the reference ceiling. Persist the resolved scalar reproducibly (for example validated f32 bit pattern) with policy version, chosen D/P and reason/provenance. Unity is exactly 1.0 and bypasses multiplication; it does not promise clipping protection. This margin is an explicit initial design constant, not a measured true-peak guarantee. No gain ramp or track-dependent attenuation is introduced.

### Peak domain and conversion order

The existing path is FFmpeg decode (including mandatory codec behavior and verified padding) → swresample into the selected output rate/layout as packed f32 → bounded PCM queue → native sample conversion. Add the immutable scalar **after swresample and before each PCM push**, including the flush path. Replayed submitted samples are already scaled and must not pass through gain again. Never clamp decoded float PCM to [-1, 1] before the gain; lossy decoded samples can exceed nominal full scale.

The metadata ceiling calculation protects the **declared decoded sample-peak reference**. A source peak alone does not prove a universal peak bound after resampling/downmix, nor any inter-sample/analog true-peak limit. Document that limitation in diagnostics/evidence; do not label all converted output clipping-safe. Same-rate/layout qualified fixtures must satisfy the numerical output ceiling. Mixed-rate/layout fixtures must match the independently converted reference multiplied by the common scalar and explicitly record post-conversion peaks. If a stronger output-peak guarantee is claimed for a conversion, first derive and test a conservative conversion bound and resolve it album-wide before audio; never silently add a limiter or recalculate different gains on later tracks. Unsupported metadata/representation qualification must choose unity at admission, not a different per-track gain at handoff.

Initial supported representation allowlist: original PCM-in-WAV, FLAC, ALAC/AAC-in-M4A and MP3, with ordinary decoder normalization disabled and the provider pair referring to that original decoded representation. Admission uses trimmed case-insensitive `Song.suffix`: `wav` → PCM-WAV, `flac` → FLAC, `m4a` → ALAC/AAC-M4A, `mp3` → MP3. Missing/unknown suffix or contradictory content type freezes whole-album unity; do not infer codec from title/URL or request each track separately. Different qualified formats/rates/layouts may coexist in an album. Provider suffix/content type establishes only admission eligibility; decoder inspection must confirm the actual codec/container before audio is enqueued (WAV PCM s16le/s24le/s32le, FLAC, MOV/M4A ALAC or AAC, MP3 respectively). Keep gain qualification separate from seek capability; sequential originals can qualify without HTTP Range. Unknown types, Vorbis/WMA and Opus are unity for the entire admitted album in policy v1 until separately qualified. Include at least one lossless original end-to-end provider fixture; an implementation that returns unity for every valid supported album does not satisfy this story. Use original representations for the supported provider policy. When the frozen policy is unity, all existing playable formats and representations remain playable with unchanged gain; gain qualification adds no format-based playback rejection. Only for a qualified non-unity policy, if stream resolution later returns an incompatible representation or contradictory baseline, preserve the frozen policy/session and expose a recoverable preparation error before its audio, rather than silently applying an unsafe adjustment or switching gain for that one member. No quality adaptation is added here. A decoder/PCM error retains the existing same-occurrence album failure/retry semantics.

### Bounded admission, ownership and persistence

`handle_playback_play_album` already reserves an owner token before provider IO, uses a 60-second deadline/cancellation, orders the complete album, then reduces `Song` to `TrackSource`. Resolve gain **before** that reduction and pass a typed internal album plan through commit. Do not accept arbitrary client-supplied gain or fetch metadata on the owner thread. Retain the 10,000-occurrence maximum and bounded normalized metadata overhead; no per-track HTTP requests, file probes, parallel task per song, analysis pass or full-album media reads. The existing album response is the evidence source. Metadata work shares the original admission deadline and stale-token rejection.

There is currently no persisted album mode/gain or album membership: `PersistedSession` stores session/cursor and `playback_occurrences` stores sources/outcomes. Introduce the smallest private versioned album context required here, scoped by portable server ID, album ID and original occurrence membership. A session-wide scalar alone is unsafe because existing AppendQueue/SelectCurrent operations can introduce or select other material. Original album members retain their frozen policy; appended nonmembers and standalone PlayTrack use unity. Clear/ReplaceQueue/PlayTrack/new PlayAlbum invalidate the old context only with their successful atomic commit. A failed replacement keeps old gain and audio coherent. Future 15.13 owns richer manual-mode transitions, but existing mutation paths must work now.

Migrate playback persistence v2 transactionally to the next version. Legacy sessions migrate to unity with unknown/non-album context, retaining queue/cursor/outcomes and paused restore. Validate finite/ranged scalar and policy fields, coherent provenance, membership, source identity and supported version during normal restore and retry_restore. Corrupt or unsupported new records use existing recoverable restoration failure without destroying the last state; do not silently reinterpret corrupt data as valid unity. All structural/terminal writes and frozen storage-retry plans must preserve the policy. Checkpoint-only writes must not replace it. Keep migration isolated from device manifests and provider credentials.

The policy is immutable for a committed album, including unity. Carry it in generation/control-epoch/occurrence-fenced preparation. Both CPAL and Pulse initial and successor decoders, seek, Retry and output recreation use that occurrence's stored policy. Late metadata cannot mutate it. Keep output-loss pause/no-reroute, explicit Resume, safe Quit, source-qualified identities, command deduplication and owner-only state authority unchanged.

### Source files: current behavior, change and preservation

Paths below are repository-relative. Read complete files before editing; giant modules should receive small, focused changes, with new pure logic extracted rather than expanding unrelated code.

| File | Current behavior | Required change / preserve |
| --- | --- | --- |
| `hifimule-daemon/src/domain/models.rs` | `Song` has media/order fields, derives Eq, no loudness metadata | Add optional bounded equality-safe normalized evidence; preserve existing JSON compatibility and all construction sites. |
| `hifimule-daemon/src/providers/subsonic.rs` | `SongDto`, `song_from_dto`, `get_album` ignore replayGain | Tolerantly retain optional pair/provenance through complete album mapping; preserve authentication, ordering and classic-server fallback. |
| `hifimule-daemon/src/providers/jellyfin.rs` | Current normalized song mapping has no gain/peak contract | Initialize explicit unsupported/absent evidence if the shared model changes; no speculative new endpoint or gain conversion. |
| `hifimule-daemon/src/playback/album.rs` | Stable disc/track ordering, duplicate preservation, 10,000 limit | Reuse ordering; keep pure gain resolution in a small `playback/loudness.rs` helper (NEW), exported privately from `mod.rs`. |
| `hifimule-daemon/src/rpc.rs`, `playback/commands.rs`, `playback/session/album_admission.rs` | Reserve → async provider resolution → source-only commit → effects | Carry resolved policy/album scope atomically; keep receipt replay, shutdown guard, cancellation and stale resolution rejection. |
| `hifimule-daemon/src/playback/model.rs`, `playback/session.rs` | Serialized owner, tokens, terminal plans, transport, no album mode | Add private frozen context and occurrence selection; preserve all replace/append/select/clear/seek/retry/handoff paths and wire schema unless genuinely changed. |
| `hifimule-daemon/src/playback/persistence.rs` | v2 schema, atomic structure/outcome/cursor updates and paged lookup | Versioned durable policy/membership, migration/validation/rollback and restart retention; retain bounded paging and exact recovery transactions. |
| `hifimule-daemon/src/playback/audio.rs`, `playback/audio/pulse_output.rs` | Continuous output with initial/successor decoder slots and native control fencing | Supply immutable occurrence gain at all decoder starts; preserve selected endpoint, Pulse priming/cork, WASAPI replay and presentation reconciliation. |
| `hifimule-daemon/src/playback/decoder.rs` | Worker-owned decode, resample, PCM insertion and flush; no gain | Apply one shared pure scalar primitive to both ordinary and flushed PCM; preserve seek discard, decoder EOF/error, known padding and full-queue cancellation. |
| `hifimule-daemon/src/playback/output.rs`, `audio/handoff.rs`, `continuity.rs` | Boundary rendering, submitted-tail replay and presentation identity | Prefer regression tests without production changes; do not add callback DSP state or re-scale replay. |
| `hifimule-daemon/src/playback/decoder_continuity_tests.rs`, `commands_tests.rs`, `session/album_admission_tests.rs`, `rpc/album_tests.rs` | Production decoder/reference and owner/admission regression tests | Add gain/lifecycle/membership coverage beside existing fixtures; exercise both backend call sites, not only helper math. |
| `docs/api-contracts-hifimule-daemon.md`, `docs/playback-installed-test-checklist.md` | Playback ownership/persistence/native continuity and installed evidence contracts | Document internal gain policy, migration, qualified formats/providers and digital-versus-physical evidence. |

Search all `Song` literals when adding a field; a default serde field does not update Rust literals. Preserve optional-field compatibility in provider fixtures and consumers. A dedicated provider-domain metadata type avoids exposing raw tag payloads to the UI. No UI change is required for ordinary unity fallback; if a new recoverable reason is user-visible, use existing status/error presentation and EN/FR/ES/DE localization, not a new gain panel.

### Testing requirements

- **Math/parser:** zero/negative/positive gain; above-unity peak; excessive boost; exact acceptance bounds and tolerances; missing/null/string/array/wrong-object fields; non-finite direct values; extreme exponent; partial pair; conflicting multi-disc tags; order independence; one invalid last row; zero peak; unsupported baseGain/reference/Opus; ignored track/fallback/embedded gain. Use independent f64 expected calculations with scalar absolute tolerance 1e-7 and packed-f32 sample absolute tolerance 2e-6 for bounded fixtures. Document any different codec/reference tolerance rather than using broad RMS to hide a time shift.
- **Production audio:** two tracks with different known amplitudes and the same policy preserve their ratio. Include positive/negative values, intentional exact zeros, impulses, short tracks, decoder drain/resampler tail, mono/stereo and 44.1↔48 kHz. Unity must preserve existing decoded samples bit-for-bit. For qualified unchanged-format fixtures assert peak <= ceiling + 2e-6. For converted fixtures record actual peak and compare each sample with independently converted unity reference times frozen gain; do not imply a universal post-conversion bound.
- **Continuity:** retain 15.9 exact frame counts and unique boundary markers, including alternating A→B→A slots and tail/head in one callback. Test Pause/repeated Pause/partial replay/Resume so the same samples have one gain application, not its square. Failed short decode must not become clean EOF. Gain metadata must not alter padding or silence trimming.
- **Admission/persistence:** stale metadata after Stop/seek/replacement; duplicate command receipt; invalid optional tags still queue/play unchanged; failure before transaction commit; v1/v2 migration; reopen database and validate scalar/membership/cursor; offline paused restore; corrupt/future policy version; source Retry and storage Retry; appended different album; same track IDs on different servers; SelectCurrent; PlayTrack/new album replacement resets only on success.
- **Integration:** real adapter fixture → ordered album → owner commit → initial/successor production decode proves metadata is not dropped. Embedded gain tags plus provider gain prove exactly one adjustment. Transcoded or baseline-incompatible source cannot inherit a false protection claim. Keep standalone playback and device-sync behavior unchanged.
- **Resources/platforms:** metadata adds bounded per-entry fields and one small policy, no source slot. Preserve 8 MiB compressed per slot / 16 MiB aggregate; PCM min(500 ms, 1 MiB) per slot / 2 MiB aggregate; at most two source/decoder slots. Record build commit, OS/architecture, loaded FFmpeg ABI, backend, fixture hash, D/P/scalar/reason, output format, reference peak, converted peak, sample error and frame counts. Native/physical evidence is separate from digital results and OS volume settings.

Use the controlled runtime wrapper, **not bare cargo test** (the build guard rejects an unverified runtime):

```sh
rtk npm run build:daemon -- test -p hifimule-daemon playback::
rtk npm run build:daemon -- test -p hifimule-daemon providers::
rtk npm run build:daemon -- test -p hifimule-daemon
rtk proxy python3 -m unittest discover -s scripts/tests -p 'test_playback_installed_evidence.py'
rtk git diff --check
```

Run focused tests first, then the full daemon suite after shared provider/model/persistence edits. Build actual Windows and Linux daemon targets in addition to macOS; a vendored-library compile or extracted harness does not validate callers/native behavior. Only run UI checks when its contract/files change. Production decoder reference tests need a version-matched FFmpeg executable on PATH or `HIFIMULE_TEST_FFMPEG`, in addition to the controlled linked libraries. Missing fixture/runtime/network permissions are explicit prerequisites, not silent passing skips. Do not copy previous test counts as current evidence.

### Previous-story and git intelligence

The five latest commits are `66c37bb Review 15.9`, `efab103 Dev 15.9`, `6fca620 Dev 15.9`, `8ae700c Dev 15.9`, and `d4d73f1 Review 15.8`. They establish the current two-slot path and review corrections; earlier story descriptions of one stream per track are stale.

Preserve Windows public CPAL pause dispatch/native Resume/reset clock accounting, submitted-tail replay across repeated Pause, and replay-aware completion. Preserve disabled-consumption boundary gating, retained-pipeline epoch authorization, exact successor offsets across alternating slots, inherited seek qualification, failure precedence and atomic owner handoff. Pulse uses acknowledged cork and played-position accounting. CoreAudio has no exact native stop cursor and retains its documented fallback. Worker-side gain keeps these output contracts intact.

Review 15.9 removed guessed AAC priming trim. Unknown AAC priming and unknown-size MP3 are not automatically continuity-qualified; lossless FLAC needs no trim. Gain must not change sample counts or reopen those padding heuristics. Prior test counts are historical regression evidence only. Physical captures, full platform builds and remaining source races remain explicitly open in the predecessor's narrative.

### Architecture, dependencies and technical references

Use repository pins: Rust edition 2024/MSRV 1.93.0; patched CPAL =0.18.2; ffmpeg-next/sys =9.0.0; controlled FFmpeg 9.0.1; crossbeam-queue =0.3.12; libpulse-binding =2.30.1; patched Souvlaki =0.8.3. `audio-runtime.json` and the build wrapper define the actual loaded ABI (63.1.101/61.1.101/7.1.101); do not replace it with a version string from system pkg-config. The runtime disables avfilter and network, so adding a loudnorm filter graph or FFmpeg network IO is not this implementation. No library upgrade is required.

Primary sources checked 2026-09-18:

- [OpenSubsonic ReplayGain](https://opensubsonic.netlify.app/docs/responses/replaygain/): optional album gain/peak and distinct base/fallback gain; installed-provider coverage still needs evidence.
- [ReplayGain scaling](https://replaygain.hydrogenaudio.org/player_scale.html) and [peak representation](https://replaygain.hydrogenaudio.org/peak_data_format.html): exponential dB conversion and decoded sample peak relative to full scale. The project's unity fallback overrides the old proposal's suggested guessed fallback.
- [Jellyfin upstream DTO](https://github.com/jellyfin/jellyfin/blob/master/MediaBrowser.Model/Dto/BaseItemDto.cs): gain field availability is not proof of a complete installed gain/peak contract.
- [RFC 7845](https://www.rfc-editor.org/rfc/rfc7845): Opus output gain/R128 conventions require explicit baseline handling; do not reapply mandatory header gain.
- [FFmpeg releases](https://ffmpeg.org/download.html): 9.0.1 is listed as stable; repository runtime pins and verification remain authoritative. This is not a security audit or justification to upgrade dependencies.

Project context's provider abstraction and managed-zone safety remain foundational; its greenfield label is stale. The base UX document predates playback and does not mandate loudness controls. Preserve existing server navigation, accessible controls and source identity. Later Preview must preserve this main-session context; later Radio must reuse the scalar/metadata primitives with its own track policy rather than introducing a second DSP implementation.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 15, Story 15.10; adjacent 15.9/15.11; queue policy 15.13; Radio gain 15.19]
- [Source: `_bmad-output/planning-artifacts/prd.md` — Playback Extension FR61/74 and P-NFR1/2/4]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Playback Audio Pipeline, Provider Integration, Implementation Contracts, Validation Refinements]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md`; `project-context.md`]
- [Source: `_bmad-output/implementation-artifacts/15-9-preserve-album-continuity-across-prepared-track-boundaries.md` — Review Findings, remediation and outstanding evidence]
- [Source: `_bmad-output/implementation-artifacts/playback-feasibility-results.md` — provider metadata coverage and experimental limitations]
- [Source: source-file table; `Cargo.toml`; `hifimule-daemon/Cargo.toml`; `hifimule-daemon/audio-runtime.json`]

## Dev Agent Record

### Agent Model Used

GPT-6.

### Implementation Plan

- Normalize only the complete OpenSubsonic album ReplayGain pair into equality-safe internal evidence, then resolve one versioned policy before converting songs to queue sources.
- Persist the album source, original occurrence membership and reproducible scalar/provenance in the owner transaction, and select unity for nonmembers.
- Carry the immutable scalar through initial, seek and successor preparation in both native paths; multiply packed f32 output once after resampling and before queue insertion.
- Verify the pure policy, provider parsing, admission fencing, migration/restore and production decoder behavior with the controlled runtime, then record installed-platform limits separately.

### Debug Log References

- Red/green focused tests covered resolver math/fallback, OpenSubsonic parsing, persistence migration/context and the production decoder gain boundary.
- Controlled playback suite: 246 passed, 6 ignored; provider suite: 128 passed; final daemon suite: 949 passed, 6 ignored.
- Installed-evidence validator: 33 passed. `cargo fmt --check`, `git diff --check`, macOS ARM64 daemon build and daemon clippy completed successfully; clippy retains repository baseline warnings.
- Linux ARM64 and Windows ARM64 daemon cross-builds were attempted but the controlled toolchain lacked target `core`/`std` and matching native linker/runtime inputs. Those installed rows, macOS x64 and physical capture remain explicitly unverified in the checklist.
- Review correction: decoded the typed Subsonic body from the original raw response object so `RawValue` retains valid and overflowing ReplayGain tokens while API failures keep their sanitized classification. Subsonic provider regressions: 59 passed.
- Review correction: fresh album requests now cancel and supersede pending same/different-album resolutions, stale results remain fenced with `ALBUM_SUPERSEDED`, and the UI silently consumes only that expected outcome. Admission regressions: 14 passed; album RPC regressions: 5 passed; corrected full daemon suite: 953 passed, 6 ignored; UI production build passed.
- Adversarial review correction: classic Subsonic gain is gated off, malformed API error fields retain sanitized generic mapping, supersession wins over obsolete provider failures, and persisted membership is digest-validated. Provider suite: 61 passed; corrected full daemon suite: 955 passed, 6 ignored; UI production build, formatting and diff checks passed.

### Completion Notes List

- Added a deterministic album-wide resolver with strict evidence bounds, consistency tolerances, static `-1 dBFS` sample-peak reference protection and exact-unity fallback reasons.
- Retained tolerant OpenSubsonic metadata without changing the public Song JSON contract; malformed and extreme optional values remain playable at unity.
- Migrated playback persistence to v3 and atomically fenced source album, original occurrence membership, scalar and provenance; restore rejects corrupt, future or incoherent policies.
- Passed member gain through admission, seek, retry/resume, successor preparation, CPAL and Pulse. Appended nonmembers use unity and successful replacements clear the old context.
- Applied gain exactly once to normal and flushed packed-f32 samples after swresample. Qualified non-unity playback verifies the resolved original container/codec before enqueueing audio.
- Documented signal-domain limits and current platform evidence. Digital macOS ARM64 checks passed; unrun installed and physical-output rows remain open as required by AC8.

### File List

- `Cargo.toml`
- `_bmad-output/implementation-artifacts/15-10-preserve-an-album-s-relative-loudness-with-consistent-gain.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `docs/api-contracts-hifimule-daemon.md`
- `docs/playback-installed-test-checklist.md`
- `hifimule-daemon/src/auto_fill/fetch.rs`
- `hifimule-daemon/src/auto_fill/pipeline.rs`
- `hifimule-daemon/src/domain/models.rs`
- `hifimule-daemon/src/playback/album.rs`
- `hifimule-daemon/src/playback/audio.rs`
- `hifimule-daemon/src/playback/audio/pulse_output.rs`
- `hifimule-daemon/src/playback/commands.rs`
- `hifimule-daemon/src/playback/continuity.rs`
- `hifimule-daemon/src/playback/decoder.rs`
- `hifimule-daemon/src/playback/decoder_continuity_tests.rs`
- `hifimule-daemon/src/playback/loudness.rs`
- `hifimule-daemon/src/playback/mod.rs`
- `hifimule-daemon/src/playback/model.rs`
- `hifimule-daemon/src/playback/native.rs`
- `hifimule-daemon/src/playback/persistence.rs`
- `hifimule-daemon/src/playback/session.rs`
- `hifimule-daemon/src/playback/session/album_admission.rs`
- `hifimule-daemon/src/playback/session/album_admission_tests.rs`
- `hifimule-daemon/src/providers/jellyfin.rs`
- `hifimule-daemon/src/providers/subsonic.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-daemon/src/rpc/album_tests.rs`
- `hifimule-ui/src/rpc.ts`

### Change Log

- 2026-09-18: Created implementation-ready Story 15.10 with metadata, persistence, audio and evidence contracts.
- 2026-09-18: Implemented and verified frozen album ReplayGain policy, durable membership, native-path gain application and platform evidence tracking; moved story to review.
- 2026-09-18: Corrected raw ReplayGain response decoding and made rapid album selection latest-request-wins with silent typed supersession.
- 2026-09-18: Applied adversarial review fixes for capability gating, tolerant API errors, cancellation precedence and exact persisted membership validation; reopened incomplete platform evidence.

- 2026-09-18: Applied all six review patches, added eight regression tests, fixed destination gain projection on Next, and retained in-progress status for outstanding installed-platform evidence.


### Saved-session compatibility correction

The user's installed v3 database contained the early album-context shape
(`source`, `memberCount`, `policy`) without `membershipDigest` or
`representations`. Strict deserialization/validation rejected that legitimate
older session, and the existing restoration guard then rejected every new
PlayAlbum request. This supersedes the earlier review note accepting failed
restoration for development v3 records.

Playback schema v4 now transactionally derives only a missing legacy digest
from the validated ordered original membership, preserves queue/cursor/outcomes
and frozen gain, and represents absent non-unity format evidence explicitly as
unverified. Restoration remains paused; a new album can be admitted normally.
Unverified non-unity members cannot bypass preparation qualification. Existing
invalid digests, invalid membership and corrupt v4 records remain errors.
Regression tests cover all older field combinations at unity and non-unity,
new album admission after restoration, migration rollback, restore Retry, and
refusal to silently repair corrupt existing evidence.

Compatibility-fix validation: three migration regressions passed; full controlled daemon suite 966 passed, 6 ignored; macOS ARM64 daemon build, formatting and diff checks passed. The user database was inspected read-only; migration is applied by the updated daemon at startup.
