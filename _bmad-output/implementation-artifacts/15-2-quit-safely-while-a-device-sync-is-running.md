---
baseline_commit: a7e887e2c8d7d7b7c9af5f39417c011a752a49d6
---

# Story 15.2: Quit safely while a device sync is running

Status: done

## Story

As a HifiMule user,
I want explicit Quit to stop background work in an orderly way,
so that HifiMule exits without leaving my device falsely marked as successfully synchronized.

**Requirements:** Shutdown foundation for FR60 and FR75; P-AR2; P-NFR3–4; inherited managed-device integrity and interrupted-sync recovery requirements.

**Dependency:** Story 15.1, completed in `e863368` (review) after `23eeae2` (implementation). This story replaces its interim active-sync Quit refusal. It uses current sync operations; audio stop and playback checkpoint participation are documented below for Stories 15.3–15.4. Do not implement playback or add playback persistence tables.

## Acceptance Criteria

1. **Idle Quit and window independence.** Given an idle daemon, when the user selects Quit HifiMule, then new work is rejected, ownership resources are released and the daemon exits. Closing only the UI continues to leave the daemon running.
2. **Cancel and drain active work.** Given one or more active device sync operations, when Quit is selected, then cancellation is requested for every active operation and new sync work is prevented. The UI or remaining desktop status surface reports that shutdown is waiting for device writes to finish safely. Preparation before an operation ID exists is included.
3. **Preserve write integrity.** Given cancellation during transfer, conversion or managed metadata writing, when work stops, then incomplete work is not recorded as completed; atomic manifest and managed-file integrity guarantees remain intact, and unmanaged files are not altered.
4. **One shutdown operation.** Given repeated Quit or UI reopening during shutdown, when those requests arrive, then they observe the same shutdown operation without another daemon or new work. A reopened UI displays authoritative shutdown state.
5. **Independent failure cleanup.** Given device disconnection or an operation failure during shutdown, when cleanup proceeds, then affected work retains an interrupted or failed outcome with recoverable state. Other operations continue cleanup, and the failed operation is not reported successful.
6. **Explicit deadline outcome.** Given an operation has not reached a safe cancellation point within the documented deadline, when that deadline expires, then HifiMule exposes the waiting/failure state defined below. It does not silently force termination, discard recovery state or claim a clean exit. Only work explicitly classified as safely abandonable may be abandoned.
7. **Recovery after relaunch.** Given shutdown interrupted a sync, when HifiMule relaunches and the device reconnects, then the existing reconciliation/repair path identifies incomplete work. Successful daemon shutdown alone never establishes that the device is fully synchronized.
8. **Cross-platform evidence.** Given Windows, macOS and Linux builds, when idle Quit, active-transfer cancellation, metadata-write cancellation, repeated Quit, disconnect and stalled-operation scenarios run, then tests verify exit or explicit unresolved status, ownership cleanup on exit, and persisted device integrity. Injected failures complement a real-device smoke check; they are not equivalent evidence. Record tested platform and architecture.

## Tasks / Subtasks

- [x] Implement the shutdown coordinator and admission contract below (AC 1, 2, 4, 6).
  - [x] Replace `CheckIdle`/`try_begin_idle_shutdown` refusal with one serialized request, durable launch fencing, cancellation and drain phases. Keep tray/event-loop processing nonblocking.
  - [x] Track preparation, operation workers, admitted mutations and outstanding device work through completion; retain guards through spawned-task lifetimes.
  - [x] Make operation creation inherit committed cancellation; prove start/Quit and prepare/execute races cannot reset cancellation or admit new work.
  - [x] Keep health responsive while blocked, preserve the five-second warning deadline and release ownership only after actual teardown.
- [x] Make all production sync paths stop safely (AC 2, 3, 5, 7).
  - [x] Apply cancellation to provider preparation, single/multi-provider execution, auto-sync, staged producers, writer, deletion and playlist/manifest finalization.
  - [x] Centralize or otherwise enforce identical outcome precedence in RPC and auto-sync finalizers: failed/interrupted never becomes Complete; manifest persistence failure is an operation failure.
  - [x] Preserve dirty/pending/verification evidence for interrupted or failed work, including cancellation plus file errors and cancellation during final metadata commit.
  - [x] Serialize authoritative manifest persistence and atomically replace the local MTP recovery cache; propagate path/write failures and prevent stale snapshots from overwriting newer dirty state.
  - [x] Continue removal/failure bookkeeping during shutdown without admitting new device initialization, auto-sync or scrobbler work.
- [x] Expose authoritative shutdown through existing desktop surfaces (AC 1, 4, 6).
  - [x] Extend authenticated health with the bounded snapshot below; bridge it through native Tauri without exposing credentials.
  - [x] Render waiting, delayed and precommit-persistence-failure states in tray and UI; reopening uses the same owner and does not hydrate mutating recovery endpoints.
  - [x] Add accessible status text and working refresh/close actions, with catalog parity in English, French, Spanish and German.
- [x] Verify safe boundaries and regressions (AC 1–7).
  - [x] Add deterministic race/failure tests using barriers and injectable device I/O; inspect persisted bytes and outcomes, not just cancellation flags.
  - [x] Exercise producer/writer cancellation, late finalization, disconnect, multiple registered operations, blocked mutation, launch fencing and read-only shutdown observation.
  - [x] Run relevant lifecycle/daemon/native UI tests, frontend build and focused UI behavior tests; record actual results.
- [x] Extend installed smoke coverage and record platform evidence (AC 8).
  - [x] Exercise real tray Quit and UI reopen against built production binaries, including a real-device transfer and interrupted recovery.
  - [x] Capture artifact hash/version, OS/architecture, owner PID/instance, shutdown identity, elapsed time, operation outcome, integrity and ownership cleanup. Mark unavailable checks unverified.

### Review Findings

Review date: 2026-09-12. Reviewed `a7e887e..aefe9f2` with Blind Hunter, Edge Case Hunter and Acceptance Auditor. Triage: 0 decisions, 8 patches, 1 deferred, 1 dismissed. The initial diagnostic pass did not modify production files; the eight patches below were subsequently approved and applied.

- [x] [Review][Patch] F1 / P1 — Bind manifest updates to the original operation device [hifimule-daemon/src/sync.rs:2124]

  Sources: blind + edge. The caller has already captured device I/O, but `execute_provider_sync` reads the selected device again after awaiting `begin_sync_job`. Selecting B while A's job starts can make the new per-file commits update B's manifest while the writer still writes to A. The RPC/auto-sync paths also re-read selection between initial dirty marking and execution. Carry one captured device identity, manifest and I/O through preparation, execution and finalization; do not infer the operation target from later UI selection. Violates AC3 and the explicit device-selection/persistence constraint. Validate with two devices and a barrier in job startup.

