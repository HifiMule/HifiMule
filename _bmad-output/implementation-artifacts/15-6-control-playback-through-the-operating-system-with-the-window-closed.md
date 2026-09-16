---
baseline_commit: 0ff836482119940cccd3808d9b1deefb907b72ea
---
# Story 15.6: Control playback through the operating system with the window closed

Status: review

## Story

As a HifiMule user,
I want native media controls to operate the same player as the UI,
so that I can pause and resume music while working without reopening HifiMule.

**Requirements:** Native controls portion of FR56 and FR58; Resume portion of FR57; P-NFR3–4; native event-loop portion of P-AR1 and P-AR10; applicable P-UX-DR4 and P-UX-DR13.

**Dependencies:** Stories 15.1–15.5 are done. Extend their production lifecycle, serialized session owner, single-track player and selected-output safety. Preparation date: 2026-09-16.

**Scope:** Native Play/Pause/Toggle/Stop, now-playing projection, daemon-owned registration and desktop-menu Resume. Native seek belongs to 15.7, Next to 15.8, full Playback destination to 15.12, floating bar to 15.14, Play something/Radio menu wiring to 15.20, and server listening reports to 15.21. Do not advertise those unfinished transport actions or add a browser player, global keyboard hook, second event loop, cross-application media-priority override or reporting side effect.

## Acceptance Criteria

1. **Use one transport authority.** Given a playing or paused track, when supported native Play, Pause, Toggle or Stop arrives, it executes through the same serialized session owner as the equivalent UI command. Native and UI state reflect the actual result, without a separate native transport state.
2. **Control with the UI closed.** Given a playing track, closing the main UI leaves native Pause and Resume operational without opening a window. Reopening the UI shows the resulting state and position without replaying the native command.
3. **Resume the existing session from the desktop menu.** Given an existing paused session with a playable current track and usable selected output, Resume continues it without opening the main window or creating Radio. Missing source/output or restoration failures remain paused and are reported accessibly. No resumable session means the action is unavailable or clearly explained.
4. **Publish truthful native state.** Track replacement, pause, buffering, failure and completion update supported now-playing fields and transport availability from authoritative state. Missing fields clear previous-track data. Local OS metadata changes never submit media-server listening reports.
5. **Reject unsupported or unsafe actions.** An unsupported command or Play while the selected output is unavailable neither fabricates success nor silently reroutes output. Disable or omit unsupported native actions wherever the platform permits.
6. **Respect command races and shutdown.** Commands during replacement, output switching or shutdown follow established generation and shutdown rules. They cannot restart audio after Quit or activate obsolete session work.
7. **Own registration for the daemon lifetime.** Startup, UI close/reopen and Quit use the existing daemon native loop. UI relaunches do not duplicate registration. Explicit Quit clears the app's registration and stale metadata as supported by the OS.
8. **Record platform evidence accurately.** Installed Windows, macOS and Linux builds separately test native API delivery and physical media-key routing with the UI open and closed. Record OS, architecture, desktop session and command path. API success is not physical-key evidence; desktop routing limitations are recorded without installing a global keyboard hook.

## Tasks / Subtasks

- [x] Extract the shared transport command/effect path (AC: 1–3, 5–6)
  - [x] Move the existing RPC Resume backend effect into a playback-owned service used by RPC, native controls and menu actions.
  - [x] Add bounded native ingress; resolve Toggle and current target inside the serialized owner; retain lifecycle admission and generation fencing.
  - [x] Preserve explicit RPC identities, deduplication, selected-output policy, provider routing, bounded preparation and error handling.
- [x] Implement the native adapter and dependency patch (AC: 1, 4–7)
  - [x] Integrate the proven Souvlaki baseline into Tao ownership with the command mask, replacement metadata and cleanup corrections below.
  - [x] Implement and test Windows SMTC, macOS remote commands/now-playing and Linux MPRIS mappings, including actual Stop support and disabled future controls.
  - [x] Publish bounded authoritative projections without blocking the native loop or audio callback; reject stale publication and handle registration failures explicitly.
- [x] Add desktop-menu Resume and accessible failure feedback (AC: 2–3, 5–7)
  - [x] Add daemon tray/app-menu Resume, projected availability and localized explanation; preserve Open UI, retry-saving-session and Quit.
  - [x] Surface asynchronous source/output failures without opening or focusing a window. Keep restored sessions paused until an explicit command.
  - [x] Add English, French, Spanish and German messages and locale coverage.
