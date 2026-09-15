---
baseline_commit: 4dae294accd384fd2664fe5d1c49d64efc300064
---

# Story 15.4: Play a selected library track through the daemon

Status: in-progress

## Story

As a HifiMule user,
I want to play a track from a configured music server,
so that I can listen directly in HifiMule without opening another player.

**Requirements:** Initial FR56/58 audible path, FR71 source quality, FR64/75 safety; P-NFR2–6, P-AR5–6, streaming portion of P-AR9, applicable P-AR10/12 and P-UX-DR11/13/14.

**Dependencies:** Stories 15.1–15.3 are done. Preparation baseline: `b3d30a2e110817995de8decd84ae7333a1558ca0` (Review 15.3). This is a production single-track vertical slice: daemon source resolution, controlled native decode, shared default output, browser Play and basic transport. Output selection/media keys (15.5–6), seek controls (15.7), advancement/gapless/gain (15.8–10), preview, full Playback destination/floating bar, Radio, reporting and adaptive quality remain later stories.

## Acceptance Criteria

1. **Play from its source.** Given a playable track on a configured server, when its explicit Play action is activated, the daemon resolves the captured portable server/track identity using existing credentials, buffers and decodes through the controlled runtime, and emits audio through shared output. The UI receives selected-track metadata and actual loading/playing/error state, never an authenticated stream URL. Changing the browsed server or connected device does not redirect playback; no physical device is required.
2. **Independent initial quality.** Given original/alternative representations, selection follows the quality and capability contract below, independently of device sync transcoding. Unsupported provider/representation combinations are explicit. No automatic quality adaptation is claimed.
3. **Pause, Resume and Stop.** Pause holds the same occurrence and consumed position; Resume continues it, including a restored paused session. Stop silences output, cancels its work, retains the queue/current occurrence and checkpoints position zero. Completed-track Resume deliberately restarts that occurrence at zero. No automatic restart follows daemon launch.
4. **Replacement and fencing.** Playing another track or stopping during resolution, fetch, decode or output invalidates obsolete work. Late completions cannot emit audio, publish metadata or update position for the new generation. One session owns output; repeated command identities do not replay audible side effects.
5. **Window independence and usable controls.** Audio continues with the UI closed. Reopening reads authoritative state without issuing Play again. Play/Pause/Resume/Stop have localized accessible names, keyboard activation and visible focus; existing row selection, multi-selection, drag, basket and playlist interactions retain their behavior.
6. **End and failure.** Natural end drains valid decoder/converter output and becomes paused with explicit completed status at the actual terminal position. Source starvation becomes buffering; terminal source/decode failure becomes a recoverable paused error. Neither repeats nor advances to another entry, submits a dislike or grows compressed/PCM storage without bounds.
7. **Output loss.** Loss/invalidation of the active endpoint pauses with a recoverable output error. Music never automatically migrates to another endpoint. A deliberate retry follows the explicit output policy below.
8. **Quit with sync.** Committed Quit fences new work and stops audio before freezing the final position/checkpoint, while sync cancellation proceeds independently. Session persistence and worker joining use the 15.2/15.3 shutdown contract; relaunch stays paused. Audio callbacks never perform blocking IO, persistence or acquire sync/database locks.
9. **Installed evidence.** Installed Windows x64, Linux x64, macOS x64 and macOS ARM64 builds verify supported-format fixtures and configured-server Play/Pause/Stop, restored Resume, replacement during loading, window closure, output loss and shutdown. Record actual loaded native versions, shared endpoint, source-server version, compressed/PCM high-water marks and outcomes. Mock/local-file/ARM64 VM success does not certify untested provider, hardware or architecture combinations.

## Tasks / Subtasks

- [x] Establish controlled audio build and runtime identity (AC: 1, 7, 9)
  - [x] Pin dependencies/build recipe below, bundle private native libraries and record verified source/archive hashes, build flags and notices in a runtime manifest.
  - [x] Add runtime-version/ABI checks and clean-install dependency checks to normal Build and release packaging for all existing targets.
  - [x] Reproduce the six-format native fixture matrix using the production decoder before connecting live provider streams.
- [x] Extend provider-neutral playback resolution (AC: 1–2, 6)
  - [x] Add typed private representation/request description and default unsupported trait behavior; implement Jellyfin and Subsonic original streaming separately from sync download/transcoding.
  - [x] Route captured portable identities through `get_provider_by_server_id`; sanitize all errors and prevent credential forwarding across redirects.
  - [x] Test actual response type, unavailable source, unsupported format, alternative selection and two servers with identical raw track IDs.
- [x] Extend the existing serialized session owner (AC: 3–4, 6, 8)
  - [x] Add atomic PlayTrack and generation-fenced transport operations, bounded deduplication and orthogonal playback status/metadata.
  - [x] Integrate all pre-existing queue operations with the audio owner, preserving active playback on append and stopping obsolete generations on clear/select/replace.
  - [x] Add consumed-position sampling, explicit Stop/completion semantics, paused restoration and checkpoint handling without introducing a second session manager.
- [X] Implement bounded fetch, decode/conversion and shared output (AC: 1, 3–4, 6–9)
  - [x] Add cancellable bounded compressed input with custom FFmpeg IO, worker-confined decoder/converter contexts and preallocated PCM handoff.
  - [x] Support tested WAV, FLAC, ALAC/M4A, MP3, AAC/M4A and Opus input, mono/stereo and explicit endpoint sample conversion; reject unsupported layouts safely.
  - [x] Implement callback gating, output loss, starvation, clean EOF/drain, restored-position preparation and bounded worker retirement.
  - [X] Measure/tune the selected buffer parameters against the streaming matrix and record final values before acceptance.
- [x] Add browser Play and minimal transport UI (AC: 1, 3, 5–7)
  - [x] Add a dedicated track action and a compact transport region in the existing browser shell; wire through the authenticated native RPC proxy.
  - [x] Reconcile snapshots with one bounded polling schedule; use daemon metadata independent of browse context and preserve no-device browsing.
  - [x] Add four-locale strings, keyboard/focus/live-region behavior and behavioral regression tests.
- [ ] Integrate shutdown and produce evidence (AC: 8–9)
  - [x] Add audio-stop acknowledgement/worker completion to committed Quit without delaying sync cancellation or reopening admission.
  - [X] Run deterministic router/session/audio tests, controlled HTTP fixtures, native installed smoke tests and relevant existing regressions.
  - [X] Save sanitized per-target evidence and explicitly retain any unavailable checks as unverified; do not mark this story done based solely on the probe.

### Review Findings

Review date: 2026-09-14. Full diff against `4dae294accd384fd2664fe5d1c49d64efc300064`; blind adversarial, edge-case, and acceptance layers completed, followed by source verification and deduplication. User selected Apply every patch. The findings below retain their original descriptions; checked items were resolved on 2026-09-15.

