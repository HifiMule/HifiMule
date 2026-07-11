---
stepsCompleted: [1, 2]
inputDocuments: []
session_topic: 'Improve HifiMule sync throughput by evaluating alternatives to the current sequential server-read then device-write transfer path'
session_goals: 'Identify practical architecture and implementation alternatives that maximize throughput, especially by overlapping server download/transcode and device writes, while preserving correctness and device compatibility'
selected_approach: 'progressive-flow'
techniques_used: ['First Principles Thinking', 'What If Scenarios', 'Constraint Mapping', 'Solution Matrix', 'Decision Tree Mapping']
ideas_generated: []
context_file: ''
---

# Brainstorming Session Results

**Facilitator:** Alexis
**Date:** 2026-07-11

## Session Overview

**Topic:** Improve HifiMule sync throughput by evaluating alternatives to the current sequential server-read then device-write transfer path.

**Goals:** Identify practical architecture and implementation alternatives that maximize throughput, especially by overlapping server download/transcode and device writes, while preserving correctness and device compatibility.

### Context Guidance

The initial observation is that HifiMule appears to copy each item through two non-overlapped phases: fetch raw or encoded media from the server, then write the resulting file to the device. End-to-end throughput therefore trends toward the average of both phases rather than the maximum throughput possible when reads and writes are pipelined.

### Session Setup

We will explore alternatives that improve sync speed by increasing overlap, reducing idle time, and respecting device constraints such as MTP/WPD behavior, filesystem semantics, and limited random-write capability.

## Technique Selection

**Approach:** Progressive Technique Flow
**Journey Design:** Systematic development from exploration to action.

**Progressive Techniques:**

- **Phase 1 - Expansive Exploration:** First Principles Thinking + What If Scenarios for maximum idea generation.
- **Phase 2 - Pattern Recognition:** Constraint Mapping for separating real constraints from assumed constraints.
- **Phase 3 - Idea Development:** Solution Matrix for comparing architecture alternatives.
- **Phase 4 - Action Planning:** Decision Tree Mapping for choosing prototypes and validation steps.

**Journey Rationale:** Sync throughput depends on a chain of coupled stages: server fetch, optional transcode, temporary storage, device write, progress reporting, cancellation, and cleanup. A progressive flow lets us first widen the option space, then narrow it around what the device APIs and HifiMule architecture can actually support.

## Technique Execution Results

**First Principles Thinking + What If Scenarios:**

- **Interactive Focus:** Identify the smallest portable unit of work for pipelining HifiMule sync.
- **Key Insight:** Because HifiMule connects to multiple server types, currently Jellyfin and Navidrome with more possible later, the track is the universal unit that can survive backend differences.

