# HifiMule 0.18.0

Release date: 2026-10-12

## Highlights

- **Large-library syncs**: Reduce memory retained during transfers and batch device metadata checkpoints instead of rewriting the full manifest after every file.
- **Smaller device manifests**: Store synced-file metadata in a compressed, lossless format, with legacy JSON migration on successful writes.
- **Sync performance settings**: Tune copy buffers, temporary staging, and checkpoint frequency through `config.json`, with changes applied to the next sync without restarting.
- **Accurate Auto-Fill capacity**: Account for space reclaimed from managed files and keep Auto-Fill budgets separate from the size of your selected basket.

---

## Added

### Sync performance settings

- Configure `copy_buffer_kib`, `staging_max_tracks`, `staging_max_mib`, `checkpoint_track_updates`, and `checkpoint_interval_seconds` in the optional `sync_performance` section of `config.json`.
- Apply one validated settings snapshot to each manual or automatic sync. Missing or invalid fields use their defaults, and changes during a transfer apply to the next run.
- Log effective settings, staging and device-write rates, queue waits, and checkpoint reasons and durations to help identify transfer bottlenecks.

### Passthrough profile confirmation

- Add an explicit Device Settings action for eligible mass-storage devices to confirm that existing files match the selected passthrough profile, clearing a stale profile-change flag without copying or deleting audio.
- Require confirmation and a complete recovery scan; reject the action during active syncs, with interrupted-sync evidence, or after the connection or managed folders change.

---

## Changed

- Large syncs retain less transfer metadata in memory, and regular UI polling uses compact device summaries instead of full per-track manifest responses.
- By default, checkpoint accumulated track metadata after 1,000 updates or ten minutes, whichever comes first, and flush remaining updates at finalization. Initial dirty and pending recovery state is still persisted before device files change.
- Store device manifests as `.hifimule.json.zst` using column JSON and checksummed Zstd compression. Preserve track records, device settings, basket contents, and recovery state when migrating legacy JSON on successful writes.
- Increase the default mass-storage copy buffer from 8 KiB to 1 MiB; MTP/WPD transfers keep their native copy behavior.
- Allow automatic syncs involving large managed-file cleanup once at least 24 hours have passed since the last successful sync. Recent successful syncs retain the cleanup cooldown.

---

## Fixed

- Correct basket capacity bars, over-capacity warnings, and Auto-Fill budgets by including reclaimable managed-file space and avoiding double-counting existing selections or treating Auto-Fill ceilings as required space.
- Preserve manual versus Auto-Fill origin through force syncs, missing-file recovery, and server ID changes so later capacity calculations remain consistent.
- Treat an unset profile and explicit passthrough as equivalent when saving clean device settings, avoiding unnecessary full-library replacement. Retain uncertain profile changes until successful sync or explicit confirmation establishes the baseline.
- Record the profile actually used by a successful automatic sync, and reject invalid selected profiles before execution.
- Exclude Windows optical drives from manifest discovery so an empty CD/DVD drive no longer produces a misleading device read failure.
- Retry transient Windows manifest replacement locks within a bounded window, retaining the previous manifest and recovery evidence if publication fails.

---

## Internal

- Spool prepared sync plans to temporary SQLite storage and stream operations in bounded batches, with summary responses, paged details, and single-use plan IDs checked against device and configuration changes.
- Bind deferred checkpoints to their original physical connection and drain pending persistence before operation completion or shutdown.
- Add a read-only `scripts/inspect-manifest.mjs` tool for inspecting legacy and compressed manifests as ordinary JSON.
- Expand sync-plan, capacity, checkpoint, compression, migration, recovery, and performance-setting regression coverage, with memory and manifest-size measurement tools and fixtures.
- Update sync performance, API, data-model, architecture, and development documentation, and translate the profile confirmation controls.
