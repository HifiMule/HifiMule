---
baseline_commit: c595ff1613b5b149674ca9b617d53bbf3be5b755
---

# Story 16.12: Adapt playback quality at track boundaries using buffer health

Status: review

<!-- Ultimate context engine analysis completed - comprehensive developer guide created -->

## Story

As a HifiMule user,
I want playback to choose the best quality my connection can sustain,
so that I can keep listening with minimal interruption without manually tuning streaming settings.

## Scope

Select among **verified playable provider representations** using source- and representation-scoped delivery/buffer evidence. Adapt only when preparing a later track. Preserve the current track, output, position, padding, gain, queue occurrence, reporting identity, generation fences, Radio failure semantics, and prepared album continuity.

This story includes provider alternative discovery, deterministic quality ordering, a bounded/hysteretic estimator, track-boundary decision and preparation policy, truthful daemon/UI status, and evidence-oriented tests. It does **not** implement mid-track replacement, device-sync throttling (16.13), sustained installed soak/release validation (16.14), a second player/event loop, portable-device transcoding policy reuse, or automatic skipping when bandwidth is insufficient.

## Acceptance Criteria

1. **Highest sustainable verified representation**
   - Given a source exposes verified playable alternatives, when a track is prepared, the daemon selects the highest sustainable representation using one documented quality order plus measured delivery/refill and compressed/PCM buffer evidence.
   - Provider alternatives and their ordering are capability-verified. Bitrate alone does not rank unlike codecs or prove perceptual quality.
   - Portable-device `transcoding_profile_id` and other sync preferences never influence playback selection.
   - With no usable capacity history, the documented startup policy and bounded startup buffer apply; the daemon does not manufacture a throughput estimate.

2. **Hysteretic downgrade at a later boundary**
   - Given sustained depletion/refill evidence that the active representation is not supportable, when the next track is selected, a lower supported sustainable alternative is selected when available.
   - The estimator uses an explicit observation window, minimum evidence requirements, downgrade threshold and hysteresis. A single isolated sample cannot oscillate quality.
   - The authoritative playback snapshot/event exposes the selected quality and a discreet, localized, screen-reader-accessible reason when quality was reduced.

3. **Conservative recovery**
   - Given sustained recovery evidence, when a later track is selected, the daemon may upgrade only after the documented recovery window and threshold are satisfied.
   - Startup bursts, a cache hit, or one fast transfer cannot trigger an upgrade. Tests prove bounded switch frequency under jitter.

4. **No mid-track replacement or identity corruption**
   - An estimator recommendation never replaces the active representation during this story.
   - The next boundary applies a new selection without changing the current occurrence, playback position, padding policy, album/Radio gain policy, report identity, output route or paused intent.
   - Existing prepared album continuity remains required whenever both tracks are ready. Adaptation cannot submit an extra completed-listen report.

5. **Safe prepared-successor policy**
   - A documented cutoff defines when a differently prefetched successor may be revoked and prepared again.
   - Replacement is bounded to one successor and cannot bypass `SuccessorFence`, generation/control/queue/preparation fences, slot ownership, cancellation, preparation deadline or compressed/PCM limits.
   - Once the callback has claimed a ready successor, it is never discarded for a late estimator recommendation. Obsolete work cannot publish or win a generation race.
   - The policy favors already-ready audio when replacement would create an avoidable gap; selected versus deferred adaptation is observable in diagnostics without exposing credentials.

6. **Truthful exhaustion and failure behavior**
   - If no lower representation exists, no alternative is verified, or none is sustainable, playback enters the existing buffering/retry path with the actual limitation.
   - Album playback does not silently skip a track. Radio retains its established unavailable-source policy, and technical delivery failure remains distinct from explicit skip/dislike/rejection.
   - HifiMule does not promise uninterrupted playback.

