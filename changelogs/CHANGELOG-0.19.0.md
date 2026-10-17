# HifiMule 0.19.0

Release date: 2026-10-17

## Highlights

- **Recognize existing device music**: Scan a populated music folder during setup and import compatible matches into your basket without copying the audio again.
- **Profile-aware capacity**: Estimate transcoded output using your device profile and show the space occupied by synced Auto-Fill music in the capacity bar.
- **Audiobookshelf tags**: Apply current book metadata to downloaded MP3 and AAC/MP4 audiobook files so portable players display corrected titles, authors, and part order.
- **More accessible interface**: Improve keyboard navigation, focus retention, screen-reader feedback, and layouts for compact windows and enlarged text.
- **Music filename collisions**: Continue sync with one retained file when selected music tracks resolve to the same destination.

---

## Added

### Existing device music

- Offer a cancellable scan after configuring a device with a populated music folder. Match files against the captured active server using metadata and compatibility with the device's output profile.
- Import unambiguous compatible matches into the managed library and basket, grouping complete artists, then complete albums, then remaining tracks. Imported files become managed music subject to ordinary removal rules; unmatched, ambiguous, and incompatible files remain unmanaged.
- Recognize compatible files added later when preparing normal manual or automatic syncs, limited to the selected music. Reuse their actual paths and measured sizes; Force Sync replaces managed selected copies.

### Audiobookshelf audiobook metadata

- Tag newly transferred MP3 and AAC/MP4 (M4A/M4B) audiobooks with the current book title as album, authors as artist and album artist, part title, server track order, valid part total, and available publication year. Normalize disc numbering to 1/1 for book-wide ordering.
- Preserve existing optional values when the server omits them, along with audio packets, embedded artwork, chapters, and unrelated tags. Tagging does not re-encode audio or change server metadata or listening progress.
- Refresh tags on existing copies through Force Sync or an otherwise required replacement. Ordinary kept files, music, and podcast transfers retain their existing behavior.
- Log before-and-after tag values after a successful staging rewrite, including when no values change.

---

## Changed

- Use planned output duration and target bitrate, with overhead, for transcoding capacity estimates across the basket, prepared sync, and Auto-Fill. Use measured bytes for reused files and source sizes for direct copies; use conservative estimates when output size is uncertain.
- Show recorded synced Auto-Fill usage as a separate capacity-bar segment and label without changing reclaimable sync capacity.
- Improve keyboard controls, accessible labels, loading and status announcements, and focus handling across library cards, playlist curation, device setup, repair, and playback controls.
- Adapt dialogs and layouts to compact desktop windows and enlarged text, and provide reduced-motion alternatives for selection and basket animations.

---

## Fixed

- Stop cancelling music sync solely because an untracked file occupies a selected track's exact destination inside the configured music folder. Replace that destination and record ownership after success; unrelated files and path protections remain unchanged.
- Keep one file when distinct selected music IDs produce the same filename. Prefer a valid selected resident, otherwise the first successfully written output; preserve playlist references and cleanup behavior across retries and Force Sync. A shared filename does not imply identical recordings.
- Avoid rejecting mass-storage initialization because a protected child directory cannot be scanned: the initial accessibility probe now checks root entries without descending into children.
- Refresh the correct device basket after importing existing music, allow retry after refresh failure, and remove obsolete initialization buttons from the scan dialog.
- Preserve basket scroll position and keyboard focus through removals and refreshes.
- Harden asynchronous playback and Radio actions, server and login validation, repair recovery, library loading, and sync-status monitoring.

---

## Internal

- Lazy-load artwork and locale data, cache basket artwork, trim component imports, and limit playback timeline repainting to visible advancing sessions.
- Bundle Inter fonts locally and improve splashscreen initialization.
- Update translated interface messages, including device initialization and the library import title.
- Add daemon and UI regression coverage for device scans, output capacity, destination collisions, audiobook tag preservation, accessibility, and recovery; align CI browser harnesses with the updated controls.
- Refresh product, design, and Audiobookshelf implementation documentation.
