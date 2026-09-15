# Story 15.4 Review Fixes Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development to implement this plan task by task. Keep changes uncommitted for user review.

**Goal:** Resolve all sixteen recorded review findings without claiming unperformed installed acceptance.

**Architecture:** The serialized playback owner controls command admission, deduplication and desired transport state. Native workers consume generation/control fences, bounded compressed data and complete PCM frames; UI controls reconcile authoritative snapshots without losing focus or leaking polling.

**Tech Stack:** Rust 2024, Tokio, CPAL 0.16, ffmpeg-next 9, TypeScript, Shoelace, Node tests, Python evidence tools.

**Spec:** `_bmad-output/implementation-artifacts/15-4-play-a-selected-library-track-through-the-daemon.md`, Review Findings R1–R16 and the selected implementation contract.

## Global Constraints

- Preserve schema-v1 RPC, portable source identity, sanitized errors and paused restoration.
- Compressed prefetch: 8 MiB maximum; decoder read chunks at most 64 KiB, with a separate 1 MiB transport-chunk reserve; no growing disk spool.
- PCM: at most 500 ms and 1 MiB; consume complete mono/stereo frames.
- Start/refill: 100 ms or clean short-track EOF. Preparation: 60-second wall budget, cancellable throughout.
- Preserve bounded worker retirement and shutdown acknowledgement; callbacks must not allocate, block or acquire locks.
- Prefix shell commands with `rtk`; use the controlled daemon wrapper: `rtk node scripts/build-daemon.mjs test -p hifimule-daemon -- --test-threads=1`.
- Add a failing behavioral regression before implementation. Do not use source-text checks for UI behavior.
- Only one implementation subagent edits at a time. No commits or pushes. Parent performs task review and final independent review.

### Task 1: Accessible, disposable transport UI (R12–R16)

**Files:** `hifimule-ui/src/components/PlaybackControls.ts`, `hifimule-ui/src/main.ts`, `hifimule-ui/src/rpc.ts` if needed, `hifimule-ui/src/styles.css`, `hifimule-i18n/catalog.json`, `scripts/tests/playback-ui.test.mjs` and a test DOM utility if necessary.

**Interfaces:** Existing `PlaybackSessionSnapshot` and `playbackControl`. Preserve public wire schema. Use stable primary/stop DOM nodes, one polling schedule, and explicit mount/dispose ownership.

- [ ] Write/run failing rendered behavioral tests: loading offers Pause; primary focus survives Pause→Resume; output-loss retry explains current default output; failures render a localized error; disposal prevents further polling and removes listeners; late snapshots do not render after disposal.
- [ ] Implement stable control reconciliation and visible structured failures. Primary action is Pause for authoritative playing/buffering and Resume for paused; retry copy depends on OUTPUT_LOST. Disable controls while the corresponding request is in flight. Avoid announcing unchanged status on every progress update.
- [ ] Retain the active controller in `main.ts`, dispose it on login/shutdown/layout removal, and prevent duplicate mounting. Test actual lifecycle behavior through a production mount owner where necessary.
- [ ] Run Node behavior tests, localization parity, TypeScript and Vite build. Report changed files, red/green output and limitations.

### Task 2: Owner-serialized transport effects (R1, R2, R10, R11)

**Files:** `hifimule-daemon/src/playback/session.rs`, `model.rs`, `audio.rs` for narrowly scoped engine control/start coordination, `hifimule-daemon/src/rpc.rs` and their tests.

**Interfaces:** Preserve public schema. A private owner result/effect claim distinguishes newly admitted commands from replay. Desired transport state/control ordering must be available when a pipeline installs and when worker state events are applied. Terminal events require bounded reliable retention, not a lossy `try_send`.

- [ ] Write/run failing tests for PlayTrack replay, Resume→Pause→replay Resume, Pause during deferred source start, stale Active/Buffering after Pause, Resume while Active, and full owner command mailbox followed by terminal failure/completion.
- [ ] Move/claim audible effects under owner authority exactly once. Ensure the effect is invalidated by later Pause/Stop/replacement or uses the latest desired gate. Avoid introducing another session manager. Dedup across apply/control must not permit command-ID reuse with a different payload.
- [ ] Keep Resume idempotent while already active/loading. Make worker events obey desired transport/control ordering. Retain pending terminal transitions in bounded storage while preventing deadlock with owner/engine locks and shutdown.
- [ ] Run targeted router/session/audio regressions, then report interfaces consumed by Task 3, tests and any concern.

### Task 3: Bounded stream and output lifecycle (R3–R9)

**Files:** `hifimule-daemon/src/playback/streaming.rs`, `decoder.rs`, `audio.rs`, `hifimule-daemon/audio-runtime.json`, decoder/audio tests, evidence collector/validation tests only where accounting semantics change.

**Interfaces:** Consume Task 2 generation/desired-control coordination. Preserve existing original provider streaming. Prefer bounded verified range-backed random access to maintain tail-moov M4A support; explicitly reject unsupported random access instead of allocating whole-file disk history. Keep capability/request details private.

- [ ] Write/run failing transport fixtures: packet-boundary HTTP failure must return an error; compressed queue plus history never exceeds the shared byte budget; unsupported evicted seek cannot spool to disk; cancellation joins all workers; mono/stereo underrun preserves channels; short-track final output is retained through presentation; preparation trickle exceeds a controllable deadline and fails recoverably.
- [ ] Drive error-observing FFmpeg packet reads and inspect shared source failure before successful completion. Preserve clean decoder/resampler drains and restored frame discard.
- [ ] Replace disk history with bounded access. Keep an aggregate compressed byte budget including queued chunks/history, expose truthful high-water telemetry, and retain required formats. Add controlled HTTP range fixtures if range access is implemented; verify status/content-range and same-origin credentials.
- [ ] Introduce complete PCM frame handoff/refill gating without callback locks/allocations. Track backend presentation of the final buffer before Completed. Use fake callback timestamps for deterministic drain tests and retain endpoint loss checks through drain.
- [ ] Enforce a 60-second preparation deadline spanning resolution/fetch/probe/restored discard, and ensure cancellation exits join decoder workers before acknowledgement. Native output claims still require installed reruns.
- [ ] Run targeted tests, then the complete serial daemon suite and relevant script/evidence tests. Report measured caps and runtime limits.

### Task 4: Integration verification and story closure

- [ ] Independently review the complete patch for spec compliance and new regressions; fix confirmed findings and rerun affected checks.
- [ ] Run daemon, lifecycle, Tauri library/localization where supported, all script/evidence tests, TypeScript/Vite, formatting and relevant clippy. Record environment limitations accurately.
- [ ] Check off R1–R16 only once implemented and verified. Update story/sprint to in-progress while Linux x64/macOS x64 installed evidence remains open; invalidate any old native evidence claims affected by the changed audio pipeline until rerun.
- [ ] Report the fixes, test results and remaining installed validation. Leave changes uncommitted.