- [x] [Review][Patch] **R1 / P1: Deduplicate audible effects together with owner commands** [hifimule-daemon/src/rpc.rs:671]. Both PlayTrack and Resume perform audio work after receiving a successful owner result, including cached results. Replaying PlayTrack retires the live pipeline and starts at zero; replaying an earlier Resume after Pause reopens the same-generation gate. Carry a newly-accepted/effect disposition from the serialized owner or dispatch the effect exactly once there. Add router-level replay tests, including Resume → Pause → replay Resume. Violates AC4.
- [x] [Review][Patch] **R2 / P1: Preserve Pause across preparation and late worker events** [hifimule-daemon/src/playback/audio.rs:482]. Pause only closes an existing pipeline gate and preserves generation; if it arrives during resolution/fetch, the eventual start creates `gate = true` and begins playback anyway. Same-generation Active/Buffering events can also overwrite an accepted Pause in the owner (`session.rs:648`). Persist desired transport state/control ordering across startup and validate worker events against it. Test Pause before pipeline creation and an Active event racing with Pause. Violates AC3–4.
- [x] [Review][Patch] **R3 / P2: Propagate terminal demux/source errors instead of reporting completion** [hifimule-daemon/src/playback/decoder.rs:189]. The pinned wrapper's packet iterator ends on terminal read errors as well as EOF. The decoder then drains and can return success; `audio.rs:838` checks shared stream failure only when decode already failed. A timeout/reset after complete packets can therefore become Completed at a truncated position. Use an error-observing packet-read loop and check the recorded source failure before accepting clean completion. Violates AC6.
- [x] [Review][Patch] **R4 / P2: Preserve complete PCM frames and enforce refill after starvation** [hifimule-daemon/src/playback/audio.rs:880]. The callback independently pops individual interleaved samples and substitutes silence for each empty slot. Concurrent refill between stereo channels can place a right-channel sample into a left-channel output slot and shift subsequent channel alignment. The callback also keeps consuming immediately after underrun; the 100-ms fill threshold is applied only at startup. Hand off/consume complete channel frames and remain silent until the refill threshold or clean short-tail condition is met. Violates AC6 and the start/refill contract.
- [x] [Review][Patch] **R5 / P2: Join the decoder on ordinary cancellation** [hifimule-daemon/src/playback/audio.rs:862]. The normal cancellation/generation-change exit returns without joining the decoder handle, detaching it. `stop_and_join` waits only for the outer audio worker and may acknowledge retirement/shutdown while native decode and temporary-file work still run. Cancel and join the decoder on every exit path, preserving the shutdown blocker until all owned workers finish. Violates AC4/8 and bounded worker retirement.
- [x] [Review][Patch] **R6 / P2: Remove the unbounded compressed seek-history spool** [hifimule-daemon/src/playback/streaming.rs:168]. Every evicted byte is appended to an uncapped temporary file when seek history is enabled, which applies to non-MP3/non-FLAC inputs. Seeking to the end drains the whole source first. Long inputs can consume whole-track disk space or exhaust the temporary volume while the advertised memory counter stays bounded. Implement bounded/verified random access or reject unsupported access explicitly; do not substitute a growing disk spool. Violates AC6 and the explicit no-growing-disk-spool contract.
- [x] [Review][Patch] **R7 / P2: Account for all retained compressed data in the budget and telemetry** [hifimule-daemon/src/playback/streaming.rs:114]. The input channel can retain 128 × 64 KiB independently of the reader's 8-MiB sliding window; high-water updates count only the latter (`streaming.rs:177`). Backpressure can retain nearly 16 MiB of compressed payload while reporting at most 8 MiB, excluding producer storage. Use one shared byte budget or report/enforce the aggregate, and update evidence validation accordingly. Violates the bounded-resource and AC9 measurement contracts.
- [x] [Review][Patch] **R8 / P2: Drain the submitted output tail before publishing Completed** [hifimule-daemon/src/playback/audio.rs:833]. Empty PCM plus finished decode causes immediate Completed publication and return, dropping the CPAL stream. PCM removed by the callback may still be queued for backend presentation, so this can truncate a short track/final buffer on backends that discard pending output on stream destruction. Track actual output presentation/drain completion and keep the stream alive through its tail. Violates AC6 and the natural-EOF contract.
- [x] [Review][Patch] **R9 / P2: Enforce the complete preparation deadline** [hifimule-daemon/src/playback/audio.rs:742]. Startup waits for PCM/decoder completion without the specified 60-second wall deadline. The HTTP timeout covers headers and individual no-byte-progress intervals, not demux probing or restored-position decode/discard. A source delivering small chunks more frequently than 15 seconds can keep initial/restored playback Loading indefinitely. Apply a cancellable deadline across resolution, fetch, probe and position preparation, retaining the required recoverable state. Violates AC6 and the preparation contract.
- [x] [Review][Patch] **R10 / P2: Make Resume on an already active pipeline idempotent** [hifimule-daemon/src/playback/session.rs:1234]. Resume unconditionally sets Loading, while `resume_existing` only opens the already-open gate. The worker's local active flag stays true and emits no new Active event, leaving audible playback permanently labeled Loading. A repeated Resume with a new command ID or an in-flight second click triggers this independently of replay deduplication. Preserve active state or explicitly reconcile it. Violates AC1/3.
- [x] [Review][Patch] **R11 / P2: Retain terminal events when the owner mailbox is full** [hifimule-daemon/src/playback/session.rs:315]. `publish_event` discards `try_send` failures. With all 64 command slots occupied, a one-shot Failed/Completed event is lost after its worker exits, leaving stale Active/Loading state and losing the terminal position/error. Provide reliable bounded delivery or retained/coalesced state transitions; test mailbox saturation during completion and failure. Violates AC6.
- [x] [Review][Patch] **R12 / P2: Keep Pause available during loading and buffering** [hifimule-ui/src/components/PlaybackControls.ts:48]. Only Active selects Pause; Loading selects Resume even though pending/starved playback will automatically emit audio when ready. Users cannot pause preparation or starvation while preserving the occurrence/position. Derive the available action from authoritative desired transport state and cover the loading/buffering keyboard path with a rendered behavioral test. Violates AC3/5.
- [x] [Review][Patch] **R13 / P2: Preserve focus when the primary transport action changes** [hifimule-ui/src/components/PlaybackControls.ts:51]. Rendering destroys the focused Pause button, replaces it with Resume, then searches only for the old action to restore focus; the inverse transition has the same problem. Keyboard users lose their place after activating transport. Restore focus to the equivalent primary control and verify the transition in rendered tests rather than source-pattern assertions. Violates AC5.
- [x] [Review][Patch] **R14 / P2: Explain default-output reacquisition before retry** [hifimule-ui/src/components/PlaybackControls.ts:48]. OUTPUT_LOST still offers the generic Resume label and a generic loss message. After headphones disconnect, this action can deliberately reopen on speakers without the specified explanation. Add localized wording that retry resumes on the current default output, before activation. Violates AC7 and the explicit output retry policy.
- [x] [Review][Patch] **R15 / P2: Dispose playback polling when its layout is removed** [hifimule-ui/src/main.ts:458]. The controller is not retained for disposal; only pagehide invokes destroy. Although the existing layout marker prevents ordinary reload remounts, removing the last server replaces the layout with login, and shutdown rendering also removes it without disposal. Adding a server later creates another polling controller while the old detached one continues. Retain/destroy the active controller on those transitions and remove its pagehide listener. Violates the single-polling/disposal contract.
- [x] [Review][Patch] **R16 / P2: Surface transport command failures** [hifimule-ui/src/components/PlaybackControls.ts:57]. The asynchronous handler has only try/finally. Conflict, checkpoint and connection failures become unhandled rejections and silently re-enable the button; a failed Stop checkpoint is not represented by the transient playback error field. Catch and present the structured failure, reconcile authoritative state, and exercise error behavior through the rendered controls. Violates usable controls and the persistence-error contract.

Review validation: the controlled Windows daemon command completed with **739 passed, 0 failed, 5 ignored**; the Node script suite passed with one platform skip; all **8 Python evidence tests passed**; TypeScript and the Vite production build passed (existing bundle warnings). No new regression tests or native listening scenarios were executed for these findings; the defects above are established by source/control-flow inspection. Linux x64 and macOS x64 installed acceptance remain existing open evidence tasks. One speculative Jellyfin profile finding was dismissed because no concrete broken supported-provider path was established. No decision-needed or pre-existing deferred findings remain.

## Dev Notes

### Selected implementation contract

These are preparation decisions, not claims that audio APIs or runtime packages already exist. Build/runtime and measured streaming verification are implementation acceptance work. The architecture's broader unresolved gates do not authorize implementing later playback features.

#### Native runtime and packaging

- Keep Rust edition 2024/MSRV 1.93.0 and existing workspace dependency versions. Add exact `cpal = 0.16.0`, `ffmpeg-next = 9.0.0` and lock `ffmpeg-sys-next = 9.0.0`; use `default-features = false` with codec, format and software-resampling support needed by the implementation. Use the experiment's exact `crossbeam-queue = 0.3.12` for preallocated bounded handoff if retaining that queue implementation. Do not add Symphonia, CLI FFmpeg or Python to the production audio path.
- Select **FFmpeg 9.0.1**, built from the official signed release source, as the controlled native baseline. Build shared, audio-only libraries with native decoders/demuxers for the six fixture formats and libswresample; disable programs, network protocols, GPL/nonfree components and unrelated video/device/filter features. Network access belongs to the daemon HTTP layer/custom AVIO, not FFmpeg URL opening. Resolve the exact dependency closure with the build, record configure flags and hashes, and fail if a required demuxer/decoder/converter is absent. Never substitute Homebrew/distro/rolling downloaded binaries.
- Package libavcodec/libavformat/libavutil/libswresample and their actual shared dependency closure. Windows: private DLLs beside the installed daemon and matching import libraries at build time. macOS: retain `Contents/Resources/bundled-libs`, with daemon references through `@loader_path/../Resources/bundled-libs` and library-to-library sibling references through `@loader_path`; sign nested libraries before the app. Linux: private `$ORIGIN`-relative libraries in every AppImage/deb package, built against the existing Ubuntu 22.04 x64 baseline. Preserve launchd, NSIS/WiX and sidecar placement.
- Capture runtime library versions and loaded paths from the installed process, plus source hash/configuration and architecture. Build-time pkg-config success is insufficient: the Linux probe initially loaded FFmpeg 8 despite version-9 metadata. Reject ABI/version mismatch in build/install validation. Shipping libraries must load on a clean machine without system FFmpeg or developer search paths; an absent library must produce an actionable launch failure, never indefinite connection polling.
- Existing native evidence used avcodec/avformat 63.1.101 and avutil 61.1.101, but do not blindly copy those numbers into the new build's manifest: derive and verify the selected 9.0.1 build, including swresample. Test Windows x64, Linux x64 and both macOS architectures separately. Preserve appropriate source/license notices for the deliberately selected build.
- FFmpeg wrapper contexts remain created/used/destroyed on their owning worker. The experiment documented an `Rc`/unsafe Send concern in the released wrapper; audit the exact wrapper paths used and prevent contexts or borrowed frames crossing threads regardless of upstream Send declarations. Only owned bounded PCM/data and sanitized status cross boundaries.

#### Provider boundary and initial quality

Add a playback-specific method to `MediaProvider` in `providers/mod.rs`, for example `resolve_playback(track_id) -> PlaybackDescription`, with an explicit unsupported default so existing test providers remain valid. Keep safe normalized metadata in `domain/models.rs` and private request types in the provider boundary; no provider-specific URL construction in session/audio/UI code.

