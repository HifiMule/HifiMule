# HifiMule 0.20.0

Release date: 2026-10-24

## Highlights

- **Encode on this computer**: Choose local sync encoding per server and convert original audio to your device's format without relying on server transcoding.
- **Custom device profiles**: Create, clone, and edit profiles in the app, with controls for supported formats, conversion quality, media folders, and folder artwork.
- **Folder artwork**: Add covers and portraits to music, audiobook, and podcast folders, with optional desktop folder icons.
- **M4B audiobook presets**: Preserve compatible M4B originals with new presets for modern DAPs, Rockbox, PocketBook, and Android / Voice, plus expanded Garmin support.
- **Library audio audit**: Check local audio with the bundled `mediacheck` tool; AC3 sources join the formats supported for playback, local conversion, and auditing.

---

## Added

### Local sync encoding

- Choose **Server (default)** or **This computer** under **Sync encoding** in each server's edit dialog. The choice applies to manual and automatic syncs, including baskets combining multiple servers.
- Download original audio and convert locally only when the selected device profile requires it. Supported outputs include MP3, AAC, FLAC, ALAC, Vorbis, Opus, and WAV, subject to the profile's container and audio limits.
- Convert Audiobookshelf books and podcast episodes for devices that cannot play their originals. Compatible MP3 and AAC/M4B files can copy directly under profiles that permit them; M4B is preserved as an original format rather than produced by conversion.
- Overlap downloads, local encoding, and device writes. Encoding concurrency and staged-track capacity default to automatic sizing from available CPU threads, with independent overrides in `config.json`.
- Preserve source tags and attached cover artwork in supported output containers, validate completed output before transfer, and report encoding failures with source diagnostics. Logs include download, encoding, staging, and device-write timings.

### Device profile manager

- Open **Device profiles** from the native menu without a connected device, or access New/Edit actions from device setup and Device Settings.
- Create a custom profile, clone a bundled or custom profile, and edit custom profiles through a structured editor. Bundled system profiles remain available as read-only starting points.
- Set supported formats, original-copy or conversion policy, output format and bitrate, and sample-rate, bit-depth, channel, and original-bitrate limits.
- Set default music, playlist, audiobook, and podcast folders, plus whether new devices should create folder artwork. Rockbox presets enable folder artwork by default; individual devices can override the choice.
- Keep device-setting drafts when creating or selecting a profile. Shared-profile audio changes are detected on later syncs, including after a device reconnects; changing names or folder defaults alone does not require audio replacement.

### Folder artwork and desktop icons

- Enable **Add thumbnail** during device setup or in Device Settings to export `folder.jpg` previews bounded to 500 pixels, preserving aspect ratio.
- Use artist portraits and album covers for music, author portraits and book covers for audiobooks, and show covers for podcasts. Artwork is created alongside audio transfers; use Force Sync to populate missing previews on otherwise unchanged files.
- Add optional Windows Explorer, macOS Finder, KDE, or GNOME Files folder icons. Native activation depends on the host and transport; MTP devices receive JPEG previews without desktop icon adapters.
- Reuse previously exported artwork and preserve existing user images and custom icons. Artwork failures do not stop audio sync, and cleanup removes only unchanged HifiMule-owned artwork when its final managed descendant is removed.

### M4B audiobook and podcast presets

- Add **Modern DAP — Lossless + M4B Audiobooks**, **Audiobooks / Podcasts — Preserve M4B**, **Rockbox — M4B Audiobooks**, **PocketBook — M4B Audiobooks**, and **Android / Voice — M4B Audiobooks** presets.
- Preserve verified Audiobookshelf AAC/M4B originals with their `.m4b` extension when the target profile explicitly supports it. Direct-copy audiobook tagging preserves embedded chapters and audio.
- Separate original-copy bitrate ceilings from conversion targets so speech presets can retain compatible originals while converting incompatible audio to compact MP3 96 kbps. Audiobookshelf conversion requires **This computer** encoding.
- Expand **Garmin Music Watch** with explicit AAC/M4B support and separate `Audiobooks` and `Podcasts` folder defaults. M4B recognition, AAC variants, chapter navigation, and resume behavior depend on the device, firmware, and playback app.

### Library auditing and AC3 audio

- Bundle the `mediacheck` command-line tool with the application to recursively audit a local library without a server connection or a running HifiMule session.
- Fully decode supported files, leave originals unchanged, and write a report distinguishing healthy, recoverable, and bad sources, with progress, final counts, and exit codes for automation.
- Support quoted Windows UNC and mapped-drive paths, skip filesystem redirects and AppleDouble sidecars, and report inaccessible subtrees as incomplete audits.
- Support AC3 source playback, local conversion to supported target formats, and auditing through the bundled audio runtime. Known multichannel speaker layouts can downmix for playback; unknown layouts are rejected rather than guessed.

### Optional sync reconciliation

- Choose **Reconcile files before syncing** during device setup or in Device Settings when other applications also manage the device's files. It defaults off for new devices and existing devices without a saved preference, avoiding the extra scan on ordinary syncs.
- Keep the optional initial device-import scan independent of this setting. Enabled reconciliation logs reused, unmatched, ambiguous, and incompatible file counts across manual and automatic syncs.

---

## Changed

- Show expected encoded file size separately from the safety reserve in the basket. Converted-track reservations use one percent of the estimated payload plus 64 KiB per track; capacity checks and Auto-Fill retain the full reservation, while direct copies and measured reused files need no extra reserve.
- Organize music folders by album artist and name newly written music files with disc and track numbers, such as `1-01 Title.flac`, using disc 1 when no valid disc number is supplied.
- Keep distinct music tracks in separate files when their destination names collide. Add a stable hash suffix only to colliding filenames and retain existing assignments across later syncs.
- Validate numeric settings inline in device profiles, Auto-Fill, and playback selection, with allowed-value hints and focus on the first invalid field when saving.
- Make sync result messages expandable and selectable, show long time estimates in hours and minutes, and reconnect sync monitoring after transient daemon errors.

---

## Fixed

- Preserve Jellyfin playlist entry order when expanding playlists for sync.
- Admit supported Jellyfin AAC and ALAC playback when the server describes their MP4 container with an equivalent alias.
- Preserve tray navigation when handing off to an already running UI, and show a playlist's albums before an artist is selected in playlist curation.

---

## Internal

- Consolidate shared sync preparation, provider expansion, Auto-Fill budgeting, playback actions, and lifecycle handling; remove obsolete helpers, models, RPC adapters, and unused dependencies.
- Extend the controlled audio runtime and packaging for local encoding, AC3, and `mediacheck`, including third-party audio notices and more resilient build downloads.
- Add daemon, CLI, lifecycle, UI, and packaging regression coverage, update both JavaScript suites in CI, and translate the new settings and profile controls into English, French, Spanish, and German.