- [x] [Review][Patch] F2 / P1 — Make failure precedence atomic with clean finalization [hifimule-daemon/src/rpc.rs:5429]

  Sources: edge + auditor. All three finalizers clear dirty/pending based on cancellation and local errors before checking an independently recorded Failed status. They then sample `already_failed`, fetch another operation snapshot, and overwrite its status using the earlier sample. Device-removal bookkeeping can record failure between those steps, after which the finalizer can publish Complete and a success notification. Serialize failure recording with the clean-commit decision and preserve current Failed state atomically when publishing the terminal outcome; apply the same rule to both RPC paths and auto-sync (`main.rs:1282`). Violates AC3/5. Validate disconnect/failure before clean commit and between status observation and terminal publication.

- [x] [Review][Patch] F3 / P2 — Route MTP cache-path failures through manifest rollback [hifimule-daemon/src/device/mod.rs:938]

  Sources: blind + edge. `get_local_mtp_manifest_path(...)?` returns directly after mutating the in-memory manifest, bypassing the new `persist_result` rollback branch. Failure to create/resolve the cache directory can therefore leave an unpersisted clean manifest in memory even though finalization reports failure. Resolve the path before mutation or include path resolution in the captured persistence result. Violates AC3/7 and the authoritative-cache failure constraint. Validate path-resolution failure, not only failure of the eventual file write.

- [x] [Review][Patch] F4 / P2 — Durably commit the authoritative MTP cache rename [hifimule-daemon/src/device/mod.rs:446]

  Source: blind. The new atomic helper syncs the temporary file and renames it, but never syncs the parent directory. On filesystems requiring directory synchronization, a host crash can lose the directory update after a dirty commit reported success and device transfers began, leaving the prior clean recovery cache. Finish the durability boundary with supported-platform directory synchronization and explicit error handling, as the lifecycle atomic writer already does. Violates the durable dirty-state and authoritative-cache persistence constraints. Keep this distinct from the optional on-device MTP mirror.

- [x] [Review][Patch] F5 / P2 — Recover the UI after a failed launch fence and expose Retry Quit [hifimule-ui/src/main.ts:96]

  Sources: blind + edge. `observeShutdown` replaces the application whenever any shutdown snapshot exists, including retained `fenceFailed` snapshots after admission reopens, then disposes itself. The replacement screen offers only Refresh and Close and never restores the usable application or provides Retry Quit. Reopening the UI reaches the same trap. Handle precommit failure separately, provide the specified retry action, and restore normal operation when appropriate. Violates the precommit-failure contract. Validate initial failure, failure after the waiting view appears, and reopening after failure.

- [x] [Review][Patch] F6 / P2 — Maintain one shutdown polling schedule across Refresh clicks [hifimule-ui/src/main.ts:175]

  Sources: blind + edge + auditor. Refresh calls `poll`, which always schedules another recurring timer without cancelling the existing schedule. The in-flight gate prevents overlap but does not prevent multiple permanent timer chains. A reproduction using the actual transpiled renderer and fake timers showed two clicks leave three timers, then three requests in one scheduled second. Keep a single scheduler, bound refresh frequency, and cancel the full schedule on disposal. Violates the at-most-once-per-second observation contract.

- [x] [Review][Patch] F7 / P2 — Show tray waiting and deadline states while launch fencing is pending [hifimule-daemon/src/main.rs:732]

  Source: auditor. The fence worker starts without a waiting tray state; `shutdown_pending` and `shutdown_started` are set only after persistence succeeds. A blocked generation write therefore never triggers the tray's five-second warning, and ordinary Idle/Syncing messages continue to overwrite its status. Track the precommit warning from the initial request and give it tray precedence without cancelling work, reopening admission or retrying a pending write. Violates AC6 and the separate fencing deadline contract.

- [x] [Review][Patch] F8 / P2 — Bind shutdown observation to its owner and the native health deadline [hifimule-ui/src/main.ts:169]

  Source: auditor. The new shutdown observer uses the generic `rpc_proxy`, which re-reads discovery on each call, skips owner validation for `daemon.health`, uses a 15-second timeout, and applies epoch checks only to `get_daemon_state` (`src-tauri/src/lib.rs:524–598`). The renderer accepts the response without checking its original instance/shutdown identity. This new observation path can follow a replacement owner or apply stale responses and can remain pending beyond the five-second warning deadline. Add a narrowly scoped observation path bound to the originally observed descriptor/epoch, validate health identity, and use `HEALTH_TIMEOUT` (two seconds). Violates AC4 and the shutdown-observation contract; no owner launch/election should occur during refresh.

- [x] [Review][Defer] F9 / P2 — Manual sync cancellation can race final clean-manifest persistence [hifimule-daemon/src/sync.rs:863] — deferred, pre-existing

  Source: edge. Manual `request_cancel` only stores a token and does not participate in the finalization gate, so a cancellation accepted after the finalizer's token check can still end in a clean manifest and Complete outcome. This manual-cancel check/write race predates Story 15.2; the new gate serializes committed Quit only. Record it separately from this story's shutdown patches.

Initial review verification: the eight focused daemon shutdown tests passed with host access; the initial restricted run passed six and failed two because local TCP listener binding returned `PermissionDenied`. The four frontend lifecycle/shutdown helper tests passed. The renderer-level timer reproduction exposed F6 despite those passing helper tests. No full suite or physical-device smoke was rerun during the initial diagnostic pass; existing user-confirmed device evidence was accepted as recorded. One proposed pre-transfer early-return resource leak was dismissed as an unproven safety defect after inspecting the Windows shell worker's RAII shutdown/join.

### Review Patch Completion — 2026-09-12

All eight approved patches are applied. F9 remains deferred and its manual-cancel semantics are unchanged.