7. **Correct measurement attribution**
   - Measurements are keyed at least by portable source-server identity plus stable representation identity and cannot leak across incompatible origins or representations.
   - Policy explicitly handles/excludes transcode startup, partial/failed requests, seeks/ranges, cached responses and short/bursty transfers. It does not treat a fast cache hit as sustained origin capacity.
   - Decoded PCM depth, codec bit depth, physical-device write throughput and sync progress are not interpreted as server delivery bandwidth.
   - Authenticated URLs and headers remain daemon-private; public diagnostics contain stable identifiers and sanitized quality/status fields only.

8. **Existing lifecycle fences remain authoritative**
   - During seek, Preview, output loss, Stop, queue edits or session replacement, generation and paused-state rules still control admission and output.
   - Measurement completion cannot restart stopped audio, auto-resume after output loss, reroute output, mutate a replacement session or revive stale preparation.
   - Only current-generation `Resolved`/`HandoffPresented` transitions update the visible selected representation and quality reason.

9. **Measured, reproducible evidence**
   - Pure deterministic tests cover stable-fast, sustained-slowdown, recovery, jitter, outage, cache/startup exclusion, source/representation isolation, no viable fallback, maximum switching frequency and unknown-capacity startup.
   - Provider integration tests verify actual alternatives and ordering for every provider that claims the capability; providers without verified alternatives explicitly remain single-representation.
   - Boundary/race tests cover no mid-track switch, late ready-successor retention, bounded pre-cutoff replacement, stale generation rejection, Stop/seek/Preview/output loss, album gain/padding/continuity and exactly-once reporting identity.
   - Story 16.12's completion evidence uses deterministic profiles plus provider integration on supported development/test environments to set and verify the versioned thresholds. It records selected representations, compressed/PCM high-water bounds, switching count, buffering events and actual recovery. Reliability is demonstrated from outcomes, not inferred from a quality label.
   - Story 16.14 owns sustained installed Windows/macOS/Linux soak and release certification. Story 16.12 must leave a reproducible evidence corpus and harness for that work; it must not claim installed-platform certification early.

## Tasks / Subtasks

- [x] **Task 1: Freeze the quality and measurement contract before changing selection** (AC: 1, 2, 3, 7, 9)
  - [x] Inventory the real Jellyfin, Subsonic/Navidrome and Audiobookshelf playback responses supported by the shipped adapters. Record which alternatives are verified on which server versions; an adapter must not advertise an untested transcode.
  - [x] Define a daemon-private stable `RepresentationId`/descriptor containing only comparable normalized quality/capability fields. Keep URL, headers and provider-specific request details out of RPC state.
  - [x] Separate deterministic quality ordering from sustainability filtering. Preserve the existing bounded maximum of eight representations and deterministic tie breaking.
  - [x] Define and version the numeric policy from controlled measurements: startup assumption, sample eligibility, observation window, minimum evidence, depletion/downgrade threshold, recovery threshold/window, hysteresis, preparation cutoff, replacement limit, and compressed/PCM budgets. Record units on every value.
  - [x] Document how unknown capacity, cached/startup/seek/partial samples, transcoded startup and request failures affect evidence. Use a monotonic clock; wall-clock timestamps must not drive rate calculations.

- [x] **Task 2: Expose verified playback alternatives through the provider boundary** (AC: 1, 6, 7, 9)
  - [x] Extend `PlaybackDescription`/`PlaybackRepresentation` in `hifimule-daemon/src/providers/mod.rs`; keep source-routed authentication and provider URLs daemon-side.
  - [x] Complete at least one shipped provider/server-version path (Jellyfin or Subsonic/Navidrome) that exposes two or more verified playable representations with deterministic ordering, reusing its existing streaming/transcoding URL builder rather than portable-device profile selection. Preserve exact source-server routing.
  - [x] Keep Audiobookshelf or any unverified provider explicitly single-representation rather than inventing parity.
  - [x] Prove that the chosen multi-representation provider performs an end-to-end downgrade and conservative recovery on later track boundaries under controlled profiles. A release where every provider remains single-representation does not satisfy this story.
  - [x] Update selector and adapter tests for ordering, bounded lists, unsupported codecs/containers, unknown metadata, authentication, capability absence and deterministic fallback.

