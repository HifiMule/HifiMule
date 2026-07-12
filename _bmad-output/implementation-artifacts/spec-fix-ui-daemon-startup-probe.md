---
title: 'Fix UI daemon startup probe'
type: 'bugfix'
created: '2026-07-12'
status: 'done'
baseline_commit: '0faa0bdf0a96117fbfcd1c23f1544a789600f399'
context:
  - '{project-root}/_bmad-output/planning-artifacts/architecture.md'
---

<frozen-after-approval reason="human-owned intent -- do not modify unless human renegotiates">

## Intent

**Problem:** When an HifiMule daemon is already running, UI startup can still decide the daemon is unavailable, attempt `sc start`, and spawn a sidecar fallback. This creates duplicate daemon startup attempts and contradicts the intended startup attach behavior.

**Approach:** Make the UI startup availability probe use the daemon's dedicated `daemon.health` JSON-RPC method, matching smoke tests and release checks. Keep the existing startup cascade intact: health check first, Windows Service attempt second, sidecar fallback last.

## Boundaries & Constraints

**Always:** Preserve the existing startup cascade and sidecar fallback. Treat any successful health RPC response as proof that the daemon is already reachable. Keep localhost port `19140` unchanged.

**Ask First:** Changing daemon process ownership, Windows Service registration behavior, launchd behavior, or sidecar packaging.

**Never:** Remove the Windows Service fallback, remove the sidecar fallback, add a new port/protocol, or make UI startup depend on full daemon state just to detect reachability.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Existing daemon reachable | `daemon.health` returns HTTP success on `127.0.0.1:19140` | UI sets sidecar status to `"startup"` and skips service/sidecar spawn | N/A |
| No daemon reachable | Health RPC connection fails or returns non-success | UI continues to existing Windows Service/sidecar fallback | Existing fallback logging/status remains |
| Service starts daemon | Initial health fails, `sc start` succeeds, follow-up health succeeds | UI sets sidecar status to `"service"` and skips sidecar spawn | Existing service failure path remains |

</frozen-after-approval>

## Code Map

- `hifimule-ui/src-tauri/src/lib.rs` -- UI startup daemon detection, Windows Service fallback, sidecar spawn, and status state.
- `hifimule-daemon/src/rpc.rs` -- Implements `daemon.health` and `get_daemon_state`; confirms health is the lightweight reachability RPC.
- `scripts/smoke-tests/*` -- Existing install smoke tests already poll `daemon.health`.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-ui/src-tauri/src/lib.rs` -- Change `check_daemon_health()` to call `daemon.health` -- align startup reachability detection with the daemon's dedicated health RPC.

**Acceptance Criteria:**
- Given an already-running daemon that responds to `daemon.health`, when the UI starts, then it logs the daemon as already running and does not attempt `sc start` or spawn a sidecar.
- Given no reachable daemon, when the UI starts, then the existing service and sidecar fallback behavior still runs.

## Spec Change Log

## Verification

**Commands:**
- `rtk cargo test -p hifimule-ui` -- expected: UI crate tests compile and pass.

## Suggested Review Order

- Startup probe now asks the daemon's dedicated health method.
  [`lib.rs:46`](../../hifimule-ui/src-tauri/src/lib.rs#L46)

- JSON-RPC body validation prevents method-not-found from counting healthy.
  [`lib.rs:38`](../../hifimule-ui/src-tauri/src/lib.rs#L38)

- Focused unit coverage locks healthy, error, and non-ok health responses.
  [`lib.rs:355`](../../hifimule-ui/src-tauri/src/lib.rs#L355)