- [x] Validate production integration and document evidence (AC: 1–8)
  - [x] Add deterministic command, effect, metadata, registration and shutdown-race tests using production seams.
  - [x] Extend the existing installed checklist/collector to distinguish API, UI reopen and physical-key observations, with strict evidence validation.
  - [x] Run applicable daemon, lifecycle, i18n, UI, dependency/build and evidence checks; record installed target results and outstanding environments honestly.

## Dev Notes

### Selected implementation contract

These are preparation decisions, not claims of implemented behavior. They close this story's adapter ownership, command/metadata, cleanup and Resume behavior gate. Keep native integration private to the daemon: no new public RPC or database migration is required merely to deliver media keys.

#### One command path, including the audio effect

`PlaybackSession` already serializes `OwnerCommand` work. Its `ControlAction` contains Pause, Resume and Stop. `rpc.rs::handle_playback_control` currently performs an essential second step: after owner admission it examines private `SessionSnapshot.resume_audio`, tries `audio::global().resume_existing(generation)`, and otherwise resolves the current source and starts at the saved position under a 60-second preparation deadline. **Calling only `control_with_guard` from a native callback will not reproduce RPC Resume.**

Extract that orchestration into `playback/commands.rs`; keep RPC parameter validation/error serialization in `rpc.rs`. The service owns shared session/server-manager/database handles, not a new audio engine. Both RPC and native/menu paths consume the same admitted effect. Preserve the private effect flag: replayed command results must not start audio again. Provider resolution remains through `get_provider_by_server_id` and `MediaProvider::resolve_playback`, using the current occurrence's portable server ID rather than the browsed server. Preserve `RESUME_UNAVAILABLE`, specific `OUTPUT_*` failures, sanitized diagnostics and existing generation-aware failure publication.

Native callbacks enqueue a small typed intent to a bounded ingress (capacity 64); they do not wait for the session owner, access SQLite, resolve a provider or call audio directly. A daemon worker drains intents through the shared service and acquires `SyncOperationManager::try_admit_mutation`; carry that guard through owner execution just as RPC does. Full ingress rejects the command with a bounded/rate-limited diagnostic and accessible failure where possible; never grow an unbounded task or thread list. Do not coalesce Toggle or reorder accepted transport intents. Await owner admission, then dispatch the admitted effect under the existing bounded preparation ownership and continue draining commands: a slow 60-second Resume preparation must not block a later Pause/Stop from reaching the owner.

Add an internal native intent operation to the owner. It resolves the current occurrence and Toggle **at owner execution**, then reuses the existing control logic and effect admission. Native keys are intents for the authoritative current session at serialization time, not commands built from a stale adapter snapshot. Menu Resume uses this same path. Give each admitted intent a unique command ID; one OS callback is submitted once. Preserve the existing strict generation/occurrence checks for UI RPC requests. After the owner admits an effect, its generation remains attached through source resolution, output opening and publication; a later Pause, Stop, replacement or Quit must win over superseded work.

| Native/menu input | Owner behavior |
| --- | --- |
| Play | Resume the existing current occurrence using current output policy. Already playing/loading with play intent is an idempotent no-op. Never creates a queue or Radio. |
| Pause | Existing Pause semantics, including while buffering; already paused is harmless. No current track means unavailable/no action. |
| Toggle / PlayPause | Resolve playing/buffering intent to Pause, otherwise to Resume, within owner ordering. Two accepted toggles execute in order. |
| Stop | Existing playback Stop: gate/retire audio, rotate generation, reset position to zero, retain current occurrence/queue and checkpoint. **Never Quit the daemon.** |
| Desktop Resume | Existing paused session only; revalidate availability at execution. No UI launch, no replacement queue. Stopped/completed current occurrences follow existing Resume/replay semantics, including position zero at completed replay. |
| Next, Previous, Seek, SetPosition, fast-forward/rewind, OpenUri, unsupported setters | Disabled/omitted; unexpected delivery returns unsupported or safely rejects without mutation. Do not translate into a different supported action. |
| Native Quit request, if exposed by an OS interface | Use the existing coordinated application Quit path; never translate into playback Stop or direct process exit. Prefer not advertising optional Quit in this adapter. |

