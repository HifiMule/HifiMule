# HifiMule 0.13.0

Release date: 2026-07-13

## Highlights

- **Faster sync throughput**: A new bounded producer/writer pipeline overlaps downloading and transcoding from your servers with writing to the device, so the device writer spends far less time sitting idle between tracks.
- **Fair multi-server syncs**: Per-server producers and priority buckets keep one slow server from blocking ready tracks, and explicit playlist/basket content is written before Auto-Fill filler.
- **One sync engine for every provider**: Jellyfin now runs through the same provider pipeline as Navidrome/Subsonic, so all servers get the same staging, fairness, and diagnostics.
- **Read/write speeds you can see**: Sync now reports average read and write speeds in the UI and in readable, timestamped logs.

---

## Added

### Sync throughput pipeline (Epic 14)

- Provider downloads and transcodes now stream into bounded, temp-only staging files before the device write, instead of buffering whole tracks in memory.
- A dedicated producer/writer split prepares upcoming tracks while the current track is written, with the ready queue capped by both track count and byte size.
- Per-server producers group work by portable `server_id` so at least one producer can run per server, and priority buckets write explicit playlist/basket content ahead of Auto-Fill content.
- The device writer stays single-lane and serial, and staging files are always cleaned up after success, failure, or cancellation.

### Sync speed reporting

- Sync now displays average read and write speeds in the basket sidebar (`Read {read} MB/s - Write {write} MB/s`), translated across English, French, Spanish, and German.
- Provider sync logs a weighted average throughput line per server producer and one for the serial writer, using the existing `duration(MB/s)` format.

---

## Changed

- Jellyfin sync (both manual and auto-sync) now routes through `execute_provider_sync`, the Epic 14 pipeline, so it benefits from staging, per-server fairness, and pipeline diagnostics like every other provider.
- Daemon and UI log entries now use a readable, sortable local timestamp (`[YYYY-MM-DD HH:MM:SS]`) instead of raw Unix epoch seconds.

---

## Fixed

- Auto-Fill selections now keep their provider track number, so synced filenames use the correct `NN - Title` prefix instead of falling back to `00 - …`.
- Playlists are now rewritten when the transcoding profile changes, so `.m3u` track paths match the new file extensions after a profile switch.
- The UI startup probe now uses the daemon's dedicated `daemon.health` method, so an already-running daemon is detected reliably and the UI no longer launches a duplicate sidecar.
- Auto-sync no longer tries to resolve the virtual `__auto_fill_slot__` basket marker (bare or server-scoped) as a real media item, which previously caused Jellyfin to return HTTP 400.
- The basket sidebar repaint glitch is fixed.

---

## Internal

- Legacy `execute_sync` and its path-private helpers and tests were removed; `execute_provider_sync` is now the single file-transfer implementation.
- New regression coverage for the provider pipeline: staging, backpressure/queue bounds, cancellation cleanup, weighted per-server and writer throughput metrics, Jellyfin dispatch routing, timestamp formatting, and the auto-fill slot-marker predicate.
- The i18n catalog gained the `basket.sync.average_speeds` string across all supported locales.
