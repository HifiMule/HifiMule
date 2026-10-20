---
title: Restore sync monitoring automatically after transient daemon errors
type: bugfix
created: 2026-10-20
status: done
baseline_commit: a665e4b8596b871f0edcc2e8675a79c65cd82fb4
context:
  - investigations/sync-monitoring-disconnection-investigation.md
---

<frozen-after-approval reason="User approved automatic recovery with backoff on 2026-10-20">

## Intent

**Problem:** After three failed sync status requests, the sidebar permanently stops polling. The unavailable warning persists even after the same daemon becomes responsive; Check state currently restarts polling manually. The investigation reproduced this client behavior, while the initial transient error on this machine remains unconfirmed.

**Approach:** Keep monitoring the tracked sync with capped retry backoff after the warning appears. Let Check state request an earlier check through the same single-flight polling loop. Restore progress or reconcile completion/failure/cancellation from the next authoritative response.

## Boundaries & Constraints

**Always:** Preserve the tracked operation and syncing state during unavailable monitoring. Keep the existing warning, accessible live region, keyboard focus behavior and manual check action. Use one outstanding request per polling session; prevent the manual action from opening a parallel session. Discard responses after stop, destruction or an operation change. Preserve existing successful polling cadence and daemon identity validation. Keep background failures from implying transfer failure.

**Ask First:** Changing synchronization semantics, daemon ownership rules, RPC health deadlines or installed application binaries.

**Never:** Start another sync to recover status, infer completion from silence, change device writes, or claim the original machine failure is diagnosed by this fix.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected behavior | Error handling |
| --- | --- | --- | --- |
| Healthy transfer | Running status | Refresh progress at the existing 500 ms cadence | Preserve existing terminal handling |
| Brief errors | Fewer than three consecutive failures | Keep tracked operation and retry normally | Existing error reporting |
| Sustained outage | Three or more failures | Show unavailable warning and keep retrying with increasing capped delays | Preserve safe controls and operation identity |
| Automatic recovery | Same operation becomes responsive | Restore progress and announce restored status | Reset failure count and normal cadence |
| Manual retry | Warning and pending backoff | Check immediately without creating another loop | A failed check re-enables the button promptly |
| Pending automatic check | Manual click while RPC is pending | Share the outstanding check | Do not issue overlapping status requests |
| Terminal recovery | Complete, failed or cancelled status after outage | Stop polling and invoke the matching existing handler | No duplicate terminal handling |
| Stop/destroy/change | Pending retry or late response | No further old-session requests or UI changes | Ignore stale responses |

</frozen-after-approval>

## Code Map