- F1: RPC execution captures path, manifest and I/O together; auto-sync resolves the triggering device identity. `SyncTarget` carries that destination through the worker, and preparation existence/capacity reads and playlist updates use the same identity.
- F2: `finalize_operation` now owns the failure/cancellation/clean-commit/terminal-publication boundary for both RPC paths and auto-sync. Stale operation snapshots cannot replace an existing terminal outcome. Failure errors remain available; success-only history and notifications follow the resulting status.
- F3–F4: cache-path errors reach rollback; Unix parent-directory synchronization follows rename. Post-rename uncertainty is reported and dirty recovery bytes are restored best-effort. The optional MTP mirror is written only after the authoritative cache succeeds, so a failed authority cannot publish a clean mirror. Windows retains atomic replacement and file synchronization; directory synchronization uses the supported Unix path.
- F5–F6: failed fencing offers authenticated Retry Quit and Continue using HifiMule. Continuing reloads only the webview and dismisses that failed attempt. The shutdown view disposes the active sidebar; one rate-limited poll scheduler handles Refresh and disposal.
- F7: tray status comes from the coordinator snapshot throughout fencing, including the separate five-second warning, and takes precedence over ordinary state updates. A retry is accepted only after the previous fence has failed; repeated queued requests are consumed once.
- F8: native RPC observation uses the startup-verified owner, rejects stale epochs, validates health identity, and bounds health requests with the two-second lifecycle timeout. Refresh never elects or adopts a replacement owner.

Post-patch verification on macOS arm64: daemon **657/657**, lifecycle **13/13**, native UI **6/6**, frontend lifecycle/shutdown behavior **8/8**, and smoke-evidence tests **3/3** passed. Frontend production build and formatting passed; catalog parity verified **416 keys across four locales**. Targeted all-target clippy completed without errors; pre-existing warnings remain. Regression coverage inspects actual device bytes/manifest isolation during a selection change at a job-start barrier, dirty recovery after finalization/cache failures, authenticated retry admission, delayed non-cancelling fencing, stale terminal updates, real rendered recovery actions, timer multiplication and disposal. A bounded follow-up review found no new defects in F1/F2/F7 integration.

The final automated runs required host access for local HTTP fixtures and macOS APIs. Physical-device/installed-artifact smoke was not rerun for these review patches; earlier user-confirmed evidence remains recorded in the Dev Agent Record against its original artifacts. No git commit or release artifact was produced.

## Dev Notes

### Selected shutdown contract — implementation gate resolved

This is the selected contract for this story, not a description of behavior already implemented. Reuse the native lifecycle owner, existing RPC boundary and `SyncOperationManager`; a narrow daemon `shutdown.rs` module is appropriate if it separates coordination from `main.rs`. Do not build another application framework or event loop.

**States and ownership**

| Phase | Required behavior |
| --- | --- |
| `running` | Normal admission. UI closure has no effect on daemon work. |
| `fencing` | One Quit request temporarily closes admission and persists the existing monotonic launch-generation advance. Already-admitted work may finish normally. Do not cancel it before the fence is durably committed. |
| `cancelling` | Fence succeeded: publish `stopping`, retain one shutdown ID, signal preparation and all active operations. No return to running after this point. |
| `draining` | Await device-safe completion, admitted mutation/task guards and cleanup. A terminal operation label alone does not prove its worker or native I/O has returned. |
| `waiting` | Five-second deadline expired with unfinished work. Publish `SHUTDOWN_TIMEOUT` plus blockers; admission remains closed and cleanup continues. |
| `finalizing` | All work capable of changing device/local recoverable state has drained. Stop observers, finish/join their blocking work, drain RPC and join it, then release discovery/ownership and exit. Deadline overrun remains visible until exit. |

An unsuccessful generation persistence attempt returns to `running` only before cancellation/teardown starts, with `QUIT_PERSISTENCE_FAILED` shown in tray/UI and a real Retry Quit action. Keep ownership and work intact. Repeated Quit during fencing attaches to the pending attempt; after commit it returns the same shutdown ID without incrementing generation again, resetting the deadline, or restarting cancellation. Use existing generation-file durability and lock protections.

Admission and operation registration must be serialized sufficiently to close the check/start race. Do not hold an admission/coordinator mutex across provider/device I/O, channel waits, joins or generation persistence. Reuse short critical sections, the existing pipeline guard and mutation guards with explicit ownership; a separate per-device commit serializer may span that device's persistence without locking global admission or observation. The current product admits a single global pipeline; do not introduce multi-device sync concurrency. Still cancel every registered active operation, and test that one failing operation does not prevent another from draining.

**Cancellation owner and ordering**

1. The daemon coordinator owns committed shutdown; UI/tray requests do not own worker lifetime.
2. Close admission for all new mutations/background starts and commit the generation fence.
3. Request cancellation of the active preparation pipeline and each registered running operation. An operation registered by already-admitted preparation must inherit cancellation rather than create a fresh false token. No queued producer or late provider result may restart the pipeline.
4. Await acknowledged work completion and safe finalization. Keep the core runtime alive for async cleanup; dropping it immediately would abandon futures required to restore metadata consistency.
5. After safe work drains, perform final runtime/RPC teardown and release `OwnerGuard`. Preserve the review fix that joins blocking work before owner release.

Later Stories 15.3–15.4 insert audio stop acknowledgement and final session checkpoint after fencing and before the sync-cancellation phase, following architecture's audio-stop → checkpoint → sync-cancel order. A checkpoint error must be recorded and surfaced, must not claim preserved playback, and must not prevent cancellation of device work. The same `waiting`/failure machinery retains the owner when required persistence remains unresolved and permits an explicit retry of the failed checkpoint. Document this extension boundary; add no fake player, playback tables or no-op framework solely for later use.

**Deadline and abandonment policy**

- Retain **5,000 ms from committed Quit** as the total orderly-shutdown warning deadline, covering cancellation, drain and teardown. Use a monotonic clock; tests inject time or use controlled short deadlines. Do not reset it per file, device, phase or repeated Quit.
- Fencing has a separate **5,000 ms warning from the initial request**. Publish `phase: "fencing"` immediately with matching elapsed/deadline values and show delayed launch-fence persistence if it exceeds that bound. Run persistence off the native event loop. While a write is still pending, retain admission/ownership and do not retry or roll back: it may still commit. Only a completed failed fence permits Retry Quit. On successful commit, start the cancellation/drain deadline once; preserve the shutdown ID. This distinguishes precommit waiting from committed cancellation.
- Surface waiting immediately and `SHUTDOWN_TIMEOUT` at deadline. Continue waiting without a forced-exit deadline for device writes, flush/verify/rename, manifest/playlist writes, native MTP/WPD work and admitted mutations with uncertain side effects. Never recommend unplugging a device while such a write remains pending.
- Pure provider reads, unstarted work and asynchronous host-temp-only preparation can stop at known cancellation-safe boundaries. Release staging leases and join workers before deleting files they still own. Cancellation of a future is not proof that underlying blocking work stopped.
- Blocking native calls and unknown-side-effect operations are not abandonable. A failed operation may stop blocking exit only after its worker returned and recoverable state is retained. Continue cleaning other operations while one stalls.
- Do not use `process::exit`, task abort of a device writer, runtime `shutdown_timeout`, or dropping a JoinHandle as evidence of safe completion. Read-only UI status refresh and tray status remain available; no Force Quit action is introduced.
- Health transport continues using the existing bounded native lifecycle timeout. UI shutdown polling is at most once per second, one request at a time, and does not launch a replacement owner. A connection failure is an unknown/unreachable state, not proof of clean shutdown; after confirmed exit, show that the daemon stopped and require a later deliberate launch to restart.