- [x] **Task 3: Add a pure, deterministic adaptation policy and scoped observations** (AC: 1, 2, 3, 6, 7, 9)
  - [x] Add `hifimule-daemon/src/playback/adaptation.rs` (preferred) or an equivalently focused module for the pure policy only. `streaming.rs` remains the fetching/prefetch/observation orchestration owner; do not create a second pipeline, session or player owner.
  - [x] Model eligible observations and decisions independently of HTTP/audio hardware so time and network profiles can be deterministic in tests.
  - [x] Key history by source plus stable representation identity. Bound sample count/age and all retained diagnostic state.
  - [x] Implement downgrade/recovery hysteresis, unknown-capacity startup, cache/startup exclusion, and a decision reason suitable for sanitized UI projection.
  - [x] Instrument `playback/http_source.rs` and `playback/streaming.rs` outside the callback to capture byte/time/no-progress and compressed-buffer facts. Classify cache behavior only from explicit trustworthy provider/HTTP evidence; otherwise mark suspiciously short/fast samples ineligible or low-confidence under the documented policy, never “cached” from speed alone. Preserve the 15-second no-byte-progress timeout, <=1 MiB response-chunk validation, and existing bounded seek-window semantics unless measurements justify a reviewed change.

- [x] **Task 4: Apply decisions only through existing preparation and boundary fences** (AC: 2, 3, 4, 5, 8)
  - [x] Integrate selection where `commands.rs::spawn_successor_preparation` resolves the source and `audio.rs::prepare_successor` admits one prepared successor. Do not switch inside the output callback.
  - [x] Extend `PlaybackSession::successor_candidate`/preparation inputs with typed adaptation context while preserving generation, control, queue and preparation tokens.
  - [x] Use `continuity.rs::SuccessorFence` as the cutoff authority. Revocation/reprepare is legal only before callback claim, within the one-successor bound and the existing preparation deadline; retire the old slot before publishing replacement.
  - [x] Preserve current-track selection for seek and Preview unless a distinct new occurrence is prepared. Adaptation must never manufacture a reporting transition.
  - [x] If output-level depletion is required, expose allocation-free atomic counters/signals sampled by the owner worker; never allocate, block, perform I/O, take estimator/session/sync locks, or update persistence in the CPAL/Pulse callback.

- [x] **Task 5: Preserve album gain, padding and continuous handoff across quality changes** (AC: 4, 5, 6, 9)
  - [x] Resolve the current `qualified_gain_suffix`/album-admission incompatibility deliberately: non-unity album gain currently requires `PlaybackProvenance::Original` and the admitted suffix. Do not bypass this check merely to enable transcodes.
  - [x] Either prove a selected alternative has compatible decoded representation/gain semantics and freeze that evidence in album admission, or constrain that album occurrence to a compatible representation and expose the limitation. The choice must be tested and documented.
  - [x] Preserve known encoder padding/recorded silence policy, channel/rate conversion, continuous native output, exact occurrence/source identity and the one-prepared-successor contract from Stories 15.9–15.10.
  - [x] A technical successor miss surfaces buffering/retry and never causes early predecessor completion or album-track skipping.

- [x] **Task 6: Publish authoritative, accessible quality state** (AC: 2, 6, 8)
  - [x] Add structured selected-quality/adaptation status and reason to `playback/model.rs` snapshot/events and the TypeScript RPC contract. Keep the existing camelCase JSON convention and strict DTO validation.
  - [x] Reconcile the current UI gap: Rust exposes `representation`, while `hifimule-ui/src/rpc.ts` does not type it. Add one authoritative wire representation rather than parallel inferred UI state.
  - [x] Render a discreet explanation in `components/PlaybackControls.ts` through the existing `role=status`/live-region pattern. Do not steal focus or expose URLs, headers or server internals.
  - [x] Add localized strings and all-locale parity coverage in `hifimule-i18n/catalog.json`; preserve generic buffering/retry and source-unavailable messaging.

