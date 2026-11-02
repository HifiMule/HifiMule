# Rockbox database and ratings

For data origins, a complete integration file inventory and diagrams, see
[Rockbox tagcache: data sources and files](./rockbox-tagcache-data-flow.md).

Device settings and the profile editor include a **Rockbox database** section.
Both options start disabled. Profile defaults apply during device setup; they do
not replace preferences on an existing device.

- **Maintain database after sync** creates or updates the Rockbox tagcache after
  successful manual or automatic media synchronization. It also works when no
  audio transfers are needed. A new database contains successfully synchronized
  HifiMule-managed audio. Existing indexed tracks outside HifiMule's ownership
  remain indexed, including tracks on another storage volume.
- **Sync ratings when supported** reads device edits before media changes and
  fills unrated device tracks from their original server/account at the end.
  This is independent of maintenance: ratings-only sync updates an existing
  database only when there are ratings to fill, without scanning audio metadata.
  It does not create a missing database. Tracks need an unambiguous manifest
  mapping and a provider supporting numeric ratings. Navidrome is currently the
  only supported server; other providers and untracked audio are skipped.

Use a mounted mass-storage connection and the standard `.rockbox` database
location. MTP and a custom Rockbox **database path** are unsupported. Install
Rockbox on the device first; HifiMule does not create its firmware directory.

Choose **Rockbox 4.0** or **Rockbox 3.15** for new databases. Existing databases
keep their format and byte order. Unknown versions and corrupt databases are
left unchanged. The path prefix is normally `/`; another storage volume may
need a firmware-visible prefix such as `/<microSD0>`. Existing paths retain
their paths only when they match the selected prefix. Find the volume's prefix
on the player under **System → Rockbox Info**. Select the mounted volume's
firmware-visible prefix explicitly: HifiMule does not guess another volume from
a matching filename suffix. Ordinary prefixes such as `/foo/bar` work as well.
Entries under other prefixes remain untouched.

## Ratings policy

Enable **Gather runtime data** in Rockbox so ratings are stored in the database.
Rockbox's positive ratings 1–10 become Navidrome ratings 1–5, rounding upward:
1/2 → 1, 3/4 → 2, 5/6 → 3, 7/8 → 4, 9/10 → 5.

An initial zero is treated as unrated and does not clear a server rating. After
a nonzero device rating has been observed, changing it back to zero clears the
Navidrome numeric rating, including when the positive import was uncertain.
Favorites are separate and remain unchanged.

When Rockbox has no rating and its original server has one, HifiMule copies it
to Rockbox: server 1–5 becomes 2, 4, 6, 8 or 10. Existing positive Rockbox ratings
are never overwritten. Newly indexed tracks can receive a rating in the same
sync. A deliberate device clear takes precedence and cannot be filled back in
that sync; an uncertain clear stays pending. A later server rating can fill a
stable unrated track on a subsequent sync.

When a database is recreated, its initial zero ratings do not clear previously
tracked ratings. Server ratings can fill the rebuilt entries during that sync.
If the server is unavailable, HifiMule remembers that those zeros came from
initialization and retries later, while retaining pending device edits.

Unchanged device ratings are not reapplied, so later edits in Navidrome survive.
New or changed ratings are read back to confirm success. Failed or uncertain
imports remain pending in HifiMule's local database, including ratings captured
before a track is removed from the device. Later enabled syncs retry the latest
pending value against the same source account. Switching accounts never
redirects another account's pending ratings.

Server backfills establish a synchronized baseline, so they are not echoed back
as device edits. HifiMule records intent before writing the device and reconciles
an interrupted write on reconnect. If publication evidence is ambiguous, it
leaves both ratings unchanged and reports a warning. Server lookup failures
also leave device ratings unchanged. Library ratings are fetched in bounded
pages, rather than making a request for every unrated device track.

## Preservation and recovery

Database maintenance preserves ratings, play counts, play time, last-played and
resume fields for retained tracks. A replacement of the same source track can
carry these values to its new path. Build metadata and file timestamps are
regenerated. Missing managed paths are removed only with recorded ownership;
HifiMule does not infer ownership from a folder or a track title.

Before publishing, HifiMule validates the complete database, saves prior files,
and journals replacement with the master index last. Handled errors roll back;
interrupted transactions recover on the next enabled sync. Publication spans
multiple files, so an interrupted device should reconnect to HifiMule before
being used in Rockbox. Recovery preserves an externally changed file and reports
the conflict rather than overwriting it.

Pending Rockbox commits or a nonempty changelog must be committed on the device
first. A stale RAM database snapshot is invalidated only during committed
replacement. HifiMule keeps separate managed-path evidence until database
publication succeeds, allowing deletion cleanup to resume after a cancelled
media sync. Database and rating failures appear in the normal sync result or
warnings; inspect those before ejecting.

Binary compatibility is tested with independent packed fixtures for both formats
and byte orders. Physical-device and live-server testing is separate evidence.

Existing metadata tags are preserved as raw bytes, including legacy text encodings.
HifiMule does not guess an encoding or replace undecodable characters. Firmware
paths remain UTF-8 and undergo the same path and ownership validation.

Newly scanned text tags are limited to 520 UTF-8 bytes, matching Rockbox's normal
tagcache text limit. Text ends at its first NUL, and truncation preserves complete
Unicode characters. Adjustments produce warnings with the track, field and byte
counts. Long podcast descriptions are shortened in the database copy; source
audio is unchanged. Existing indexed metadata retains its original bytes until a
managed track needs refreshing. Filename paths are never shortened.

Metadata scanning also supports ID3 tags prepended to RIFF/WAVE audio, including
files named `.mp3`. It reads the validated audio container and the leading tags
without rewriting the file. Unreadable or unsupported audio produces a per-track
warning containing the device-relative filename and the underlying error. Its
existing source entry and runtime history are retained; a new track stays
unindexed. Other tracks can still be updated. History for an unreadable moved
track remains available for a later successful retry.

Missing manifest-owned audio produces a warning naming the exact path. It stays
unindexed in a new database; an existing source entry and its history are retained
for retry. HifiMule never substitutes a similarly named file.
If the exact file returns, a later sync can index it and fill its rating from the
server, even if the database was already created while that file was missing.

Other I/O failures, conflicting source ownership and invalid database structure still
abort publication. Audio files are never
repaired or rewritten by database maintenance.
