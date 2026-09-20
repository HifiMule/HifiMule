---
title: 'Make macOS smoke UI relaunch deterministic'
type: 'bugfix'
created: '2026-09-20'
status: 'done'
baseline_commit: 'b5a9c8d22d24a48776f1bf3d92b8958ee52f4127'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The 0.15.0 arm64 installed smoke test successfully hydrates its initial and concurrent UIs, then times out waiting for the reopened UI. The daemon remains healthy with the same identity. The script sends SIGTERM without confirming UI exit and subsequently uses ordinary `open`, which does not guarantee a fresh process receiving the new smoke ID.

**Approach:** Make the installed macOS smoke harness verify that its UI processes have terminated before relaunch, request a fresh application instance for each marked launch, and report useful process diagnostics when termination or hydration fails. Preserve the real webview acknowledgment as the success gate.

## Boundaries & Constraints

**Always:** Keep installation through the DMG and launches through macOS `open`; retain fresh smoke IDs, live-UI evidence, daemon identity comparisons, authentication checks, and crash recovery. Scope process handling to this installed app. Use bounded termination waits compatible with macOS system Bash.

**Ask First:** Changes to application lifecycle semantics or weakening installed UI evidence requirements.

**Never:** Treat daemon health alone as successful UI attachment, reuse an old acknowledgment, expose the daemon token, kill unrelated processes, or replace the timeout with an arbitrary longer sleep.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Normal reopen | Both launched UIs exit after SIGTERM | Verify exit, then launch a fresh marked UI attached to the original daemon | Require matching hydration evidence |
| Delayed exit | A UI remains alive temporarily | Wait within a fixed deadline before reopening | No new launch while old UI is alive |
| Stuck UI | UI survives the termination deadline | Fail at UI close with scoped process diagnostics | Do not continue to an ambiguous hydration timeout |
| Crash recovery | Original daemon killed, previous UI still present | Close previous UI before starting a fresh marked instance | Require replacement daemon identity |
| Missing acknowledgment | Fresh UI fails to hydrate | Preserve failure with stage and marker diagnostics | Never print owner token |

</frozen-after-approval>

## Code Map

- `scripts/smoke-tests/smoke-macos.sh` — DMG installation, LaunchServices launches, UI process termination, daemon identity assertions, cleanup.
- `scripts/smoke-tests/smoke-common.sh` — fresh marker generation and strict hydration polling; existing behavior remains authoritative.
- `scripts/smoke-tests/test-ui-evidence.py` — existing regression checks preventing health-only or stale-owner success.
- `hifimule-ui/src-tauri/src/lib.rs` — `report_ui_ready` reads `--smoke-id` from process startup arguments. Reactivating an existing instance cannot replace those arguments.

## Tasks & Acceptance

**Execution:**
- [x] `scripts/smoke-tests/smoke-macos.sh` — centralize fresh marked launches and bounded UI shutdown; apply consistently to reopen and crash recovery; include safe failure diagnostics.
- [x] `scripts/smoke-tests/test-macos-lifecycle.py` — add a mocked-command harness that exercises delayed exit, stuck UI, launch arguments, and crash recovery ordering without installing or terminating a real application.
- [x] `scripts/smoke-tests/test-ui-evidence.py` — run existing evidence regression tests to ensure acknowledgment strictness is preserved.

**Acceptance Criteria:**
- Given an installed application, when a marked launch occurs, then `open` requests a new instance with that launch's unique smoke ID.
- Given running test UIs, when a close/reopen transition runs, then the script confirms their exit before requesting the next instance.
- Given a UI that never exits, when the bounded deadline expires, then the script fails at the close stage with useful diagnostics.
- Given a successful reopened UI, when its hydration acknowledgment is checked, then both its live PID and unchanged daemon identity remain required.
- Given daemon crash recovery, when a replacement UI starts, then the script still requires a new daemon identity and real hydration evidence.

## Spec Change Log

## Design Notes

The log narrows the failure to the third UI launch, but does not prove whether LaunchServices reused an instance or a new process failed to hydrate. The current script cannot distinguish these cases. macOS's local `open(1)` manual documents `-n` as requesting a new instance even if one is already running. Combining it with verified termination addresses the launch ambiguity without hiding surviving UI processes.

The UI acknowledgment uses immutable startup arguments, so a healthy daemon cannot demonstrate that the new marker reached a new UI. Initial and concurrent launches already demonstrate that the installed artifact can hydrate. Application Rust or frontend changes are not justified by the supplied evidence.

## Verification

**Commands:**
- `rtk bash -n scripts/smoke-tests/smoke-macos.sh` — shell syntax passes.
- `rtk python3 scripts/smoke-tests/test-macos-lifecycle.py` — process sequencing and failure regressions pass.
- `rtk python3 scripts/smoke-tests/test-ui-evidence.py` — matching live UI evidence remains mandatory.

**Installed verification:** Rerun the release smoke workflow against the same 0.15.0 DMG on macOS arm64; both reopen and crash recovery must produce fresh UI acknowledgment evidence. Mock tests cannot establish installed WebKit behavior, so report this separately if the artifact or CI execution is unavailable.


## Verification Results

- Bash syntax and whitespace checks passed.
- Four mocked macOS lifecycle tests passed, covering independently delayed UI exits, stuck UI, missing acknowledgment, and a relative symlink install root.
- Three existing UI evidence tests passed.
- Local macOS `ps` probe confirmed `comm` reports an executable path without arguments.
- Independent blind, edge-case, and acceptance reviews completed. Review patches normalized the install directory and strengthened staggered-exit coverage.
- Installed 0.15.0 DMG verification remains pending: no local artifact was available and this workflow performs no remote operations.

## Suggested Review Order

- Verify all installed UI processes exit before relaunch.
  [smoke-macos.sh:36](../../scripts/smoke-tests/smoke-macos.sh#L36)

- Ensure every marked launch creates a fresh instance and preserves strict hydration evidence.
  [smoke-macos.sh:70](../../scripts/smoke-tests/smoke-macos.sh#L70)

- Normalize install paths before literal executable matching.
  [smoke-macos.sh:123](../../scripts/smoke-tests/smoke-macos.sh#L123)

- Trace reopen and crash recovery through the shared helpers.
  [smoke-macos.sh:166](../../scripts/smoke-tests/smoke-macos.sh#L166)

- Inspect mocked process sequencing and failure coverage.
  [test-macos-lifecycle.py:1](../../scripts/smoke-tests/test-macos-lifecycle.py#L1)