- [x] **Task 7: Prove bounded behavior and regression safety** (AC: 1–9)
  - [x] Unit-test selector and pure estimator policy with fake monotonic time and scripted observations.
  - [x] Extend mock-server tests in `streaming.rs`/`http_source.rs` for burstiness, startup delay, cache classification, outage, range/seek, cancellation, timeout and sanitized diagnostics.
  - [x] Extend boundary/generation tests in `audio/queue_edit_tests.rs`, `decoder_continuity_tests.rs`, `session/album_admission_tests.rs`, `session.rs` and `output.rs` as appropriate.
  - [x] Assert high-water marks against the current two-slot baseline unless the measured policy explicitly revises it: 8 MiB compressed per slot / 16 MiB aggregate, 64 KiB compressed chunks, 1 MiB network chunks, 7,274,496-byte retained compressed window, 1 MiB PCM cap per slot / 2 MiB aggregate, 500 ms PCM target, 100 ms startup/refill, and a 60-second preparation deadline.
  - [x] Add UI DOM/contract tests for reduced-quality explanation, generic buffering, localization, reconnect snapshot authority and no focus theft.
  - [x] Record provider/platform evidence and any unavailable environment honestly. Do not promote partial unit coverage to Windows/macOS/Linux integration evidence.

- [x] **Task 8: Update owned documentation and handoff for 16.13/16.14** (AC: 7, 9)
  - [x] Update `docs/api-contracts-hifimule-daemon.md`, `docs/data-models-hifimule-daemon.md`, `docs/playback.md` and `docs/architecture-hifimule-daemon.md` with representation identity, quality ordering, estimator units/thresholds, public status contract, cutoff and failure behavior.
  - [x] Expose only the bounded playback-health signal needed by Story 16.13; do not implement sync throttling here or conflate slow device writes with source delivery.
  - [x] Extend `docs/playback-installed-test-checklist.md` and the existing `docs/playback-evidence/` conventions with a reproducible network-profile/evidence format for Story 16.14 sustained and installed validation.

## Dev Notes

### Current Implementation — Read Before Editing

| File / owner | Current state | Story change | Preserve |
|---|---|---|---|
| `hifimule-daemon/src/providers/mod.rs` | `PlaybackRepresentation` carries request, codec/container, bitrate/rate/depth, provenance and seek data. `select_playback_representation` examines at most 8 entries; original wins, then lossless quality, then same-codec lossy bitrate. | Add stable representation identity/capability evidence and separate quality order from sustainability. | Determinism, bounded input, daemon-private requests and provider-neutral boundary. |
| `providers/{jellyfin,subsonic,audiobookshelf}.rs` | Each currently returns one original representation. Existing provider-specific stream/transcode URL helpers already exist. | Advertise only verified alternatives and capabilities; reuse provider helpers. | Source-server routing, auth secrecy, direct-play fallback, provider-specific semantics. |
| `playback/streaming.rs` | Bounded reader: 8 MiB compressed cap, 64 KiB chunks, 1 MiB network reserve; exposes failure/preparation/high-water but no refill-rate observations. | Add bounded instrumentation and adaptation-owned observations. | Seek window, EOF order, cancellation, timeouts, high-water enforcement. |
| `playback/http_source.rs` | Authenticated daemon-side HTTP, 15 s no-byte-progress timeout, <=1 MiB chunks, verified range/validator/content type. | Capture monotonic delivery facts and sanitized cache/startup classification. | URL/header secrecy, range validators, cancellation and error semantics. |
| `playback/audio.rs` | Pipeline owns compressed/PCM high-water; selection occurs before fetch. One `PreparedSuccessor`; worker emits Active/Buffering. | Consume an adaptation decision during preparation; publish scoped observations/status. | Active representation, worker ownership, callback isolation, reporting transitions. |
| `playback/commands.rs` / `session.rs` | One sequential successor coordinator; candidates freeze generation/control/queue/preparation fences, source identity and gain. | Carry typed adaptation context and admit only current-generation decisions. | Serialized session ownership, occurrence identity, paused/output-loss behavior. |
| `playback/continuity.rs` | `SuccessorFence` has ready/claimed epochs; revoke fails after callback claim. | Define bounded pre-claim replacement around this fence, if required. | Claim authority and stale/duplicate handoff protection. |
| `playback/output.rs` | Callback consumers emit silence/refill buffering without allocation. | At most add atomic depletion signals sampled elsewhere. | No locks, allocation, blocking I/O, DB/session/sync work or estimator logic in callbacks. |
| `playback/{album,model}.rs` and album admission | Album context freezes admitted suffixes; non-unity album gain rejects non-original/mismatched suffixes. | Establish verified alternative compatibility or explicit constraint. | Album gain, padding, continuous handoff, source/occurrence identity. |
| `hifimule-ui/src/rpc.ts` / `components/PlaybackControls.ts` | UI has generic playback messages; TS snapshot omits Rust's representation field. | Type and render authoritative quality/reason status accessibly. | Daemon authority, existing live region, generic error/buffering UX. |

