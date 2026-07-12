---
title: 'Route Jellyfin Sync Through the Epic 14 Provider Pipeline'
type: 'bugfix'
created: '2026-07-12'
status: 'done'
baseline_commit: 'feb8a71e8425f16ad19e384717914c61a46373c4'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/investigations/epic-14-pipelines-unused-investigation.md'
  - '{project-root}/_bmad-output/planning-artifacts/architecture.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Ordinary single-server Jellyfin sync calls legacy `execute_sync`, so Epic 14's bounded producer/staging/writer pipeline is bypassed. Large Jellyfin syncs remain serial and never emit the queue or pipeline-summary diagnostics implemented by Epic 14.

**Approach:** Reuse the existing `JellyfinProvider` and `execute_provider_sync` path for both manual/RPC and daemon-initiated Jellyfin sync, then remove legacy `execute_sync` and its obsolete tests/helpers once no callers remain. Keep exactly one transfer implementation.

## Boundaries & Constraints

**Always:** Preserve device write serialization, cancellation, manifest dirty/finalization behavior, per-file progress, direct-play/transcoding selection, provider-specific suffix validation, deletion/re-add cleanup, playlist generation, and error reporting. Keep all Jellyfin traffic behind `MediaProvider` on the corrected path. Remove `execute_sync` after migrating both callers, including code and tests that exist only for that path. Add one focused runnable regression check proving Jellyfin selects the provider pipeline.

**Ask First:** Any persistent schema or RPC contract change; any change to `DeviceIO`; any discovered `execute_sync` behavior that cannot be preserved by the provider path without expanding scope.

**Never:** Leave an unused compatibility wrapper or second transfer implementation after migration; add a dependency, feature flag, configuration toggle, parallel device writes, or unrelated auto-fill changes.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Manual Jellyfin sync | Selected Jellyfin server, ordinary single-server delta | Executes through `execute_provider_sync`; Epic 14 entry/queue/summary diagnostics are reachable | Existing operation failure reporting remains intact |
| Jellyfin auto-sync | Connected Jellyfin server triggers daemon sync | Uses the same provider pipeline and serial device writer | Cancellation and connection failures preserve current status transitions |
| Direct play or transcode | Jellyfin track with no profile, compatible profile, or required transcode | Existing `JellyfinProvider::download_url` behavior determines the stream and validated suffix | Unsupported or unconfirmed outputs remain recoverable per-file errors |
| Non-Jellyfin or cross-server | Existing Navidrome/Subsonic or multi-server delta | Existing `execute_provider_sync` behavior is unchanged | Existing routing errors remain unchanged |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/rpc.rs` -- Manual sync dispatch; currently excludes Jellyfin via `active_non_jellyfin_provider` before falling back to `execute_sync`.
- `hifimule-daemon/src/main.rs` -- Daemon auto-sync has separate Jellyfin and provider execution calls.
- `hifimule-daemon/src/sync.rs` -- Legacy `execute_sync` to remove, Epic 14 `execute_provider_sync` to retain, shared helpers to preserve, and provider-pipeline regression tests.
- `hifimule-daemon/src/providers/jellyfin.rs` -- Existing `MediaProvider` implementation and direct/transcode URL tests.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/rpc.rs` -- route selected Jellyfin manual sync through the already-resolved provider and `execute_provider_sync` without altering other RPC branches.
- [x] `hifimule-daemon/src/main.rs` -- route Jellyfin daemon auto-sync through `JellyfinProvider` and `execute_provider_sync`, reusing the existing credentials/client inputs.
- [x] `hifimule-daemon/src/sync.rs` -- delete `execute_sync` and path-private helpers/tests after caller migration; retain shared cleanup, playlist, manifest, and progress behavior through `execute_provider_sync`.
- [x] Existing dispatch/provider tests -- add the smallest regression check that fails if Jellyfin stops selecting `execute_provider_sync`.

**Acceptance Criteria:**
- Given a selected Jellyfin server and a single-server sync, when sync execution starts, then `execute_provider_sync` handles the delta and Epic 14 diagnostics are emitted.
- Given existing non-Jellyfin or cross-server syncs, when execution starts, then their provider routing remains unchanged.
- Given cancellation or a provider/device error, when the corrected Jellyfin path exits, then the operation and daemon state retain their existing completed, cancelled, or failed semantics.
- Given the migration is complete, when the daemon is searched and compiled, then `execute_sync` has no definition or callers and only `execute_provider_sync` implements file transfer.

## Spec Change Log

## Design Notes

Change dispatch to reuse the existing provider adapter. Once both callers compile against `execute_provider_sync`, delete `execute_sync` rather than keeping a wrapper or dormant fallback.

## Verification

**Commands:**
- `rtk cargo test -p hifimule-daemon test_execute_provider_sync` -- expected: all provider pipeline, staging, backpressure, and cancellation tests pass.
- `rtk cargo test -p hifimule-daemon jellyfin` -- expected: Jellyfin provider and routing regressions pass.
- `rtk cargo check -p hifimule-daemon` -- expected: compilation succeeds without new warnings or errors.

## Suggested Review Order

**Unified dispatch**

- Manual sync resolves every selected provider before creating an operation.
  [`rpc.rs:5134`](../../hifimule-daemon/src/rpc.rs#L5134)

- Auto-sync connects the selected server and uses one provider workflow.
  [`main.rs:301`](../../hifimule-daemon/src/main.rs#L301)

- Existing connection construction is reused without changing provider cache state.
  [`server_manager.rs:116`](../../hifimule-daemon/src/server_manager.rs#L116)

**Single transfer implementation**

- Epic 14 remains the sole file-transfer executor.
  [`sync.rs:1777`](../../hifimule-daemon/src/sync.rs#L1777)

- Non-verifying devices are checked before manifest success is recorded.
  [`sync.rs:2574`](../../hifimule-daemon/src/sync.rs#L2574)

- Empty auto-sync resolution restores the daemon to idle.
  [`main.rs:788`](../../hifimule-daemon/src/main.rs#L788)

**Regression coverage**

- End-to-end RPC test proves Jellyfin dispatch reaches the provider pipeline.
  [`rpc.rs:8069`](../../hifimule-daemon/src/rpc.rs#L8069)

- Provider integration test proves authenticated Jellyfin download staging.
  [`sync.rs:4423`](../../hifimule-daemon/src/sync.rs#L4423)

- Missing writes cannot create successful manifest records.
  [`sync.rs:4485`](../../hifimule-daemon/src/sync.rs#L4485)