### Authoritative status and UI behavior

Extend **`daemon.health` → `result.data`** additively, retaining `status: "ok" | "stopping"`, protocolVersion, instanceId, pid, daemonVersion and errorCode. Add nullable `shutdown` with `schemaVersion: 1`, `shutdownId` (UUID for the one attempt), `phase`, `elapsedMs`, `deadlineMs: 5000`, `deadlineExceeded`, `activeOperationCount`, `pendingMutationCount`, `blockers` and `blockersTruncated`. Blockers use stable operation/device identifiers where known plus a typed reason (`preparing`, `transfer`, `metadata`, `nativeIo`, `mutation`, `teardown`); preparation can have a null operation ID. Bound the list to 32 entries and include total counts. Error text is sanitized/localized by code and contains no credentials or authenticated URLs.

Snapshot reads must not acquire device/provider locks or perform recovery I/O; publish a small coordinator-owned snapshot accessible to the separate RPC runtime. Preserve authentication on every request. Keep existing stopping rejection for unrelated RPCs. Health is sufficient here: do not reopen broad mutating endpoints just to render progress. This is a lifecycle status extension, not the future playback queue/event schema; retain protocol compatibility for additive fields and test old-field parsing.

Native `LifecycleState::Stopping` currently becomes a generic error before normal proxying. Extend that path narrowly to deliver verified health shutdown detail even when ordinary application RPCs are unavailable. Both newly opened and already open windows need a visible authoritative stopping state. Disable new-work controls, suspend ordinary hydration/mutation attempts and suppress automatic launch/retry behavior. Closing the window remains allowed. An explicit Refresh status checks the same owner; it does not relaunch. Preserve keyboard access, focus and a polite live status region; deadline/failure text must be visible without relying on tooltip color alone.

`get_sidecar_status` currently caches startup Ready rather than observing later daemon transitions; adding fields to that cache alone is insufficient. Add live observation with owner identity and stale-epoch protection. `rpc.ts` currently flattens structured failures into `Error(message)`; retain lifecycle code/detail if using that path, while preserving numeric `-8` browse reauthentication. Replace the obsolete `lifecycle.quit_blocked_active_sync` manual-cancel wording in every locale.

Tray waiting/timeout status takes precedence over late `Idle`, `Syncing` or device-state messages. Retain Open UI so users can inspect a stalled shutdown. Repeated Quit observes current state. Use wording such as “Quitting HifiMule — waiting for device writes to finish safely” and “Shutdown is taking longer than expected. HifiMule is still waiting for device writes.” Do not emit a sync-complete/safe-to-eject notification merely because cancellation drained.

### Sync integrity and finalization requirements

Use existing `DeviceIO`, provider pipeline and manifest machinery. Preserve per-server producer fairness, byte/file backpressure, temp-only disk staging, managed-zone boundaries, transcoding profiles and provider identities.

Conversion currently uses provider HTTP representations; no local FFmpeg process needs shutdown handling here. Preserve cancellation-aware producer priority barriers and cancel peer producers on failure before joining all of them. Compatibility skips remain warnings and count as handled progress; do not redefine all `files_completed` counters as durable sync records. Assert durable completion separately through verified files and manifest entries.

| Boundary | Shutdown requirement |
| --- | --- |
| Preparation/delta, before operation ID | Cancel the pipeline and prevent transition to execution. No global dirty marker is needed if no device changes began; preserve existing evidence. |
| Download/server conversion/host staging | Stop new producers and cancel safe fetch work; release buffers and leases only after their owners stop. Never count incomplete staged content as transferred. |
| Active managed file write | Finish the current safe write/flush/verification boundary, or preserve existing incomplete-write evidence on error. No next file begins after cancellation. |
| Managed deletion/replacement | Keep deletion idempotent and scoped to managed paths; preserve verified replacement ordering and reconciliation data. Cancellation cannot delete unmanaged content. |
| Playlist or manifest write | Complete the atomic write or report failure with old/new recoverable evidence. Serialize cancellation versus the final clean-commit decision; do not clear dirty/pending for cancelled/failed work. |
| Disconnect/failure | Preserve failed outcome and evidence; never let a late successful callback overwrite failure. A disconnected device's manifest may be unwritable: preserve existing dirty state rather than promise an impossible repair write. |

Outcome precedence: an existing failure or any write/verification/manifest failure remains `Failed`; otherwise interrupted work is `Cancelled`; only fully verified, durably finalized work is `Complete`. A legitimately completed operation before Quit stays complete. Once cancellation wins the finalization race, successful completion of the current file cannot promote the whole interrupted sync to Complete. Already verified items may remain represented accurately, but pending work, dirty markers and interruption evidence must survive. Apply success-gating to history/cursor updates and completion notifications as well as the operation enum.

Current finalizers in RPC single-provider, RPC multi-provider and daemon auto-sync skip clean-manifest updates only for **cancellation with zero errors**. Cancellation plus errors can clear dirty/pending; clean-manifest failure can be merely logged before Complete. Fix these paths together. Add a final cancellation/failure check integrated with the clean-commit boundary; a loose check followed by an awaited write is not sufficient to close the race.

`DeviceManager::update_manifest` currently mutates memory, releases its state lock and then persists a snapshot. Concurrent snapshots can reach disk out of order. For MTP, the on-device copy is intentionally best effort, but the authoritative local cache uses a plain write and silently ignores path-resolution failure. Introduce per-device serialized persistence without holding the global device-state lock over I/O; use atomic local replacement with flush and propagated failures. Keep backend-specific authority: a failed optional MTP mirror is distinct from failed authoritative cache persistence. Ensure dirty evidence is durably established before managed changes and that a failed commit cannot leave in-memory state falsely clean. Bind updates to the operation's device identity rather than a changed UI selection.