`output_policy` remains authoritative: invalid config, pending switch or unavailable selected endpoint blocks Resume. An uncertain source (`SourceAvailability::Unknown`) is not proof that it is offline; resolve it normally. A known missing source or invalid restoration disables Resume or supplies an explanation. The browse-selected `get_daemon_state.serverConnected` flag is not evidence that the current playback source can resume. Asynchronous failure leaves the preserved session paused and retryable. Reconnection, registration and metadata refresh never trigger Resume themselves.

#### Native lifetime and platform ownership

Create `playback/native.rs` (platform helpers may live in `playback/native/`). Own the native registration on the daemon's existing main-thread Tao loop in `main.rs`, after single-instance ownership. Keep the current `ControlFlow::WaitUntil` strategy; do not spin or introduce Winit/a second application loop. Callback ingress and native state publication are separate channels. Native handles stay on their required platform thread; audio workers retain their current ownership.

`DaemonCoreHandle` currently does not expose playback/server-manager handles. Publish a narrow command-service sender and latest-state receiver through the existing core/RPC initialization; the actual `ServerManager` is constructed in `rpc.rs::run_server`, populated after vault migration, and must be shared. Never create another manager or call `PlaybackSession::restore` again to manufacture a tray-owned session. Server logout/removal may evict credentials/providers without clearing the session; fail Resume recoverably without changing its source. The core command loop awaits shutdown completion and can remain occupied during checkpoint retry, so committed-Quit native cleanup must run from the main-thread lifecycle path rather than a later message waiting behind that shutdown. Preserve the separate health/RPC runtime that remains responsive while core workers drain, including the stopping-state `playback.retryCheckpoint` exception.

| Platform | Adapter ownership and mapping |
| --- | --- |
| Windows | SMTC in the signed-in desktop daemon. Create one hidden, non-activating Tao native window for the required HWND; retain it until media handlers are detached and registration is released. Never borrow a disposable Tauri window's handle. Explicit SMTC button availability follows the command mask; clear/update the display updater on track changes and teardown. |
| macOS | Existing main-thread Tao AppKit loop with Accessory activation policy. Own remote-command targets and now-playing publication there; add Stop explicitly. No visible window is needed. Remove targets and clear now-playing information on teardown. Do not replace Tao's application delegate or claim system-wide media priority. |
| Linux | One stable application MPRIS identity on the signed-in user's session bus (e.g. `org.mpris.MediaPlayer2.hifimule`), with truthful Player capabilities. No native window is required merely for MPRIS. Retain the bus worker/registration through daemon lifetime; release its name and worker on Quit. X11/Wayland routing is desktop-controlled. |

The desktop menu is the existing daemon tray menu, which remains reachable with Tauri closed. Add a localized Resume item; enable it from the latest owner projection when there is a resumable current occurrence, acceptable restoration and usable selected output, with no pending switch/shutdown. Owner revalidation handles a stale enabled menu. Preserve the existing Open UI, retry-saving-session and Quit items. A failed background Resume updates a persistent accessible menu status/explanation and uses the existing native notification mechanism for a user-triggered failure; honor OS notification/DND policy, do not repeatedly notify each poll, and do not open/focus the UI. Reopened controls retain their authoritative snapshot/error presentation.

Registration errors must not terminate otherwise functional UI playback or masquerade as working media keys. Expose a localized native-controls-unavailable status and sanitized diagnostic. Clean partial registration before any bounded retry, and never retry on every UI reconnect. Normal UI close/reopen does not register or detach controls.

At shutdown admission, stop admitting new native commands and project unavailable controls. Preserve existing launch-generation fencing, cancellation/checkpoint retries and failure recovery. If Quit fencing fails and the daemon intentionally remains usable, reconcile availability to its actual lifecycle state; do not leave it falsely advertised as exited. On committed Quit, clear metadata, disable controls and detach handlers on their owner thread before dropping the Windows HWND/native owner and exiting the loop. Bound shutdown handling and surface teardown failures without bypassing managed-device safety or claiming that a cleanup request is a completed acknowledgment.

#### Dependency decision and required Souvlaki corrections

Use exact **Souvlaki 0.8.3**, the native-control feasibility baseline and current docs.rs release checked on 2026-09-16. The production manifests do not yet include it. Introduce a narrow repository-local patch under `third_party/souvlaki`, selected with the workspace `[patch.crates-io]` and an exact daemon dependency. Preserve its license notices and record the upstream version and local diff in a short README. Keep the default D-Bus backend explicit; do not switch to the optional zbus backend without validating its separate lifecycle/capability implementation. Keep platform binding types isolated from the daemon's existing Windows 0.58 types.

