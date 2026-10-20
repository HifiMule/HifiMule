# Investigation: Frequent sync monitoring disconnections

## Hand-off Brief

1. **What happened.** User reports frequent unavailable sync status on this Mac; clicking Check state recovers it.
2. **Where the case stands.** Deterministically reproduced the permanent monitoring stop; available machine logs do not establish a daemon crash or the original RPC error.
3. **What is needed next.** Add bounded automatic retries for the same tracked operation and record sanitized failure timing to identify the original transient error.

## Case Info

- Date opened: 2026-10-20
- Status: Concluded; initial failure trigger requires incident evidence
- System: macOS; HifiMule Hub
- Evidence: screenshot supplied by user, source code, version history; machine logs being located

## Problem Statement

User report: I do not know the reason but we frequently get disconnection message on this machine. Clicking on check state reconnect to the daemon.

## Evidence Inventory

| Source | Status | Notes |
| --- | --- | --- |
| User screenshot | Available | Sync status unavailable and Check state action |
| Source | Available | BasketSidebar.ts startPolling and startDaemonStatePolling |
| Version history | Available | Working tree clean at a665e4b |
| Incident timestamp and UI console | Missing | Needed to identify original RPC error |
| Daemon logs | Partial | Being located from application configuration |

## Confirmed Findings

### Polling stops after three errors

Evidence: hifimule-ui/src/components/BasketSidebar.ts:1697-1710. The catch increments consecutiveFailures, stops polling at three, preserves isSyncing and currentOperationId, and displays the warning. The Check state button invokes startPolling again (line 340).

### Background state polling cannot recover this state

Evidence: hifimule-ui/src/components/BasketSidebar.ts:923. The daemon state interval returns while isSyncing is true.

## Hypothesized Paths

### Hypothesis 1: The daemon disconnects frequently

- Status: Open
- Would confirm: matching daemon exits, restarts, or unavailable local RPC responses.
- Would refute: daemon remains healthy while only operation-status requests fail.
- Resolution: User observation alone does not distinguish daemon outage from status-check errors.

### Hypothesis 2: Transient errors leave monitoring permanently unavailable

- Status: Confirmed (source behavior)
- Resolution: Three consecutive request failures stop operation polling; the other polling path skips isSyncing; manual Check state starts polling again. Original transient error source remains unknown.

## Investigation Backlog

| Path | Status | Notes |
| --- | --- | --- |
| UI failure and recovery chain | Done | Permanent polling stop confirmed |
| RPC health validation and timeout | Done | Two-second preflight before every status call |
| Machine incident logs | Done | Logs inspected; original UI error absent |
| Regression verification | Done | Isolated source-method reproduction proves permanent stop and same-operation manual recovery |

## Conclusion

Confidence: High for the persistent monitoring failure; Low for the initial transient failure trigger. Three failures permanently stop the operation polling; background state polling also skips the syncing state. Manual Check state restarts the same status check. No daemon restart is needed for this behavior. The exact original request error remains unobserved.


## Additional Confirmed Findings

### Every operation status call has a two-second health preflight

Evidence: hifimule-ui/src-tauri/src/lib.rs:226, :242, :245, :743; hifimule-lifecycle/src/lib.rs:19. The UI validates the bound daemon owner before sending the status request. A timeout is returned as OWNER_CHANGED even when ownership has not actually changed. A later call can validate the same bound daemon successfully; the error does not itself replace the owner.

### The status handler does not perform device I/O

Evidence: hifimule-daemon/src/rpc.rs:9146; hifimule-daemon/src/sync.rs:1259, :1194, :1223. Operation status reads an in-memory operation under a Tokio RwLock, and sync finalization releases this lock before committing the manifest. This argues against a device-write lock directly blocking the operation-status handler.

### Available logs do not establish daemon crashes

Read-only delegated inspection covered /Users/akartmann/Library/Application Support/HifiMule/ui.log, daemon.log, /tmp/hifimule-daemon-stdout.log and /tmp/hifimule-daemon-stderr.log. No matching panic, RPC shutdown, connection-refused, health-check timeout or OWNER_CHANGED diagnostic was found. Stdout has one runtime/server startup (lines 2–3). Log rotation and absent UI console errors limit this finding.

