---
baseline_commit: 50a2f46ca03244c4fcc6fbd343ab7081f2914d88
---
# Story 15.5: Choose an audio output and recover safely from disconnection

Status: done

## Story

As a HifiMule user,
I want to choose my headphones or audio interface as the playback output,
so that music plays where I intend and never unexpectedly moves to another speaker after a disconnection.

**Requirements:** Output-selection portion of FR58; FR64; P-NFR3–4 and P-NFR6; output lifecycle portion of P-AR14; relevant P-UX-DR7 and P-UX-DR13.

**Dependencies:** Stories 15.1–15.4 are marked done. Preparation baseline: `5dc5a65cff2aeb8d7397e1df56e44b09878ce13a` (2026-09-16). Extend the production single-track player and its serialized owner. Native media keys (15.6), public seeking (15.7), queue advancement, full Playback destination, floating bar (15.14), Radio, preview, gain and adaptive quality remain later stories.

## Acceptance Criteria

1. **Identify outputs.** Given available shared outputs, opening the selector lists accessible names and identifies the selected output. Outputs with identical names remain distinct by stable identity and understandable secondary labels; raw implementation identifiers are not primary labels.
2. **Switch deliberately.** Given a playing or paused track, selecting another available output opens it in shared mode and preserves current occurrence, logical position and previous playing/paused intent after success. Old and new outputs never play simultaneously. A brief interruption is allowed; device switching is not claimed gapless.
3. **Lose an output safely.** Given the selected endpoint disconnects or becomes unusable, playback stops emitting through that endpoint, preserves the queue/current position and becomes paused with a recoverable output explanation. It never automatically routes music to another device.
4. **Recover without surprise.** Given output-loss-paused playback, reconnecting the endpoint or changing the system default leaves playback paused until explicit Resume. Selecting a replacement while paused also stays paused.
5. **Restore the preference.** Given a saved preference, restart resolves the platform-supported stable identity and stays paused. An absent or uncertain match remains visibly unavailable and requires an explicit available choice; no name matching or default substitution is allowed.
6. **Fence races and failures.** Given a failed switch or a race with Stop, another selection or removal, stale asynchronous completions cannot replace the latest selection or revive audio. UI reports the actual active/unavailable output and recoverable error while retaining the queue. Extend this invariant to Pause, PlayTrack and committed Quit.
7. **Share system audio.** Playback requests shared output, permits other applications to play and does not manipulate Do Not Disturb. ASIO and exclusive-mode controls are outside scope.
8. **Verify real behavior.** Windows, macOS and Linux checks cover switching, unplug/replug, absent startup endpoint, failed open, duplicate names and sleep/wake. Verify ownership and preserved transport state/position. Physical-output smoke evidence must demonstrate no unexpected speaker playback; label virtual-device evidence separately and identify each tested architecture.

## Tasks / Subtasks

- [x] Add endpoint discovery and durable preference (AC: 1, 5, 7)
  - [x] Implement the versioned local configuration contract below, atomic save, bounded parsing and missing/corrupt/future-version handling.
  - [x] Add platform-qualified stable identity, bounded enumeration, duplicate-name labels, availability and current-default annotation.
  - [x] Upgrade only the required audio dependency surface and update runtime identity/build prerequisites.
- [x] Integrate selection into the existing owner and RPC boundary (AC: 2–6)
  - [x] Add strict list/select contracts, output revision, shared deduplication and exactly-once effect admission.
  - [x] Gate/capture the old generation, preserve occurrence/position, rotate generation for replacement and serialize retirement/open.
  - [x] Commit only current results; preserve Pause/Stop/Quit precedence and truthful selected-versus-active state.
- [x] Implement pinned shared platform output and loss handling (AC: 2–4, 6–8)
  - [x] Bind CoreAudio/WASAPI to the selected stable endpoint; add endpoint-specific notifications and bounded reconciliation.
  - [x] Implement Linux explicit shared sink output with movement prohibited; handle removal, server loss and suspend/wake safely.
  - [x] Reuse PCM/refill/presentation accounting and bounded internal position preparation; keep source credentials private.
- [x] Extend the existing compact controls (AC: 1–6)
  - [x] Add a labeled output selector, selected/active/unavailable status and explicit retry actions without requiring a physical sync device.
  - [x] Replace the obsolete current-default Resume wording with selected-output behavior; preserve focus, polling disposal and transport errors.
  - [x] Add matching English, French, Spanish and German strings and behavioral UI coverage.
- [x] Validate contracts and installed output safety (AC: 1–8)
  - [x] Add deterministic owner/backend/config/RPC race tests and retain 15.4 regressions.
  - [x] Update API documentation, installed checklist and sanitized evidence collection for output identity and transitions.
  - [x] Run the checks below and record installed per-target physical/virtual evidence, failures and unavailable environments honestly.

### Review Findings

Full review on 2026-09-16 of `50a2f46ca03244c4fcc6fbd343ab7081f2914d88..0b732fe` (37 files, +4,822/-173). Blind Hunter, Edge Case Hunter and Acceptance Auditor all completed. Triage: **9 patch findings, 0 decisions, 0 deferred, 2 dismissed**, after deduplicating 17 observations into 11 candidates. All nine findings were subsequently fixed with user authorization; completion and validation are recorded below.