Verify cache-key continuity end-to-end: `update_manifest` writes using `manifest.device_id`, whereas `emit_mtp_probe_event` reads using the backend `dev_id`. Cover differing identities in reconnect tests and preserve existing cache evidence when reconciling their mapping; never silently fall back to an older clean device copy when a matching authoritative dirty cache exists. `handle_device_removed` may select another connected device, so draining the removed device must not update the newly selected device's manifest.

Drain every producer even after a join error/panic; the current `?` in the producer-join loop can skip remaining joins and `end_sync_job`. Run required job cleanup on every exit path and classify `end_sync_job` failure as unresolved/failed where device safety is uncertain. Per-file manifest errors currently only warn while counters already advanced: propagate those errors and distinguish physical transfer from durably recorded completion. Do not begin optional new playlist/empty-folder work after cancellation; finish only required in-flight integrity/recovery cleanup.

`handle_sync_execute` registers a Running operation before initial dirty-manifest persistence and multi-provider resolution. Every subsequent early return must terminalize that registration with its failure diagnostic and release guards; otherwise shutdown can wait forever for a worker that was never spawned. Cover dirty-write failure and provider-resolution failure explicitly.

Do not call `sync_get_resume_state` to hydrate shutdown/reopen UI: it performs cleanup of dirty/temp evidence. Recovery remains an explicit existing reconciliation operation after relaunch and device reconnection, when normal admission has resumed.

### Source tree change and preservation map

| File | Current behavior → required change; preserve |
| --- | --- |
| `hifimule-daemon/src/main.rs` | Owns tray, `CoreCommand`, separate core/RPC runtimes, idle Quit and auto-sync. Replace active-sync refusal with coordination; defer runtime drop until cleanup; fix auto-sync finalization. Preserve one native event loop, generation fencing, completed-channel/owner-release order and detached UI launch. Device events currently reject all events when admission closes: allow shutdown-safe Removed/failure bookkeeping without admitting new work. |
| `hifimule-daemon/src/sync.rs` | Owns PipelineGuard, MutationGuard, operation cancellation flags and producer/writer execution. Add committed cancel/drain semantics and registration-race protection; preserve global pipeline admission, staged-file ownership, fair scheduling, write verification and normal user Stop behavior. Track task completion separately from status. |
| `hifimule-daemon/src/rpc.rs` | Health uses lifecycle atomics; every non-health method is rejected while stopping; request guards drain server work. Extend bounded health snapshot and correct single/multi-provider finalizers. Preserve authentication, image proxy protections, mutation coverage, JSON envelopes and provider-specific error behavior. |
| `hifimule-daemon/src/device/mod.rs` | Manifest mutation precedes unsynchronized persistence; MTP local cache is authoritative but non-atomic. Serialize per-device snapshot commits and use atomic authoritative persistence with surfaced errors; preserve best-effort MTP mirror semantics, multi-device selection, managed identity and existing recovery entry points. |
| `hifimule-lifecycle/src/lib.rs` | Shared secure owner/discovery, launch tickets, generation fence, health identity validation and stopping classification. Add typed optional shutdown health detail as needed; preserve private native token, constant-time auth, ACL/path checks, compatible protocol and no takeover of a live/stopping owner. |
| `hifimule-ui/src-tauri/src/lib.rs` | Native launch/attach and authenticated proxy reject stopping owners. Add a bounded read-only shutdown-status path without running start-daemon retries. Preserve owner-generation capture, detached launch, splash timing, token secrecy, no-proxy/no-redirect health and close-only UI lifetime. |
| `hifimule-ui/src/main.ts`, `src/rpc.ts`, `src/lifecycleDeadline.ts`, `splashscreen.html` | Lifecycle hydration and error presentation lack detailed shutdown observation. Display verified waiting/timeout state in existing and reopened UI; preserve bounded startup/hydration, first-run distinction, current selection and stale-attempt protection. Extend deadline helper only if needed; never tie shutdown polling to an implicit launch. |
| `hifimule-i18n/catalog.json` | Shared Rust/TypeScript catalog: add matching shutdown keys/placeholders in all four locales through existing utilities. |
| `hifimule-daemon/src/tests.rs`, co-located lifecycle/RPC/sync tests, `hifimule-lifecycle/tests/contract.rs` | Existing operation fixtures, ownership subprocess tests and teardown regression tests are the foundation. Extend them rather than invent a disconnected probe. |
| `scripts/smoke-tests/smoke-common.sh`, `smoke-macos.sh`, `smoke-linux.sh`, `smoke-windows.ps1`, `.github/workflows/smoke-test.yml` | Installed authenticated lifecycle evidence already exists. Extend for active Quit/integrity; retain sanitized evidence, UI acknowledgement, exact executable selection and cleanup only of test-owned processes/packages. |

Supporting inspection: `hifimule-daemon/src/device_io.rs` and device modules provide verified writes, MTP worker lifetime and recovery; `scrobbler.rs` can mutate device/local state. Reuse their APIs and drain admitted work. If implementation changes these or `transcoding.rs`, read each modified file fully and record the changed boundary. `service.rs` is legacy Windows service support and currently rejects service ownership; do not reintroduce service fallback. Update its compile interface only if core-handle changes require it. Do not refactor unrelated large modules or change installer startup enrollment.

### Architecture, libraries and technical research

