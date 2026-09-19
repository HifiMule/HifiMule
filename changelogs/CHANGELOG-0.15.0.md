# HifiMule 0.15.0

Release date: 2026-09-20

## Highlights

- **Desktop playback**: Play tracks and albums from your configured music servers directly in HifiMule, with pause, resume, stop, seek, Back, Next, and a persistent floating playback bar.
- **Listening queue and previews**: Build and edit an upcoming queue, preview a full track without losing your main session, and return to the music you were already hearing.
- **Background listening**: Audio and other background work continue when the window closes, while reopening the app reconnects to the same daemon and current playback state.
- **Cross-platform audio**: Select a shared audio output, use supported operating-system media controls, and run playback through the controlled FFmpeg runtime packaged for Windows, macOS, and Linux.
- **Reliable listening sessions**: Queues and positions are checkpointed for a deliberately paused restore, with safe shutdown coordination alongside device syncs.

---

## Added

### Desktop playback

- Play an individual library track or an album through the background daemon without exposing authenticated stream URLs to the UI.
- Play albums in disc and track order, advance automatically, and use Next or Back without rebuilding the queue. Back restarts the current track after three seconds or returns to the preceding occurrence near the beginning.
- Pause, resume, stop, and seek from the floating bar. Seeking is enabled only for source and format combinations qualified by the playback runtime, including supported Jellyfin originals and Navidrome raw streams.
- Choose a shared audio output and preserve the current position when switching successfully. If the selected output disappears, playback pauses instead of silently moving to another speaker.
- Control supported playback actions through Windows media controls, macOS media commands, Linux MPRIS, and the tray while the main window is closed.
- Preserve album-relative loudness with a consistent, peak-protected gain when trustworthy ReplayGain metadata is available, with unity gain as the safe fallback.

### Queue, Preview, and session continuity

- Add one track or a multi-selection to the listening queue, reorder or remove upcoming occurrences, and keep deliberate duplicate entries distinct.
- Open the always-available **Playback** destination to review the current track, upcoming queue, and paged listening history without changing a physical-device basket.
- Preview a complete track in a separate audition, then return to the preserved main queue and its prior playing or paused intent.
- Restore the queue and last checkpointed position after relaunch in a paused state, including when the source server is temporarily unavailable.

### Playback interface

- Keep transport, timeline, output, source, and recovery controls available in a floating playback bar while browsing the library or working with a device basket.
- Use localized playback controls and status messages in English, French, Spanish, and German, with keyboard operation, visible focus, and restrained live-region announcements.

---

## Changed

- Closing the main window now leaves the single background daemon running, so playback and ongoing work survive until you explicitly quit HifiMule.
- Explicit Quit now coordinates playback checkpointing and active device-sync cancellation, waits for safe write boundaries, and reports delayed or failed shutdown instead of silently forcing an unsafe exit.
- Playback is the deterministic destination when no physical device is selected or a selected device is removed; device-only basket and sync actions remain restricted to physical devices.
- The library browse-mode bar now uses compact icon-and-label controls with responsive wrapping, localized accessible names, and more room for library content.
- Daemon builds now use the platform-aware build wrapper and a pinned, validated FFmpeg runtime with packaged native dependencies and third-party notices.

---

## Fixed

- Reopening the UI no longer starts a competing daemon or loses authoritative playback, queue, sync, or shutdown state.
- Playback destination navigation no longer hides access back to the library or applies physical-device operations to the listening queue.
- Preview audio now starts after the selected output opens, while paused previews remain silent until explicitly resumed.
- Album starts, seek behavior, output replacement, and Back operations now reject stale asynchronous work so an obsolete decoder or UI command cannot take over the current session.
- Playback source credentials are resolved for the track's own provider, preventing browsing a different server from redirecting playback or device operations.
- Linux PulseAudio seeking builds correctly, and Windows CI shutdown tests synchronize on the actual shutdown fence instead of scheduler timing.

---

## Internal

- Added versioned playback persistence for session state, queue revisions, output preferences, previews, album gain policy, and replay-safe listening attempts, with transactional migration and corruption recovery.
- Added bounded HTTP streaming, decode, PCM handoff, queue paging, successor preparation, command deduplication, and generation fencing across playback mutations and native output workers.
- Added dedicated CoreAudio, WASAPI, and PulseAudio output handling, including endpoint identity, loss detection, acknowledged switching or retirement, and bounded server-buffer diagnostics.
- Added deterministic audio fixtures and regression coverage for WAV, FLAC, MP3, AAC/ALAC in M4A, Opus, Vorbis, AIFF, and WMA paths, plus seek, continuity, queue, lifecycle, accessibility, and packaging contracts.
- Added architecture-specific release evidence, runtime verification, smoke-test tooling, and candidate-build contracts for Windows x64, macOS x64/ARM64, and Linux x64 artifacts.