- [x] [Review][Patch] **R1 [P1] Reject physical Pulse routes that can switch ports underneath a pinned sink.** [`hifimule-daemon/src/playback/devices/pulse_stream.rs:172`] — AC3: the active port is verified only during open; ongoing verification checks sink name, moved notifications and connection/stream failure. Unplugging analog headphones can change the same sink to its speaker port without moving the stream, so `DONT_MOVE` does not prevent speaker playback before the one-second discovery reconciliation. Enforce the physical routing guarantee or report such endpoints unsupported; polling alone cannot establish no-reroute safety. Sources: edge+auditor. PulseAudio documents this automatic headphone/speaker port behavior in its [module-switch-on-port-available documentation](https://wiki.freedesktop.org/www/Software/PulseAudio/Documentation/User/Modules/#module-switch-on-port-available). This is a code-path finding, not a newly performed physical test.
- [x] [Review][Patch] **R2 [P1] Fence stale silent validation before retiring the current pipeline.** [`hifimule-daemon/src/playback/audio.rs:427`] — AC6: the dispatcher can capture an idle output-selection effect, then a newer PlayTrack can install its pipeline before `validate_selected_output` obtains the start lock. Validation unconditionally takes and retires that newer pipeline before checking the old generation, then exits with `GENERATION_CONFLICT`, leaving the newer playback without its worker. Check the generation/selection under the start lock before touching `current`, as `start` already does. Sources: blind+edge.
- [x] [Review][Patch] **R3 [P2] Exclude mutable display names from stream-open identity checks.** [`hifimule-daemon/src/playback/session/output_selection.rs:136`] — AC1/5/6 and the identity contract: discovery can update a device's friendly name while its stream is opening or filling initial PCM. `output_opened` compares the whole `OutputPreference`, including `display_name`, and rejects the unchanged stable endpoint. The native worker treats this as supersession and suppresses the failure even though the generation is still current, leaving playback buffering/switching. Compare backend, stable ID and the required identity properties, retaining generation fencing. Sources: blind+edge.
- [x] [Review][Patch] **R4 [P2] Clear the opening marker after a terminal preparation failure.** [`hifimule-daemon/src/playback/session/output_selection.rs:388`] — AC6 and truthful output status: a successful preference save sets `opening_generation`, but provider resolution, source fetch or decode failures with non-output error codes do not clear it. After a switch while the provider is offline, playback reports its error while output status remains `switching` indefinitely; no output open is still running. Add a generation-aware terminal completion path that clears opening state and reconciles actual output availability on every failed preparation. Source: blind.
- [x] [Review][Patch] **R5 [P2] Keep endpoint-local discovery errors from stopping an unrelated healthy output.** [`hifimule-daemon/src/playback/session/output_selection.rs:409`] — AC3/6: any inventory error is treated as loss of the active endpoint, including truncation or another device's unreadable description/identity. A healthy selected endpoint that still resolves is paused and marked lost when an unrelated problematic endpoint appears. Distinguish incomplete/per-device enumeration from loss or uncertainty of the selected endpoint; retain fail-closed behavior for actual selected-identity or backend failures. Source: blind.
- [x] [Review][Patch] **R6 [P2] Retain the actual opened descriptor until close acknowledgment.** [`hifimule-daemon/src/playback/session/output_selection.rs:426`] — AC6 and the selected-versus-active contract: discovery-driven loss clears `output.active` immediately after requesting Stop, before the native worker has acknowledged retirement. With a stalled cork/flush or worker join, the API/UI falsely report no opened output while the worker still owns it. Keep the active generation/descriptor until `output_closed`, with an unavailable/retiring status; do not use a Stop request as proof of closure. Source: blind.
- [x] [Review][Patch] **R7 [P2] Reconcile discovered options while preserving selector focus.** [`hifimule-ui/src/components/PlaybackControls.ts:116`] — AC1 and selector-refresh UX: focusing the selector initiates discovery, but `renderOptions` suppresses every update while it is focused. An endpoint attached since the previous inventory is fetched but absent throughout that interaction; the user must blur and reopen or use the separate refresh button. Update options without replacing the focused select so opening/keyboard focus can expose newly discovered outputs. Sources: blind+auditor.
- [x] [Review][Patch] **R8 [P2] Correct the Linux evidence buffer limit to 38,400 bytes.** [`scripts/playback-installed-evidence.py:312`] — AC8: the runtime correctly requests 100 ms at 48,000 Hz, two channels and four bytes/sample, totaling 38,400 bytes. The validator and installed checklist instead cap this at 19,200 bytes while calling it 100 ms. A server honoring the configured runtime limit is rejected. Align the validator/checklist with the format-derived bound and add a boundary regression. Sources: blind+edge+auditor.
- [x] [Review][Patch] **R9 [P2] Require actual output identity/state evidence in installed records.** [`scripts/playback-installed-evidence.py:338`] — AC8: each scenario's before/after `output` need only be a dictionary; empty `{}` objects pass, as the passing-record test demonstrates. Consequently the new gate accepts records containing no selected/active identities or output status and cannot verify the recorded switch/loss distinction. Validate the sanitized output schema, including scenario-appropriate identity/state fields and explicitly allowed null states, and reject empty evidence. Source: blind.

Review validation: **118 playback tests passed, 6 opt-in diagnostics ignored; 26 UI/Linux-runtime Node tests passed; 9 Python playback-evidence tests passed.** The first playback run had five failures because the sandbox prohibited local mock HTTP listeners; rerunning with approved access passed all 118. Existing compiler warnings remain. No physical routing tests were performed during this review; prior user-reported platform acceptance is unchanged. The two dismissed candidates were the stopped/completed switch's source preparation (required by the chosen contract) and a separate browser-dependent focus-loss claim not independently established in this review.


Review fixes completed 2026-09-16: R1 rejects physical Pulse sinks unless their single reported port is the active one, with the same policy checked against the opened sink; unsupported routes remain visible with an explanation, and virtual routing stays explicitly labeled. R2 fences silent validation before pipeline retirement. R3 compares stable endpoint identity independently of friendly names. R4 finishes opening state on current-generation terminal failures while retaining retirement warnings. R5 isolates per-endpoint/partial inventory errors while preserving fail-closed behavior for selected identity loss or discovery failure. R6 retains active ownership until the matching close acknowledgment. R7 reconciles option nodes in place and picks up asynchronous inventory completion through the existing polling lifecycle. R8 uses the correct 38,400-byte buffer bound and captures updated buffer diagnostics. R9 requires complete sanitized output/transport evidence, distinct switch identities and unavailable startup state.

Post-fix validation: **797 daemon tests passed / 6 opt-in diagnostics ignored; 66 Node tests passed; 12 Python evidence tests passed; 7 i18n tests passed; UI TypeScript/Vite build; normal daemon clippy; formatting and diff checks passed.** Clippy retains existing repository warnings and has no playback diagnostics. A standalone source probe type-checked the actual Pulse discovery/stream adapters with their pinned bindings on macOS; this is not a Linux native build or physical routing test. The final selector polling adjustment was rechecked with all 11 UI tests and another UI build. No new physical acceptance evidence was fabricated; the earlier user-reported platform checks are preserved as historical evidence. Story and sprint status are done; R1's conservative Linux endpoint availability restriction is documented in the API/installed checklist.

## Dev Notes

### Selected implementation contract

These are preparation decisions, not claims of implemented or tested behavior. They close the story's identity, migration, default-change and switching gates. Keep one daemon session authority and one audible output. The UI projects authoritative state through the existing authenticated native RPC proxy.

#### Configuration, identity and migration

Create `playback/config.rs` and store `playback.json` under `paths::get_app_data_dir()`. Architecture explicitly places Playback settings in a versioned local configuration file; keep the queue/checkpoints in existing SQLite. Do not add an output preference to a physical-device manifest or alter the session database schema merely for this setting.

Version 1 contains `{ schemaVersion: 1, output: null | { backend, stableId, displayName, identityProperties } }`. `identityProperties` contains only bounded platform identity/disambiguation facts, never credentials, network stream URLs or transient device handles. Backend plus stableId is the identity; displayName is presentation only. Reject files larger than 64 KiB and bound individual strings to 4 KiB. Preserve unsupported/corrupt file evidence and expose `OUTPUT_CONFIG_INVALID` without deleting a valid listening session. No existing output setting exists to migrate: missing file means unselected, not a fabricated saved default. Display the current default as a suggested explicit choice; the first Play/Resume without a selection explains that an output must be selected. Queue selection may still succeed without audible output.

Save using a same-directory temporary file, flush and platform-correct atomic replacement. Do not delete the old file before replacement. Serialize saves in accepted output-command order on one non-audio worker; coalesce superseded writes before commit and fence their results. Publish `selected` only from the actual committed preference; keep the in-flight choice separately as `pending`. On failure reconcile the last successful durable commit, clear the failed pending choice, remain safely paused if switching has begun, and report `OUTPUT_PREFERENCE_SAVE_FAILED`. An older completion cannot overwrite a newer committed selection. Test A-save/B-save ordering including B failure and Stop during a save; Stop may retain an accepted preference change but can never permit its stale audio effect.

Corrupt/future configuration blocks normal selection with an actionable reset explanation. An explicit “Reset output setting” action, followed by an available choice, may archive the original file and replace it with version 1; require `replaceInvalidConfig: true` on that selection request. Default is false, and never infer consent from startup or Resume. Archive failure leaves the original untouched and returns an error. This is explicit recovery, not an implicit migration. Startup loads configuration without opening an audio stream and never auto-resumes.

| Platform | Identity and shared routing contract |
| --- | --- |
| Windows | WASAPI render endpoint's opaque endpoint ID, surfaced through CPAL stable ID. Resolve the exact endpoint, not its friendly name or default role. Shared mode only; endpoint removal/state changes invalidate its active handle. Re-enumeration order never changes identity. |
| macOS | CoreAudio persistent device UID via CPAL stable ID; numeric AudioDeviceID is a runtime handle only. Bind the actual AudioUnit to that endpoint. Observe selected-device alive/configuration changes; do not rely solely on the stream error callback. A changed UID is unavailable, even if the name matches. |
| Linux | Explicit PulseAudio sink name plus available stable hardware/port/profile properties, through PulseAudio or PipeWire's Pulse-compatible server. Numeric sink index is resolved anew and never persisted. Resolve the named sink with confidence; conflicting identity properties or ambiguous hardware is unavailable. Open to that sink with movement prohibited. Do not expose an ALSA `default`, `pulse`, `hw:*` or other unverified pseudo-route as a safely pinned physical output. |

Duplicate labels use manufacturer/transport/port details where available and a stable session-local ordinal as the last display disambiguator. That ordinal is never used to restore identity. Virtual outputs may be selectable when routing identity is explicit, but diagnostics and evidence must identify them as virtual; selecting a virtual output cannot certify its downstream physical routing. If a backend cannot confidently identify/pin an endpoint, report it unavailable/unsupported rather than guessing.

The system default is informational after selection. Changing it while the selected endpoint remains healthy neither switches nor pauses that endpoint. No continuously following “System default” choice is introduced. The UI may offer “Use current default: {name}”, which resolves and saves that concrete endpoint exactly like any explicit choice. If it vanishes between enumeration and selection, fail without substitution.

#### Backend and dependency decision

The repository currently pins CPAL 0.16.0. Select exact **CPAL 0.18.2** for this story's Windows/macOS identity APIs, and **libpulse-binding 2.30.1** as a Linux-only dependency for explicit sink movement control. Keep Rust 1.93.0/edition 2024, FFmpeg 9.0.1, ffmpeg-next/ffmpeg-sys-next 9.0.0 and crossbeam-queue 0.3.12. Do not upgrade the decoder, web framework or unrelated dependencies.

CPAL's 0.17+ stable `DeviceId` API separates identity from descriptions. For the 0.18 migration, adapt sample-rate aliases, by-value stream configuration, unified error kinds, timestamp methods and explicit stream start. Recheck negotiated sample formats, rates and latency rather than assuming old defaults. Update `audio-runtime.json` binding identity and tests that assert it. CPAL 0.18.2 declares Rust 1.85, within the project's MSRV; target-specific dependencies still require real builds. [Sources: CPAL release, manifest and upgrading guide below.]

CPAL 0.18.2 has an optional Pulse backend, but its tagged `build_output_stream_raw` sets `start_corked` and leaves other movement flags at their defaults; it even accounts for the server moving streams. Therefore enabling that backend alone is not evidence of this story's no-reroute guarantee. Use a focused Linux adapter with libpulse's asynchronous API, an explicit sink and `DONT_MOVE` plus `START_CORKED`. Observe sink/stream/context state; do not enable blanket fail-on-idle-suspend behavior that mistakes ordinary sink power saving for removal. The daemon does not change the user's server routing policy.

Bound Pulse server-side stream buffering separately: initially request 100 ms target/max queued audio, capped at 1 MiB at the negotiated sample format, and a 10 ms minimum request. Inspect returned buffer attributes and fail explicitly if the bounded contract cannot be met; record final measured values if tuning is necessary. Do not accept unlimited/default server buffer lengths. Track submitted audio, silence and server playback timing separately so logical position excludes pending server audio. Adapt the presentation-clock boundary to Pulse timing instead of treating a write callback as proof of physical playback. Pause requires a cork acknowledgment; switching/Stop require cork plus flush/close acknowledgment before another endpoint can emit. Natural EOF uses drain acknowledgment. Bound acknowledgments to a 250 ms warning threshold: on a stall keep output gated and report pending retirement, retain owned workers and prohibit new audio until retirement is confirmed. This is a waiting/error boundary, not permission to claim the old stream is gone.

Keep native handles confined to their owning backend worker and join it on shutdown. Integrate its mainloop as an audio backend worker, not a second Tao application loop. Notifications enqueue/coalesce facts; they never perform DB/network work or acquire the session owner lock. Start with one endpoint reconciliation every 1 second plus refresh on selector opening; native notifications and stream errors gate loss immediately. Polling is a safety reconciliation, not a substitute for preventing backend migration. On wake, invalidate questionable handles, reconcile identities and remain paused if continuity cannot be proved. No automatic reopen after loss/server restart.

Add Linux `libpulse-dev` build prerequisites and pkg-config preflight to `scripts/linux-audio-runtime.mjs` and the Linux apt lists in `.github/workflows/{build,release}.yml`, retaining existing ALSA requirements where CPAL compilation needs them. Its library-closure bundler currently seeds only FFmpeg and libmtp: explicitly seed libpulse and its non-system dependency closure, preserving the system-library exclusions, then assert clean installed AppImage/deb resolution. `tauri.linux.conf.json` already maps `bundled-libs/*`; change that mapping only if necessary. Preserve the private FFmpeg source/hash/signature/ABI process and Windows/macOS packaging. Document selected shared backend and loaded versions in evidence; no global OS audio settings, exclusive access or ASIO.

#### RPC and authoritative state

Add `playback.listOutputs` and `playback.selectOutput` through the current router/proxy. These names are new planned APIs. List request: `{ schemaVersion: 1 }`; return the existing response envelope with instanceId, outputRevision, descriptors and authoritative selection state. Descriptors carry opaque `outputId`, accessible displayName/detail, backend, availability, isDefault and identity confidence; daemon-only handles never cross RPC. Bound enumeration to 256 descriptors and return an explicit truncation/error state rather than silently hiding results. Cache discovery off the RPC runtime; allow one in-flight enumeration, coalesced refreshes and a 5-second deadline with stale/unavailable status on timeout. A timeout does not abandon a native worker or permit another concurrent discovery thread; retain its ownership and discard obsolete results.

Select request: `{ schemaVersion: 1, instanceId, sessionId, commandId, expectedOutputRevision, expectedGenerationId, outputId, replaceInvalidConfig?: boolean }`. Identity/revisions follow existing UUID and canonical decimal-string conventions. Reject unknown fields, invalid IDs, stale owner/session/generation/outputRevision and shutdown admission. Selection works with an empty queue. Do not overload expectedQueueRevision: output changes do not mutate the listening queue.

Reuse the owner's shared bounded command-ID namespace and effect claim mechanism for Apply/Control/Select: identical replay does not save/reopen again; changed payload under the same ID conflicts. Preserve the current retention limits. Responses acknowledge accepted/committed intent with current authoritative metadata, not guaranteed audible success. Conflict is the existing 409 contract. Backend completion carries both output revision and playback generation, and only the latest accepted operation may publish success/error.

Add an `output` object to the schema-v1 session snapshot: `revision`, `selected` (saved descriptor or null), `pending` (requested descriptor during a switch or null), `active` (actual opened descriptor or null), `status` (`unselected`, `available`, `switching`, `unavailable`, `error`), and optional sanitized `{ code, retryable }`. An output can be available while no stream is open. Keep this distinct from transport and playback error; a preference failure is not a corrupt queue. Update stateSequence on meaningful output changes; queueRevision remains unchanged. Include the same state in list/select responses so UI need not infer an active endpoint from the last click.

Use existing `OUTPUT_LOST`/`OUTPUT_UNAVAILABLE` where applicable and add `OUTPUT_SWITCH_FAILED`, `OUTPUT_IDENTITY_AMBIGUOUS`, `OUTPUT_SHARED_UNSUPPORTED`, `OUTPUT_CONFIG_INVALID`, `OUTPUT_PREFERENCE_SAVE_FAILED` as necessary. Map backend diagnostics to sanitized localized codes; never use raw endpoint IDs or errors as primary user text. Every Play/Resume entry point must consult this output policy; removing only the UI default-retry label leaves a backend bypass. The current Resume router collapses backend failures into `RESUME_UNAVAILABLE`; preserve actionable output codes instead of hiding them under that source-position error.

#### Switching, position and race ordering

| Trigger | Required transition |
| --- | --- |
| Select while playing/loading/buffering | Accept current command; gate old output and capture latest consumed position; save the selected intent; rotate generation; retire old pipeline; open/prepare selected endpoint at the saved position; unmute only if current desired state still requests playback. Keep loading/switching explicit until actual samples are consumed. |
| Select while paused/stopped/completed | Follow the same endpoint validation/persistence/open ownership rules, but keep its gate closed. Preserve existing position and stopped/completed semantics. Selection alone never acts as Resume. With no current occurrence, validate/open then release a silent stream without fetching a source or creating a queue entry. |
| Select already selected healthy endpoint | Return current state without restarting audio or changing position. An unavailable endpoint may be explicitly revalidated, but stays paused. |
| Selected output loss | Gate immediately, freeze latest consumed position, invalidate the output generation and retire work; retain selected preference and queue, publish paused recoverable error and checkpoint the position. No fallback, default lookup or auto-retry. |
| Reconnect/default change | Refresh available/default information only. A reconnected matching endpoint can become available, but no new stream emits until explicit Resume. |
| Failed open/prepare | Selected intent remains visible when successfully saved; active becomes null after old retirement; retain position/queue and paused error. Do not automatically roll back audibly to the old output. |
| Pause during switch | Update desired state/control epoch and gate immediately. Late open/Active events cannot reopen audio; successful selection may finish paused. |
| Stop/PlayTrack/new selection/Quit during switch | Invalidate old effects. Stop retains its zero-position semantics; PlayTrack retains new source/occurrence semantics. Only the latest accepted generation/selection may open output. Quit leaves admission fenced and joins all workers. |

Sample position after gating and flushing the existing progress ingress, before rotating generation. The existing worker emits sequence values starting at 1: reopening under the same generation would both reuse the old pipeline through `resume_existing` and violate progress ordering. Rotate generation for an actual pipeline replacement while preserving session ID, occurrence ID, queue revision and logical millisecond position. Do not count buffered-ahead frames or silence as listened position.

Use the established internal restore-position preparation (bounded decode/discard or verified range access); no public seek command is needed. Reconfigure the converter for the new endpoint rate/channels and discard old endpoint PCM only after capturing consumed position. Preparation failure must retain the position, never play from zero while displaying it. Preserve the 60-second complete preparation deadline and 8 MiB aggregate compressed/500 ms-or-1 MiB PCM limits. Repeated switching must not create unbounded retained pipelines, fetches or decode workers.

Gate the old output before any new output can emit, and retire/acknowledge old backend presentation before starting the new endpoint. Preserve `PresentationClock` tail accounting and document observed physical stop latency; do not substitute an arbitrary sleep or claim instantaneous silence. Never hold the session mutex while joining workers that call snapshot/generation/event APIs. A stalled retirement blocks new audio safely while Stop/status/Quit remain responsive, retaining the existing shutdown blocker rather than abandoning a worker. Keep at most one active and one retiring pipeline, coalescing pending selections to the latest.

### Source tree change and preservation map

| File / area | Current state → change; preserve |
| --- | --- |
| `hifimule-daemon/src/playback/audio.rs` | Opens default CPAL endpoint and checks its display name against the default every 100 ms → selected identity/backend handle and safe switch/loss lifecycle. Preserve generation checks, serialized starts, decoder join, preparation deadline, private provider resolution, bounded buffers and typed publishable failures. |
| `playback/output.rs` | Existing whole-frame PCM/refill consumer, presentation clock and owned decoder worker → reuse for platform output; adapt sample conversion/backend hooks only as needed. Preserve no half-frame consumption, silent pauses, final-tail drain and join-on-drop. This file already exists; do not recreate it as a blank device manager. |
| `playback/{model,session}.rs` | Existing strict contracts, coalesced progress, bounded serialized owner/dedup and control-epoch fencing → add output intent/revision/snapshot and effect dispatch. Preserve queue identities, exactly-once admitted effects, reliable terminal events and checkpoint/shutdown ordering. |
| `playback/config.rs` (NEW), `playback/devices/` (NEW), `playback/mod.rs` | Add only output setting and focused endpoint/backend adapters, register modules. Keep media-key integration in later `native.rs`; no second session manager. |
| `playback/persistence.rs`, `src/paths.rs` (REUSE) | Existing bounded SQLite session restoration and app-data resolution. Reuse checkpoint APIs and path helper; output settings require no SQL migration. Preserve future-schema/corrupt-session evidence and transactional queue restoration. |
| `hifimule-daemon/src/rpc.rs` | Existing authenticated Playback routing, shared effect claims and admission guards → add list/select and propagate output state. Preserve health/shutdown responsiveness and all source-qualified Play/Resume paths. |
| `hifimule-daemon/src/main.rs` | Existing owner/audio startup and coordinated Quit → load config/start endpoint monitor and join it on committed shutdown. Preserve singleton ownership, one Tao loop, detached UI lifetime and independent sync cancellation. |
| `hifimule-ui/src/rpc.ts` | Existing typed Playback snapshot and authenticated calls → add output types/list/select; preserve structured RpcError and captured owner/generation. |
| `hifimule-ui/src/components/PlaybackControls.ts` | Existing persistent primary/Stop buttons, localized state, single 500 ms poll and disposal → add selector/status with one coalesced discovery request; preserve focus, meaningful-only announcements and action error handling. |
| `hifimule-ui/src/styles.css`, `hifimule-i18n/catalog.json` | Add scoped output layout/focus styles and four-locale labels/errors; preserve compact transport/browser layout and existing strings except obsolete default-retry behavior. |
| `Cargo.toml`, `Cargo.lock`, daemon manifest/runtime manifest; build/release/platform packaging | Scoped CPAL migration and Linux shared backend dependency/identity changes. Preserve FFmpeg hashes/ABI/source builds, installer resource separation and sidecar launch. Read the specific build files fully before editing their Linux dependency lists. |
| `scripts/tests/playback-ui.test.mjs`, `scripts/tests/linux-audio-runtime.test.mjs`, installed evidence/checklist, API docs | Extend real component harness, Linux prerequisite/closure fixtures and authenticated output scenarios. Replace the existing assertion that OUTPUT_LOST Resume opens the current default. Retain source/decoder/lifecycle regressions and sanitization. |

### UX and integration rules

Use a labeled Shoelace select or equivalent keyboard-operable control inside the existing compact transport. Keep it mounted outside the frequently replaced library content; do not build the future floating bar/settings page. Show unavailable saved selections, actual active output, switching and meaningful recoverable messages. Use textContent/escaped labels. Preserve focus while polling or renaming/re-enumerating devices; do not rebuild the focused selector every 500 ms. Coalesce selector refresh with the existing controller lifecycle and ignore responses from a disposed controller/old daemon. No new unbounded timer per hotplug or selection.

Output selection remains available without a physical sync device or current track. Preserve browser multi-selection, drag, basket and playlist actions. Selecting an output must not select a server, start a track, clear a queue or change source quality. Resume names the selected output when recovery needs explanation; disable it with a clear reason while selection is absent/unavailable. Stop and Pause remain usable during asynchronous switching: the current component's single `busy` flag disables both buttons, so introduce separate output-pending and transport-pending state. Selecting an output must not set transport busy; derive Pause availability from desired transport intent through switching/loading. A failed command must reconcile authoritative state and display its structured failure.

### Previous story and Git intelligence

- `5dc5a65` marked 15.4 done; it did not add fresh installed evidence. `876bbb5` (Review 15.4) changed the HTTP/output pipeline and resolved R1–R16; preserve owner-level effect claims, desired-state ordering, retained terminal events, range-backed bounded input, full PCM frames/refill, worker joining, presentation drain, preparation deadline and UI disposal/focus/errors.
- CoreAudio was observed migrating from Jabra to MacBook Speakers without an error callback. The current default-name poll is an interim workaround, not a persistent-identity design or proof that no samples can escape during polling latency.
- Internal cancellation once suppressed OUTPUT_LOST publication; retain typed failure publishability. Pause→Resume once remained loading despite audible output; only current-generation consumption may republish Active.
- Recent commits `f0d52c2` and `3586043` recorded completion/evidence; `f108017` migrated Actions to Node 24. Do not regress build tooling while adding native prerequisites.
- Post-review historical validation reports 759 daemon tests passing (5 ignored), 86 playback tests passing (5 ignored), 59 Node tests (one skip), 8 Python evidence tests and frontend/lifecycle checks. Installed Windows x64/macOS ARM64 evidence predates the review changes; Linux x64/macOS x64 records remain absent. These are not 15.5 test results.
- Project context's greenfield/planning status is stale. Its provider abstraction and managed-device ownership principles remain valid; approved playback amendments/current code supersede older no-device restrictions.

### Testing requirements

Exercise production RPC/owner behavior using fake endpoint adapters and deterministic barriers, then real installed shared outputs. Helper-only tests cannot prove admission, persistence or physical no-reroute behavior.

| Area | Required checks |
| --- | --- |
| Identity/config | Duplicate names/different IDs, enumeration reorder, rename with same ID, recreated device with changed ID, ambiguous Linux properties, absent endpoint, first run, malformed/oversized/future config, atomic interrupted save and write failure. Restart always paused and never changes source/queue. |
| Switching | Playing/paused/loading/buffering/stopped/completed/idle selection; same-endpoint no-op; new endpoint with different sample rate/format; consumed position preserved within documented frame/rounding tolerance; no overlap; prepare failure never silently rewinds. |
| Races | A→B→C with delayed completions; switch versus Pause/Stop/PlayTrack/removal/Quit; replayed ID and changed-payload conflict; stale instance/generation/output revision; saturated mailbox; late old Active/Failed/progress; timeout followed by late open. Assert backend open/close and audible-effect counts. |
| Loss | Active and paused unplug; identical-name replacement; default change with selected healthy; reconnect without resume; server restart; sleep/wake; explicit Resume only on exact selected endpoint; unexpected backend move is rejected/gated. |
| Resources/lifecycle | Bounded rapid switching/retirement, decoder and monitor joining, unchanged compressed/PCM limits, real-time callback discipline, UI close/reopen, shutdown during stuck open/retirement, concurrent sync. |
| UI | Keyboard select/transport, duplicate-name descriptions, visible focus across updates, polite loss/recovery status, unavailable selection, save/open errors, single polling/disposal and no automatic Resume call. Four-locale parity. |
| Installed | Windows x64, Linux x64, macOS x64 and ARM64: two outputs, physical unplug to built-in speaker fallback risk, replug, failed open, sleep/wake, default change, simultaneous other-app audio. Record target/package hash/backend/versions, sanitized identity distinction, state/position before/after, actual audible destination, switch/stop latency and high-water values. Label any additional ARM64/virtual runs accurately. |

Use the controlled daemon wrapper from the repository root:

```sh
rtk proxy node scripts/build-daemon.mjs test -p hifimule-daemon playback:: -- --test-threads=1
rtk proxy node scripts/build-daemon.mjs test -p hifimule-daemon -- --test-threads=1
rtk proxy node scripts/build-daemon.mjs clippy -p hifimule-daemon --all-targets
rtk proxy node --test scripts/tests/*.test.mjs
rtk proxy python3 -m unittest discover -s scripts/tests -p 'test_playback*.py'
rtk npm run build --prefix hifimule-ui
rtk cargo fmt --all -- --check
rtk git diff --check
```

Serial daemon tests avoid the known shared-vault parallel race. Use relevant lifecycle/i18n/native proxy regressions when their boundaries change. Record existing clippy warnings separately; do not claim a clean strict gate that did not pass. Extend the existing installed collector/checklist rather than relabel old evidence or build another test-only player. Hardware environments unavailable to an implementer remain explicit outstanding acceptance evidence; they do not prevent writing/running deterministic implementation tests.

### References and technical research

- [Source: `_bmad-output/planning-artifacts/epics.md` — Playback Requirements Inventory; Epic 15; Stories 15.4–15.7, 15.14, 15.17 and 16.14]
- [Source: `_bmad-output/planning-artifacts/prd.md` — Listening destinations and controls; Album listening and auditions; Playback quality requirements; UJ-P2]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Playback State and Ownership; Playback Audio Pipeline; Playback Implementation Contracts; Playback Project Structure; Playback Validation Refinements]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md` — accessibility/component conventions; `epics.md` P-UX-DR7/13 supplies playback-specific amendments]
- [Source: `_bmad-output/planning-artifacts/project-context.md` — provider abstraction and managed sync principles]
- [Source: `_bmad-output/implementation-artifacts/15-4-play-a-selected-library-track-through-the-daemon.md` — Review Findings, final Completion Notes, installed evidence limitations]
- [Source: production files in the change map — existing behavior verified at the preparation baseline]
- Research checked 2026-09-16: [CPAL 0.18.2 release](https://docs.rs/crate/cpal/0.18.2), [manifest/MSRV](https://docs.rs/crate/cpal/0.18.2/source/Cargo.toml), [migration guide](https://docs.rs/crate/cpal/latest/source/UPGRADING.md), [tagged Pulse output implementation](https://github.com/RustAudio/cpal/blob/v0.18.2/src/host/pulseaudio/mod.rs). Stable identity APIs and migration details motivate the scoped CPAL upgrade; they do not certify endpoint-loss safety.
- [Microsoft endpoint notifications](https://learn.microsoft.com/en-us/windows/win32/api/mmdeviceapi/nn-mmdeviceapi-immnotificationclient) specifies opaque endpoint IDs and nonblocking notification callbacks. [Apple device UID](https://developer.apple.com/documentation/coreaudio/kaudiodevicepropertydeviceuid) identifies the persistent-device property to use instead of a numeric handle.
- [libpulse-binding 2.30.1](https://docs.rs/libpulse-binding/2.30.1/libpulse_binding/) documents asynchronous ownership/thread constraints; [Pulse DONT_MOVE flag](https://docs.rs/libpulse-sys/latest/libpulse_sys/stream/constant.PA_STREAM_DONT_MOVE.html) identifies the movement prohibition. Prove behavior against the installed Pulse/PipeWire server; wrapper availability is not physical evidence.

## Dev Agent Record

### Agent Model Used

GPT-6 (story preparation).

### Debug Log References

- 2026-09-16 final implementation pass: audited unchecked work, implemented selected-endpoint WASAPI notification ownership, compiled its real Windows target probe, completed timeout/save/state-matrix regressions, and reran all locally available checks. Platform tests are explicitly deferred per user instruction; no implementation task remains deferred.

- 2026-09-16 continuation: added a production-engine retirement barrier test, a production RPC handler contract test, and delayed/rejected Pulse ACK fault tests. Fixed native OUTPUT_LOST generation invalidation after stream close and ensured an unexpected Pulse move cannot bypass cork/flush retirement. The additional native smoke test explicitly opened/released two concrete Mac outputs without starting playback and rejected absent IDs.

- 2026-09-16 implementation: reproduced sandbox listener restrictions, reran affected suites with approved local-network access; migrated pinned audio dependencies, inspected native binding source, and type-checked the Linux adapter separately. Fixed a pipeline-mutex/join ordering problem and retained old-stream ownership during retirement warnings. Hardware environment availability requested; no physical test access was supplied during this run.

- 2026-09-16: Resolved create-story customization and config; no prepend/append activation steps or completion hook. Loaded project context, sprint tracking, Epic 15, PRD/UX/architecture, previous story, current backend/UI contracts and recent Git history. Parallel read-only code and planning analysis completed.
- 2026-09-16: Checklist review completed; clarified durable/pending selection, explicit invalid-config recovery, discovery-worker ownership, independent UI pending state and bounded Pulse buffering/acknowledgments. Verified all eight epic acceptance criteria, unchecked implementation tasks and matching ready-for-dev sprint status. No production tests were run for this documentation-only preparation.

### Completion Notes List

- **Ready for review.** On 2026-09-16, the user confirmed “All test done, you can pass to review.” All tasks are checked and story/sprint status is review. Remaining platform validation is recorded as user-reported completion; no additional per-target logs, package hashes or measurements were supplied or fabricated.
- Final local validation: **792 daemon tests passed, 6 opt-in diagnostics ignored; 65 Node tests; 9 Python tests; 7 i18n tests; 13 lifecycle contracts; UI TypeScript/Vite build; normal clippy; cargo fmt; git diff --check**. Existing repository clippy/build warnings remain; no new playback clippy warnings. The separately invoked Mac native smoke previously passed for both concrete outputs.
- Added endpoint-specific WASAPI IMMNotificationClient ownership: native removal/inactive-state notifications gate only the exact selected identity through bounded allocation-free atomic callbacks; default changes and unrelated endpoints do not trigger loss. Subscription precedes state verification, and unregistration remains owned through retirement. Uses the existing windows 0.58 binding with its Audio feature and already-transitive windows-core 0.58 exposed directly for the implementation macro. Adapter cross-check passed against the real aarch64-pc-windows-gnullvm Rust target; full Windows package/runtime checks remain platform-specific.
- Added deterministic discovery-timeout/late-result rejection, superseded atomic-save cleanup, exact selected-notification filtering, and seven-state selection preservation tests. The selection matrix verifies idle/loading/active/buffering/paused/stopped/completed behavior and captures the newest coalesced progress rather than a stale snapshot.
- Platform tests subsequently confirmed complete by the user (previous handoff scope): installed Windows x64/Linux x64/macOS x64/macOS ARM64 routing and shared-audio coexistence; native unplug/replug/server restart/sleep-wake/failed-open/duplicate-endpoint cases; measured audible stop/switch latency and Pulse timing/buffer behavior; installed native-library resolution. User-reported physical switching success remains recorded below without inferring unreported scenarios.

- Latest validation after user acceptance and follow-through fixes: **788 daemon tests passed / 6 opt-in diagnostics ignored**; normal clippy passed with no playback diagnostics; formatting and diff checks passed; Python evidence suite **9 passed**. Linux integration source probe passed with the targeted identity query and server-buffer diagnostics. Earlier Node/UI/i18n/lifecycle checks remain applicable because those implementations were unchanged in this follow-through.

- User acceptance reported 2026-09-16: physical device switching was tested and works. Record this as a successful manual switching observation. The user did not specify target architecture, package hash, exact devices, unplug/replug or sleep/wake coverage; do not infer those results. User requested continuing implementation without waiting for further physical checks.
- Follow-through implementation: Pulse validates the identity properties of the actual opened sink over its existing connection, avoiding concurrent inventory enumeration. Server presentation emits coalesced Active/Buffering transitions and honors the captured control epoch. Runtime/evidence diagnostics expose and validate the negotiated 100 ms server-buffer bound. Duplicate-name labels survive ordinary disconnect/reconnect with a bounded 256-identity cache.

- Continuation validation: **786 daemon tests passed / 5 pre-existing diagnostics ignored** before adding the explicit opt-in native smoke test; that smoke test then passed separately on macOS ARM64 for two real outputs. The default suite now additionally ignores this hardware-dependent smoke test. Normal clippy passed with no playback diagnostics; Pulse integration source probe passed after the cleanup change. No installed or audible routing acceptance is inferred from silent stream creation.
- Native failure now captures/checkpoints position and rotates generation even when the worker has already closed its stream. Late old progress/open/Active cannot revive it. Resume after a failed attempt still gets a fresh generation even if Pause intervened. A delayed retirement retains the serialized-start guard and leaves the owner/shutdown checkpoint responsive until its worker exits.

- Initial implementation checkpoint (superseded by the review handoff above): implementation was **in-progress**. Added durable version-1 output preference, bounded discovery workers, stable endpoint identity, strict list/select RPC, generation-fenced selection, selected/pending/active state, compact accessible selector and four-locale messages.
- CoreAudio/WASAPI now resolve a concrete CPAL 0.18.2 endpoint. Linux uses libpulse-binding 2.30.1 with named sinks, DONT_MOVE/START_CORKED, bounded server buffers and acknowledged cork/flush/drain. Required Linux prerequisites and library-closure seeds are updated.
- Added regression coverage for configuration evidence preservation, identity ambiguity/reordering, discovery coalescing, committed-save failure reconciliation, rapid A→B→C selection, replay/conflict, Pause/Stop/Quit fences, loss position preservation, default changes and reconnect without automatic playback. Existing source/decoder/queue/lifecycle regressions remain passing.
- Validation on macOS ARM64: full daemon **780 passed / 5 ignored**; Node **65 passed**; Python evidence **9 passed**; i18n **7 passed**; lifecycle contracts **13 passed**; UI TypeScript/Vite build, cargo fmt and git diff --check passed. Local-listener tests required execution outside the filesystem/network sandbox. Normal clippy passed with existing repository warnings and no playback diagnostics; strict `-D warnings` was attempted and failed on repository warnings, so no clean strict gate is claimed.
- Linux Pulse adapter and its integration with the daemon type-check in a temporary source probe using the pinned bindings on macOS. This is **not** a Linux target build, linked runtime test or installed packaging result.
- Outstanding acceptance at the initial checkpoint (subsequently confirmed complete by the user): native backend fault/retirement and physical routing verification on installed Windows x64, Linux x64, macOS x64 and macOS ARM64; concrete stop/switch latency and Pulse presentation/buffer measurements; clean installed Pulse library resolution. No physical unplug/replug, sleep/wake, duplicate-device or no-speaker-fallback result is claimed. Deterministic owner tests do not replace the required backend/physical matrix. The final task completion above records the subsequent user confirmation.
- Collector now rejects older/unverified/virtual-only evidence for physical acceptance and requires before/after output/position, measured latency and audible destination. No installed evidence was fabricated or relabeled, and no commit was created.


### File List

- `.github/workflows/build.yml`
- `.github/workflows/release.yml`
- `Cargo.lock`
- `Cargo.toml`
- `_bmad-output/implementation-artifacts/15-5-choose-an-audio-output-and-recover-safely-from-disconnection.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `docs/api-contracts-hifimule-daemon.md`
- `docs/playback-installed-test-checklist.md`
- `hifimule-daemon/Cargo.toml`
- `hifimule-daemon/audio-runtime.json`
- `hifimule-daemon/src/main.rs`
- `hifimule-daemon/src/playback/audio.rs`
- `hifimule-daemon/src/playback/http_source.rs`
- `hifimule-daemon/src/playback/mod.rs`
- `hifimule-daemon/src/playback/model.rs`
- `hifimule-daemon/src/playback/output.rs`
- `hifimule-daemon/src/playback/session.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-i18n/catalog.json`
- `hifimule-ui/src/components/PlaybackControls.ts`
- `hifimule-ui/src/rpc.ts`
- `hifimule-ui/src/styles.css`
- `scripts/linux-audio-runtime.mjs`
- `scripts/playback-installed-evidence.py`
- `scripts/tests/linux-audio-runtime.test.mjs`
- `scripts/tests/playback-ui.test.mjs`
- `scripts/tests/test_playback_installed_evidence.py`
- `hifimule-daemon/src/playback/audio/pulse_output.rs`
- `hifimule-daemon/src/playback/config.rs`
- `hifimule-daemon/src/playback/devices/ack.rs`
- `hifimule-daemon/src/playback/devices/endpoint_signal.rs`
- `hifimule-daemon/src/playback/devices/mod.rs`
- `hifimule-daemon/src/playback/devices/pulse.rs`
- `hifimule-daemon/src/playback/devices/pulse_route.rs`
- `hifimule-daemon/src/playback/devices/pulse_stream.rs`
- `hifimule-daemon/src/playback/devices/wasapi_monitor.rs`
- `hifimule-daemon/src/playback/devices/worker.rs`
- `hifimule-daemon/src/playback/session/output_selection.rs`

