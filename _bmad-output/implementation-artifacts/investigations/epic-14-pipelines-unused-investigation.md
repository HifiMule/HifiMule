# Investigation: Epic 14 pipelines appear unused

## Hand-off Brief

1. **What happened.** The 2026-07-12 sync log contains per-file preparation, download, and write events but no pipeline events; the displayed messages are confirmed to originate in `hifimule-daemon/src/sync.rs`.
2. **Where the case stands.** Active at initial scoping; whether Epic 14 is bypassed or runs before this transfer phase remains open.
3. **What's needed next.** Map the runtime caller chain and pipeline-selection logs to determine whether the pipeline path is selected, silent, or bypassed.

## Case Info

| Field            | Value |
| ---------------- | ----- |
| Ticket           | N/A |
| Date opened      | 2026-07-12 |
| Status           | Active |
| System           | Windows development build; exact binary revision and device manifest not yet captured |
| Evidence sources | User-provided sync log excerpt; source tree; Epic implementation artifacts |

## Problem Statement

User hypothesis (verbatim): "It seems that the epic 14 code is not used. In the log I see no information on pipelines."

Observed window: 2026-07-12 11:47:50–11:48:49. The excerpt shows five files entering preparation, stream resolution, path construction, download, and write stages without a pipeline-related message.

## Evidence Inventory

| Source   | Status | Notes |
| -------- | ------ | ----- |
| User-provided log excerpt | Partial | Five of 3,471 files; 2026-07-12 11:47:50–11:48:49; no startup or pre-transfer selection phase included |
| Source code | Available | Exact `Preparing file` message occurs at `hifimule-daemon/src/sync.rs:1775` and `hifimule-daemon/src/sync.rs:2608`; exact `resolving stream` message occurs at `hifimule-daemon/src/sync.rs:1832` |
| Epic 14 implementation artifacts | Available | Three completed stories: 14.1 temp-only staging, 14.2 bounded producer/writer pipeline, 14.3 per-server fairness and priority buckets |
| Version control | Available | Story/dev/review history exists from `cb01a65` through `b3b6339`; current investigation file is the only untracked path |
| Tests | Partial | Epic artifacts claim provider-pipeline, backpressure, cancellation, and cleanup tests; exact runnable inventory/results not yet collected |
| Static analysis | Missing | No fresh build or analysis run has been performed in this investigation |
| Issue tracker | Missing / N/A | No ticket was supplied |
| Runtime configuration / device manifest | Missing | Needed to establish whether a configurable pipeline was enabled and selected |
| Exact executable revision | Missing | Needed to prove the running binary contains the workspace implementation |

## Investigation Backlog

| # | Path to Explore | Priority | Status | Notes |
| - | --------------- | -------- | ------ | ----- |
| 1 | Identify Epic 14 stories and promised runtime behavior | High | Done | Epic 14 is the provider transfer pipeline, distinct from Epic 12/13 auto-fill selection |
| 2 | Trace both `Preparing file` call sites backward to sync entry points | High | Open | Determine transfer path and any upstream selection phase |
| 3 | Trace pipeline engine callers and runtime discriminator | High | Open | Determine selected, silent, or bypassed |
| 4 | Compare current source revision with running executable | High | Open | Exclude stale binary/build |
| 5 | Inspect effective device manifest and server-specific pipeline | High | Open | Exclude default/disabled configuration |

## Timeline of Events

| Time        | Event | Source | Confidence |
| ----------- | ----- | ------ | ---------- |
| 2026-07-12 11:47:50 | File 1 begins transfer preparation; `profile=false` | User-provided log excerpt | Confirmed |
| 2026-07-12 11:48:49 | File 5 completes writing; no pipeline message appears in the supplied window | User-provided log excerpt | Confirmed |

## Confirmed Findings

### Finding 1: The supplied messages are transfer-stage messages from sync.rs

**Evidence:** `hifimule-daemon/src/sync.rs:1775`, `hifimule-daemon/src/sync.rs:1832`, `hifimule-daemon/src/sync.rs:2608`

**Detail:** The exact log templates shown by the user exist in the current source tree. This anchors the observation in the file-transfer path, but does not yet establish where pipeline selection occurs relative to it.

### Finding 2: The observed run used execute_sync, not the Epic 14 path

**Evidence:** `hifimule-daemon/src/sync.rs:1831-1844`; repository-wide source search finds `resolving stream` and `get_item_stream` only at this location.

**Detail:** The supplied sequence uniquely identifies `execute_sync`. Epic 14 is implemented in the separate `execute_provider_sync` function beginning at `hifimule-daemon/src/sync.rs:2453`.

### Finding 3: Single-server Jellyfin dispatch bypasses execute_provider_sync

**Evidence:** `hifimule-daemon/src/rpc.rs:5136-5161`, `hifimule-daemon/src/rpc.rs:5292-5334`, `hifimule-daemon/src/rpc.rs:5414-5477`; daemon auto-sync equivalents at `hifimule-daemon/src/main.rs:903-915` and `hifimule-daemon/src/main.rs:1478-1490`.

