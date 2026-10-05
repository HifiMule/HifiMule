# HifiMule 0.17.0

Release date: 2026-10-05

## Highlights

- **Radio mode**: Start a continuously replenished listening session from your music library, an artist, a genre, or a playlist, with related-artist discovery and controls for selection priorities.
- **Fedora support**: Linux releases now include an x86_64 RPM for Fedora 44, with bundled audio libraries and guidance for enabling the tray on GNOME.
- **Keep your discoveries**: Save a listening snapshot, export it as playlists on its source servers, or add it to a connected device's basket.
- **Smoother listening**: Playback gains track-boundary quality adaptation for eligible Navidrome streams, protection during device sync, and more reliable audio-output startup.

---

## Added

### Radio mode

- Start Radio from the playback bar or tray without first configuring selection sources. HifiMule uses the current music server, or the first available music server when the current server is not a music library.
- Start artist, genre, or playlist Radio directly from library cards and rows.
- Configure sources across music servers using searchable playlist, artist, and genre pickers, and choose priorities such as random, favorites, play count, date added, quality, excavation, rediscovery, and rarity.
- Automatically replenish a small upcoming queue, keeping manual queue edits under the listening session's control.
- Continue through provider-supplied artist connections when eligible tracks are exhausted, with a fresh starting point when no usable connection remains within the selected scope.
- Avoid repeated copies of the same recording across servers when reliable recording metadata identifies them, while retaining distinct performances and versions.
- Apply track loudness metadata to Radio tracks when available.
- Expose supported source-server preferences during playback: Like, Dislike, and clear on eligible Jellyfin sources; Like and clear on eligible Navidrome sources.
- Report listening to supported source servers and show delivery status in the playback controls.

### Saved listening snapshots

- Save an immutable local snapshot of the session's accepted history and upcoming queue, then browse saved snapshots and their tracks.
- Export a snapshot as source-specific playlists on eligible servers, with per-server results and recovery controls for partial or uncertain operations.
- Add a snapshot to a connected device's basket or replace that basket after reviewing and confirming the export plan.

### Fedora and Linux desktop integration

- Package an x86_64 RPM for Fedora 44 alongside the existing DEB and AppImage bundles, including the UI, daemon, private audio libraries, manifest, and notices.
- Declare Fedora runtime dependencies and recommend the optional `gnome-shell-extension-appindicator` package.
- Show dismissible GNOME tray setup guidance when the indicator host is confirmed missing, with an extension link and a Check again action.
- Register the packaged daemon to start at graphical login for RPM and DEB installations. A per-user autostart override can disable it.

---

## Changed

- Each new Radio session uses fresh randomness and prefers an alternative to the previous starting track or a confidently identified copy when one is playable.
- Full-library Radio startup ranks candidates across bounded pages and allows more retrieval time for large libraries.
- Eligible Navidrome playback can choose between a qualified original and MP3 at 192 kb/s when preparing later tracks, using delivery and buffer evidence.
- Device sync applies bounded admission delays when current playback evidence indicates contention, while healthy or unrelated sync work keeps its normal scheduling.
- Long listening sessions use bounded history and queue persistence with recoverable checkpoints.

---

## Fixed

- Restore Wayland mouse input through the updated Linux WebView dependencies and provide minimize, maximize, and close controls in the Linux window decoration.
- Initialize a fresh playback configuration with an available default audio output and restore a saved output after discovery, reducing startup failures caused by unfinished output detection.
- Resolve Jellyfin artist selection through stable provider identifiers and show source names in the Radio pickers instead of requiring raw identifiers.

---

## Internal

- Add persistent Radio state, recording identity, listening-report and preference journals, snapshot storage, and export recovery records.
- Log Radio candidate selection, preparation, refill, and artist-transition decisions in `daemon.log`.
- Extend build and release workflows to extract RPM artifacts and verify their private native audio dependency closure.
- Align the Tauri JavaScript API and CLI with the Rust runtime and update Fedora build dependencies.
- Expand playback, provider, persistence, export, Linux packaging, tray guidance, and UI regression coverage, with network, sync coexistence, and long-session evidence tools.
- Extend translations and refresh playback API, data-model, development, deployment, and release documentation.