Device writes of roughly 22–41 seconds and queue depth seven appear at daemon.log:5972, :5984, :6044. This establishes I/O latency but does not establish health-endpoint starvation.

## Deduced Conclusions

The warning means status monitoring failed, rather than proving that synchronization stopped or that the daemon exited. Immediate manual recovery fits a transient request failure followed by a permanent client monitoring stop.

## Additional Hypotheses

### Hypothesis 3: Temporary health-check latency causes the initial failures

- Status: Open
- Supporting indicators: two-second preflight deadline; real device I/O latency.
- Would confirm: original UI failure reports a health-check timeout, correlated with measured health latency.
- Would refute: original failure is another RPC error while health remains responsive.
- Resolution: Not established. Device I/O delay alone is insufficient evidence of executor or health starvation.

## Missing Evidence

| Gap | Impact | Collection |
| --- | --- | --- |
| Original frontend RpcError and timestamp | Distinguishes health timeout, transport, invalid operation and other errors | Persist sanitized method, code, error category and elapsed time |
| Health preflight latency during incident | Tests timeout hypothesis | Record timed health failures; read-only localhost sampling was sandbox denied |
| Sleep/resume correlation | Tests machine suspension contribution | Correlate future incident timestamps with sleep/resume |

## Source Code Trace

| Element | Detail |
| --- | --- |
| Warning origin | BasketSidebar.ts:324 renderSyncUnavailable |
| Failure trigger | BasketSidebar.ts:1697 catch around sync_get_operation_status |
| Permanent stop | BasketSidebar.ts:1700 calls stopPolling after three failures |
| Manual recovery | BasketSidebar.ts:340 starts polling again |
| No background recovery | BasketSidebar.ts:923 skips state checks while isSyncing |
| Possible transient source | lib.rs:242 two-second health validation timeout |

## Reproduction Plan and Results

Executed two isolated Node VM checks against the actual startPolling/stopPolling method source, with RPC and timer dependencies stubbed:

1. Fail three consecutive RPCs. Observed three requests, syncMonitoringUnavailable=true, isSyncing=true, operation ID preserved, and no recovery interval scheduled.
2. Make the same daemon/status stub healthy. No request occurs until startPolling is called as the Check state button does. One successful request clears unavailable state and restores the same operation.

Both checks passed. These reproduce the client recovery defect; they do not reproduce the original machine transport error. Existing browser fixture hifimule-ui/tests/sync-status-hardening.html:48, :56, :72 covers manual recovery, overlapping requests and stale-response suppression, but not automatic recovery.

## Recommended Next Steps

### Fix direction

Use bmad-quick-dev to retain safe unavailable-state controls while automatically checking the same operation with capped backoff. Keep checks single-flight; let Check state request an earlier retry without creating a second in-flight request. Cancel retries on destruction or operation changes and preserve completion, failure and cancellation reconciliation. Never interpret missing status as transfer completion or start a replacement sync.

### Diagnostic

Record sanitized failure category/code and elapsed timing, without RPC parameters, tokens or media names. Preserve the daemon owner identity checks. Avoid treating a larger timeout as a proven fix before measuring the original failure.

## Timeline

- 2026-10-20: User reports recurring warning and successful manual Check state recovery.
- 2026-10-20: Source inspection confirms monitoring stops permanently after three failures; source-method checks reproduce it.
- 2026-10-20: Local log inspection finds continued transfers but no initial UI error or matching daemon-crash evidence.

## Implementation Status

No application code changed. This case concludes the investigation and records the concrete fix direction and remaining evidence gap.


## Follow-up: 2026-10-20

The user approved automatic recovery with backoff. Implemented capped retries for the same operation, shared single-flight manual checks, monotonic retry timing, and a bounded native status-request duration. The existing health timeout and daemon ownership validation are preserved. Verification passed: 205 UI Node tests, production build, Rust check, six lifecycle tests and browser focus/announcement checks. See [implementation and review](../spec-sync-monitoring-auto-recovery.md). The original transient request failure remains unconfirmed; application installation was outside this change.