### Architecture Compliance

- Keep the existing pipeline: provider stream -> bounded compressed prefetch -> owning FFmpeg decoder/processing worker -> bounded PCM -> shared native output.
- `playback/streaming.rs` owns fetching/prefetch/adaptation; provider APIs stay in `providers/`; SQLite ownership stays in `db.rs`/playback persistence; lifecycle/native loop stays in `main.rs`; RPC only validates/forwards.
- One daemon session manager remains authoritative. Do not create browser playback, a second daemon/player/event loop, UI-owned estimator state or a callback-owned policy engine.
- Every async operation carries generation identity. Cancellation and stale-result rejection are both required.
- Keep Rust/SQLite `snake_case`, JSON/TypeScript `camelCase`, explicit wire units and strict request DTOs.
- Authenticated URLs and credentials never cross the daemon boundary.

### Library and Framework Requirements

- Use the repository pins; do not upgrade dependencies as part of this story: Rust edition 2024 / MSRV 1.93, Tokio `~1.49`, CPAL `0.18.2`, `ffmpeg-next`/`ffmpeg-sys-next` `9.0.0`, controlled FFmpeg `9.0.2`, crossbeam-queue `0.3.12`, libpulse-binding `2.30.1`, patched Souvlaki `0.8.3`.
- Tokio time is monotonic and test-controllable; prefer an injected/test clock around the pure policy instead of sleeps. Tokio documents that its `Instant` aligns with paused/advanced test time.
- CPAL 0.18 stream instants are monotonic only within a stream; origins are not guaranteed comparable across streams. Do not combine timestamps from different output streams into one delivery estimate.
- `ffmpeg-next` 9 supports `input_from_stream_with_interrupt`; preserve interrupt-driven cancellation of stalled open/read work. FFmpeg decode metadata is not origin-network throughput evidence.
- No new dependency is justified unless the standard library and existing Tokio/atomic primitives cannot satisfy the bounded policy and tests.

### Testing Requirements

- Tests must assert decisions and quantitative bounds, not merely that a quality label changed.
- Prefer pure table/property-style estimator tests, deterministic fake time, scripted provider responses and local mock HTTP servers. Do not depend on public network services in the default suite.
- Keep provider adapter tests separate from generic policy tests; capability absence is a valid explicit result.
- Run targeted daemon suites, full daemon tests, Clippy on touched Rust, frontend typecheck/build, UI DOM tests and i18n parity. Record restricted-environment failures separately from product failures.
- Story 16.12 requires deterministic and provider-integration evidence sufficient to fix and verify its policy. Record unavailable provider/platform environments honestly. Sustained installed Windows/macOS/Linux certification is explicitly handed to Story 16.14 and must not be claimed by this story.

### Previous Story Intelligence

Story 16.11 established several patterns that remain relevant even though its basket-export domain is separate:

- Freeze stable identities/revisions at plan time and revalidate inside the owning mutation/admission boundary.
- Replay/admit idempotently before repeating expensive live preflight; bound provider paging/work with deadlines and cancellation.
- Fence stale UI/background results by relevant identity/generation and expose recoverable uncertainty truthfully.
- Move strict Rust DTOs, RPC routing/admission, TypeScript types, i18n, UI and tests together.
- Never claim provider/platform evidence unavailable in the executing environment.