- `hifimule-ui/src/components/BasketSidebar.ts` — owns polling, warning, manual action, live region and destruction.
- `hifimule-ui/tests/sync-status-hardening.html` — browser regression fixture for focus, announcements, manual recovery and stale replies.
- `hifimule-ui/tests/syncPolling.test.mjs` — deterministic timer/RPC regression coverage for the polling lifecycle.
- `hifimule-ui/src-tauri/src/lib.rs` — bounded operation-status transport requests so the shared polling guard can recover from a stalled response.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-ui/src/components/BasketSidebar.ts` — retain the polling interval after failures; gate attempts with a retry deadline and expose a shared immediate-check callback for the manual action.
- [x] `hifimule-ui/tests/syncPolling.test.mjs` — exercise capped backoff, autonomous recovery, manual interruption of backoff, pending-request sharing, failed manual checks, terminal states and stale response suppression using fake time and RPC responses.
- [x] `hifimule-ui/tests/sync-status-hardening.html` — adapt the fixture to the shared immediate manual action and verify automatic recovery preserves the accessible controls.
- [x] `hifimule-ui/src-tauri/src/lib.rs` — apply the existing 15-second polling timeout to sync status requests, preserving the health deadline and owner checks.
- [x] Build and run focused regressions; review the final diff for lifecycle and focus regressions.

**Acceptance Criteria:**
- Given three failed status checks, when the daemon later returns running status, then the sidebar recovers without a click and continues tracking the same operation.
- Given persistent errors, when retry time advances, then request delays increase to a finite maximum and requests never overlap within the session.
- Given a pending backoff, when Check state is clicked, then the next request starts immediately and shares the current session.
- Given an outstanding automatic request, when Check state is clicked, then it waits for that response and cannot launch another request.
- Given a failed manual request, when the response rejects, then the manual button becomes usable while automatic retries remain scheduled.
- Given recovered terminal status, when processed, then the matching terminal handler runs once and polling ends.
- Given a stopped or destroyed sidebar, when old requests settle or time advances, then they cannot update progress or restart polling.

## Spec Change Log

- 2026-10-20: Initial implementation plan follows the explicit approval to add automatic recovery with backoff. The earlier investigation document is the only pre-existing untracked file and belongs to this same request.

- 2026-10-20: Review patches bound status transport to the existing polling deadline and use monotonic retry time. These close the exposed stalled-request path and prevent clock corrections from extending backoff. Keep single-flight manual/automatic sharing, current operation fencing, the mounted warning and existing terminal handlers.

## Design Notes

Retain the 500 ms interval as the existing lifecycle owner. Gate actual requests with a next-attempt deadline after unavailable monitoring, using 2, 4, 8, 16 and then 30 seconds. This avoids adding an independent retry timer that could survive stopPolling. The manual callback bypasses the deadline but honors the same in-flight guard. The native status request has a 15-second transport deadline so that guard cannot remain held indefinitely by a stalled response; the separate two-second owner-health preflight remains unchanged. Retry deadlines use monotonic performance time, unaffected by wall-clock corrections. A successful running response clears the failure count and deadline; terminal responses invalidate the session before existing terminal handlers run. Reset unavailable retry-button state after each failed request rather than waiting for another three errors. Keep the persistent unavailable DOM mounted across background failures to preserve focus.

## Verification

- `rtk proxy node --test hifimule-ui/tests/syncPolling.test.mjs` — all targeted timing and lifecycle regressions pass deterministically.
- `rtk npm run build --prefix hifimule-ui` — TypeScript and Vite production build succeed.
- Existing UI Node regression suite — verify related basket and RPC behavior.
- Browser fixture when a browser runtime is available — confirm unavailable/recovered focus and live announcements.


## Verification Results and Review

- Final UI Node suite: 205 tests passed, including 18 focused polling regressions.
- Production TypeScript/Vite build: passed; existing mixed static/dynamic import warnings remain.
- Offline Rust check: passed; existing tray-support dead-code warnings remain.
- Focused UI Rust lifecycle tests: six passed.
- Browser regression fixture: all 21 checks passed in English and French; French was rerun after monotonic timing changes. Verified automatic and manual recovery, keyboard focus, live announcements and stale response handling.
- Diff whitespace check: clean.
- Three independent reviewers covered blind adversarial analysis, edge paths and acceptance criteria. Two actionable review patches were applied: bounded status request duration and monotonic deadlines. Other suggestions concerned pre-existing test-harness conventions or optional additional coverage; no unresolved actionable findings remain.
- The original transient machine failure is still unconfirmed. This change repairs monitoring recovery. It does not install or replace the currently running application.

## Suggested Review Order

**Polling and recovery**

- Keep retries bound to one tracked operation and resume after transient failures.
  [BasketSidebar.ts:1665](../../hifimule-ui/src/components/BasketSidebar.ts#L1665)

- Manual checks bypass backoff while sharing the automatic request guard.
  [BasketSidebar.ts:332](../../hifimule-ui/src/components/BasketSidebar.ts#L332)

- Stopping invalidates callbacks and prevents late responses from updating the sidebar.
  [BasketSidebar.ts:1729](../../hifimule-ui/src/components/BasketSidebar.ts#L1729)

**Transport bound**

- Bound status requests so stalled responses release the retry guard.
  [lib.rs:772](../../hifimule-ui/src-tauri/src/lib.rs#L772)

**Regression coverage**

- Exercise retry timing, monotonic deadlines, session identity and terminal recovery.
  [syncPolling.test.mjs:97](../../hifimule-ui/tests/syncPolling.test.mjs#L97)

- Verify browser focus and live announcements across manual and automatic recovery.
  [sync-status-hardening.html:43](../../hifimule-ui/tests/sync-status-hardening.html#L43)

