# Investigation: Missing sync logs before preparation

## Hand-off Brief

The Sync button awaits incremental change detection before invoking the preparation handler instrumented in e4df4fc. That detection handler and its potentially lengthy Subsonic album fallback had no lifecycle logs. The correction instruments detection and tags only the initial existing state request with sync-start intent, without changing sync decisions.

## Case Info

- Date opened: 2026-10-18
- Status: Concluded
- Evidence: source code, e4df4fc, user report; running daemon state not verified.

## Problem Statement

User: “there is no log when we start the sync, so something is missing.” Clarification: “it's before preparation.”

## Evidence Inventory

| Source | Status | Notes |
| --- | --- | --- |
| UI and daemon source | Available | Direct call chain shows uninstrumented work before preparation |
| Previous implementation | Available | e4df4fc begins manual run logging at preparation |
| Running daemon/log | Missing | Not needed to reproduce source coverage gap; exact live wait remains unverified |

## Confirmed Findings

- `hifimule-ui/src/components/BasketSidebar.ts:1392`: Sync first awaits `attachToRunningSync`.
- `hifimule-ui/src/components/BasketSidebar.ts:1417`: it then awaits `itemIdsWithIncrementalChanges` before `sync_prepare` at line 1454.
- `hifimule-ui/src/components/BasketSidebar.ts:1613`: Subsonic plus a saved token triggers `sync_detect_changes`.
- `hifimule-daemon/src/rpc.rs:8297` at baseline: `handle_sync_detect_changes` awaited the provider without start/end file logs.
- `hifimule-daemon/src/providers/subsonic.rs:832` at baseline: change detection requests indexes, then falls back to album inspection if indexes contain no changes.
- `hifimule-daemon/src/providers/subsonic.rs:112` at baseline: album fallback sequentially requests each known album; full dumps also paginate.

## Deduced Conclusions

A slow provider request during change detection leaves the daemon log silent before the previous implementation's first preparation record. Instrumenting only preparation cannot cover this interval.

## Hypothesized Paths

1. Missing pre-preparation logging — **Confirmed** by the UI/daemon call chain. Refutation check: `sync_prepare` is reached only after detection returns; no prior lifecycle log exists in that handler.
2. Exact live stall is album fallback — **Open**. New stage logs will distinguish index lookup, full dump, and album fallback.
3. UI is waiting to attach to an existing pipeline — **Open**. The first UI check polls state until an operation appears or the pipeline becomes inactive; no new sync request is made during that wait.

## Source Code Trace

`handleStartSync` → running-operation check → incremental changes (Subsonic with saved token) → `sync_prepare` → `sync_execute`.

## Investigation Backlog

- Add lifecycle logs before detection's first await; preserve errors and stale-token fallback.
- Time provider bulk substeps without logging individual tracks, tokens, or URLs.
- Add a blocked-provider regression proving logs appear before preparation can start.
- Document the separate pre-preparation phase and its interpretation.

## Conclusion

**High confidence** in a concrete logging omission. The user's precise running-daemon wait has not been independently observed. This follow-up continues the approved sync logging work.

## Reproduction Plan

Block a fake provider's change request, invoke `handle_sync_detect_changes`, and assert its start record exists before releasing the provider. Verify success, error, stale-token fallback, and dropped-future terminal records without changing returned behavior.

## Follow-up: 2026-10-18

The source trace also confirms that the first `get_daemon_state` RPC is indistinguishable from routine polling. Add an optional diagnostic marker on only the first click-triggered state request, and log before snapshot awaits. Subsequent polls and recovery attachment keep their existing payloads. This closes the earlier attach wait as well as incremental detection.

## Resolution and verification

Implemented the initial `sync_start_check` marker before snapshot awaits, plus `incremental_change_detection` and Subsonic index/full-dump/album-fallback lifecycle records. Ordinary state polling stays quiet. No sync decisions or response shapes changed; only the first click-triggered existing state request carries the optional diagnostic flag.

Passed: 6 lifecycle fixtures, 2 existing change-detection tests, 11 Subsonic change tests, 7 frontend contract tests, UI TypeScript/Vite build, and whitespace validation. Blocked snapshot and provider fixtures confirm starts are emitted before either wait returns. Existing Vite chunk warnings remain. Exact live-device latency remains unmeasured.