- Rust edition 2024/MSRV 1.93.0; locked Tokio 1.49.0, Axum 0.8.8, Tauri 2.10.3, reqwest 0.12.28 and serde 1.0.228. Reuse current std/Tokio channels, atomics, UUID and ownership library. No dependency upgrade or new audio dependency is required.
- Keep Rust snake_case / JSON camelCase, provider abstraction and native-only credentials. The historical project-context “greenfield” label and fixed-port/child-process lifecycle prose are superseded by current production code and approved playback amendments.
- Official Tokio documentation checked 2026-09-12 currently presents 1.53.1; this is research context, not the chosen project version. Started `spawn_blocking` tasks cannot be aborted; runtime timeout stops waiting without stopping their work. Therefore retain real joins and owner lifetime through blocking-device completion. [Tokio spawn_blocking](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html).
- Shutdown consists of notification followed by awaited completion; cancellation allows cleanup before return. Reuse existing cancellation primitives and explicit task tracking rather than adding tokio-util solely because its tutorial uses it. [Tokio graceful shutdown](https://tokio.rs/tokio/topics/shutdown).

### Testing requirements

| Scenario | Required observable evidence |
| --- | --- |
| Idle Quit and UI-only close | Owner exits/releases descriptor+lock on Quit; closing UI preserves PID and work. |
| Start versus Quit; prepare versus create_operation | One admission outcome; no new work after fencing; late operation inherits cancellation; preparing state works without operation ID. |
| Single/multi-provider and auto-sync | All real production entry points cancel; same terminal outcome and manifest policy. Do not validate only unused legacy executors. |
| Producer fetch/conversion and staged writer | No post-cancel file starts; staging leases/queues clean up; currently writing work reaches a safe boundary; fairness/backpressure regressions remain green. |
| Manifest/playlist/deletion boundary | Barrier-controlled cancellation before/during/after commit; partial file never listed complete; old/new manifest valid; unmanaged sentinel files unchanged. |
| Cancellation + errors; manifest write failure | Failed/Cancelled as appropriate, dirty/pending retained, no false success history/notification. |
| MTP cache path/write failure and snapshot ordering | Last committed cache remains valid; errors propagate; dirty cannot be overwritten by a stale clean snapshot; device selection cannot redirect a running sync's metadata. |
| Producer panic and end_sync_job failure | Remaining workers are joined; required cleanup executes; unresolved device cleanup cannot produce clean exit. |
| Initial dirty write/provider resolution fails after registration | Operation becomes Failed, no orphan Running record or worker wait remains, no managed transfer begins without durable dirty state. |
| Disconnect and several registered operations | Removal bookkeeping continues after admission closure; failure cannot be overwritten; other cleanup progresses while a worker remains blocked. |
| Admitted mutation/scrobbler/blocking native work | Guard remains held until side effects finish; no runtime abort of critical cleanup; owner retained even if operation enum is terminal. |
| Deadline and eventual release | At 5 seconds health/tray/UI show unresolved state, same ID and owner; release barrier later permits exit without another Quit. |
| Repeated Quit and reopening | Same ID/generation/deadline; no new owner, mutation replay or destructive resume-state call; accessible waiting view. |
| Generation persistence failure/stall | No cancellation begins; owner/work survive; pending write shows fencing warning without concurrent retry; completed failure permits Retry Quit; committed shutdown never reopens admission. |
| Relaunch after interrupted sync | Existing reconciliation sees incomplete state; verified files are not confused with fully successful sync. |
| Polling during final RPC drain | Repeated health requests do not indefinitely prevent final shutdown; outstanding responses drain and process exits without reporting false clean device state. |

Use existing `lifecycle_shutdown_tests::teardown_completion_waits_for_blocking_device_work`, `duplicate_launch_cannot_succeed_from_stale_descriptor`, lifecycle contract subprocess tests, `rpc::tests::make_test_state`, operation/recovery fixtures and provider pipeline tests. Preserve native UI `lifecycle_tests` for pre-Quit generations, observed-owner no-election and stale epochs; extend `health_reports_access_protocol_identity_and_stopping_distinctly`. Add narrow coordinator tests with real synchronization barriers; avoid timing-only tests. Handler tests alone bypass middleware, so include authenticated router tests for stopping status and rejected mutation. Frontend compilation is not UI interaction evidence; test reopening/poll cancellation/error presentation explicitly. The existing `lifecycleDeadline.test.mjs` uses Node tests with TypeScript transpilation and can host the pattern for an extracted shutdown observer/presenter. Installed `poll_ui_ready` acknowledges only hydrated UI; add a distinct shutdown-render acknowledgement and coverage in `test-ui-evidence.py` rather than reusing a healthy marker.

Commands: `rtk cargo test -p hifimule-lifecycle`, `rtk cargo test -p hifimule-daemon`, `rtk cargo test -p hifimule-ui --lib`, appropriate targeted `rtk cargo clippy`, `rtk cargo fmt --check`, and `rtk npm run build` from `hifimule-ui`. Run existing `.mjs` behavior tests using their Node test harness, plus shell/PowerShell smoke validation where available. All shell commands use RTK. Distinguish environment-blocked tests from product failures and do not claim unrun tests passed.

Reuse `BlockingFirstWriteDeviceIo` and `cancellation_cleans_queued_staged_files`; extend beyond the existing queued-file absence assertion to current-write completion and durable recovery state. Isolate or serialize router tests touching process-global `LIFECYCLE_STATE`, `ACTIVE_LOCAL_REQUESTS` and OnceLock identity so stopping fixtures do not poison unrelated parallel tests.

Installed evidence must exercise actual shipped binaries on Windows, macOS and Linux and identify architecture. Reuse release/smoke matrices, including macOS x86_64/aarch64 where shipping. Real-device checks must record MSC/MTP transport actually tested, persisted manifest/managed files, unmanaged sentinel preservation and recovery after restart. Failure injection cannot certify physical-device behavior, and ARM64 VM evidence cannot certify x64.

### Previous story intelligence and Git context

- Latest five commits: `e863368` Review 15.1; `23eeae2` Dev 15.1; `09b6a8b` Story 15.1; `ac5217c` Add design for playback; `907e776` cross-platform playback session feasibility proof. Production 15.1 changes take precedence over the earlier experimental supervisor/probe.
- Review hardened delayed-launch fencing, Windows ACLs, bounded native hydration, stopping health on a separate runtime, blocking-work completion and RPC drain. Preserve those fixes when adding active cancellation.
- Story 15.1 documents installed ARM64 MSI/deb/DMG checks and user-observed idle tray Quit on all three OSes, followed by 12 review patches. Its updated installed-platform evidence remains pending rerun; do not reuse pre-review installer results as current 15.2 acceptance evidence.
- Discovery covered Epic 15's then-approved 29-story sequence (historical IDs before the 2026-09-19 split), PRD, architecture, UX, project context, prior story/review and current lifecycle/sync/UI paths. This story prepares lifecycle cancellation only; session restoration belongs to 15.3, audible playback to 15.4 and playback-vs-sync QoS to 15.27.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Story 15.2, Epic 15 implementation gates, Stories 15.1/15.3/15.4, P-AR2]
- [Source: `_bmad-output/planning-artifacts/prd.md` — FR60/FR75, interrupted recovery, write/verify/commit and atomic manifest requirements]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Playback Implementation Contracts; Playback Deployment and Implementation Sequence; atomic manifest requirements]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md` — Headless Sync Feedback; Responsive Design & Accessibility]
- [Source: `_bmad-output/planning-artifacts/project-context.md` — managed ownership and provider abstraction principles]
- [Source: `_bmad-output/planning-artifacts/playback-epic-validation.md`, `playback-story-review.md` — approved scope and preparation gates]
- [Source: `_bmad-output/implementation-artifacts/15-1-close-and-reopen-the-ui-without-restarting-the-daemon.md` — lifecycle contract, review corrections and installed-evidence limitations]
- [Source: production files in the change/preservation map, `Cargo.toml`, `Cargo.lock`, `.github/workflows/release.yml`]

## Dev Agent Record

### Agent Model Used

GPT-6 (Codex)

### Debug Log References

- 2026-09-12: Story preparation analyzed planning artifacts, prior implementation/review, current production paths and official Tokio shutdown documentation. No production implementation or runtime validation was performed by story creation.
- 2026-09-12: Implemented serialized fencing/cancellation/drain coordination, a cancellation-vs-clean-commit gate, bounded health snapshots, nonblocking generation persistence, and deterministic coordinator deadline/race tests.
- 2026-09-12: Hardened single-provider, multi-provider and auto-sync finalizers; serialized per-device manifest commits; added atomic MTP cache replacement and portable/backend identity continuity; propagated producer, per-file metadata and `end_sync_job` failures.
- 2026-09-12: Added live/reopened shutdown UI observation, accessible refresh/close presentation, four-locale catalog parity, and a shutdown-render smoke acknowledgment bound to owner and shutdown identities.
- 2026-09-12: Validation on Darwin arm64: lifecycle 13/13, daemon 649/649, native UI 5/5, frontend behavior 4/4, smoke-evidence 3/3; workspace check, frontend build, fmt and targeted clippy pass. Installed Windows/Linux/macOS real-device active-Quit evidence remains UNVERIFIED.
- 2026-09-12: Built the production Linux arm64 `.deb` in Ubuntu, installed it, ran the installed-artifact lifecycle/UI/crash-recovery smoke successfully, and removed the package. The run recorded Linux/aarch64 ownership and timing evidence while correctly leaving physical-device tray Quit UNVERIFIED; the run also exposed and prompted a fix for the Linux smoke binary-presence check.
- 2026-09-12: Built the final production Windows ARM64 MSI in the Windows UTM VM: `HifiMule_0.14.0_arm64_en-US.msi`, SHA-256 `2E4433900981709F740BC65DA7630B0ADD6BABAD359888A5F0AB43952256D128`. The installed-artifact smoke passed daemon health, authenticated UI attachment, concurrent launch/UI close-reopen identity preservation, stale-owner crash recovery and uninstall; post-run checks found no UI/daemon process and no `C:\\Program Files\\HifiMule` directory. Physical-device active Quit remains UNVERIFIED.
- 2026-09-12: The first Windows installed-artifact smoke correctly failed before daemon publication because an elevated launch inherited an existing runtime directory owned by `BUILTIN\\Administrators`; lifecycle validation requires the actual token user SID. The failed MSI was removed, `protect_windows_path` was hardened to assign the current user as owner before applying its private DACL, and both host (13/13) and Windows-native lifecycle contracts pass before the replacement MSI rebuild.
- 2026-09-12: Replacement Windows smoke exposed two VM-specific stale-state/loopback boundaries: an existing elevated `launch-generation.json` also needed ownership normalization, and this Windows firewall configuration times out rather than refuses a closed legacy-port probe. Generation reads now migrate the file through the same private-owner protection, while legacy-endpoint timeout falls back to an exclusive bind check before reporting `LEGACY_ENDPOINT_OCCUPIED`.
- 2026-09-12: Final post-smoke host regression after the Windows fixes: formatting and workspace check pass; lifecycle 13/13 and daemon 649/649 pass. The daemon suite requires normal host access because its mock HTTP servers and macOS system-configuration APIs are intentionally unavailable inside the restricted filesystem sandbox.
- 2026-09-12: The first physical-device macOS Quit attempt was not Story 15.2 evidence: `/Applications/HifiMule.app` launched the prior Story 15.1 daemon and its runtime log emitted `QUIT_BLOCKED_ACTIVE_SYNC` at 13:00:22, a refusal path no longer present in the Story 15.2 daemon. Built a fresh ad-hoc-signed macOS arm64 DMG from this workspace for retest: `HifiMule_0.14.0_aarch64.dmg`, SHA-256 `06254c5ae8adb138ef83d592340e621df8f5fe573800b729c9e21297aad65b78`.
- 2026-09-12: User-confirmed physical-device retest with that macOS arm64 DMG passed active tray Quit on an MSC volume (`/Volumes/Music`). The 190-file operation stopped during file 43 rather than completing the queue; the in-flight verified write finished, the next staged file was removed without being written, staging cleanup completed, and the daemon logged graceful shutdown. Relaunch 16 seconds later rediscovered the same device with `dirty: true` and the interrupted `pending_item_ids`, confirming recoverable state was retained. Unmanaged-sentinel inspection was not performed.
- 2026-09-12: User-confirmed Linux arm64 installed-artifact retest passed active-sync tray Quit: after starting a device sync and selecting Quit, the daemon stopped instead of completing the full sync. This run confirms the Linux Quit/cancellation/exit path; transport type, post-relaunch recovery state and unmanaged-sentinel integrity were not independently recorded.
- 2026-09-12: User-confirmed Windows ARM64 installed-artifact retest passed active-sync tray Quit: after starting a device sync and selecting Quit, the daemon stopped instead of completing the full sync. This completes user-observed active-sync tray-Quit/daemon-exit coverage on all three target operating systems; Windows transport type, post-relaunch recovery state and unmanaged-sentinel integrity were not independently recorded.
- 2026-09-12: User completed the follow-up recovery/integrity procedure successfully over MSC/filesystem transport on Linux arm64 and Windows ARM64: an unmanaged sentinel was created, active tray Quit interrupted a large multi-file sync, relaunch detected the interruption without reporting false success, a subsequent sync completed normally, the sentinel remained unchanged, managed files were neither corrupt nor zero-byte, temporary/staging files were absent, and already completed tracks remained playable.
- 2026-09-12: User-confirmed macOS arm64 physical MTP test passed after replacing a bad USB cable: HifiMule detected the MTP device, started a sync, and tray Quit interrupted it. The open UI displayed the authoritative shutdown message while draining, then transitioned to the no-daemon message after exit. Together with deterministic MTP cache/integrity tests and the three-OS MSC recovery checks, this closes AC8 evidence.

### Implementation Plan

- Fence launch admission and persist generation before committing cancellation; drain existing pipeline and mutation guards without blocking the native event loop.
- Serialize cancellation against final clean-manifest commit, preserve failure precedence and recovery evidence, and bind metadata writes to the operation device.
- Publish one bounded authenticated shutdown snapshot and render it through tray, native proxy and live/reopened UI observers.
- Validate deterministic race/deadline behavior and extend installed evidence without treating injected tests as physical-device certification.

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- Preparation resolves shutdown phases, cancellation ownership, five-second warning behavior, blocked-write policy, health status and future playback participation. Implementation tasks remain unchecked.
- Implemented AC1–AC7 code and automated regression coverage. Quit now fences new work, commits cancellation once, waits for real worker/mutation completion, preserves ownership during overruns and exposes the same shutdown UUID through health, tray and UI.
- Interrupted/failed syncs retain dirty/pending recovery evidence; late success cannot overwrite failure; final manifest failure and unresolved device cleanup fail the operation; successful cancellation emits no completion/safe-eject notification.
- Authoritative manifest writes are per-device serialized; MTP cache replacement is atomic and flushed, and reconnect lookup preserves a matching portable-identity dirty cache when the backend probe ID differs.
- Windows lifecycle protection now normalizes runtime/file ownership to the current token user before installing the owner/SYSTEM/Administrators-only DACL, including elevated MSI smoke launches whose newly created directory would otherwise be owned by the Administrators group.
- Windows generation reads migrate stale elevated ownership before validation, and legacy-endpoint detection distinguishes a firewall-induced connect timeout from a genuinely occupied port by requiring an exclusive bind failure.
- Ubuntu arm64 installed-artifact smoke passed against `HifiMule_0.14.0_arm64.deb`, including owner cleanup, UI attachment and stale-owner crash recovery; the package was uninstalled after the run. Physical-device active-Quit remains UNVERIFIED because the Linux VM had no attached USB device/tray session.
- Windows ARM64 installed-artifact smoke passed against `HifiMule_0.14.0_arm64_en-US.msi` (SHA-256 `2E4433900981709F740BC65DA7630B0ADD6BABAD359888A5F0AB43952256D128`), including owner cleanup, UI attachment, close/reopen identity preservation and stale-owner crash recovery. The MSI was uninstalled and independent post-run checks confirmed no HifiMule process or installation directory remained.
- macOS arm64 physical MSC evidence passed against `HifiMule_0.14.0_aarch64.dmg` (SHA-256 `06254c5ae8adb138ef83d592340e621df8f5fe573800b729c9e21297aad65b78`): active tray Quit stopped a 190-file sync at its safe current-write boundary, discarded queued staging, shut the daemon down, and preserved dirty/pending recovery state observed on relaunch. Unmanaged-sentinel preservation remains UNVERIFIED for this run.
- Linux arm64 and Windows ARM64 MSC/filesystem user testing confirms active tray Quit, automatic daemon exit, interrupted-state detection after relaunch, successful follow-up sync, unmanaged-sentinel preservation, absence of corrupt/zero-byte or temporary managed files, and playability of already completed tracks.
- AC8 is complete: active-sync tray Quit and daemon exit are verified over MSC/filesystem transport on macOS, Linux and Windows ARM64, interrupted recovery is verified on all three, unmanaged-sentinel/managed-file integrity is verified on Linux and Windows, and macOS arm64 physical MTP verifies device detection, active-transfer cancellation, authoritative shutdown UI and final daemon disappearance.

### File List

- `_bmad-output/implementation-artifacts/15-2-quit-safely-while-a-device-sync-is-running.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `hifimule-daemon/src/device/mod.rs`
- `hifimule-daemon/src/device/tests.rs`
- `hifimule-daemon/src/main.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-daemon/src/sync.rs`
- `hifimule-i18n/catalog.json`
- `hifimule-lifecycle/src/lib.rs`
- `hifimule-ui/src-tauri/src/lib.rs`
- `hifimule-ui/src/main.ts`
- `hifimule-ui/src/shutdownStatus.ts`
- `hifimule-ui/tests/shutdownStatus.test.mjs`
- `scripts/smoke-tests/smoke-common.sh`
- `scripts/smoke-tests/smoke-linux.sh`
- `scripts/smoke-tests/smoke-macos.sh`
- `scripts/smoke-tests/smoke-windows.ps1`
- `scripts/smoke-tests/test-ui-evidence.py`