The private description contains source-qualified identity, bounded display metadata, optional duration, an ordered bounded set of representations (maximum 8), actual/unknown codec/container/bitrate/sample-rate/bit-depth, direct versus transcoded provenance, and verified range/offset capability. Each representation carries a daemon-only HTTP request/authentication descriptor. It must not implement unrestricted debug/serialization that prints secrets. Wire metadata exposes only safe fields and never headers, tokens, URLs or server error bodies.

Use `server_manager::get_provider_by_server_id` to translate portable identity to the machine-local credential/provider cache. `selected_provider` and the UI's local server UUID are wrong for playback. `get_song` supplies metadata; resolve credentials when opening/reopening a request rather than persisting authenticated URLs.

Initial deterministic ranking: supported original first; then explicitly available lossless alternatives (higher known sample rate/bit depth within decoder/output support); then explicitly available lossy alternatives ordered by known bitrate within the same codec. Across incomparable lossy codecs retain provider order, rather than equating bitrate with fidelity. Unknown quality stays unknown. Choose once before output; do not change quality during the track or claim measured sustainability. Capability rejection before output may try the next advertised representation, bounded by the eight-entry list; authentication/network failure is not evidence to downgrade.

- **Jellyfin:** extend its adapter using the existing item/PlaybackInfo/audio request machinery, with a playback-specific direct-play profile independent of physical-device profiles. Prefer the original authenticated audio/download response. Only advertise negotiated alternatives actually supported by the returned source/server capability; preserve required authentication inside the daemon.
- **Subsonic/OpenSubsonic:** use authenticated `stream` with `id` and `format=raw` for the original representation. A transcode is eligible only when adapter/server capability evidence supports it; do not assume arbitrary `format`/`maxBitRate` will be honored. Classic server branding alone does not prove OpenSubsonic extensions. Audio `timeOffset` is not generally available without its extension.
- Existing `download_url(None)` remains a sync API. Reusing internal URL/auth helpers is appropriate; exposing that method as the complete new playback capability contract is not.
- Both original-provider paths fit this shared vertical slice. Do not implement a comprehensive transcode-negotiation matrix merely to populate alternatives. If production provider inspection uncovers a required incompatible integration too large for this story, split ordered provider enablement before executing that expansion and preserve approved coverage; do not silently label an entire promised provider supported on a mock result.

Validate HTTP status and media response before decode, including HTTP-200 XML/JSON provider errors. Limit diagnostic-body reads. Keep authenticated redirects same-origin by default; do not forward credentials to another origin without an adapter-defined authenticated route. Do not let nested playlists cause FFmpeg to open files/network URLs. No persistent whole-track cache or managed-device staging is used.

#### Commands, state and persistence

Extend `playback.applySession`'s existing schema-v1 envelope with `operation: { type: "playTrack", source: { serverId, trackId } }`. Preserve instance/session/command identity, expected queue revision, unknown-field rejection and bounded command deduplication. PlayTrack atomically replaces the queue with one newly generated occurrence, resets position and generation, and commits before initiating asynchronous source work. Return admission/committed state, not a false claim of audible success. Network/decode failure leaves that selected occurrence recoverable.

Add `playback.control` with `{ schemaVersion: 1, instanceId, sessionId, commandId, expectedGenerationId, occurrenceId, action: "pause" | "resume" | "stop" }`. Fence the current occurrence/generation at execution; reject stale commands using `409` plus authoritative metadata. It shares the existing owner's deduplication scope and bounded mailbox. Reusing an ID with a different payload fails. Record the accepted result before asynchronous effects so an RPC timeout/retry cannot restart audio. Register it as mutating, retain admission guards through owner execution, and reject normal controls during committed shutdown.

Use existing `idle/paused/buffering/playing/stopping` transport variants. Add an orthogonal snapshot `playback` object with `status: "idle" | "loading" | "active" | "paused" | "stopped" | "completed" | "error"`, optional sanitized error `{ code, retryable }`, current safe metadata/representation and optional `durationMs`. This is additive wire data; preserve schema-v1 paging and numeric representations. Completed/error transport is paused, avoiding an unnecessary persistence enum migration. Transient failure/status metadata need not survive restart; durable current/position does, and restart always restores paused.

| Event | Required transition |
| --- | --- |
| PlayTrack | Silence/fence prior generation, commit one new occurrence, enter buffering/loading; playing/active only after current-generation PCM begins consumption. |
| Pause | Gate output/consumption, capture consumed position and publish paused; same occurrence/generation and PCM cursor remain. Do not rely solely on CPAL pause support. |
| Resume | Continue paused cursor. After process restart/released source, recreate the source and prepare its checkpointed position without emitting discarded samples; publish loading until ready. |
| Stop | Gate output, rotate generation, cancel work, clear buffered PCM, retain queue/current, set position zero and checkpoint it; paused/stopped (empty queue remains idle). |
| Natural EOF | Drain decoder and resampler, then submitted PCM/output tail; publish paused/completed at actual terminal position, with no next/repeat. Resume from completed is deliberate restart at zero in a fresh generation, retaining occurrence. |
| Starvation | Emit silence without advancing media position, enter buffering; resume only for the same generation after the refill threshold. |
| Terminal fetch/decode error | Cancel producer, retain last consumed position, publish paused/error and offer deliberate Resume/retry. Never treat arbitrary decode errors as EOF. |
| Output loss | Atomically gate output, cancel/release stream, keep position, paused/error `OUTPUT_LOST`. No automatic endpoint lookup/reopen. Explicit Resume may reacquire the then-current shared default only with UI wording that makes that action clear. |

Use error codes `SOURCE_UNAVAILABLE`, `PLAYBACK_UNSUPPORTED`, `DECODE_FAILED`, `OUTPUT_UNAVAILABLE`, `OUTPUT_LOST`, `PLAYBACK_TIMEOUT` and `RESUME_UNAVAILABLE`, retaining existing structured persistence/version/busy/conflict errors. Restore errors still block mutation and remain observable independently of audio. Do not downgrade checkpoint failure into an audio error or reload DB state after a live persistence failure.

Progress uses actual consumed media frames mapped to milliseconds, not downloaded bytes, decoded-ahead frames or wall time. Silence/paused callbacks do not advance it. Retain generation/occurrence/sample sequencing and 250-ms coalescing; ordinary progress/transport does not change queueRevision, but accepted state changes advance checked stateSequence. Clip only to verified duration where appropriate; unknown duration stays unknown. Do not infer completion merely from metadata duration.

Internal positioning is necessary for restored Resume even though seek controls belong to 15.7. Use verified random access/range and demux seeking where available; otherwise cancellably decode and discard up to the target with the same bounded buffers. Never emit from zero while displaying the saved position. A failed/unsupported preparation retains paused position with `RESUME_UNAVAILABLE`; no silent rewind. No seek RPC or slider is added here.

Integrate old `applySession` operations: append to a nonempty active session preserves current transport/output/generation (today AppendQueue incorrectly forces paused); clear/select/replace must gate the old stream before publishing their new paused state. Stop/reset needs fresh generation because existing ingress rejects backward samples. `checkpoint_playback_position` currently saves position/sequence only; ensure transition handling persists the selected position coherently without relying on transport persistence to prove playback. Retain 5-second dirty checkpoints, offline restoration and bounded SQLite queries.

#### Bounded audio pipeline

Provider HTTP → bounded compressed prefetch → worker-local FFmpeg demux/decode/libswresample → bounded preallocated PCM → one CPAL shared stream. Custom AVIO can wait on the decoder worker; never on the audio callback or RPC runtime. Implement interrupt/cancellation and explicit EOF/error signaling. Random-access containers such as tail-moov M4A require verified range-backed AVIO or explicit unsupported behavior, not whole-file buffering disguised as streaming.

The following are selected initial engineering limits, **not measured performance claims**. Instrument high-water values and tune thresholds against the required HTTP/native matrix before acceptance; record any changed final constants and rationale here.

| Resource | Initial limit / behavior |
| --- | --- |
| Compressed prefetch | 8 MiB maximum; chunks at most 64 KiB; backpressure producer, no full-body collection or growing disk spool. |
| PCM | At most 500 ms of interleaved f32 and at most 1 MiB, using the smaller bound at negotiated output rate/channels; fixed preallocated storage. |
| Start/refill threshold | 100 ms PCM or clean short-track EOF with available frames; no indefinite wait for a short track to fill the threshold. |
| Demux/probe | 1 MiB probe input and 5-second analysis budget; cap individual compressed packet/decoded frame allocations at 1 MiB each where enforceable, reject oversized/unsupported media. Track FFmpeg internal allocation/RSS separately; queue limits do not bound total native heap. |
| HTTP | 10-second connect, 15-second no-byte-progress timeout; distinguish intentional producer backpressure/pause from a network stall. At most one representation stream/range request active per generation. |
| Preparation | 60-second wall budget for initial open or restored-position preparation, cancellable throughout; failure retains source/position and explicit retry state. |
| Worker retirement | One active pipeline and at most one retiring pipeline; coalesce pending replacements to the latest accepted generation, never spawn an unbounded thread per click. |
| UI refresh | One in-flight snapshot request, normally every 500 ms while mounted; stop on disposal/shutdown and discard stale owner/session/sequence responses. |

Support mono/stereo input with explicit sample-rate/channel/sample-type conversion to a supported shared endpoint configuration. Keep the output rate stable for this track. Use libswresample for required conversion and drain it on EOF; reject unsupported layouts/rates explicitly rather than assuming every endpoint is f32/48 kHz/stereo. Preserve known encoder padding handling and recorded silence; no fixture-length trimming, ReplayGain or album-continuity claim.