**Unmodified 0.8.3 is insufficient for AC4–5:** it has no public per-command capability setter; Windows enables future buttons, Linux D-Bus capabilities are hardcoded true, macOS registers future commands but omits Stop, and Windows metadata setters skip absent fields instead of clearing old values. Add a small capability API and complete metadata replacement/clear support in the patch, plus macOS Stop registration. Merely dropping unsupported callbacks does not fix false OS advertising. Test the actual patched backend projections, not only a daemon-side mask. Optional platform properties that genuinely cannot be represented must be documented as limitations, not silently claimed supported.

Patch cleanup as well as capabilities: Windows 0.8.3 discards the position-change subscription token and removes only the button token. Retain and remove both tokens, take/reset ownership on detach, and make partial initialization and repeated detach safe. macOS detach must clear now-playing state and invalidate pending artwork completions before removing targets. MPRIS detach joins its service thread: do not hold a session/sync lock or block audio; make cleanup progress and failure observable. The crate's Drop swallows detach errors, so explicit checked teardown is required. Tao's `WindowExtWindows::hwnd()` returns an `isize`; use its raw-pointer conversion for `PlatformConfig`, keeping Souvlaki's Windows 0.44 binding types separate from the daemon's 0.58 types.

Also set MPRIS application-level `CanQuit`/`CanRaise` false unless those actions are actually implemented; default true values are not acceptable. A rejected Seek/SetPosition must not emit the upstream unconditional `Seeked` signal or change position. Extend the callback boundary to report immediate admission/rejection where supported (macOS currently always returns success): full ingress, unsupported and stopping rejection must not claim accepted delivery. Accepted asynchronous intent is not proof of successful playback; publish the eventual owner result and failure separately.

Do not upgrade playback dependencies as part of this integration. Current production baseline is Rust 1.93.0/edition 2024, Tao ~0.31, tray-icon ~0.19, CPAL **0.18.2**, ffmpeg-next/ffmpeg-sys-next **9.0.0**, crossbeam-queue **0.3.12**, and Linux libpulse-binding **2.30.1**. The probe's CPAL 0.16.0 and Winit 0.28.7 are historical evidence, not production version choices. Extend existing platform build prerequisites/package closure checks for the selected native-control backend; do not rely on undeclared developer-machine libraries.

#### Authoritative projection and metadata

Publish a small immutable native view from the owner, with instance/session/generation identity, state sequence, current occurrence, metadata, position and capabilities. Generic `DaemonState::Idle/Syncing/Error` describes device/sync activity, not playback: sync completion may publish Idle while music continues, so never derive native transport from that channel. Use a latest-value slot/watch channel; do not accumulate state history or page the queue/SQLite on every native-loop tick. Project on meaningful changes and use existing bounded progress samples when a platform needs position. Keep counters in their existing wire forms (decimal revision/sequence strings, UUID generations, integer milliseconds); native duration conversions must be checked, including MPRIS microseconds. Presentation never becomes authoritative progress.

| Authoritative state | Native presentation |
| --- | --- |
| Playing with current-generation active audio | Playing; publish current position and supported Pause/Stop. |
| Buffering/preparing or output switch | Windows may use its changing state; otherwise use a non-advancing paused presentation, retaining the owner's playing intent for Toggle. Never invent consumed progress or show a stale track. |
| Paused / output loss / recoverable failure | Paused, actual frozen position; Play availability follows output/source/restoration policy. |
| Idle / stopped / completed / shutdown | Stopped/non-advancing. Retain metadata only if it belongs to the still-current occurrence; empty/cleared session and final Quit clear it. Completed replay follows the owner's established Resume rule. Shutdown disables commands. |

Metadata is a **replacement**, not a patch: use current `PlaybackTrackMetadata` title/optional artist/album and `duration_ms`. Clear all old fields when current identity changes before new metadata resolves; only publish metadata whose source matches the current occurrence. Missing artist/album/duration remains absent. The current model has no safe artwork field: omit/clear artwork for this story instead of exposing authenticated provider URLs or adding an unrelated image-fetch pipeline. Clear unsupported fields retained by prior native publications. Do not use OS now-playing metadata APIs as a reason to call provider scrobble/reporting endpoints.

