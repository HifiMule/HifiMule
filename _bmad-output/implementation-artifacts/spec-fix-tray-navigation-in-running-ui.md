---
title: 'Preserve daemon menu navigation when the UI is already running'
type: 'bugfix'
created: '2026-10-24'
status: 'done'
baseline_commit: '1677ac5641c598a9fc65a539ff7928617d295d47'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** In installed builds, Playback settings, Device profiles, and Choose audio output bring an existing UI forward without opening the requested surface. They work with the UI closed; playback/Radio controls use separate daemon paths.

**Approach:** Preserve targeted requests across launch and single-instance handoff. A launcher serving an existing menu request must not replace it with generic foreground activation. Reuse existing route handlers and ownership.

## Boundaries & Constraints

**Always:** Retain one UI owner per profile, manual-launch foreground behavior, readiness gating, and close/reopen handoff. Preserve all three destinations while minimized or starting. Keep mailbox validation and bounded waits; delayed launchers must not overwrite newer choices.

**Ask First:** Changes to the lifecycle protocol version, persisted activation schema, or unrelated playback/Radio behavior.

**Never:** Restart processes as a workaround, introduce another IPC service, redesign these surfaces, or alter audio/sync state.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Behavior | Error Handling |
|----------|--------------|-------------------|----------------|
| Running | Each affected entry; existing UI owner | Restore window and open requested surface | Retain access/focus failure logging |
| Closed/starting | Target arrives before hydration | One UI opens destination after readiness | Retain startup failure UI |
| Duplicate | Launcher follows posted menu request | Preserve its target; exit after acknowledgment | Take ownership if old UI exits |
| Newer choice | Delayed launcher follows an earlier click | Preserve latest mailbox request | Retain bounded coalescing |
| Manual | Direct launch while UI runs | Fresh generic foreground request | Retain bounded handoff |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/main.rs` — tray targets and release/debug binary launches.
- `hifimule-ui/src-tauri/src/lib.rs` — duplicate arbitration in `run`, live delivery in `watch_ui_activation`, cold delivery in `report_ui_ready`.
- `hifimule-lifecycle/src/lib.rs` — validated activation mailbox, IDs, acknowledgments, ownership, bounded handoff.
- `hifimule-lifecycle/src/bin/lifecycle-owner-probe.rs` and `hifimule-lifecycle/tests/contract.rs` — real-process handoff harness.
- `hifimule-ui/src/main.ts` — existing frontend handlers for all three surfaces.

## Tasks & Acceptance

**Execution:**

- [x] `hifimule-daemon/src/main.rs` — pass the successfully posted request ID to directly spawned UI binaries, covering release launch and debug reopen. Correlate initial npm/Tauri dev launches too, forwarding application arguments through both runner layers. Preserve launch-error logging and runner reuse.
- [x] `hifimule-ui/src-tauri/src/lib.rs` — recognize launch correlation before arbitration. Correlated duplicates wait on that request; manual duplicates still post a fresh generic request. Carry correlation into a new owner’s readiness dispatch: deliver the matching current request once even if acknowledged by the prior owner, but never replay a superseded ID. Preserve readiness gating.
- [x] `hifimule-lifecycle/src/lib.rs` — share the minimal validated request-selection logic between production startup and the process probe. Preserve schema/privacy guarantees and update the comment assuming identical activation intents.
- [x] `hifimule-lifecycle/src/bin/lifecycle-owner-probe.rs` and `hifimule-lifecycle/tests/contract.rs` — cover all three destinations across process handoff, manual launches, already acknowledged requests, owner exit, newer requests, and malformed/missing correlation. Assert target IDs/flags survive and invalid input cannot corrupt the mailbox.
- [x] `hifimule-ui/src-tauri/src/lib.rs` — check argument parsing/startup wiring and existing cold/live dispatch.

**Acceptance Criteria:**

- Given each matrix scenario, when activation completes, then the expected surface opens with one UI owner and unchanged daemon ownership.
- Given active playback or sync, when menu navigation occurs, then navigation does not interrupt that work.
- Given ordinary manual launch and close/reopen tests, when the fix is applied, then existing lifecycle contracts continue to pass.

## Spec Change Log

- Review iteration 1 (bad_spec): readiness acceptance covered acknowledgment and takeover only as mailbox/lock checks, missing actual destination dispatch after an acknowledged request changes UI owner. Clarified that startup correlation must reach readiness dispatch and be consumed once only for the current matching request. Also made correlation explicit for initial npm/Tauri dev runners so delayed runners cannot overwrite newer choices. Avoid the known-bad default route after takeover and generic overwrite from an uncorrelated dev runner. KEEP the request-ID argument, manual-launch fresh activation, validation, current schema/protocol, superseded-mailbox preservation, bounded lock-before-ack handoff, and three-target process coverage. Test hardening (patch): bound new subprocess waits and clean up timed-out children; avoid scheduling-sensitive startup timing assertions.

## Design Notes

The duplicate overwrites the target with `request_ui_activation`. Pass `--ui-activation-request <id>` to correlate it with posted work, even if already acknowledged. `wait_for_ui_handoff` checks ownership before acknowledgment, retaining takeover. Superseded launchers must not rewrite the mailbox; manual launches must not adopt stale requests.

## Verification

**Commands (repository root):**

- `rtk cargo test -p hifimule-lifecycle` — existing and new mailbox/process handoff contracts pass.
- `rtk cargo test -p hifimule-ui --lib` — native startup and window activation checks pass.
- `rtk cargo check -p hifimule-daemon` — modified tray launch path compiles with the repository's audio-runtime environment.
- `rtk npm --prefix hifimule-ui run build` — frontend type checking and bundling pass.

**Manual checks:**

- On a macOS release build with an isolated profile, exercise all three entries with the UI running, minimized, closed, and starting. Confirm the correct surface and one UI owner; repeat different choices to catch delayed-launch overwrites. Report if native verification is unavailable.

### Iteration 1 verification — 2026-10-24 (superseded by review re-derivation)

- `rtk cargo test -p hifimule-lifecycle`: **27 passed**. Initial sandbox run passed 26 and could not bind the existing loopback health-test listener; the complete suite passed with local loopback access.
- `rtk cargo test -p hifimule-ui --lib`: **12 passed**.
- `rtk npm run build:daemon -- check -p hifimule-daemon`: **passed**, running the required Cargo check through the repository's supported wrapper after verifying the controlled FFmpeg 9.0.2 runtime. Existing deprecation/dead-code warnings remain.
- `rtk npm --prefix hifimule-ui run build`: **passed** (TypeScript and Vite); existing mixed static/dynamic import warnings remain.
- `rtk git diff --check` and scoped `rustfmt --check` for lifecycle source/probe/contracts: **passed**. Workspace-wide formatting check reports extensive pre-existing formatting differences; unrelated files were not reformatted.
- Production and probe share argument parsing and request selection. Real-process tests cover each menu destination before acknowledgment, acknowledgment before duplicate launch, cold ownership, owner exit during handoff, delayed superseded IDs, manual launch replacing stale targeted requests, and missing/malformed/repeated correlation arguments. IDs, flags, exclusive UI ownership, and daemon ownership are asserted.
- Inspected both direct daemon spawn paths, UI parsing before arbitration, lock-before-ack ordering, `report_ui_ready`, and the live activation watcher. Existing readiness gates and destination dispatch are preserved.
- Native installed-release menu interactions (including minimized windows and active playback/sync) were not exercised. Those manual checks remain for release verification.


### Iteration 2 verification — 2026-10-24

- `rtk cargo test -p hifimule-lifecycle`: **27 passed**, with local loopback access required by the existing health test. New process regressions exercise the production readiness dispatcher, asserting each actual destination after an acknowledged request changes owner and then no destination on the next readiness call. They also cover takeover during handoff, superseded acknowledged and unacknowledged choices, manual foreground requests, invalid arguments, target preservation, and unchanged daemon ownership.
- New probe tests use five-second output/exit bounds and kill/reap cleanup on timeout or assertion failure. They do not assert a process-startup latency threshold.
- `rtk cargo test -p hifimule-ui --lib`: **12 passed**. Startup correlation is stored per owner and consumed by the shared dispatcher only inside the existing hydrated/epoch/ready checks in `report_ui_ready`; live routing and lock-before-ack ordering remain intact.
- `rtk npm run build:daemon -- check -p hifimule-daemon`: **passed**, after controlled FFmpeg 9.0.2 runtime verification. Existing warnings remain.
- Inspected `scripts/run-tauri.mjs`, the UI npm script, and installed Tauri CLI `dev --help`. The npm launch now uses its argument separator plus both Tauri runner/application separators, preserving correlation for delayed initial dev runners as well as direct debug/release launches.
- `rtk git diff --check` and scoped lifecycle `rustfmt --check`: **passed**. No unrelated formatting changes.
- Frontend sources are unchanged; the successful iteration 1 TypeScript/Vite build remains applicable. Native installed-release/minimized-window and active playback/sync manual interactions remain untested.

## Final Review

Independent blind, edge-case, and acceptance reviews of iteration 2 found no actionable defects. Iteration 1 findings are addressed in the re-derived implementation; new subprocess tests have bounded waits and timeout cleanup. Native macOS release interaction remains the recorded verification limitation.

## Suggested Review Order

**Launch and handoff**

- Preserve the menu request ID across every daemon launch path.
  [main.rs:1484](../../hifimule-daemon/src/main.rs#L1484)

- Reuse correlated requests while retaining fresh activation for manual launches.
  [lib.rs:1092](../../hifimule-ui/src-tauri/src/lib.rs#L1092)

- Validate correlation without modifying the mailbox.
  [lib.rs:619](../../hifimule-lifecycle/src/lib.rs#L619)

- Forward application arguments through npm and Tauri.
  [main.rs:1567](../../hifimule-daemon/src/main.rs#L1567)

**Readiness delivery**

- Deliver the current matching destination once, including after prior acknowledgment.
  [lib.rs:664](../../hifimule-lifecycle/src/lib.rs#L664)

- Apply destination selection only after frontend hydration.
  [lib.rs:164](../../hifimule-ui/src-tauri/src/lib.rs#L164)

**Regression coverage**

- Exercise production readiness decisions after process ownership changes.
  [lifecycle-owner-probe.rs:53](../../hifimule-lifecycle/src/bin/lifecycle-owner-probe.rs#L53)

- Bound subprocess reads and exits, cleaning up children on timeout.
  [contract.rs:218](../../hifimule-lifecycle/tests/contract.rs#L218)