The callback performs only bounded reads/sample conversion, silence filling and atomic state/counter updates. Preallocate its storage; do not log, allocate/free heap-owned chunks, call provider code, block on channels, or acquire session/database/sync locks there. Pause/Stop/replacement/output loss gate the callback immediately; late PCM is generation-tagged or belongs to an isolated retired queue. Account for frames already submitted to the backend: document observed stop/drain latency instead of claiming instantaneous physical silence. No unbounded callback scan to discard stale queues.

Shared output means CoreAudio/WASAPI shared mixing, and Linux desktop PipeWire/Pulse-compatible ALSA routing, not raw exclusive hardware selection. CPAL 0.16's Linux default ALSA backend does not itself prove desktop mixing; verify the actual selected route with another application. If safe shared routing is unavailable, expose an output error rather than silently opening exclusive hardware. Do not change OS system audio settings.

#### Shutdown integration

Preserve the committed launch fence, same shutdown ID and 5-second warning deadline. Order the normal committed path as output gating acknowledgement/final consumed-position capture → initiate the frozen session checkpoint → request sync cancellation, without awaiting the SQLite write. Output gating/counter capture must use the independent audio control boundary, not wait for decoder/network workers to join. Bound the acknowledgement wait to 250 ms; if it stalls, still cancel sync, retain an audio-stop blocker and ownership, and defer the final capture/checkpoint until output is confirmed stopped. Never claim a clean checkpoint of moving audio. Joining audio/fetch/decode must also precede ownership release; dropping a task handle is not proof it stopped.

Expose an additive audio-stop/worker blocker through existing shutdown status if teardown remains pending or fails. Keep health and tray/UI responsive; no forced process exit, silent worker abandonment or admission reopening. Retain checkpoint-specific retry for the frozen session and existing sync-integrity handling. Closing only the UI never invokes Stop or committed Quit.

### Source tree change and preservation map

| File / area | Current state → change; preserve |
| --- | --- |
| `hifimule-daemon/src/playback/{model,session,persistence,mod}.rs` | Existing single-owner paused-session/persistence module → add transport dispatch and audio events. Preserve UUID/source identity, canonical revisions, bounded mailbox/dedup/pages, blocked restoration, coalesced progress and final checkpoint worker joining. |
| `hifimule-daemon/src/playback/{audio,decoder,streaming}.rs` (NEW) | Architecture's output/PCM, worker-local decoder/conversion and bounded fetching modules. Do not create Radio/preview/output-picker skeletons. Keep unsafe custom-AVIO ownership contained and tested. |
| `hifimule-daemon/src/providers/{mod,jellyfin,subsonic}.rs`, `src/domain/models.rs` | Existing MediaProvider trait, normalized metadata, authenticated HTTP helpers and device download/transcode APIs → playback-specific description/resolution. Preserve provider defaults, URL sanitation, configured credentials, portable routing and sync behavior. |
| `hifimule-daemon/src/server_manager.rs` | Existing portable-ID lookup/cache is the routing seam; reuse it, do not replace selected-server state or add a parallel credential cache. |
| `hifimule-daemon/src/{rpc,main,sync}.rs` | Authenticated dispatch, two runtimes, Tao loop and shutdown coordinator → wire one audio owner and stop acknowledgement. Preserve mutation guards, independent health, safe sync drain, checkpoint retries and ownership-release ordering; update all AppState fixtures. |
| `hifimule-daemon/src/db.rs` | Database integration seam; playback schema/queries live in `playback/persistence.rs`. Change only if necessary; preserve existing server/device/history tables and transactional restoration evidence. No new Radio/reporting tables. |
| `hifimule-ui/src/{library,rpc,main}.ts`, `src/styles.css` | Existing browser/controllers and native bridge → dedicated Play action, typed transport calls and compact mounted status. Preserve captured server identity, browser state, shutdown polling and existing layout. |
| `hifimule-ui/src/components/MediaCard.ts` | `library.ts::renderGrid` delegates cards here; add Audio-only explicit Play while preserving normal card navigation, basket, drag and selection. |
| `hifimule-ui/src/components/TracksBrowseView.ts` | Existing track rows/multi-selection → explicit Play affordance with event isolation; retain bulk actions, drag and paging. Other track-rendering sites in `library.ts` must offer the same action. |
| `hifimule-ui/src/components/PlaybackControls.ts` (NEW) | Minimal transport/status component; read-only snapshot projection, not another queue/session owner. No floating bar/full destination/settings in this story. |
| `hifimule-ui/src-tauri/src/lib.rs` | Existing authenticated proxy/lifecycle observation → allow new methods through established boundary; keep owner/epoch fencing, no-election refresh and detached lifetime. |
| `hifimule-i18n/catalog.json` | Four-locale shared catalog → matching playback labels/errors/placeholders; retain shutdown retry wording. |
| `Cargo.toml`, `Cargo.lock`, `hifimule-daemon/Cargo.toml`, `scripts/prepare-sidecar.mjs`, `scripts/bundle-macos-libs.mjs`, `.github/workflows/{build,release}.yml`, `hifimule-ui/src-tauri/tauri.conf.json` | Add exact runtime build/bundle validation; preserve existing installer, sidecar, startup and smoke-test paths. Existing mac traversal detects Homebrew libmtp dependencies only: explicitly include the controlled FFmpeg prefix/dependency closure and validate rewritten load paths. Tauri currently maps only libmtp into AppImage; add all required private audio libraries to actual package layouts. |
| `docs/api-contracts-hifimule-daemon.md`, daemon/UI tests, `scripts/` | Document additive contract, implement isolated evidence harness and regressions. Retain `scripts/playback-session-evidence.py` as session-only evidence. |

### UI contract and preservation

Use explicit per-track Play buttons (native button semantics, localized name including track). Capture portable source identity when rendering the row; asynchronous handlers must not resolve from whichever server is selected later. Stop propagation/prevent drag initiation on the dedicated action as appropriate so Play does not toggle selection or basket membership. Keep card/row double-click and existing keyboard selection unchanged.

`BrowseTrack` and mapped display items currently lack source identity. Capture the portable ID when initiating the request/creating its view, carry it through result mapping, and reject stale fetch completion after server switches; looking up the active server at click or merely checking `container.isConnected` is insufficient. `navigateToBrowseItem` deliberately treats Audio as a leaf no-op; keep plain-click behavior and introduce only the dedicated action.

Mount a compact transport/status region in the existing browser shell with current title/source, loading/error status, Pause/Resume and Stop. Keep it available when navigating browser modes and when no physical device exists, using daemon metadata on reconnect. A browsing error or server removal cannot destroy controls for the current session. Reserve layout space so controls do not cover the last row. This interim region is not the later floating bar or Playback destination.

Mount from `renderMainLayout` outside the frequently destroyed `#library-content`; playback must survive `TracksBrowseView` destroy/remount. Dispose UI polling on `pagehide` and shutdown rendering without stopping daemon audio. Preserve structured playback errors in `rpc.ts` (the current wrapper discards error data into plain Error). Do not extend selected-server automatic reauthentication to playback: any explicit reconnect/credential recovery must target the playing source's server. The native generic proxy already forwards playback methods; preserve it rather than adding another bridge. Follow existing Shoelace controls, design tokens and Outfit/Inter typography.

Disable only unavailable actions with an understandable reason. Output-loss retry must say it will resume on the current default output; do not auto-retry on device arrival. Use polite status announcements for meaningful transitions, not a live announcement every 500-ms position update. Escape server metadata; do not insert raw titles as HTML. Preserve focus across snapshot updates and clean up timer/listeners when remounting. Bind browser timers through `globalThis` as established by the Linux startup fix.

### Previous story and Git intelligence

- `b3d30a2` (Review 15.3) corrected 12 findings: committed asynchronous checkpoint participation, permanent final fence/worker join, off-executor DB reads, exact camelCase selection, shared startup/retry validation, unavailable-schema diagnostics, fresh-session retry, nonblocking ordered progress, authoritative conflict/replay metadata, post-commit error handling, cleared persistence status and canonical integer/cursor bounds. Preserve each when adding audio.
- `0054e51` (Dev 15.3), `22a37da` (RPC/lifecycle), `c9d9a5e` (migration/interruption), `ac9a3fc` (bounded lookups) establish production APIs and real-file evidence. Do not copy the experiment's second session manager or use whole-queue snapshots to implement Play.
- Post-review evidence: macOS ARM64 full daemon 688 tests plus native/lifecycle/localization, ten frontend tests and build passed. Earlier four-platform evidence predates the latest review patches. These are historical facts, not verification of this story.
- Probe findings: native FFmpeg 9 passed six generated formats; distro FFmpeg 8 failed AAC lengths on Linux; pkg-config did not guarantee loaded libraries; file-only AVIO, first-audio-stream choice and packet-boundary WAV truncation needed review fixes. Its predecode, same-rate stereo restriction, fixed 250-ms tail wait and process-level deadline are experiments, not production streaming solutions.
- The project-context file's greenfield status is stale. Retain its managed-device ownership and provider abstraction principles; approved playback amendments and current code supersede old no-device browsing restrictions.

### Testing requirements