Native callbacks must not optimistically update play state themselves. Apply native projection only from newer authoritative snapshots; discard superseded asynchronous projection completions. Coalesce publication, bound metadata sizes defensively without altering session source identity, and avoid flooding OS APIs from progress-only updates. Native metadata errors do not mutate the listening queue or cause a second playback attempt.

### Current code, intended changes and preservation map

| File | Current behavior → story change; behavior to preserve |
| --- | --- |
| `hifimule-daemon/src/main.rs` | Owns core thread, desktop Tao tray loop, 250 ms wait, lifecycle handshake and coordinated shutdown. Add native registration/projection, command bridge and Resume menu. Preserve single instance, Mac Accessory policy, sync cancellation, persistence retry and ownership retention during delayed shutdown. |
| `hifimule-daemon/src/playback/session.rs` | Bounded serialized owner, progress/event coalescing, generation/control-epoch fences, private effect admission, dedup and checkpoints. Add internal ordered native intents and bounded publication; preserve RPC identity checks, effect uniqueness, source/queue identity and no I/O on the audio callback. |
| `hifimule-daemon/src/rpc.rs` | Authenticated strict playback RPC plus Resume provider/audio orchestration. Extract that orchestration into shared service; preserve error envelopes, admission guards, deadlines and sanitized failure mapping. Native commands do not loop back through unauthenticated HTTP. |
| `hifimule-daemon/src/playback/mod.rs` | Registers existing playback modules. Export new internal commands/native modules without moving the established engine or scaffolding a new crate/application. |
| `hifimule-daemon/src/playback/model.rs` | Existing snapshot/metadata/control types; add only internal native intent/projection types if shared. Keep schemaVersion 1 and current public wire contract unless a justified backward-compatible diagnostic field is required and documented. |
| `hifimule-daemon/src/playback/audio.rs`, `session/output_selection.rs` | Reuse existing resume/start/gating and exact selected-output policy. No planned routing redesign; preserve stable identity independent of display name, active ownership until close ACK, current-generation terminal cleanup and no default fallback. Read fully before any additional change. |
| `Cargo.toml`, `Cargo.lock`, `hifimule-daemon/Cargo.toml` | Add pinned patched native-control dependency/platform prerequisites. Preserve existing audio pins and Windows binding boundaries. New `third_party/souvlaki/` is explicitly scoped upstream code with minimal reviewed corrections. |
| `hifimule-i18n/catalog.json`, `hifimule-i18n/src/lib.rs` | Four-locale shared translations and playback key-parity test. Add native/menu/status strings under the covered `playback.*` prefix or extend parity coverage for new keys. Preserve fallback/interpolation. |
| `hifimule-ui/src/components/PlaybackControls.ts` | Existing real UI location: snapshot polling every 500 ms, state-sequence ordering, accessible error/output controls and disposal. Normally regression-only here; no browser MediaSession registration. Preserve authoritative reopening, focus, selector reconciliation and polling cleanup. `src/state/playback.ts` is an architecture proposal, not an existing file to edit. |
| `docs/api-contracts-hifimule-daemon.md`, `docs/playback-installed-test-checklist.md` | Update shared command semantics/native behavior and installed acceptance instructions. Keep public RPC and evidence claims accurate. |
| `scripts/playback-installed-evidence.py`, `scripts/tests/test_playback_installed_evidence.py` | Extend strict installed evidence rather than accept empty native observations. Preserve existing runtime, output-identity and physical/virtual checks. Use the session evidence tooling/probe helper as a reference, not another production player. |

Conditional build-workflow/script edits must be based on the pinned dependency's actual prerequisites. Read each additional existing file completely before modifying it; avoid unrelated cleanup. No provider trait, sync manifest, queue persistence or full Playback UI rewrite is required.

### Previous story and Git intelligence