Do not copy the 16.11 durable export state machine into playback adaptation; reuse only these concurrency/evidence disciplines.

### Git Intelligence

- Recent sequence: `6232f74` (Dev 16.10), `0357bef` (Review 16.10), `29a9601` (Story 16.11), `7f43223` (Dev 16.11), `098e3e7` (Review 16.11).
- Current convention is story -> implementation -> adversarial review, with focused playback modules, schema/RPC/UI/i18n/docs/tests updated together and review fixes committed separately.
- No dependency was added by the preceding export stories. The worktree was clean when this story was created.

### Latest Technical Information

- Repository pins already match the current documented CPAL `0.18.2` and `ffmpeg-next` `9.0.0` APIs; do not substitute historical architecture references to CPAL 0.16.
- CPAL 0.18 requires explicit stream start, changed default configuration preferences and exposes per-stream monotonic `StreamInstant`; existing audio initialization owns those concerns. Adaptation must not reopen/reconfigure the output stream.
- `ffmpeg-next` 9's stream-input interrupt API is the supported cancellation seam for stalled reads. Keep network timing around the HTTP/reader layer, not the decoder or audio callback.

### Project Structure Notes

Preferred new file:

```text
hifimule-daemon/src/playback/adaptation.rs  # pure policy, bounded scoped history, decisions/reasons
```

Likely update surface (confirm from implementation; do not touch files without a required contract change):

```text
hifimule-daemon/src/providers/mod.rs
hifimule-daemon/src/providers/jellyfin.rs
hifimule-daemon/src/providers/subsonic.rs
hifimule-daemon/src/providers/audiobookshelf.rs
hifimule-daemon/src/playback/{mod,streaming,http_source,audio,commands,continuity,model,session,output,album}.rs
hifimule-daemon/src/playback/audio/queue_edit_tests.rs
hifimule-daemon/src/playback/session/album_admission_tests.rs
hifimule-daemon/src/playback/decoder_continuity_tests.rs
hifimule-ui/src/rpc.ts
hifimule-ui/src/components/PlaybackControls.ts
hifimule-i18n/catalog.json
docs/api-contracts-hifimule-daemon.md
docs/data-models-hifimule-daemon.md
docs/architecture-hifimule-daemon.md
docs/playback.md
docs/playback-installed-test-checklist.md
```