Use deterministic barriers, fake endpoint callbacks and a controlled HTTP server for races, plus real output and installed builds for claims that require them. The production path must be exercised through authenticated RPC, not only private audio helpers.

| Area | Required cases |
| --- | --- |
| Contract/routing | Exact JSON, wrong token/owner/generation, repeated command ID, changed payload with same ID, timeout/retry, shutdown rejection, two servers sharing track ID, browse-server switch during open, removed/offline source, no URL/credential leakage. |
| Session | Play replaces once; Pause/Resume retains occurrence; Stop persists zero; completed Resume restarts deliberately; restart at nonzero position stays paused then resumes accurately; append while playing preserves state; clear/select/replace gate old output; progress never increments queue revision. |
| Streaming | Chunked/slow HTTP, short track, HTTP-200 provider error, range honored/ignored, tail-moov M4A, truncated body, stalled source, rejected redirect, malformed media, nonseekable restore, cancellation while AVIO waits, preparation timeout, unsupported format/layout and bounded alternative attempts. |
| Audio | Six-format decoded frame/PCM matrix, resampling 44.1↔48 kHz, mono/stereo and integer endpoint conversion, decoder/converter draining, silence on underrun, consumed-position accuracy, no callback allocation/blocking, queued old-generation PCM, rapid replacement, Pause while buffering, backend error during final drain. |
| Output | Missing default, shared mixing with another app, unplug/invalidation, no automatic fallback, explicit retry after default changes, backend pause unsupported. Capture endpoint/backend identity; silent counters alone do not prove audible output. |
| Lifecycle | Close/reopen during playing and buffering; simultaneous Play/Quit; stalled decode or persistence while sync cancels; frozen latest position, checkpoint retry, pending worker blocker, no ownership release before joins, paused restart. |
| UI | Real rendered Play keyboard action, selection/bulk/drag unaffected, no-device path, changed-server race, single polling lifecycle, stale snapshot ignored, escaped metadata, preserved focus, localized accessible loading/error/retry and Stop. |
| Resources | Compressed/PCM high-water under fast producer/slow consumer, long track, paused producer, repeated replacements and intermittent network. Record RSS/native heap separately, remaining worker count and cleanup; do not call buffer caps a total-memory guarantee. |

Run `rtk cargo test -p hifimule-daemon`, `rtk cargo test -p hifimule-lifecycle`, `rtk cargo test -p hifimule-ui --lib`, localization tests, frontend behavior tests/build, `rtk cargo fmt --all --check` and relevant clippy. Preserve existing real-file restoration, shutdown and provider/sync regression suites. Tests involving process-global lifecycle state require serialized fixtures. Distinguish sandbox localhost/audio restrictions from product failures.

Extend normal Build evidence and installed smoke checks for all four shipping target rows. Record OS/architecture, source revision, package/runtime hashes, actual library versions/paths, endpoint route, server brand/version, fixture, observed transition, decoded/consumed frames, buffer high-water, timeout/underrun and cleanup outcome. Include a configured Jellyfin and configured Subsonic/OpenSubsonic original-stream smoke; unsupported server variants remain documented. Never use the user's live DB for injected corruption. No code execution, native runtime validation or listening tests are claimed by story preparation.

### Current technical research and references

Checked 2026-09-12: official FFmpeg download identifies 9.0.1; docs.rs identifies ffmpeg-next 9.0.0 and newer CPAL 0.18.2. Retain CPAL 0.16.0's measured baseline; adopting the newer error/API model requires separate platform validation. FFmpeg security listings are version-specific, not a blanket security certification of this custom build. Verify the actual dependency lock/build against relevant advisories when implementing.