**[Pipeline #1]: Bounded Track Producer/Consumer**
_Concept_: Treat each track as the portable pipeline item. Server-specific workers fetch or transcode tracks into a bounded staging queue, while device-specific workers consume completed staged tracks and write them to the device.
_Novelty_: This improves sync overlap without forcing all server integrations to expose identical chunk-level streaming semantics.

**[Staging #2]: Disk-Backed Track Queue**
_Concept_: Stage fetched or transcoded tracks as temporary files on disk before device write. The queue is bounded by track count, byte size, or both, keeping HifiMule memory usage predictable while letting the operating system page cache accelerate reads from recently written staged files.
_Novelty_: This avoids treating disk as merely an overhead; it delegates memory pressure and caching behavior to the OS, which is usually better equipped to balance file cache against application memory.

**[Lifecycle #3]: Temp-Only Staging**
_Concept_: Treat staged tracks as sync-run temporary artifacts and delete each staged file after a successful device write. Failed, cancelled, or interrupted syncs clean up their staging directory using normal temp-file lifecycle rules.
_Novelty_: This keeps the first throughput improvement focused on pipeline overlap instead of persistent-cache invalidation, storage management, or user-facing cache settings.

**[Concurrency #4]: Adaptive Producers with Per-Server Minimums**
_Concept_: Use an adaptive number of server-side producers, increasing prefetch/transcode concurrency when the device writer is starved and decreasing it when the staging queue is full, errors rise, or temp-byte limits are near. In multi-server syncs, keep at least one active producer per server so a slow or transcoding-heavy server does not block faster servers from feeding the shared device writer.
_Novelty_: This treats the sync plan as a multi-source pipeline rather than a single ordered stream, improving throughput while preserving fairness across Jellyfin, Navidrome, and future server integrations.

**[Ordering #5]: Priority Buckets with Ready-First Writes**
_Concept_: Partition planned work into semantic priority buckets, such as explicit playlist/basket tracks before Auto-Fill or lower-priority extras. Within each bucket, write whichever staged track is ready first, avoiding unnecessary blocking on slow servers or slow transcodes while preserving the broad intent of the sync plan.
_Novelty_: This balances speed and compliance: user-requested content keeps precedence, but the transfer engine does not sacrifice throughput to strict per-track ordering when order has little practical value.

**[Backpressure #6]: Hard Staging Cap with Clear Progress States**
_Concept_: Bound staging by bytes, track count, or both. When the cap is reached, producers wait for the device writer to free space; the UI/reporting distinguishes preparing or staging tracks from writing tracks to the device.
_Novelty_: This keeps the initial pipeline simple and predictable while making the new concurrent behavior understandable to users.

**[Granularity #7]: Completed-File Writes Only**
_Concept_: Device writes consume only fully staged track files. HifiMule does not stream partially downloaded/transcoded tracks into the device writer in the first implementation.
_Novelty_: This preserves portability across WPD, MTP, filesystem-style devices, and server integrations while still gaining cross-track overlap.

**Constraint Mapping:**

**[Constraint #1]: Single Device Write Lane**
_Concept_: Most target devices have low write performance and poor tolerance for concurrent writes, so the device side should write one file at a time. The throughput goal is not parallel device writes; it is keeping the single writer continuously fed by avoiding pauses between files for download or encode work.
_Novelty_: This reframes the optimization around eliminating writer starvation rather than maximizing raw concurrency.

**[Constraint #2]: Moderate Fixed Cap for V1**
_Concept_: Start with a moderate fixed staging cap, such as a small number of tracks and/or a bounded byte budget, instead of adaptive staging limits. Collect queue depth, writer idle time, staging wait time, and temp-byte metrics to decide whether adaptive caps are justified later.
_Novelty_: This creates a measurable path from simple v1 behavior to possible v2 adaptation without guessing up front.

**[Failure #8]: Retry Once, Then Stop on Second Write Failure**
_Concept_: If a device write fails, retry the write once before aborting the sync on another failure. Producers should stop accepting new work, staged temp files should be cleaned up, and the sync result should preserve enough detail to explain the failed track and retry outcome.
_Novelty_: This policy handles transient device flakiness without risking a long sequence of partial or inconsistent writes when the device is unstable.

**[Failure #9]: Bucket-Specific Producer Failure Policy with Backend Hooks**
_Concept_: If a producer fails to download or transcode a track, retry once, then apply the priority bucket's policy. Explicit user-requested playlist or basket content can stop the sync, while Auto-Fill content can skip, replace, or continue once the fill target has already been satisfied. Server adapters may provide backend-specific fallbacks, such as trying original quality after a Jellyfin transcode failure.
_Novelty_: This treats source failures as content availability issues rather than device-health failures, which keeps Auto-Fill resilient without weakening explicit sync guarantees.

**[Progress #10]: Device-Centric UI with Pipeline Diagnostics in Logs**
_Concept_: The normal UI should use device-written progress as the primary measure, with a secondary status line for staging state such as preparing tracks, staged tracks ahead, waiting for server, or waiting for device. Detailed queue depth, producer count, writer idle time, staged bytes, and per-stage timing belong in logs or diagnostics.
_Novelty_: This makes the user-visible progress match what is actually on the device while still exposing enough detail to tune and debug the pipeline.

## Solution Matrix Outcome

**Selected V1 Architecture:** Adaptive per-server producers with a single device writer.

**Rejected for V1:**

- Chunk streaming pipeline: too dependent on device/backend capabilities.
- Persistent transcode cache: introduces cache invalidation and storage management before the throughput win is proven.
- Parallel device writers: poor fit for low-performance devices and existing MTP/WPD serialization.

**Fallback V1 Option:** Single producer plus single writer if adaptive producer scheduling is too large for the first slice.

## Implementation Direction

**Current code shape observed:**

- `execute_provider_sync` in `hifimule-daemon/src/sync.rs` currently buffers the provider HTTP stream, then calls `device_io.write_with_verify`.
- `DeviceIO` in `hifimule-daemon/src/device_io.rs` accepts whole-file byte slices for writes.
- `MtpBackend` already serializes device operations with an internal operation lock, matching the single-writer constraint.

**Proposed V1 pipeline:**

```text
SyncDelta.adds
  -> priority buckets
  -> adaptive per-server producer tasks
  -> temp-only disk staging directory
  -> bounded ready queue
  -> single device writer task
  -> manifest/playlists/progress update
  -> temp cleanup
```

**Core types to introduce:**

- `StagedTrack`: add item metadata, staged temp path, byte size, resolved output suffix/content type, destination relative path, timings.
- `StageQueueConfig`: max staged tracks, max staged bytes, initial producers per server, max producers per server.
- `PipelineMetrics`: producer blocked time, writer idle time, queue depth, staged bytes, fetch/transcode duration, write duration.
- `PipelineBucket`: explicit playlist/basket, replacement/quality update, Auto-Fill.

**First implementation slice:**

1. Add disk staging for `execute_provider_sync` but keep producer count at 1.
2. Replace in-memory `buffer_stream(...)->Vec<u8>` with streaming to a temp file.
3. Device writer reads the completed temp file into bytes for the existing `write_with_verify` API.
4. Delete the temp file after successful write or cleanup on failure/cancel.
5. Preserve current progress semantics, with logs for staging/write timings.

**Second slice:**

1. Split staging and writing into producer/writer tasks with a bounded queue.
2. Keep one device writer.
3. Add hard cap by staged track count and staged bytes.
4. Measure writer idle time and producer blocked time.

**Third slice:**

1. Group adds by `server_id`.
2. Keep at least one producer per server.
3. Allow adaptive producer count inside server bounds.
4. Use priority buckets with ready-first writes inside each bucket.

**Validation goals:**

- Device writes remain serial.
- After warm-up, writer idle time approaches zero when producers can keep up.
- Staged bytes never exceed the configured cap.
- Cancellation cleans temp files and stops producers.
- A write failure retries once, then aborts on another write failure.
- Auto-Fill producer failures can skip/replace once target goals are satisfied.

## Refactoring Opportunity

Use the pipeline work as an occasion to separate source reading from destination writing.

**Source readers:**

- Jellyfin reader: resolves download/transcode URL, applies Jellyfin-specific transcode fallback behavior, streams bytes into a staged temp file.
- Navidrome/Subsonic reader: resolves signed download or stream URL, applies Subsonic/OpenSubsonic format and bitrate parameters, streams bytes into a staged temp file.
- Future provider readers: implement the same staged-track contract without leaking backend-specific URL/transcode behavior into the sync executor.

**Destination writers:**

- Filesystem writer: optimized for MSC/local filesystem devices, can eventually support direct file-copy style APIs.
- MTP/WPD writer: keeps single-file write semantics, honors WPD shell-copy/libmtp behavior, retries write once, verifies according to backend capability.
- Future destination writers: implement the same completed-staged-file contract.

**Proposed boundary:**

```text
TrackReader::stage(track, profile, temp_dir) -> StagedTrack
TrackWriter::write(staged_track, destination) -> WrittenTrack
```

**Why this matters:**

- The producer side becomes provider-specific but device-agnostic.
- The writer side becomes device-specific but provider-agnostic.
- The pipeline scheduler only coordinates staged tracks, priorities, caps, metrics, cancellation, and retry policy.
- MTP/WPD limitations do not contaminate Jellyfin/Navidrome reader logic.
- Jellyfin/Navidrome transcode/download differences do not contaminate filesystem/MTP writer logic.