## Change Log

- 2026-09-12: Created Story 15.2 with production shutdown integration, integrity guardrails and acceptance evidence requirements; status ready-for-dev.
- 2026-09-12: Implemented orderly active-sync Quit coordination, durable manifest/finalization protections, authenticated shutdown UI, localization and automated evidence; AC8 physical cross-platform tray/device validation remains pending, so status stays in-progress.
- 2026-09-12: Verified the installed Linux arm64 `.deb` lifecycle smoke and corrected its broken binary-presence guard; physical-device active-Quit evidence remains pending.
- 2026-09-12: Verified and removed the installed Windows ARM64 MSI after lifecycle/UI/relaunch/recovery smoke; hardened elevated stale-generation migration and legacy-port probing uncovered by the installed run. Physical-device active-Quit evidence remains pending.
- 2026-09-12: Verified physical MSC active-sync tray Quit and interrupted-state recovery on macOS arm64 using the fresh Story 15.2 DMG; MTP evidence remains pending.
- 2026-09-12: Verified user-observed active-sync tray Quit and daemon exit on Linux arm64; detailed Linux recovery/integrity evidence and MTP coverage remain pending.
- 2026-09-12: Verified user-observed active-sync tray Quit and daemon exit on Windows ARM64, completing three-OS active-Quit coverage; MTP coverage remained pending at that checkpoint.
- 2026-09-12: Verified the full follow-up recovery and integrity checklist over MSC/filesystem transport on Linux and Windows ARM64, including sentinel preservation and a successful repair sync; MTP coverage and any unperformed shutdown-UI interaction remain pending.
- 2026-09-12: Verified physical MTP active-sync Quit on macOS arm64 with shutdown and no-daemon UI states; all Story 15.2 tasks and acceptance evidence are complete, status advanced to review.

- 2026-09-12: Applied all eight approved code-review patches; automated regression suites pass, F9 remains deferred, and story status advanced to done. Physical-device smoke was not rerun for these patches.