Avoid broad rewrites of `audio.rs`/`session.rs`; keep the estimator pure and focused, and extend existing ownership seams.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 16 and Story 16.12]
- [Source: `_bmad-output/planning-artifacts/prd.md` — FR71, FR72 and playback non-functional requirements]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Playback Audio Pipeline, Provider Integration, Implementation Contracts, Project Structure]
- [Source: `_bmad-output/planning-artifacts/playback-epic-validation.md` — FR/NFR/architecture/UX traceability]
- [Source: `_bmad-output/planning-artifacts/sprint-change-proposal-2026-09-19.md` — approved playback release split and Story 16.12 mapping]
- [Source: `_bmad-output/implementation-artifacts/epic-16-context.md` — delivery sequence and identity/reporting constraints]
- [Source: `_bmad-output/implementation-artifacts/15-7-seek-within-a-track-and-see-the-actual-playback-position.md` — current buffer/deadline baseline]
- [Source: `_bmad-output/implementation-artifacts/15-9-preserve-album-continuity-across-prepared-track-boundaries.md` — prepared-boundary contract]
- [Source: `_bmad-output/implementation-artifacts/15-10-preserve-an-album-s-relative-loudness-with-consistent-gain.md` — album gain admission]
- [Source: `_bmad-output/implementation-artifacts/16-11-add-a-listening-snapshot-to-a-connected-device-basket-or-replace-it.md` — previous-story learnings]
- [Source: `hifimule-daemon/src/providers/mod.rs` — current representation model/selector]
- [Source: `hifimule-daemon/src/playback/streaming.rs` — bounded compressed reader]
- [Source: `hifimule-daemon/src/playback/http_source.rs` — authenticated HTTP/range source]
- [Source: `hifimule-daemon/src/playback/audio.rs` — pipeline, selection and prepared successor]
- [Source: `hifimule-daemon/src/playback/continuity.rs` — successor fence]
- [Source: `hifimule-daemon/src/playback/output.rs` — callback/refill behavior]
- [Source: `hifimule-daemon/src/playback/model.rs` — public playback state/events]
- [Source: `hifimule-ui/src/components/PlaybackControls.ts` — current accessible status UI]
- [Tokio 1.49 time `Instant` documentation](https://docs.rs/tokio/1.49.0/tokio/time/struct.Instant.html)
- [CPAL 0.18.2 upgrade guide](https://docs.rs/crate/cpal/0.18.2/source/UPGRADING.md)
- [`ffmpeg-next` 9 format input documentation](https://docs.rs/ffmpeg-next/9.0.0/ffmpeg_next/format/index.html)

## Dev Agent Record

### Agent Model Used

GPT-5 Codex

### Debug Log References

- 2026-09-29: Initial Subsonic provider tests were blocked by sandbox socket permissions; the same 72-test suite passed with local mock-server access.
- 2026-09-29: Full workspace regression first exposed two stale persistence-version assertions (`15` vs current `16`); assertions were corrected and the rerun passed.
- 2026-09-29: Strict `clippy -D warnings` remains blocked by 144 pre-existing repository warnings; normal all-target Clippy completes successfully and story-touched code introduces no compile errors.

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- Added policy-v1 track-boundary adaptation with source/representation-scoped monotonic observations, bounded history, minimum evidence and asymmetric downgrade/recovery hysteresis.
- Added stable daemon-private representation descriptors and a verified Navidrome MP3 192 kb/s alternative while keeping Jellyfin and Audiobookshelf explicitly single-representation.
- Integrated decisions only into the existing successor preparation path; non-unity album gain remains constrained to the qualified original representation and current-track/Preview selection is unchanged.
- Published authoritative sanitized quality state through Rust/TypeScript and an accessible localized live-region explanation.
- Documented thresholds, ownership, failure/cutoff behavior, and the Story 16.14 evidence format without claiming installed-platform certification.
- Verification: adaptation tests, selector test, 72 Subsonic provider tests, frontend production build, locale parity, and full workspace suite (1,338 daemon passed / 8 intentional ignores plus all other workspace suites) passed. Normal all-target Clippy passed with existing warnings; strict warnings-as-errors remains pre-existing debt.

### Change Log

- 2026-09-29: Implemented Story 16.12 playback quality adaptation and reproducible evidence handoff; moved story to review.

### File List

- `_bmad-output/implementation-artifacts/16-12-adapt-playback-quality-at-track-boundaries-using-buffer-health.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `docs/api-contracts-hifimule-daemon.md`
- `docs/architecture-hifimule-daemon.md`
- `docs/data-models-hifimule-daemon.md`
- `docs/playback-installed-test-checklist.md`
- `docs/playback.md`
- `docs/playback-evidence/16-12-network-profile-template.md`
- `hifimule-daemon/src/playback/adaptation.rs`
- `hifimule-daemon/src/playback/audio.rs`
- `hifimule-daemon/src/playback/audio/queue_edit_tests.rs`
- `hifimule-daemon/src/playback/http_source.rs`
- `hifimule-daemon/src/playback/mod.rs`
- `hifimule-daemon/src/playback/model.rs`
- `hifimule-daemon/src/playback/persistence.rs`
- `hifimule-daemon/src/playback/session.rs`
- `hifimule-daemon/src/providers/audiobookshelf.rs`
- `hifimule-daemon/src/providers/jellyfin.rs`
- `hifimule-daemon/src/providers/mod.rs`
- `hifimule-daemon/src/providers/subsonic.rs`
- `hifimule-i18n/catalog.json`
- `hifimule-ui/src/components/PlaybackControls.ts`
- `hifimule-ui/src/rpc.ts`