- Latest commits: `0ff8364` Review 15.5, `0b732fe` Dev 15.5, `50a2f46` Story 15.5, `5dc5a65` Finished 15.4, `876bbb5` Review 15.4. The working tree was clean at preparation. Use current code, not the previous story's pre-implementation dependency description.
- Story 15.5's nine review fixes matter here: reject unsafe multi/unknown-port physical Pulse routes; fence before retiring a current pipeline; compare stable output identity without display name; finish terminal output preparation; distinguish unrelated discovery errors from selected endpoint loss; retain active output until close acknowledgment; preserve focused selector refresh; require correct buffer bounds and complete evidence records. Native Play must not bypass these policies.
- Story 15.4/15.5 effect and generation work prevents duplicate Resume, late Active after output loss, and stale validation retiring a newer track. Toggle must not reintroduce those races through a snapshot-then-command adapter.
- Latest recorded 15.5 validation: 797 daemon tests, 6 opt-in diagnostics ignored; 66 Node tests; 12 Python evidence tests; 7 i18n tests; UI build, normal clippy and formatting checks passed. These are historical results, not Story 15.6 validation. Physical platform acceptance was user-reported; do not invent package hashes or transfer it to native-control acceptance.
- `playback-session-results.md` records macOS ARM64 physical Toggle, Windows 11 ARM64 SMTC API Pause/Play and Ubuntu ARM64 MPRIS API Pause/Play. Windows/Linux physical keys were not tested; Wayland teardown emitted a warning. The probe maps Stop/ Quit to experimental owner exit: production Stop must retain its playback-only semantics. Its hidden window and independent controller prove a mechanism, not current shipping architecture support.
- Project context's greenfield/planning status is stale. Its provider abstraction and managed-zone principles remain valid. The approved playback PRD/architecture amendments override legacy global no-device listening restrictions while preserving physical basket/sync safety.

### Testing requirements

Use deterministic barriers/fake adapters around production ingress, owner and effect paths. A pure mapping test or a successful media-key callback alone cannot prove audible transport effects.

| Area | Required checks |
| --- | --- |
| Transport parity | UI versus native Play/Pause/Stop; Toggle while playing/paused/buffering; ordered repeated toggles; already-playing Play; no-current commands; stopped/completed Resume. Assert admitted effect counts, preserved queue/current and actual audio progress hold/advance. |
| Resume integration | Existing pipeline resume and source reopen at nonzero saved position; missing provider/auth/timeout; restored session; no output, lost output, invalid config and pending switch. Assert no default fallback, no UI launch/new Radio, no duplicated provider start. |
| Races/admission | Native intent versus PlayTrack, Pause, Stop, rapid output switches, loss and Quit; full ingress/owner queues; duplicate delivery submission; late resolution/open/Active; command arriving during cleanup. Exercise slow native Resume followed immediately by Pause/Stop through the actual ingress worker, not only owner calls. Assert generation/control-epoch fencing and no audio after committed Quit. |
| Native contract | Per-platform advertised capabilities, including Next/Previous/Seek disabled; macOS Stop delivered as Stop; unexpected unsupported command rejected; populated track → metadata-poor track → empty session clears fields/artwork/duration; failure/buffering/completion projections do not fabricate Playing. |
| Lifecycle | One registration across repeated UI launches; native owner independent of UI; partial initialization failure cleanup; registration failure leaves RPC player working; Quit detaches/clears before HWND destruction; failed Quit fence recovery; bus/desktop-session loss and warning-free cleanup or documented failure. |
| Accessibility/UI | Keyboard-accessible menu names/status in four locales, no focus stealing, accessible background Resume error, no polling duplicate, authoritative reopen state/position and existing output-selector focus regression. |
| Installed | Windows x64, Linux x64, macOS x64 and ARM64: package/build identity, native library versions, OS/desktop session, same daemon PID/instance before/after UI close/reopen, API Pause/Play/Stop, menu Resume, metadata clearing, output-loss rejection and final registration release. Separately record actual physical-key delivery open/closed; label additional ARM64 VM or virtual-output runs. |

For installed evidence, record command source, delivery observation and authoritative before/after state/position separately. Physical-key non-delivery is a routing limitation, not an API pass or an invented implementation success. Reuse the Windows remote-helper pattern scoped to HifiMule's own session, and target HifiMule explicitly in MPRIS tests; never control unrelated players. Do not log tokens, authenticated URLs or private library metadata in shareable captures. Keep missing hardware evidence explicit; available deterministic work can proceed without claiming AC8 completed.

