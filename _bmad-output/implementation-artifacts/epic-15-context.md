# Epic 15 Context: Desktop Playback

<!-- Compiled from planning artifacts. Edit freely. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Deliver daemon-owned desktop listening across Windows, macOS and Linux for the same curated, multi-server libraries HifiMule synchronizes. Users can resume a durable session, play faithful albums, audition tracks, start bounded Radio without choosing a first artist, and explicitly move discoveries into server playlists or a connected device basket, while playback remains independent of the detachable UI and preserves existing managed-device safety.

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
- Story 15.15: Configure Playback selection and start its first selected track
- Story 15.16: Replenish Radio within a bounded upcoming queue
- Story 15.17: Continue Radio through meaningful artist connections
- Story 15.18: Avoid duplicate Radio recordings across configured servers
- Story 15.19: Match Radio track loudness using available metadata
- Story 15.20: Start Radio with Play something without opening the main window
- Story 15.21: Report listening accurately to the source server
- Story 15.22: Save supported Like and Dislike preferences on the source server
- Story 15.23: Save an immutable local listening snapshot
- Story 15.24: Save a listening snapshot as playlists on its source servers
- Story 15.25: Add a listening snapshot to a connected device basket or replace it
- Story 15.26: Adapt playback quality at track boundaries using buffer health
- Story 15.27: Protect playback during sync without unnecessary throttling
- Story 15.28: Keep long listening sessions bounded and recoverable
- Story 15.29: Ship verified playback builds for Windows, macOS and Linux

## Requirements & Constraints

Playback is always available, has independent source and selection settings, and must not expose physical storage or sync actions. One authoritative session serves UI, native controls and menu commands; closing the UI leaves it running, while explicit Quit checkpoints it, stops audio, and safely coordinates active sync cancellation. Relaunch restores valid state paused. Output loss pauses without rerouting or auto-resuming.

Albums retain disc/track order, recorded silence and relative loudness; prepared supported boundaries add no extra decoded silence. Preview is a full-track audition that preserves the main queue and position, resumes it after natural completion, and returns it paused after explicit Stop. Radio keeps a bounded upcoming queue, respects session-scoped skips/removals, uses meaningful relationship evidence or an explained fresh-center fallback, and conservatively deduplicates recordings without collapsing uncertain matches, distinct performances or deliberate repeated occurrences.

Request the highest sustainable quality independently of device-sync transcoding settings. Initial adaptation occurs only at track boundaries; unsupported continuity degrades to visible buffering/retry. Radio may bypass unavailable tracks, but album playback pauses and retries. Sync backoff is conditional on measured playback risk and may not weaken device-write integrity.

Route playback, reporting, feedback and export through each occurrence's source server. Authenticated stream URLs and credentials remain daemon-side. Enable reporting and Like/Dislike only where provider semantics are verified. Exports use immutable snapshots, preserve occurrence identity and per-server order, report partial results, and retain recoverable operation state without promising remote exactly-once effects.

Bound upcoming entries, compressed prefetch, decoded PCM, candidate caches and paged history independently. Prevent obsolete asynchronous work from publishing audio or state. Keep playback keyboard-operable with visible focus, accessible names/status, and non-obscuring floating controls. Validate installed builds and shipping architectures for lifecycle, output loss, sleep/wake, physical media keys, safe shutdown, sustained sessions and real-sync coexistence; report measured budgets and failures rather than invented guarantees.

## Technical Decisions

Keep playback inside the existing Rust daemon, Tokio runtime, native event loop, SQLite store, provider abstraction, JSON-RPC/Tauri bridge and TypeScript UI. A single session manager serializes commands and owns queue order, position, Radio state and one temporary audition. Use distinct typed identities for session, queue occurrence, recording and portable server plus provider track. Queue edits carry an expected revision; progress does not change that revision. Command IDs provide bounded deduplication, and every asynchronous fetch/decode operation carries a generation fence.

Use controlled native FFmpeg decoding with CPAL shared output: provider stream to bounded compressed prefetch, decoding/conversion and processing, bounded PCM queue, then a continuous callback-safe output stream. Network, decode, persistence and sync coordination stay outside the audio callback; the callback performs no blocking I/O, allocation or sync-held locking. Prepare the next track before completion, remove only known encoder padding, and convert sample rate/channels without reopening output where supported.

Persist recoverable session state and long-running history in SQLite; store versioned Playback configuration separately from physical-device manifests. Extend providers with capability-driven playback descriptions, reporting, feedback, relationship and loudness metadata while keeping provider-specific APIs behind adapters. Reuse the selection engine through a typed, source-aware boundary with playback-owned settings and bounded candidate work. Keep snake_case in Rust/SQL and camelCase on JSON wires; finalize exact schemas, units, migrations, error states, retention windows, buffer thresholds and packaged runtime versions in the owning stories before implementation.

## UX & Interaction Patterns

Playback is the first destination and the fallback when no physical device is connected. A device arrival may select its basket but must not interrupt listening or change source. A floating bar below the browser exposes authoritative transport and status across views without hiding final rows or stealing focus. Play something starts a new configured Radio; Resume continues the saved session. Queue conflicts refresh from daemon state rather than overwriting newer edits, and reconnects fetch a versioned snapshot. Capability limits, reduced quality, waiting states, restoration failures, output loss and detected-device failures must be actionable and explicit.

## Cross-Story Dependencies

Delivery is intentionally sequential: lifecycle and safe shutdown precede persistence; persistence precedes the single-track player; output, native controls and seeking precede album transport; album continuity and gain precede preview; the Playback destination precedes queue editing and the shared bar; selection integration precedes bounded Radio, artist transitions, deduplication, Radio gain and final Play something entry points; mature occurrence/source state precedes reporting, feedback and immutable exports; adaptation precedes sync coexistence, soak validation and packaged-platform verification. Later audio work must join the established shutdown/checkpoint contract, and all stages retain existing provider routing and managed-device protections.
