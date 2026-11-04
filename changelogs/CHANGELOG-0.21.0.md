# HifiMule 0.21.0

Release date: 2026-11-04

## Highlights

- **Rockbox database maintenance**: Build and update your player's music database on the computer after sync, preserving existing playback history.
- **Navidrome rating sync**: Send Rockbox rating edits back to Navidrome and fill unrated device tracks from their original server account.
- **Incremental database updates**: Reuse local transfer metadata and unchanged indexed tags, then replace only database files whose contents change.

---

## Added

### Rockbox database maintenance

- Enable **Maintain database after sync** in Device Settings or set defaults in the device profile editor. Maintenance runs after successful manual or automatic syncs, including runs with no audio transfers; it starts disabled.
- Create databases for Rockbox 3.15 or 4.0 and retain the format and byte order of existing databases. Use mounted mass storage with Rockbox installed in the standard `.rockbox` location; MTP and custom database locations are unsupported.
- Configure the firmware-visible path prefix, with guidance for finding it under **System → Rockbox Info**. Preserve indexed tracks outside HifiMule's ownership and entries on other storage volumes.
- Preserve ratings, play counts, play time, last-played values, and resume fields for retained tracks, including unambiguous moves of the same source track.
- Validate database updates before publication, back up affected files, and recover interrupted writes on the next enabled sync. Unknown or corrupt databases remain unchanged, and recovery reports externally modified files instead of overwriting them.
- Report missing or unreadable audio by path while retaining existing entries and history for retry. Bound newly scanned text tags to Rockbox's limit with warnings, and support leading ID3 tags on RIFF/WAVE audio without modifying source files.

### Rockbox and Navidrome ratings

- Enable **Sync ratings when supported** independently of database maintenance. Navidrome is currently the supported provider; ratings-only sync updates an existing database without scanning audio metadata or creating a missing database.
- Read device rating edits before media changes and send them to the original server account. Rockbox ratings 1–10 map to Navidrome ratings 1–5, rounding upward; favorites remain separate.
- Fill unrated device tracks from Navidrome, including newly indexed tracks in the same sync, while preserving positive device ratings and deliberate clears. Enable **Gather runtime data** in Rockbox to store device ratings.
- Keep failed or uncertain edits pending for later syncs, prevent server backfills from echoing back as device edits, and avoid treating a recreated database's initial zero ratings as server-rating clears.

### Setup and troubleshooting guides

- Add guides for Rockbox database setup, rating behavior, recovery, and maintenance diagnostics, plus documentation for collecting issue-report logs and sharing device profiles.

---

## Changed

- Probe newly transferred audio locally after encoding and tag edits, then reuse its metadata only after a successful device write and manifest update. Resident tracks needing metadata still fall back to device scanning.
- Retain unchanged managed tracks' indexed tags after filesystem and ownership checks, avoiding repeated audio metadata reads.
- Back up and replace only changed Rockbox database files. Unchanged databases retain their bytes, commit counter, and RAM snapshot; ratings-only updates leave string files untouched.

---

## Fixed

- Load the Device profile manager on demand consistently from both native menu events and startup activation, removing the conflicting eager import.

---

## Internal

- Add Rockbox database and rating state storage, provider rating capabilities, and regression coverage for both database formats, both byte orders, preservation, and interrupted-write recovery.
- Add localized Rockbox controls and maintenance diagnostics covering metadata reuse, device probes, phase timings, and changed database files.
- Upgrade Rust and JavaScript dependencies and audio-runtime libraries, pin Rust and Node toolchains, and use npm consistently in release builds.
- Move Windows audio-runtime builds to MSYS2 UCRT64 and make static library selection explicit; clean up warnings after dependency upgrades.