Add `nativeEvidenceVersion: 1` and validated native observations separately from the collector's existing `outputEvidenceVersion: 1`. Require command path, UI-open/closed state, actual delivery/outcome or explicit limitation, sanitized daemon PID/instance continuity and generation/state-sequence fields. Negative tests reject empty/missing native records and API observations mislabeled as physical keys. The current collector prompts relaunch after Quit before its final snapshot: insert native deregistration/metadata-clear observations **before relaunch**, then independently verify the new instance's registration. Older output-only records cannot satisfy this story's native evidence gate.

Run from the repository root using its controlled audio build wrapper:

```sh
rtk proxy node scripts/build-daemon.mjs test -p hifimule-daemon playback:: -- --test-threads=1
rtk proxy node scripts/build-daemon.mjs test -p hifimule-daemon -- --test-threads=1
rtk proxy node scripts/build-daemon.mjs clippy -p hifimule-daemon --all-targets
rtk cargo test -p hifimule-lifecycle
rtk cargo test -p hifimule-i18n
rtk proxy node --test scripts/tests/*.test.mjs
rtk proxy python3 -m unittest discover -s scripts/tests -p 'test_playback*.py'
rtk npm run build --prefix hifimule-ui
rtk cargo fmt --all -- --check
rtk git diff --check
```

Run native patch tests and platform build/link checks for each supported target in addition to daemon tests. Serial daemon tests avoid the known shared-vault parallel race. Keep existing clippy warnings distinct from new diagnostics. Build/fixture evidence does not certify physical media-key routing.

### References and current technical research

