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


## Follow-up: 2026-10-20 #2

### New Evidence

User reports the issue persists and asks whether increased pipeline capacity exceeds the daemon thread count. Reopened investigation; no application changes requested at this stage.

- Machine reports 18 logical CPUs. Configuration currently overrides staging_max_tracks=8, staging_max_mib=2048, checkpoint_track_updates=1000, checkpoint_interval_seconds=600.
- Read-only process inspection finds an installed daemon with roughly 82 minutes uptime and a recently launched development UI. The recovery fix is being exercised by a newer UI against the existing daemon.
- Sync continues to enqueue/dequeue files; queue depth often reaches seven. Logged device writes range from a few seconds to over 25 seconds in the current window.
- Source anchor: staging capacity changed from two to eight in commit85f82dc. Thread/pipeline execution relationship is being traced independently.

### Hypothesis 4: Increased staging causes daemon runtime starvation

- Status: Open
- Would confirm: blocking work monopolizes the runtime serving health/status during sync, with matching measured local RPC delays.
- Would refute: a larger queue only changes buffering, while all work yields and health/status remain responsive during the incident.
- Investigation plan: trace the runtime serving manually initiated sync, inspect blocking calls, and measure read-only health/status latency without changing sync settings.


### Confirmed Architecture and Queue Semantics

- Core Tokio workers are capped at min(available CPUs, 4): hifimule-daemon/src/main.rs:21, :369, :775. This machine reports 18 logical CPUs.
- RPC uses a separate OS thread and current-thread Tokio runtime: main.rs:470-478. Manual sync execution uses tokio::spawn within the RPC handler (rpc.rs:8942, :9046), and its producer tasks inherit that runtime (sync.rs:3406).
- Commit 85f82dc changed staging_max_tracks from two to eight. It bounds staged files via a semaphore and ready queue (sync.rs:3371-3372). For one Jellyfin source, this remains one sequential producer plus one sequential writer; permits include staging, queued and actively written files (sync.rs:3867, :4113, :4121, :4204-4219). Eight slots do not require eight worker threads.
- The per-track writer calls device_io.free_space().await (sync.rs:4380). MscBackend::free_space directly executes synchronous get_storage_info_free_bytes (device_io.rs:590-591), which reaches macOS libc::statvfs (device/mod.rs:2971-2977, :3017). The async signature does not offload that syscall.
- Additional synchronous RPC-runtime work includes calculate_delta (rpc.rs:8383), per-add manifest scans (sync.rs:5383), SQLite/JSON plan pages (sync_plan.rs:145-198), and per-track logging filesystem operations (main.rs:29-46, :67-72).
- Bulk file transfer itself uses awaited tokio::fs operations (device_io.rs:455-500); tagging and manifest compression are already offloaded with spawn_blocking. Slow copy duration alone is not proof of RPC starvation.

### Live Measurements

Read-only authenticated sampling used credentials only inside the probe process; no token or RPC payloads were recorded. Sandbox-denied attempts and a corrected probe parsing error were excluded from final measurements.

- Time window: 2026-10-20 09:53:53–09:54:03, machine local time (+0200 in process sample).
- Active operation present during the probe.
- daemon.health: 20 successful probes, median 1.04 ms, maximum 2.00 ms, zero errors.
- sync_get_operation_status: 20 successful probes, median 0.62 ms, maximum 1.09 ms, zero errors.
- Compact get_daemon_state: one successful probe, 2.66 ms.
- Sanitized timing artifact: /tmp/hifimule-rpc-latency-20261020.jsonl.
- Five-second OS thread sample: /tmp/hifimule-daemon-threads-20261020.txt, PID 84002, 2026-10-20 09:53:38 +0200.

### Thread Sample Findings

The sample confirms a separate single-thread RPC runtime with manual sync frames (sample lines 374, 378, 380, 384, 621-629). Its I/O reactor was idle for 3505/3636 samples, roughly 96.4 percent. Three core workers were parked throughout; the fourth was mostly parked, with some filesystem statfs work (lines 70-135). No core worker exhaustion was observed.

Filesystem metadata, sync_all and canonicalization waits were visible on blocking-pool threads (lines 680-769), while other blocking threads remained available. The sample also directly observed synchronous profile metadata/open and SQLite configuration work on the RPC thread (lines 462-480), but only in isolated samples, not a long stall.

### Updated Hypothesis 4

Status: Open for indirect influence; the direct eight-slots-versus-four-threads explanation is refuted by queue and runtime structure. Increased buffering may amplify I/O pressure, but it does not create eight writers. The measured window showed neither health timeouts nor worker saturation.

### Hypothesis 5: Synchronous Device/Filesystem Work Stalls the Single RPC Thread

- Status: Open for this incident; reachable blocking call chain confirmed.
- Deduced mechanism: a slow synchronous statvfs, database operation or file open on this thread prevents all health/status processing until it returns, even while core workers are idle.
- Would confirm: concurrent external health timeout and OS sample showing the RPC thread blocked inside that call chain.
- Would refute attribution for a specific incident: health stays fast during the warning, or incident samples locate the delay outside the daemon.

### Updated Conclusion and Fix Direction

Confidence: High on runtime/queue architecture and the blocking-call risk; Low on attribution to the reported recurring timeout. Raising the core worker cap alone does not protect the separate single-thread RPC loop. The highest-value daemon hardening is to offload blocking MSC free-space queries and isolate heavy planning/sync work from the RPC event loop. Keep the two-second health timeout and owner checks unchanged until latency evidence justifies changing them.

Next incident evidence should pair external health timings with a short OS thread sample during the actual warning. A deterministic regression can place a controlled blocking free-space operation on a current-thread runtime and probe health externally, then verify health remains responsive after the blocking operation is offloaded. Any settings comparison of staging depth two versus eight should use a matched workload and explicit authorization to change active sync settings.

Investigation status: Concluded with a concrete architectural risk and an explicit incident-evidence gap. No application code or active settings changed in this follow-up.


## Follow-up: 2026-10-20 #3

The user approved the daemon isolation fix. Heavy sync planning/estimation/auto-fill/execution handlers and their spawned descendants now use the existing core runtime. Authenticated health and lightweight controls remain on the RPC runtime. Native MSC free-space queries run on the blocking pool. Forwarding owns shutdown admission even for heavy reads; cancellation, panic and closed-core paths preserve guard cleanup and response IDs. Existing task-owned transfer pipeline guards and teardown order remain intact.

Verification: 1,605 daemon tests passed, 12 ignored, zero failures; controlled offline check/debug build and whitespace check passed. Deterministic tests use actual authenticated HTTP requests and held synchronous work, including shutdown drain and precommit-fence cases. Three independent reviews completed with two patches and no remaining actionable issues. See [implementation and review](../spec-isolate-sync-from-rpc-runtime.md).

The historical timeout cause remains unconfirmed. The installed daemon and active sync were not replaced or restarted; the rebuilt local daemon must be deployed/relaunched before live observations can validate the change.