- [FFmpeg source releases](https://ffmpeg.org/download.html), [security notices](https://ffmpeg.org/security.html), [custom AVIO](https://ffmpeg.org/doxygen/trunk/structAVIOContext.html). Trunk documentation is conceptual; compile against the pinned headers.
- [CPAL 0.16.0 documentation](https://docs.rs/cpal/0.16.0/cpal/): output configuration, optional default devices and backend-specific pause/error handling.
- [ffmpeg-next 9.0.0 source](https://docs.rs/crate/ffmpeg-next/9.0.0/source/), [upstream build notes](https://github.com/zmwangx/rust-ffmpeg/wiki/Notes-on-building): audit pinned wrappers and native linking.
- [OpenSubsonic stream](https://opensubsonic.netlify.app/docs/endpoints/stream/): raw format, bitrate request, binary/error response and conditional audio offset semantics. Jellyfin documentation portal was unavailable during preparation; use repository adapter contracts and version-specific upstream controller/OpenAPI evidence when validating its actual server response.
- [Source: `_bmad-output/planning-artifacts/epics.md` — complete Epic 15, Story 15.4 and implementation gates]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Playback Audio Pipeline, Provider Integration, Implementation Patterns, Validation Refinements and Handoff]
- [Source: `_bmad-output/planning-artifacts/prd.md` — Playback Extension; `_bmad-output/planning-artifacts/ux-design-specification.md` — Playback and accessibility amendments]
- [Source: `_bmad-output/planning-artifacts/playback-epic-validation.md`, `playback-story-review.md`, `project-context.md`]
- [Source: `_bmad-output/implementation-artifacts/15-3-preserve-and-restore-a-paused-listening-session.md` — reviewed contract, findings and evidence limits]
- [Source: `_bmad-output/implementation-artifacts/playback-feasibility-results.md`, `playback-linux-vm-results.md`, `playback-windows-vm-results.md`; `experiments/playback-probe/Cargo.toml` and native decoder]
- [Source: production files in the change map and Git commits above]

## Dev Agent Record

### Agent Model Used

GPT-6 (Codex)

### Implementation Plan

- Extend the existing playback owner and authenticated RPC contract before starting asynchronous provider/audio work.
- Resolve portable provider identity into private authenticated requests, then stream through bounded custom FFmpeg IO into one generation-gated CPAL output.
- Add captured-source Play actions and an authoritative compact transport while preserving existing browse, basket and selection behavior.
- Package the pinned runtime manifest/notices and expose sanitized installed evidence through authenticated daemon health.

### Debug Log References

- 2026-09-12: Resolved create-story customization/configuration; analyzed planning, previous-story review, production session/provider/UI/packaging boundaries and official runtime documentation.
- 2026-09-12: Dev workflow baseline captured at `4dae294accd384fd2664fe5d1c49d64efc300064`. Local runtime inspection found Darwin ARM64 with libavcodec/libavformat `63.1.101`, libavutil/libswresample `61.1.101`/`7.1.101`, but the required installed Windows x64, Linux x64 and macOS x64 environments plus configured Jellyfin/Subsonic servers are unavailable. Halted before implementation under the required-configuration/evidence gate; no task was marked complete and no runtime claim was inferred from probe fixtures.
- 2026-09-12: User explicitly requested implementation proceed and retained installed Windows/Linux/macOS/provider listening verification for their platform runs.
- 2026-09-12: Implemented and regression-tested the daemon/provider/session/audio/UI vertical slice. A macOS ARM64 `.app` packaging run exposed and then verified a fix for FFmpeg ABI-symlink bundling. Installed audible/provider/output-loss evidence remains unverified.
- 2026-09-12: Ubuntu ARM64 exposed host FFmpeg 8 (libavcodec ABI 62); added an architecture-neutral, source-hash-pinned FFmpeg 9.0.1 Linux build cache, exact receipt/ELF validation, recursive private-library closure bundling and installed AppImage/deb checks. Windows ARM64 exposed the incorrectly unconditional Unix `pkg-config` verifier; Windows now uses a target-aware MSVC FFmpeg prefix preflight and stages validated DLLs without requiring `pkg-config`.
- 2026-09-12: Reproduced the reported one-second FLAC failure with the supplied 23.9 MB track containing attached artwork. FFmpeg probing exceeded the bounded reader's retained window and could not honor the advertised backward seek. Native FLAC is now detected from its `fLaC` signature and exposed as sequential custom IO; the supplied 194-second file decodes completely.
- 2026-09-12: Split native-library Tauri resources into non-overlapping macOS/Linux/Windows platform configs after the cross-platform DLL glob broke macOS packaging. The first DMG attempt failed because sandboxed `hdiutil` returned “device not configured”; the identical disk-image operation and full Tauri build succeeded with macOS disk-image access.
- 2026-09-12: Ubuntu ARM64 then reached `ffmpeg-sys-next` bindgen but failed when glibc's `limits.h` could not find Clang's compiler resource header. Linux preparation now preflights Clang/libclang/glibc headers with exact Ubuntu install guidance, validates the native header chain, and passes the discovered resource directory and libclang path into Cargo.
- 2026-09-12: Review identified raw Cargo as an unsupported escape from native-runtime setup and a possible Clang/libclang version mismatch. Added a cross-platform daemon build wrapper that derives the rustc host, prepares the matching platform environment, and forwards Cargo arguments; standard Ubuntu now pairs libclang with the invoking Clang resource tree rather than an arbitrary configured library.
- 2026-09-12: Native Windows ARM64 still lacked an FFmpeg SDK. Added automatic provisioning from the dated BtbN LGPL shared 9.0 build with per-architecture immutable artifact names, SHA-256 validation, staged extraction/receipt validation and complete DLL staging; explicit `FFMPEG_DIR` remains a validated override.
- 2026-09-12: Follow-up review fixed the cross-target wrapper path, completed fresh-machine prerequisite and wrapper-based run/test documentation, and retained the third-party Windows binary choice as a pending contract deviation rather than claiming Story 15.4 acceptance.
- 2026-09-12: Linux ARM64 exposed a redundant daemon build-script `pkg-config` failure after the wrapper had already validated the controlled prefix. Added a sanitized, exact-value verification handoff so Cargo skips only that duplicate probe; raw Cargo remains fail-closed. Windows ARM64 exposed Node 24 passing the `Array.map` index into `path.basename` as its suffix argument; staging now uses an explicit one-argument callback.
- 2026-09-13: User smoke testing confirmed working MP3, M4A and FLAC playback on Windows, Linux and macOS with the latest changes. Recorded this as cross-platform audible evidence while retaining the full installed-evidence matrix and controlled Windows runtime contract as open gates.
- 2026-09-13: User confirmed the Windows build produced a working installation package and that installation succeeded. Combined with the playback smoke result, the Windows build/package/install/playback path is operational; formal package identity and loaded-library evidence remain open.
- 2026-09-13: Captured sanitized Windows x64 installed evidence: Windows 11 10.0.22631 x64, NSIS package SHA-256 `f6fafcbf8950580e9e3b8a4ab295894bbc9896fcc122a151d9935f53ddededd6`, and avcodec 63, avformat 63, avutil 61 and swresample 7 DLLs loaded from the installed `%LOCALAPPDATA%\HifiMule` directory. Unsupplied acceptance fields remain explicitly unverified.
- 2026-09-13: Completed a user-run Windows x64 manual FFmpeg 9.0.1 official-source proof. Localized MSVC required `VSLANG=1033`, the install required `.lib` generation from `.def`, and Rust bindgen required LLVM/libclang. The final HifiMule build and deployed build succeeded and played every tested file.
- 2026-09-13: User explicitly confirmed tested AAC, ALAC, FLAC, M4A, Opus, WAV and MP3 playback on Windows, Linux and macOS with the latest changes, closing the three-platform format smoke matrix. Per-target architecture/identity and lifecycle/resource evidence remain separate open fields.
- 2026-09-13: Replaced the default Windows BtbN binary provisioner with automated official-source FFmpeg 9.0.1 builds for x64/ARM64. The wrapper now verifies the pinned SHA-256 and detached release signature in an isolated keyring, forces English MSVC detection, builds the audio-only shared runtime, generates MSVC import libraries and validates the exact receipt/ABI before Cargo. Build and Release install the required MSYS2 tools; 45 script tests pass (1 platform skip).
- 2026-09-13: Reproduced the ordinary-PowerShell Windows build failure where Visual Studio was installed but `cl.exe`/`lib.exe` were absent from `PATH`. Added `vswhere` discovery and automatic target-matched `vcvarsall.bat` environment capture. Verified on the local x64 Build Tools installation that `LIB`, `INCLUDE` and the MSVC path are loaded; added a regression test for this launch mode.
- 2026-09-13: The next ordinary-PowerShell run exposed that NASM was not installed. Since the controlled Windows runtime is audio-only and does not require x86 assembly for format support, added a Windows-only `--disable-x86asm` configure flag and removed NASM from Windows local/CI prerequisites; Linux retains its optimized NASM build.
- 2026-09-13: The following source-build attempt exposed accidental selection of Windows/WSL `bash.exe`; Windows paths passed the Node existence check but were invisible inside WSL. Added explicit Windows-to-MSYS drive conversion and restricted shell discovery to real MSYS2 roots (`MSYS2_ROOT`, `C:\msys64`, `C:\tools\msys64`) with GNU Make validation and actionable setup guidance.
- 2026-09-13: A real MSYS2 retry verified source hash/signature but `configure` could not find `sed`, causing the misleading `Unknown option --toolchain=msvc` cascade. The tools existed under MSYS2; `--noprofile --norc` had inherited an MSVC-only path. The builder now prepends the selected MSYS2 `usr\bin`, preflights make/sed/grep/awk, and CI installs their explicit packages. Local shell validation resolves all four under `/usr/bin`.
- 2026-09-13: Adding MSYS2 to the build path changed GPG selection from a native build to MSYS2 GPG, which rejected native `C:\...` keyring/source arguments. Added executable-aware GPG path conversion so only MSYS2 GPG receives `/c/...` paths; native GPG retains Windows paths. Reproduced the failure in an isolated keyring and verified the corrected real import returns status 0.
- 2026-09-13: User retry showed the first GPG fix still saw only the bare command name `gpg.exe`, not its MSYS2-resolved path. Extended conversion to resolve bare GPG commands against the effective `PATH`; reproduced from the `hifimule-ui` working directory with MSYS2 first and verified the real isolated key import returns status 0.
- 2026-09-13: The next retry imported the release key but direct MSYS2 GPG invocation could not launch `/usr/bin/gpg-agent` and exited 2. MSYS2 GPG is now invoked through its sibling Bash runtime while native Windows GPG remains direct. Verified from `hifimule-ui` that the real isolated import returns status 0 with no agent error.
- 2026-09-13: A further real run showed the agent still failed only for the production keyring path. The adjacent `target/audio-runtime/...gnupg-UUID` path exceeded MSYS2 GPG agent socket limits; the shorter `%TEMP%` reproduction had passed. Isolated verification homes now use short `%TEMP%\hm-gpg-*` directories with guaranteed cleanup. Full verification of the actual cached 9.0.1 archive/signature/key succeeds from `hifimule-ui`.
- 2026-09-14: User confirmed the final repository-automated Windows x64 official-source flow completed successfully: FFmpeg/source verification and build, HifiMule package creation, package installation, and playback of every supported file format tested. Installer identity/hash and the remaining lifecycle/resource measurements were not supplied and remain unverified.
- 2026-09-14: Resumed Story 15.4 and closed the controlled-runtime and minimal-transport implementation tasks. Recorded the exact bounded buffer policy in the runtime manifest; added four-locale playback parity and transport accessibility regression coverage; full daemon (731 passing, 5 ignored), script, formatting and frontend build gates pass. Installed high-water/lifecycle evidence remains open.
- 2026-09-14: Added a dependency-free interactive installed-evidence collector and strict single-record/four-target validator. It authenticates from the private lifecycle descriptor without persisting its token, projects daemon health onto a public allowlist, redacts local profile paths, captures peak counters across daemon restarts, inventories private native modules, and preserves failed/unverified checks explicitly.
- 2026-09-14: The first macOS ARM64 collector run exposed the daemon's successful JSON-RPC compatibility envelope containing `error: null`. The parser now treats only a non-null error as failure, reports malformed envelopes explicitly, and has an exact-shape regression test.
- 2026-09-14: The first macOS ARM64 output-loss exercise showed CoreAudio automatically migrating the live stream from a disconnected Jabra endpoint to MacBook Speakers without invoking the stream error callback. The output worker now polls the default endpoint identity every 100 ms and maps a change or disappearance to retryable `OUTPUT_LOST`, preserving the paused occurrence instead of silently changing outputs.
- 2026-09-14: The rebuilt macOS ARM64 installed run passed package identity, private-library resolution, six formats, buffer bounds and seven lifecycle scenarios. Its output-loss note reported that sound stopped and did not resume after reconnection, but the UI remained `playing`; the record was retained as failed. Root cause was the endpoint-loss path setting the decoder cancellation flag before worker completion, causing the publishable `OUTPUT_LOST` event to be suppressed as if it were an explicit stop. Worker failures now use their typed publishability rather than the internal cancellation flag.
- 2026-09-14: Installed testing exposed Pause → Resume producing audible sound while remaining `loading`. Resume correctly set authoritative state to loading, but the long-lived audio worker retained its local `active` flag across Pause, so resumed sample consumption did not republish `Active`. The worker now clears local activity whenever its output gate is closed and publishes `Active` on the first newly consumed resumed sample.
- 2026-09-14: User reran the rebuilt installed macOS ARM64 matrix after the output-loss and Pause → Resume fixes. The strict validator accepts the record: all six formats and all eight lifecycle scenarios pass, including output loss and play/pause/resume/stop; the 8 MiB compressed and 48,000-sample PCM high-water marks remain within manifest bounds; all four FFmpeg dylibs resolve under `/Applications/HifiMule.app`; package SHA-256 is `b79a324dc51f0fd05c6424dc25066a7602df110cc70a24f91c1642f3ecdd6c33` against Subsonic 0.63.2.

### Completion Notes List

- Implemented authenticated source-qualified Play, Pause, Resume and Stop with atomic occurrence replacement, durable position semantics and generation fencing through the audio callback.
- Implemented bounded HTTP/custom-AVIO decode and resampling for the six required fixtures, a preallocated PCM handoff, shared output, starvation/completion/error transitions and shutdown joining.
- Implemented captured-source Play actions plus compact localized transport UI and authoritative single-schedule polling.
- Added FFmpeg 9.0.1 runtime identity, packaged notices, endpoint/buffer evidence fields and corrected macOS ABI-symlink bundling. Local macOS ARM64 `.app` builds and validates; audible and other installed-platform claims remain intentionally open for user evidence.
- Added controlled Linux provisioning for native x64/ARM64, cache-staleness and ELF/SONAME checks, recursive dependency closure packaging, and package-resolution verification. Windows x64/ARM64 now automatically cache and validate the pinned BtbN LGPL shared FFmpeg 9 SDK and stage its DLL closure; an exact `FFMPEG_DIR` remains supported as an explicit override.
- The Windows BtbN SDK is a dated, hash-pinned practical build unblocker, but it is not the story's required build from the official signed 9.0.1 source. This contract deviation and native installed verification remain open, so the story stays `in-progress`.
- Validation: daemon 698 tests, lifecycle 13, Tauri library 7, localization 6, frontend production build, formatting, relevant clippy (no new playback warnings), six-format production decoder, controlled HTTP/ranking tests, runtime ABI probe and signed macOS ARM64 app packaging all pass.
- Runtime packaging regression validation: thirty-five Node tests covering the cross-platform daemon wrapper and explicit Cargo target selection, sanitized runtime-verification handoff, Linux receipts/ABI rejection/compiler preflight/platform gating, Windows pinned provisioning/version/architecture/hash/override/staging-name checks and non-overlapping effective Tauri platform resources; the normal Build workflow runs the complete `scripts/tests/*.test.mjs` suite. Node syntax checks, Cargo formatting/check, native macOS wrapper check and diff whitespace checks pass. Linux and Windows now have user-reported playback smoke success; full sanitized installed-package evidence capture remains pending.
- Linux bindgen preparation now requires `clang`, `libclang-dev` and `libc6-dev`, probes native `limits.h`/`stdint.h` preprocessing for both supported Linux triples, and supplies target-specific bindgen arguments to every sidecar Cargo build. CI Build and release package lists enforce the same prerequisites.
- Root `npm run build:daemon` now uses the supported platform-aware wrapper: Linux provisions and selects the pinned runtime plus matching Clang/libclang environment, Windows provisions a hash-pinned MSVC SDK or validates an explicit `FFMPEG_DIR`, and macOS retains ABI verification. Subsequent user runs reached working playback on Linux and Windows; complete sanitized installed-package evidence remains pending.
- Cross-platform user smoke evidence (2026-09-13): AAC, ALAC, FLAC, M4A, Opus, WAV and MP3 playback is tested and working on Windows, Linux and macOS with the latest changes. Windows x64 package identity and native loaded paths are captured separately; equivalent Linux/macOS identity, output-loss/replacement/shutdown scenarios and buffer measurements remain unverified unless separately captured.
- Windows x64 installed evidence (2026-09-13): the identified NSIS package installed successfully, its SHA-256 was captured, and the running daemon loaded all four required FFmpeg ABI DLLs from the private installed HifiMule directory. The sanitized JSON record preserves unknown fields as unverified rather than inferring them.
- Windows x64 official-source feasibility evidence (2026-09-13): the manual official-source workflow reached successful HifiMule build, deployment and playback after resolving localized MSVC detection, import-library generation and LLVM/libclang discovery. This proves the replacement path on the tested host; it does not yet replace the normal BtbN provisioner or fill evidence fields that were not supplied.
- Windows installer smoke evidence (2026-09-13): the native Windows build produced a working installation package, installation completed, and the installed application was usable. This closes the basic Windows packaging/install smoke gap; package hash, exact target identity and installed FFmpeg module paths remain to be recorded.
- FLAC regression validation: supplied 194-second file, generated attached-picture fixture, runtime-generated metadata beyond the 8 MiB window, restored-position discard, extensionless/MIME provider hints and reader cancellation pass. The supplied recording was used only as local diagnostic input and was not copied into the repository.
- macOS packaging validation: the full `npm run tauri build` completed, producing a strictly verified ad-hoc-signed `.app` and a 31 MB ARM64 DMG with bundled FFmpeg dylibs. The earlier DMG failure was confirmed as sandbox-only and required no product workaround.
- Windows official-source automation now owns source/hash/signature verification, MSVC compilation and post-install import-library generation. The third-party binary manifest path was removed, normal Build/Release provision the source toolchain, and cache receipts remain target- and ABI-exact. Automated orchestration tests pass; a fresh Windows CI/native run is still required before marking the broader controlled-runtime task complete.
- Normal `npm run tauri build` launches no longer require a pre-opened Visual Studio Native Tools prompt: the runtime builder locates Visual Studio and imports the appropriate x64 or ARM64 developer environment before preflight/build subprocesses.
- Windows official-source builds no longer require NASM: optional x86 assembly is explicitly disabled only for the Windows audio-only recipe, while the selected demuxers, decoders and resampler remain enabled and receipt-validated.
- The Windows source builder no longer falls back to WSL `bash.exe`; it converts source/prefix paths to `/c/...` form and requires a genuine MSYS2 Bash plus GNU Make, preventing the misleading post-extraction `cd: C:/...: No such file or directory` failure.
- MSYS2 source-build tools are now explicitly present in the no-profile Bash `PATH`; configure prerequisites are checked before source compilation rather than surfacing missing-command cascades.
- Detached-signature verification supports both native Windows and MSYS2 GPG path semantics without weakening the isolated-keyring fingerprint/signature checks.
- Bare `gpg.exe` resolution now uses the effective subprocess `PATH`, closing the remaining cwd-dependent MSYS2 path-conversion gap.
- MSYS2 GPG now runs inside MSYS2 Bash so its agent/process environment is valid; native GPG behavior remains unchanged.
- GPG verification homes use bounded short temporary paths, avoiding MSYS2 agent socket-length failures while remaining isolated and automatically removed.
- Windows x64 official-source automation now has end-to-end user evidence through installed-package supported-format playback, superseding the earlier manual-only feasibility status. Unsupplied installer identity and lifecycle/resource evidence remain explicitly open.
- Controlled runtime packaging is now complete in source: the signed FFmpeg 9.0.1 recipe, hashes, ABI versions, flags, notices and bounded buffer policy are manifest-owned, while normal Build/Release exercise the target-specific provisioning and package checks.
- Minimal transport accessibility is complete: playback strings are parity-checked across English, French, Spanish and German; controls expose explicit localized accessible names, retain keyboard focus across authoritative snapshots, show visible focus, and publish status through an atomic polite live region.
- Validation on 2026-09-14: 731 daemon tests passed (5 diagnostic-only tests ignored), all Node script tests passed, the UI production build and Cargo formatting passed, and ordinary clippy completed with only pre-existing warnings outside the touched Story 15.4 code. Strict all-target clippy remains blocked by the repository's existing warning backlog.
- Installed evidence collection is now reproducible with `scripts/playback-installed-evidence.py collect`; validation rejects target/host mismatches, incomplete format or lifecycle coverage, missing signed-source identity, out-of-package native modules, zero/over-limit buffer counters, secrets and URLs. The story remains in progress until real installed target runs produce passing records.
- Fixed collector compatibility with the production RPC envelope: successful responses may include `error: null`; non-null structured errors remain sanitized to their numeric code, and missing result/error envelopes fail explicitly.
- Active endpoint loss is now detected even when CoreAudio silently reroutes an existing stream: the worker compares the opened endpoint with the current default endpoint outside the real-time callback, cancels decode, and returns the existing retryable output-loss failure. Validation on 2026-09-14: 732 daemon tests passed (5 diagnostic-only tests ignored), 6 collector tests and 56 Node script tests passed, formatting passed, and ordinary daemon clippy completed with only the existing repository warnings.
- The second macOS ARM64 evidence record proved every installed check except the authoritative output-loss state transition. Corrected that record to `failed` rather than accepting its contradictory pass flag. Added a regression proving `OUTPUT_LOST` remains publishable after the worker internally cancels decode; 733 daemon tests pass serially (5 ignored), along with 6 collector and 56 Node script tests, formatting, diff checks and ordinary clippy. The first parallel daemon run hit the repository's shared-vault test race; the isolated initiating test and complete serial suite pass.
- Fixed Pause → Resume status reconciliation without falsely claiming activity before audio: closing the callback gate resets only the worker's transient activity observation, and the next consumed sample republishes `Active`. The focused Resume/session tests pass; the full serial daemon suite passes with 734 tests and 5 ignored, plus 6 collector tests, 56 Node script tests, formatting, diff checks and ordinary clippy with existing warnings only.
- macOS ARM64 installed acceptance evidence now passes in full and validates independently. The four-target evidence task remains open because Windows x64, Linux x64 and macOS x64 complete collector records are still missing; macOS ARM64 evidence is not used to infer those architectures.
- Windows x64 installed acceptance evidence now passes in full and validates independently. The record covers all six required formats and all eight lifecycle/resource scenarios, loads the four FFmpeg 9.0.1 ABI DLLs from the private installed directory, and reports bounded 8 MiB compressed and 48,000-sample PCM high-water marks. The four-target evidence task remains open because Linux x64 and macOS x64 complete collector records are still missing.
- Review remediation on 2026-09-15 resolved R1–R16: owner-level effect claims and shared command identity fencing, pause/resume event ordering, retained terminal events, observable decoder/source failures, bounded range-backed HTTP IO, complete PCM-frame refill, joined cancellation, backend-tail drain tracking, the full preparation deadline, and disposable accessible UI controls with structured failures.
- Post-remediation validation: 759 daemon tests passed with 5 diagnostic tests ignored; 86 playback tests passed with 5 ignored; 59 Node tests passed with one platform skip; all 8 Python evidence tests, 20 lifecycle/localization tests, 7 Tauri library tests, TypeScript, Vite production build, formatting and diff checks passed. All-target clippy completed with the repository's existing warning backlog; warnings introduced in touched playback code were cleaned up.
- Existing Windows x64 and macOS ARM64 installed evidence predates the changed HTTP/output pipeline and therefore remains historical. Fresh installed evidence is required for all four targets; Linux x64 and macOS x64 remain absent.

### File List

- `docs/superpowers/plans/2026-09-14-story-15-4-review-fixes.md`
- `hifimule-daemon/src/playback/http_source.rs`
- `hifimule-daemon/src/playback/output.rs`
- `scripts/tests/test_playback_session_evidence.py`
- `scripts/tests/verify-audio-runtime.test.mjs`

- `.github/workflows/build.yml`
- `.github/workflows/release.yml`
- `.gitignore`
- `_bmad-output/implementation-artifacts/15-4-play-a-selected-library-track-through-the-daemon.md`
- `_bmad-output/implementation-artifacts/deferred-work.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `Cargo.lock`
- `Cargo.toml`
- `docs/api-contracts-hifimule-daemon.md`
- `docs/development-guide.md`
- `docs/playback-evidence-cross-platform-formats-2026-09-13.json`
- `docs/playback-evidence-windows-x64-2026-09-13.json`
- `docs/playback-evidence-windows-x64-official-source-2026-09-13.json`
- `docs/playback-evidence/macos-arm64.json`
- `docs/playback-evidence/windows-x64.json`
- `docs/playback-installed-test-checklist.md`
- `_bmad-output/implementation-artifacts/investigations/windows-ffmpeg-msvc-compiler-test-investigation.md`
- `hifimule-daemon/Cargo.toml`
- `hifimule-daemon/THIRD_PARTY_AUDIO_NOTICES.md`
- `hifimule-daemon/audio-runtime.json`
- `hifimule-daemon/build.rs`
- `hifimule-daemon/src/main.rs`
- `hifimule-daemon/src/playback/audio.rs`
- `hifimule-daemon/src/playback/decoder.rs`
- `hifimule-daemon/src/playback/mod.rs`
- `hifimule-daemon/src/playback/model.rs`
- `hifimule-daemon/src/playback/session.rs`
- `hifimule-daemon/src/playback/streaming.rs`
- `hifimule-daemon/tests/fixtures/generated-attached-cover.flac`
- `hifimule-daemon/tests/fixtures/generated-attached-cover.source.md`
- `hifimule-daemon/src/providers/jellyfin.rs`
- `hifimule-daemon/src/providers/mod.rs`
- `hifimule-daemon/src/providers/subsonic.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-i18n/catalog.json`
- `package.json`
- `hifimule-ui/src-tauri/tauri.conf.json`
- `hifimule-ui/src-tauri/tauri.linux.conf.json`
- `hifimule-ui/src-tauri/tauri.macos.conf.json`
- `hifimule-ui/src-tauri/tauri.windows.conf.json`
- `hifimule-ui/src/components/MediaCard.ts`
- `hifimule-ui/src/components/PlaybackControls.ts`
- `hifimule-ui/src/components/TracksBrowseView.ts`
- `hifimule-ui/src/library.ts`
- `hifimule-ui/src/main.ts`
- `hifimule-ui/src/rpc.ts`
- `hifimule-ui/src/styles.css`
- `scripts/build-daemon.mjs`
- `scripts/bundle-macos-libs.mjs`
- `scripts/linux-audio-runtime.mjs`
- `scripts/prepare-sidecar.mjs`
- `scripts/playback-installed-evidence.py`
- `scripts/tests/linux-audio-runtime.test.mjs`
- `scripts/tests/build-daemon.test.mjs`
- `scripts/tests/tauri-platform-config.test.mjs`
- `scripts/tests/playback-ui.test.mjs`
- `scripts/tests/test_playback_installed_evidence.py`
- `scripts/tests/windows-audio-runtime.test.mjs`
- `scripts/verify-audio-runtime.mjs`
- `scripts/windows-audio-runtime.mjs`

## Change Log

- 2026-09-12: Created story 15.4 and marked ready-for-dev after context/checklist validation.
- 2026-09-12: Built the single-track playback vertical slice and local macOS ARM64 package; retained cross-platform installed/provider/audio evidence items as in-progress.
- 2026-09-12: Fixed Ubuntu FFmpeg ABI drift with a private pinned Linux runtime and replaced Windows' Unix verifier with target-native FFmpeg SDK validation/staging; retained installed listening evidence as in-progress.
- 2026-09-12: Fixed large FLAC files with attached artwork failing after initial playback by correcting custom-IO seekability and adding signature-based, over-window and resume regressions.
- 2026-09-12: Isolated native-library resource globs by Tauri platform, enforced their effective mappings in the normal Build test gate, and completed signed macOS app plus DMG packaging.
- 2026-09-12: Fixed Ubuntu ARM64 bindgen header discovery with native compiler prerequisite checks and an explicit Clang resource/libclang Cargo environment.
- 2026-09-12: Added the supported platform-aware daemon build wrapper and matched Ubuntu's libclang library to Clang's resource headers; retained native ARM64 evidence as pending.
- 2026-09-12: Added hash-pinned automatic BtbN FFmpeg SDK provisioning for native Windows ARM64/x64 builds while preserving validated overrides.
- 2026-09-13: Recorded successful user-reported MP3, M4A and FLAC playback smoke tests on Windows, Linux and macOS; retained the complete installed-evidence and Windows runtime-contract gates as in-progress.
- 2026-09-13: Added sanitized Windows x64 installed-package evidence with OS identity, installer SHA-256 and private loaded FFmpeg DLL paths; retained unsupplied scenarios and measurements as unverified.
- 2026-09-13: Recorded successful Windows x64 manual official-source FFmpeg build/deploy/playback evidence and documented LLVM/libclang, localized MSVC and import-library requirements; default source-build automation remains open.
- 2026-09-13: Recorded successful AAC, ALAC, FLAC, M4A, Opus, WAV and MP3 playback tests across Windows, Linux and macOS, completing the three-platform format smoke matrix.
- 2026-09-13: Recorded successful user-reported Windows package creation and installation, completing the basic Windows build-to-installed-playback smoke path.
- 2026-09-13: Automated the signed official-source FFmpeg 9.0.1 Windows x64/ARM64 build path and removed the BtbN binary fallback; added Build/Release prerequisites, provenance validation and regression coverage.
- 2026-09-13: Fixed official-source provisioning from ordinary PowerShell by automatically loading the target-specific Visual Studio developer environment when `cl.exe`/`lib.exe` are not already on `PATH`.
- 2026-09-13: Removed the unnecessary Windows NASM prerequisite by making `--disable-x86asm` Windows-specific; retained the optimized Linux source recipe.
- 2026-09-13: Fixed the Windows source-build shell boundary with MSYS drive-path conversion, explicit MSYS2 discovery and GNU Make preflight; WSL Bash is no longer selected accidentally.
- 2026-09-13: Fixed FFmpeg configure under no-profile MSYS2 Bash by exporting its `usr\bin` path and preflighting/installing make, sed, grep and awk.
- 2026-09-13: Fixed official-source signature verification under MSYS2 GPG by converting keyring, key, signature and archive arguments to MSYS paths while preserving native-GPG behavior.
- 2026-09-13: Fixed bare-command MSYS2 GPG detection by resolving it through the effective build path; verified from the Tauri `hifimule-ui` launch directory.
- 2026-09-13: Fixed MSYS2 `gpg-agent` startup by routing MSYS2 GPG through its Bash runtime; isolated real-key import verified without agent errors.
- 2026-09-13: Fixed the remaining MSYS2 agent socket-length failure by moving isolated GPG homes to short system-temporary paths; actual FFmpeg archive signature verification passed.
- 2026-09-14: Recorded successful user-run automated Windows x64 official-source build, package creation, installation, and all-supported-format playback evidence.
- 2026-09-14: Recorded the final bounded buffer policy and added localized accessible transport regression coverage; closed the controlled-runtime and minimal-transport implementation tasks after full local validation.
- 2026-09-14: Added the cross-platform installed-playback evidence collector, sanitization controls and strict four-target validation workflow.
- 2026-09-14: Fixed installed-evidence collection for successful daemon RPC envelopes that include a null error member.
- 2026-09-14: Fixed macOS silent output migration by monitoring the opened endpoint identity and converting default-device change/disappearance into retryable `OUTPUT_LOST`; installed rerun evidence remains pending.
- 2026-09-14: Preserved the second macOS ARM64 run as failed after its note exposed stale `playing` state, and fixed internal decoder cancellation from suppressing the typed `OUTPUT_LOST` state event; installed rerun evidence remains pending.
- 2026-09-14: Fixed Pause → Resume remaining `loading` during audible playback by resetting worker activity while gated and republishing `Active` on resumed sample consumption.
- 2026-09-14: Validated the final installed macOS ARM64 evidence record with all formats, lifecycle scenarios, private native-library paths and bounded high-water measurements passing; retained the three untested target records as open.
- 2026-09-14: Validated the final installed Windows x64 evidence record with all formats, lifecycle scenarios, private native-library paths and bounded high-water measurements passing; retained Linux x64 and macOS x64 records as open.
- 2026-09-15: Applied all sixteen code-review patches, added adversarial concurrency/HTTP/output/UI regressions, and returned the story to in-progress pending fresh four-target installed evidence for the changed playback pipeline.
