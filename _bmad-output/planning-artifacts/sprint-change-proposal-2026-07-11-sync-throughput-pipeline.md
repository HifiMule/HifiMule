---
date: 2026-07-11
project: HifiMule
source: _bmad-output/brainstorming/brainstorming-session-20260711-174346.md
status: approved
scope: moderate
---

# Sprint Change Proposal: Sync Throughput Pipeline

## 1. Issue Summary

HifiMule's current provider sync path downloads or transcodes one track, buffers it, then writes that completed buffer to the device before starting the next track. The brainstorming session identified this as the main throughput bottleneck: server-side fetch/transcode and device write are serialized, so the device writer sits idle while the next track is prepared.

Evidence:

- `hifimule-daemon/src/sync.rs` routes provider sync through `execute_provider_sync`, which resolves a provider URL, buffers `response.bytes_stream()` through `buffer_stream(...)`, then calls `device_io.write_with_verify(...)`.
- `DeviceIO::write_with_verify(&self, path, data: &[u8])` accepts whole-file byte slices today.
- Architecture already requires all device writes to go through `DeviceIO`.
- MTP/WPD already serialize device operations, matching the proposed single-writer constraint.
- Story 4.2 originally required no local temporary files, but the current multi-provider/MTP reality makes temp-only disk staging the smaller correctness-preserving throughput improvement.

Problem statement: keep the device write lane continuously fed by overlapping source preparation with device writes, without introducing chunk-level device streaming, persistent cache invalidation, or parallel device writes.

## 2. Impact Analysis

### Epic Impact

Epic 4 is complete but its sync execution assumptions need amendment. Story 4.2's "no local temporary files" acceptance criterion now conflicts with the preferred throughput direction.

Epic 7 remains compatible. MTP/WPD hardening supports this change because it already treats device operations as serialized and fragile.

Epic 8 remains compatible. Provider-neutral sync should keep Jellyfin, Navidrome, Subsonic, and OpenSubsonic differences inside provider/source-reader behavior.

Epic 12 and Epic 13 are indirectly affected because Auto-Fill can generate larger sync jobs where throughput and fair multi-server staging matter.

Recommended backlog change: add a new Epic 14 rather than reopening Epic 4 implementation stories. Epic 4's foundation is done; this is a throughput evolution of the sync engine.

### Story Impact

Affected completed stories:

- Story 4.2: amend the no-temp-file requirement and preserve completed-file `DeviceIO` writes.
- Story 4.6: keep device-written progress primary; add staging status and diagnostics later.
- Story 4.9: provider-neutral transcoding/extension verification remains part of source staging.
- Story 7.1: keep single MTP write lane and retry policy aligned with MTP limits.
- Story 12.3: multi-server sync-time expansion becomes a future input to per-server producer fairness.

New proposed stories:

- Story 14.1: Temp-Only Disk Staging for Provider Sync
- Story 14.2: Bounded Producer/Writer Pipeline
- Story 14.3: Per-Server Producer Fairness and Priority Buckets

### Artifact Conflicts

PRD: Update the performance/sync requirements to allow temp-only staging and define throughput as overlapped source preparation plus serial device writes.

Architecture: Update the sync execution model. Add staged-track contract, single device writer, bounded queue, cleanup, cancellation, retry, and diagnostics.

UX specification: Add a secondary sync status line for staging states while keeping progress based on files/bytes written to the device.

Implementation artifacts: Add Epic 14 stories after approval. Do not change `sprint-status.yaml` until this proposal is approved.

### Technical Impact

The first slice should be intentionally small:

1. Replace provider-sync in-memory buffering with streaming to a temp file.
2. Read the completed temp file into bytes for the existing `DeviceIO::write_with_verify` API.
3. Delete temp files after successful write and clean the staging directory on failure or cancellation.
4. Add logs for stage duration, write duration, staged bytes, and cleanup.

Later slices can split producer and writer tasks. No new device-write API is required for the first slice.

## 3. Recommended Approach

Recommended path: Direct Adjustment with a new moderate Epic 14.