**Detail:** Cross-server routing calls `execute_provider_sync`; an active non-Jellyfin provider also calls it. Otherwise RPC execution loads Jellyfin credentials and calls legacy `execute_sync`. The daemon-initiated paths preserve the same split.

### Finding 4: Epic 14 intentionally left execute_sync unchanged

**Evidence:** `_bmad-output/implementation-artifacts/14-1-temp-only-disk-staging-for-provider-sync.md:54-59`

**Detail:** Story 14.1 explicitly says `buffer_stream` remains in the older non-provider `execute_sync` path and warns not to change it unless all callers are handled. Epic 14 therefore implemented throughput changes only inside `execute_provider_sync`.

## Deduced Conclusions

### Deduction 1: Missing Epic 14 logs are expected for this run

**Based on:** Findings 2-4.

**Reasoning:** The exact log sequence proves `execute_sync`; Epic 14 diagnostics are emitted only by `execute_provider_sync`; the dispatch deliberately retains `execute_sync` for Jellyfin fallback.

**Conclusion:** The absence of queue, staging, and pipeline-summary messages is caused by runtime branch selection, not disabled logging inside Epic 14.

### Deduction 2: Epic 14 is used, but not by ordinary single-server Jellyfin sync

**Based on:** Finding 3 and `execute_provider_sync` callers at `rpc.rs:5221`, `rpc.rs:5332`, and `main.rs:1478`.

**Reasoning:** Non-Jellyfin and cross-server branches call the Epic 14 function. The observed Jellyfin-shaped branch does not.

**Conclusion:** The user's global premise ("Epic 14 code is not used") is refuted; the narrower premise ("Epic 14 code is not used for this sync") is confirmed.

## Hypothesized Paths

### Hypothesis 1: Epic 14 runtime pipeline code is not used

**Status:** Confirmed for the observed run; Refuted globally

**Theory:** The active sync call path bypasses the Epic 14 pipeline implementation.

**Supporting indicators:** No pipeline information appears in the supplied transfer-stage log window.

**Would confirm:** Caller-chain evidence that the observed sync path never reaches the Epic 14 selection/engine functions despite a non-default enabled pipeline.

**Would refute:** Runtime or test evidence that the same sync invocation reaches the Epic 14 path before these transfer messages, or proof that Epic 14 does not specify transfer-stage pipeline logging.

**Resolution:** The unique `execute_sync` log at `sync.rs:1832` confirms bypass for this run. Callers at `rpc.rs:5221`, `rpc.rs:5332`, and `main.rs:1478` refute that Epic 14 is globally unused.

## Missing Evidence

| Gap | Impact | How to Obtain |
| --- | ------ | ------------- |
| Full log from sync request through materialization | Shows whether pipeline selection happened before the excerpt | Capture or inspect the full run log |
| Effective `.hifimule.json` | Shows whether a server-specific, enabled, non-default pipeline exists | Inspect the connected device manifest |
| Running binary revision/build time | Distinguishes wiring defect from stale executable | Compare executable metadata/revision with current checkout |

## Source Code Trace

| Element       | Detail |
| ------------- | ------ |
| Error origin  | No error; the diagnostic absence originates from legacy `execute_sync` at `hifimule-daemon/src/sync.rs:1642`, identified by its unique message at `sync.rs:1832` |
| Trigger       | Manual single-server Jellyfin execution via `rpc.rs:5414-5465`, or Jellyfin daemon auto-sync via `main.rs:903-915` |
| Condition     | Dispatch is not cross-server and the selected provider is Jellyfin; `active_non_jellyfin_provider` intentionally returns `None` for Jellyfin at `rpc.rs:2104-2110` |
| Related files | `hifimule-daemon/src/rpc.rs`, `hifimule-daemon/src/main.rs`, `hifimule-daemon/src/sync.rs`, `hifimule-daemon/src/providers/jellyfin.rs`, Epic 14 story artifacts |

## Conclusion

**Confidence:** High

The observed sync did not execute Epic 14. It ran the legacy Jellyfin `execute_sync` path, whose unique stream-resolution log appears in the excerpt. Epic 14 is wired only through `execute_provider_sync`, used by non-Jellyfin and cross-server dispatch; Story 14.1 intentionally left the older Jellyfin path unchanged.

## Recommended Next Steps

### Fix direction

**Routing convergence:** route ordinary Jellyfin sync through its existing `JellyfinProvider` and `execute_provider_sync`, covering both RPC/manual and daemon auto-sync entry points. Do not duplicate Epic 14 inside `execute_sync`; that would retain two transfer implementations.

This is non-trivial because behavior parity must be checked for Jellyfin stream/profile selection, direct-play suffixes, deletion/re-add cleanup, playlists, progress, cancellation, and manifest writes before the legacy path can be retired.

### Diagnostic