- [Source: `_bmad-output/planning-artifacts/epics.md` — Story 15.6; Playback Requirements Inventory; adjacent transport stories and Epic 15 sequencing]
- [Source: `_bmad-output/planning-artifacts/prd.md` — Playback Extension; FR56–58/64; P-NFR3–6]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Playback State and Ownership; UI and Session Control; Deployment and Implementation Sequence; Implementation Contracts; Project Structure; Validation Refinements]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md` — accessibility/component conventions; playback-specific P-UX-DR4/13 are in `epics.md`]
- [Source: `_bmad-output/planning-artifacts/project-context.md` — provider and managed-device principles]
- [Source: `_bmad-output/implementation-artifacts/15-5-choose-an-audio-output-and-recover-safely-from-disconnection.md` — Review Findings and final validation; `playback-session-results.md` — platform evidence and limitations]
- [Source: `experiments/playback-probe/src/session.rs`; `experiments/playback-probe/windows-native-remote/` — native proof and API test helper]
- Research checked 2026-09-16: [Souvlaki 0.8.3 documentation](https://docs.rs/souvlaki/0.8.3/souvlaki/) establishes supported platforms/event-loop prerequisites; [MediaControls API](https://docs.rs/souvlaki/0.8.3/souvlaki/struct.MediaControls.html) exposes attach/detach, playback and metadata updates, but no public command-mask setter. The local upstream backend source was inspected to identify the capability, Stop and clearing corrections above.
- [MPRIS Player specification](https://specifications.freedesktop.org/mpris/latest/Player_Interface.html) defines transport/capability and metadata contracts; [Windows SMTC](https://learn.microsoft.com/en-us/uwp/api/windows.media.systemmediatransportcontrols?view=winrt-26100) exposes button availability; [Apple remote command center](https://developer.apple.com/documentation/mediaplayer/mpremotecommandcenter) is the native command boundary. These APIs do not promise that a desktop routes every physical key to HifiMule.

## Dev Agent Record

### Agent Model Used

GPT-6 (story implementation).

### Implementation Plan

- Extract the existing admitted Resume effect into one playback command service and add ordered native intents to the serialized session owner.
- Own a bounded native command bridge and authoritative now-playing projection for the daemon lifetime, with explicit shutdown cleanup.
- Patch pinned Souvlaki 0.8.3 narrowly for truthful capabilities, replacement metadata, Stop support and checked teardown.
- Add desktop-menu Resume, localized status/failure feedback, strict installed evidence, and platform prerequisite coverage.

### Debug Log References

- 2026-09-16: Resolved create-story customization; no prepend/append steps. Loaded config/project context, sprint tracking, planning sources, prior story and production/native-proof code with parallel read-only analysis. Reviewed the story against the create-story checklist and incorporated command-effect reuse, adapter capability/metadata corrections, source/output safeguards and evidence distinctions.
- 2026-09-16: Resolved dev-story customization; no prepend/append workflow steps. Implemented the shared command service, native owner/bridge, tray Resume, lifecycle cleanup, four-locale copy, patched native backends, prerequisites and strict installed-evidence flow.
- Validation: daemon playback suite 128 passed/6 ignored; full daemon suite 805 passed/6 ignored; lifecycle 13 passed; i18n 7 passed; Node 67 passed; Python playback evidence 15 passed; UI production build passed; clippy passed with pre-existing warnings; formatting and diff checks passed.
- Native dependency checks: patched Souvlaki compiled on macOS ARM64 and cross-compiled for Windows ARM64 GNU. Linux backend source and prerequisite closure are covered, but no Linux Rust target/session bus is installed in this environment.

### Completion Notes List

- Added one bounded, ordered native transport ingress that reuses the authoritative session owner and the same Resume audio/provider effect as RPC.
- Added truthful Play/Pause/Toggle/Stop capability, metadata and position projection; missing/stale metadata clears, unavailable output cannot advertise Resume, and unsupported native actions reject without mutation.
- Added daemon-lifetime SMTC/MPRemoteCommandCenter/MPRIS ownership, explicit pre-exit cleanup, nonfatal registration diagnostics, and a localized tray Resume action that never opens the UI.
- Vendored exact Souvlaki 0.8.3 with documented capability, Stop, metadata replacement and teardown fixes; preserved the default D-Bus backend and added its Linux build prerequisite.
- Extended installed evidence to require each native API transition with UI open/closed, menu/UI-reopen/metadata/output-loss/lifecycle observations, and separately labeled physical-key delivery or explicit routing limitations.
- Installed Windows x64, Linux x64, macOS x64 and macOS ARM64 package/media-key runs remain outstanding. No physical-key or installed-build success is claimed from source tests or cross-compilation.

### File List

- `_bmad-output/implementation-artifacts/15-6-control-playback-through-the-operating-system-with-the-window-closed.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `.github/workflows/build.yml`
- `.github/workflows/release.yml`
- `Cargo.lock`
- `Cargo.toml`
- `docs/api-contracts-hifimule-daemon.md`
- `docs/playback-installed-test-checklist.md`
- `hifimule-daemon/Cargo.toml`
- `hifimule-daemon/src/main.rs`
- `hifimule-daemon/src/playback/commands.rs`
- `hifimule-daemon/src/playback/mod.rs`
- `hifimule-daemon/src/playback/native.rs`
- `hifimule-daemon/src/playback/session.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-i18n/catalog.json`
- `scripts/linux-audio-runtime.mjs`
- `scripts/playback-installed-evidence.py`
- `scripts/tests/linux-audio-runtime.test.mjs`
- `scripts/tests/test_playback_installed_evidence.py`
- `third_party/souvlaki/CHANGELOG.md`
- `third_party/souvlaki/Cargo.toml`
- `third_party/souvlaki/Cargo.toml.orig`
- `third_party/souvlaki/HIFIMULE_PATCH.md`
- `third_party/souvlaki/LICENSE`
- `third_party/souvlaki/README.md`
- `third_party/souvlaki/build.rs`
- `third_party/souvlaki/examples/detach_on_drop.rs`
- `third_party/souvlaki/examples/print_events.rs`
- `third_party/souvlaki/examples/window.rs`
- `third_party/souvlaki/rust-toolchain.toml`
- `third_party/souvlaki/rustfmt.toml`
- `third_party/souvlaki/src/config.rs`
- `third_party/souvlaki/src/lib.rs`
- `third_party/souvlaki/src/platform/empty/mod.rs`
- `third_party/souvlaki/src/platform/macos/mod.rs`
- `third_party/souvlaki/src/platform/mod.rs`
- `third_party/souvlaki/src/platform/mpris/dbus/controls.rs`
- `third_party/souvlaki/src/platform/mpris/dbus/interfaces.rs`
- `third_party/souvlaki/src/platform/mpris/dbus/mod.rs`
- `third_party/souvlaki/src/platform/mpris/mod.rs`
- `third_party/souvlaki/src/platform/mpris/zbus.rs`
- `third_party/souvlaki/src/platform/windows/mod.rs`
- `third_party/souvlaki/tests/playerctl_script.sh`

## Change Log

- 2026-09-16: Implemented Story 15.6 native playback controls, daemon tray Resume, native backend corrections, lifecycle cleanup, localization, build prerequisites and strict installed evidence validation; moved story to review with installed platform observations explicitly outstanding.