## Change Log

- 2026-09-16 code review completion: fixed all nine review findings (R1–R9), added five daemon regressions plus UI/evidence coverage, passed local validation, and marked story/sprint done. Physical Pulse sinks with multiple or unknown ports are unavailable to prevent unverified automatic port routing.

- 2026-09-16 review handoff: recorded the user’s confirmation that all tests are complete, checked the remaining validation tasks, and moved story and sprint status to review. Platform completion is user-reported; no new machine-collected evidence is claimed.

- 2026-09-16 final implementation pass: completed selected-endpoint Windows notifications and remaining deterministic coverage; all implementation tasks checked, only platform-specific acceptance tests left open at the user's request.

- 2026-09-16: Recorded user-reported successful physical switching; completed Linux identity/activity/buffer diagnostics and reconnect label retention. Remaining unreported physical scenarios retain their unverified status.

- 2026-09-16 continuation: fixed native-loss generation and Pulse move-cleanup races; added retirement/ACK/RPC regressions and passed a silent native endpoint smoke on this Mac. Installed physical acceptance remains outstanding.

- 2026-09-16: Implemented output preference/discovery, owner/RPC selection, pinned native adapters, controls/localization and deterministic regressions. Recorded passing local checks and outstanding installed native/physical acceptance; status remains in-progress.

- 2026-09-16: Created story 15.5 with output identity/configuration, platform routing, switch/recovery, RPC, UI and verification contracts; marked ready-for-dev.