No extra logging is required to identify this run's branch. A single-server Jellyfin reproduction should emit `resolving stream`; a non-Jellyfin/provider reproduction should emit `execute_provider_sync preparing`, queue diagnostics, and `Provider pipeline summary`.

## Reproduction Plan

Run one sync with an enabled non-default pipeline, capture logs from request start through first transfer, and correlate the selected server/pipeline with the resulting item set.

## Side Findings

- "Pipeline" is overloaded: Epics 12/13 implement auto-fill selection pipelines; Epic 14 implements the provider download/staging/device-writer pipeline.

## Follow-up: 2026-07-12

### New Evidence

- All three Epic 14 story artifacts are present and marked `done`: `14-1-temp-only-disk-staging-for-provider-sync.md:7`, `14-2-bounded-producer-writer-pipeline.md:7`, and `14-3-per-server-producer-fairness-and-priority-buckets.md:7`.
- Version history contains separate story, development, and review commits for Epic 14: `cb01a65`, `4210c22`, `da0c78d`, `1f2efd6`, `2730d9e`, `28f8e84`, `960651f`, and `b3b6339`.
- The source inventory contains Epic 14 staging/queue implementation in `hifimule-daemon/src/sync.rs` beginning around `sync.rs:2338`, while the supplied `resolving stream` message exists at `sync.rs:1832`.

### Additional Findings

**Confirmed:** Epic 14 is not the configurable auto-fill pipeline. Story 14.2 explicitly describes a bounded provider producer/writer pipeline (`14-2-bounded-producer-writer-pipeline.md:5`), and Story 14.3 extends it with per-server fairness (`14-3-per-server-producer-fairness-and-priority-buckets.md:5`).

**Confirmed:** Both the older-looking per-file stream-resolution path and Epic 14 staging/queue code coexist in `sync.rs`. Which branch produced the supplied run will be established in the next caller-chain trace.

### Updated Hypotheses

Hypothesis 1 remains **Open**, now narrowed: the observed run may have selected a non-provider/legacy transfer branch that does not use Epic 14's bounded provider pipeline.

### Backlog Changes

- Epic 14 scope inventory: Done.
- Next: trace the exact `resolving stream` path and its branch condition against the Epic 14 producer entry point.
- Still missing: full log start, effective manifest/provider identity, and executable provenance.

### Updated Conclusion

Evidence perimeter mapped. Epic 14 code exists in source and version history, but existence does not prove runtime selection. The exact log sequence strongly localizes the observed run to the `sync.rs:1832` stream-resolution path; the next outcome will determine whether that path is intentionally outside Epic 14 or an unintended bypass.

### Cause Analysis

1. The sync request reaches provider dispatch.
2. Cross-server adds select `execute_provider_sync` at `rpc.rs:5161-5221`.
3. A selected non-Jellyfin provider selects `execute_provider_sync` at `rpc.rs:5292-5332`.
4. The remaining Jellyfin branch selects `execute_sync` at `rpc.rs:5414-5465`.
5. `execute_sync` processes each add serially and emits the supplied `Preparing` / `resolving stream` / `Downloading` / `Writing` messages.
6. Epic 14's producer, staging queue, and writer diagnostics never run.

### Refutation Pass

- Searched for the exact `resolving stream` template and its `get_item_stream` call across daemon source. Both occur only in `execute_sync`; no Epic 14 path can emit that sequence.
- Searched all `execute_provider_sync` callers. Multiple live callers exist, so the code is not dead globally.

### Hypothesis Resolution

- **Confirmed:** Epic 14 was bypassed by the observed run.
- **Refuted:** Epic 14 is unused everywhere.
- **Open product/scope question:** Whether Epic 14 should now be extended to ordinary single-server Jellyfin sync. The implementation followed Story 14.1's explicit restriction, so this is a scope gap rather than evidence of an uncalled implementation.

### Final Source Trace

- `require_provider` already resolves Jellyfin as a `MediaProvider` (`rpc.rs:450-462`).
- `JellyfinProvider` implements `MediaProvider` (`providers/jellyfin.rs:132`) and supplies `download_url` (`providers/jellyfin.rs:506`), including direct-download and transcoding tests (`providers/jellyfin.rs:1589-1624`).
- The exclusion is explicit and local: `active_non_jellyfin_provider` discards the resolved provider when `server_type() == Jellyfin` (`rpc.rs:2104-2110`).
- Manual Jellyfin then falls through to direct credentials plus `execute_sync` (`rpc.rs:5414-5477`). Daemon Jellyfin auto-sync independently calls the same legacy function (`main.rs:903-915`).
- Epic 14 runs through `execute_provider_sync` (`sync.rs:2453`) and necessarily emits entry, queue, and summary diagnostics (`sync.rs:2479`, `sync.rs:3177`, `sync.rs:3355`). None can be emitted by `execute_sync`.

### Fix Classification

Non-trivial routing convergence, not a local logging bug. The smallest correct direction is to reuse the existing Jellyfin provider adapter at both sync dispatch entry points and prove behavioral parity; copying the queue into `execute_sync` would create a second Epic 14 implementation.
