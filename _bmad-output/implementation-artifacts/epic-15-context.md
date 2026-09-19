# Epic 15 Context: Desktop Playback

Updated from the approved 2026-09-19 course correction. Source of truth: planning-artifacts/epics.md and sprint-status.yaml. Completed 15.1–15.14 remain done; 15.15–15.17 are backlog.

## Goal

Release daemon-owned manual listening across Windows, macOS and Linux: durable paused restoration, selected tracks and faithful albums, output safety, auditions, editable queue, persistent playing-bar transport including Back, compact library navigation, and verified installed packages. Radio and the remaining roadmap belong to Epic 16 and do not block this release.

## Stories

- Story 15.1: Close and reopen the UI without restarting the daemon
- Story 15.2: Quit safely while a device sync is running
- Story 15.3: Preserve and restore a paused listening session
- Story 15.4: Play a selected library track through the daemon
- Story 15.5: Choose an audio output and recover safely from disconnection
- Story 15.6: Control playback through the operating system with the window closed
- Story 15.7: Seek within a track and see the actual playback position
- Story 15.8: Play an album in order and advance through its tracks
- Story 15.9: Preserve album continuity across prepared track boundaries
- Story 15.10: Preserve an album's relative loudness with consistent gain
- Story 15.11: Preview a full track without losing the main listening session
- Story 15.12: Access Playback as an always-available destination
- Story 15.13: Edit the upcoming listening queue
- Story 15.14: Control listening from a floating playback bar across views
- Story 15.15: Restart the current track or return to the previous track
- Story 15.16: Compact the library browse-mode bar with icons and smaller labels
- Story 15.17: Ship verified playback builds for Windows, macOS and Linux

## Requirements & Constraints

One daemon owns authoritative playback independently of the UI. UI/native commands share admission and state; closing the UI preserves playback, Quit checkpoints and coordinates safe sync cancellation, and relaunch restores paused. Output loss pauses without rerouting or auto-resuming. Preserve provider routing, credential privacy and managed-device integrity.

Retain ordered album playback, recorded silence, supported prepared continuity and common album gain. Full-track Preview preserves the main queue/position; natural completion or explicit Return restores approved intent, while Stop restores paused. Queue edits preserve occurrence identity, accepted order and revision checks; long history is paged.

Back (15.15) uses daemon position: above 3,000 ms restart; at or below 3,000 ms previous retained occurrence, or current restart if none. Preview Back restarts only the audition. Preserve paused intent and output inhibition. Define replay identity, immutable outcomes, forward order, persistence and native Previous availability before coding. Support keyboard activation and native Previous/media delivery where available.

Compact navigation (15.16) covers the Tracks / Albums / Recently Added and other capability-driven browse modes, not transport or basket action buttons. Use icons plus smaller visible localized labels, reduce bulk and retain readable text, usable targets, full accessible names, selected state, keyboard focus and responsive reachability. Preserve existing browse/grid-list semantics and avoid unrelated global styling changes.

## Technical Decisions

Keep the existing Rust daemon, Tokio/native event loop, SQLite, provider abstraction, JSON-RPC/Tauri bridge and TypeScript/Shoelace UI. The current PlaybackControls component owns presentation, not audio/session state. Extend renderModeBar in library.ts for compact browsing.

Retain controlled native FFmpeg decoding and CPAL shared output with separately bounded compressed/PCM buffers. Audio callbacks perform no blocking I/O, allocation or sync-held locking. Source identity is portable server plus track; occurrences remain distinct even for repetitions. Queue revisions do not advance for progress. Commands and async preparation use deduplication and generation fences. Persist transitions outside the callback and restore valid committed state paused.

## UX & Release Evidence

Keep the existing two-row bar, Library/Playing navigation, useful idle browsing/Resume and accessible status. No dead Play something controls appear before 16.6. Browsing or physical-device changes must not reroute audio. Keep final rows and controls reachable across narrow layouts, localization, zoom and keyboard navigation.

Packaging 15.17 verifies shipping OS/architectures, controlled runtime loading, installation/upgrades, daemon lifetime, native API and physical keys separately, output loss, sleep/wake, safe Quit, continuity, measured manual resource bounds and ordinary real-sync coexistence. Reconcile outstanding checks and release-relevant defects from completed stories, including recorded 15.12 R16 and 15.14 R8/R9, without promoting unverified evidence. Actual publication is separate.

## Cross-Story Dependencies

Delivered foundation 15.1–15.14 → Back 15.15 and compact navigation 15.16 → packaging 15.17. Compact navigation has no technical dependency on Back. Neither new feature nor packaging requires Epic 16. Prepare dedicated implementation stories before coding; preserve all existing done statuses. Adaptive quality, conditional backoff, Radio, reporting/preferences and immutable exports retain their contracts in Epic 16. The expanded installed validation owner is 16.14.
