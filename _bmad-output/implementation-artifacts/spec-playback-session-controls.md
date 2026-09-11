---
title: Playback session lifetime and native media controls proof
type: chore
created: 2026-09-11
status: in-progress
baseline_commit: a4b49d1
context: []
---

<frozen-after-approval reason="User authorized the next daemon lifetime and media-key proof with let's go">

## Intent

**Problem:** The existing isolated probe proves decoding and native output, but stdin controls and a short-lived CLI do not establish that music can survive a control UI closing or respond to OS media controls. Production Linux currently kills its UI-owned sidecar on UI exit.

**Approach:** Extend the standalone experiment with an optional session mode owning playback, native media registration and platform event-loop resources. A separate disposable controller connects to that session. Exercise closing/reopening the controller, native transport events and explicit shutdown on the Mac and available Windows/Ubuntu VMs. Record actual outcomes and distinguish OS transport commands from physical keyboard routing.

## Boundaries & Constraints

**Always:** Remain inside the standalone experiment and documentation. Preserve the existing decoder/callback behavior and default build. Run audio silently at volume zero and only use generated local fixtures. Keep FFmpeg objects on their creating threads, transfer only owned PCM, and retain bounded buffering. Use the signed-in desktop session and platform-native controls. Fail visibly if audio or media registration fails. Bound every experimental session to at most three minutes, with an explicit quit command and cleanup. Preserve evidence and record untested/failed scenarios honestly.

**Ask First:** Changes to production startup, installers, persistent background registrations, global OS/audio settings, or playback of private library material beyond this proof.

**Never:** Call a synthetic internal event injection proof of OS media-key delivery. Treat UI close as daemon quit. Run playback as a system service. Add streaming, Radio, taste memory, seeking, physical-device synchronization or production player UI. Certify physical gaplessness from callback counters.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Disposable controller | Session playing; controller closes | Same session remains alive and consumed-frame count advances | Controller disconnect has no transport effect |
| Reconnect | New controller attaches to live session | Same identity, playback status and progress | Missing/stale endpoint returns bounded explicit error |
| Pause/resume | OS event or controller command | Consumption stops while paused and resumes from retained queue | Record event source separately; unsupported events rejected/logged |
| Quit/deadline | Explicit quit or finite timeout | Audio and media registration stop; process exits | No indefinite joins or zombie decoder/session |
| Invalid input | Malformed command, oversized request, wrong session identity | Rejected without controlling another process | Bound request size and timeouts |
| Startup failure | Missing endpoint or media registration unavailable | Nonzero failure and explanatory log | No silent backend/device fallback |

</frozen-after-approval>

## Code Map

- `experiments/playback-probe/src/main.rs` — existing finite playback, callback state and CLI; add a narrow shared-control entry point without duplicating the decoder.
- `experiments/playback-probe/src/session.rs` — new session ownership, disposable controller and command transport.
- `experiments/playback-probe/src/media.rs` — new optional native transport/platform event-loop adapter, if separation helps clarity.
- `experiments/playback-probe/Cargo.toml` and `Cargo.lock` — optional pinned session-control dependencies only.
- `experiments/playback-probe/test_session.py` — bounded lifecycle integration harness and evidence collection.
- `experiments/playback-probe/README.md` — repeatable commands and platform requirements.
- `_bmad-output/implementation-artifacts/playback-session-results.md` — observed platform evidence and integration recommendations.
- Production reference: `hifimule-ui/src-tauri/src/lib.rs` UI-owned child shutdown; `hifimule-daemon/src/main.rs` main-thread Tao tray loop. These files are inspected, not modified.

## Tasks & Acceptance

**Execution:**
- [x] `Cargo.toml`, `Cargo.lock`, `src/main.rs`, `src/session.rs`, optional `src/media.rs` — add feature-gated native session and external controller with explicit cleanup, preserving old probe paths.
- [x] `src/session.rs` tests — verify command validation, pause preservation, controller disconnect semantics, bounded shutdown and unsupported event handling where deterministic.
- [x] `test_session.py` — start the session independently with redirected process handles, close/reopen controller, measure status/counters and clean up; report native OS events independently.
- [x] `README.md` — document exact feature/build/launch commands, controller limitations and explicit Quit versus close behavior.
- [ ] `playback-session-results.md` — record native build/runtime observations on available platforms, media registration and actual command delivery, identity/lifecycle evidence, limitations, and required production integration changes.

**Acceptance Criteria:**
- Given the existing probe, when built without the new feature, then old tests and six-format FFmpeg verification retain their established behavior and no media-control dependency is required by the default build.
- Given a desktop session and generated compatible fixtures, when the independent session starts, then it owns audio and native media registration for its bounded lifetime with no control UI required.
- Given the disposable controller has closed, when queried/reopened, then the same session identity and advancing audio counters are observed until paused, completed or explicitly quit.
- Given native controls are registered, when an actual OS transport action is issued, then logs identify the native event and its effect on playback; unavailable physical keyboard routing is recorded separately.
- Given tests end or encounter an error, when cleanup runs, then experimental processes, endpoints and media registrations are removed without changing production launch policy.

## Spec Change Log

## Design Notes

The user has authorized this bounded proof, not a production lifecycle migration. An optional cross-platform transport wrapper is acceptable if its requirements are inspected and its actual platform behavior is measured. The session owns macOS application event-loop resources, a Windows native handle/message loop, and a Linux session-bus identity. A controller may be deliberately minimal; it must be a separately owned process or window whose close cannot destroy the session. Local IPC must be restricted to loopback or user-local endpoints with bounded messages and per-session identity, not the production daemon port. Existing finite fixtures may be repeated within a bounded session to provide time for manual controls; this does not imply production queue or repeat support.

## Verification

- Default/native Rust tests and feature-enabled checks; existing six-format verifier.
- Bounded lifecycle harness on macOS, then available Windows ARM64 and Ubuntu ARM64 desktop sessions with exact runtime libraries recorded.
- Native OS transport commands and observed metadata while the controller is closed; record keyboard input limitations accurately.
- Inspect process exit, endpoint cleanup, captured counters and failure paths; run focused review before completion.

## Current evidence and open checks

Mac independent-owner lifecycle and Windows interactive-session lifecycle/native SMTC checks passed with zero underruns and explicitly recorded owner exit code 0. Default tests (9), feature-enabled tests (23), Clippy and the six-format native decoder regression passed. Three independent review passes found harness and controller issues; patches and focused re-review completed. Ubuntu runtime/MPRIS and macOS native-control delivery remain pending because macOS was locked when UI automation attempted desktop access. The spec remains in progress.

## Suggested Review Order

- Start with ownership, finite lifetime and native registration.
  [session.rs:1](../../experiments/playback-probe/src/session.rs#L1)
- Check independent controllers, native counters and recorded owner exit status.
  [test_session.py:1](../../experiments/playback-probe/test_session.py#L1)
- Inspect the separate Windows OS transport client.
  [main.rs:1](../../experiments/playback-probe/windows-native-remote/src/main.rs#L1)
- Read measured platform results and remaining verification limits.
  [playback-session-results.md:1](playback-session-results.md#L1)