Rationale:

- It preserves the existing `DeviceIO` boundary and avoids rewriting MTP/WPD semantics.
- It removes Story 4.2's outdated "no temp disk" constraint where that constraint now blocks throughput.
- It gets a measurable win with one producer plus one writer before adding adaptive scheduling.
- It defers persistent cache, chunk streaming, and parallel device writers because they add correctness risk before proving the basic pipeline win.

Effort estimate: Medium.

Risk level: Medium. Main risks are temp cleanup, cancellation, disk-space cap behavior, and progress semantics. Device corruption risk stays low because writes still use `DeviceIO::write_with_verify`.

Timeline impact: Add one new epic with three incremental stories. No rollback recommended.

## 4. Detailed Change Proposals

### Story 4.2 Amendment

Section: Acceptance Criteria

OLD:

```text
Streaming Download Architecture: The sync engine MUST fetch files directly from Jellyfin's /Items/{id}/Download endpoint and stream the response body into memory buffers WITHOUT writing to intermediate temporary files on the local disk.
```

NEW:

```text
Provider Sync Staging Architecture: The sync engine MAY stream provider downloads/transcodes into temp-only local staging files before device write. Staging files are sync-run artifacts, bounded by configured limits, and MUST be deleted after successful device write or cleaned up on failure/cancel. Device writes still go through DeviceIO.
```

Rationale: Local temp staging is the smallest portable way to overlap provider preparation with device writes across Jellyfin, Navidrome/Subsonic, MSC, and MTP/WPD.

### PRD Update

Section: Sync performance / buffered I/O requirements

OLD:

```text
Sync is fast and does not consume local temporary disk space.
```

NEW:

```text
Sync favors sustained throughput by overlapping source preparation with serial device writes. The implementation may use bounded, temp-only local staging during a sync run. Staged files are not a persistent cache and must be cleaned up after success, failure, or cancellation.
```

Rationale: The original no-temp constraint conflicts with the throughput goal once provider transcoding and MTP/WPD device writes are both in scope.

### Architecture Update

Section: Sync execution model

ADD:

```text
Provider sync uses completed-track staging as the portable pipeline unit.

TrackReader::stage(track, profile, temp_dir) -> StagedTrack
TrackWriter::write(staged_track, destination) -> WrittenTrack

V1 keeps one device writer. Producers may prepare tracks ahead into a bounded temp staging directory. The writer consumes only completed staged files and writes through DeviceIO::write_with_verify. Staging is temp-only, not a persistent cache.
```

Rationale: This separates source reading/transcoding from destination writing without forcing a new chunk-streaming contract through all providers and devices.

### UX Update

Section: Sync progress

OLD:

```text
Progress displays current file, file count, percentage, and ETA.
```

NEW:

```text
Progress remains device-centric: files and bytes written to the device are primary. A secondary status line may show staging state such as "Preparing tracks", "2 tracks staged", "Waiting for server", or "Waiting for device". Detailed queue depth, producer count, staged bytes, writer idle time, and per-stage timings belong in logs or diagnostics.
```

Rationale: Users care what is safely on the device; pipeline internals should explain delays without making progress look complete before writes happen.

### New Epic 14: Sync Throughput Pipeline

Epic goal: Improve sync throughput by overlapping provider download/transcode work with serial device writes while preserving manifest correctness, MTP compatibility, cancellation behavior, and provider-neutral routing.

#### Story 14.1: Temp-Only Disk Staging for Provider Sync

As a user syncing larger libraries,
I want provider downloads/transcodes to stage as temporary local files before device write,
so that HifiMule reduces memory pressure and prepares the next track for later pipeline overlap.

Acceptance criteria:

1. `execute_provider_sync` streams each provider response to a temp-only staging file instead of buffering the network stream directly into memory.
2. The existing `DeviceIO::write_with_verify` API remains unchanged; the staged file is read into bytes immediately before write.
3. Staged files are deleted after successful write.
4. Sync failure and cancellation clean the run staging directory.
5. Logs include staging duration, write duration, staged byte size, and cleanup result.
6. Existing provider transcode/extension validation behavior is preserved.

#### Story 14.2: Bounded Producer/Writer Pipeline

As a user with a slow device writer,
I want HifiMule to prepare upcoming tracks while the current track is being written,
so that the writer spends less time idle between files.

Acceptance criteria:

1. Split provider sync adds into one producer task and one device writer task.
2. Producer emits completed `StagedTrack` items into a bounded ready queue.
3. Queue is capped by track count and byte count.
4. Device writes remain single-lane and serial.
5. Cancellation stops producer and writer and cleans staged files.
6. Metrics include writer idle time, producer blocked time, queue depth, and staged bytes.

#### Story 14.3: Per-Server Producer Fairness and Priority Buckets

As a multi-server user,
I want sync preparation to avoid one slow server blocking faster ready tracks,
so that multi-server and Auto-Fill syncs stay responsive while respecting user priorities.

Acceptance criteria:

1. Adds are grouped by portable `server_id`.
2. At least one producer can run per server when that server has pending work and capacity is available.
3. Priority buckets write explicit playlist/basket content before lower-priority Auto-Fill content.
4. Within a bucket, the writer may consume whichever staged track is ready first.
5. Source failure retries once; explicit content failure stops the sync, while Auto-Fill content may skip/continue according to bucket policy.
6. Device write failure retries once, then aborts the sync and cleans staging.

## 5. Checklist Results

- [x] 1.1 Trigger identified: brainstorming session on sync throughput, rooted in `execute_provider_sync` sequential source-read then device-write behavior.
- [x] 1.2 Core problem defined: technical limitation discovered after multi-provider/MTP architecture matured.
- [x] 1.3 Evidence gathered: sync code shape, `DeviceIO` whole-file API, MTP serialization, completed stories.
- [x] 2.1 Current epic assessed: Epic 4 can remain complete but needs an amendment.
- [x] 2.2 Epic changes defined: add Epic 14 instead of reopening Epic 4.
- [x] 2.3 Remaining epics reviewed: Epic 7, 8, 12, and 13 are compatible with minor implications.
- [x] 2.4 New epic needed: Sync Throughput Pipeline.
- [x] 2.5 Priority/order considered: implement after current completed foundation; Story 14.1 first.
- [x] 3.1 PRD conflict found: no-temp requirement should be replaced by bounded temp-only staging.
- [x] 3.2 Architecture conflict found: sync execution model needs staged-track pipeline contract.
- [x] 3.3 UX impact found: add secondary staging status, keep device-centric progress.
- [x] 3.4 Secondary artifacts: sprint-status and new story files after approval.
- [x] 4.1 Direct adjustment viable: Medium effort, Medium risk.
- [x] 4.2 Rollback not viable: completed work remains useful.
- [x] 4.3 MVP review not needed: core MVP remains intact.
- [x] 4.4 Path selected: Direct Adjustment with new Epic 14.
- [x] 5.1-5.5 Proposal components included.
- [!] 6.3 User approval pending.
- [!] 6.4 `sprint-status.yaml` update pending approval.

## 6. Implementation Handoff

Scope classification: Moderate.

Route to:

- Product Owner / Developer: approve proposal, add Epic 14 and three stories to sprint tracking.
- Developer agent: implement Story 14.1 first.
- Architect only if Story 14.2 exposes a need to change `DeviceIO` beyond whole-file writes.

Success criteria:

- Story 14.1 preserves current sync correctness and tests while replacing network buffering with temp staging.
- Story 14.2 shows reduced writer idle time on representative syncs without concurrent device writes.
- Staged bytes never exceed the configured cap.
- Failure and cancellation leave no orphaned staging files.
- Progress remains based on device-written work, with staging state shown only as secondary status/log detail.

## Approval Gate

Approved by Alexis on 2026-07-11. Sprint status may now add Epic 14 and the proposed story placeholders. Detailed PRD, architecture, UX, and story edits should be made by the routed implementation workflows.
