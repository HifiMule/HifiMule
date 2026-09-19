# Epic 16 Context: Radio/Recommendations

Created from the approved 2026-09-19 course correction. Old Stories 15.15–15.28 are now 16.1–16.14; the proposal records the mapping. Epic and all stories remain backlog. Epic 15 supplies the delivered manual-listening and packaging foundation.

## Goal

Add playback-specific automatic selection, bounded Radio, explainable artist transitions, conservative recording deduplication and track gain. Complete the remaining roadmap with source reporting/preferences, immutable listening snapshots and exports, adaptive quality, conditional sync protection and sustained installed validation. Recommendations use configured-server metadata without a cross-session taste profile.

## Stories

- Story 16.1: Configure Playback selection and start its first selected track
- Story 16.2: Replenish Radio within a bounded upcoming queue
- Story 16.3: Continue Radio through meaningful artist connections
- Story 16.4: Avoid duplicate Radio recordings across configured servers
- Story 16.5: Match Radio track loudness using available metadata
- Story 16.6: Start Radio with Play something without opening the main window
- Story 16.7: Report listening accurately to the source server
- Story 16.8: Save supported Like and Dislike preferences on the source server
- Story 16.9: Save an immutable local listening snapshot
- Story 16.10: Save a listening snapshot as playlists on its source servers
- Story 16.11: Add a listening snapshot to a connected device basket or replace it
- Story 16.12: Adapt playback quality at track boundaries using buffer health
- Story 16.13: Protect playback during sync without unnecessary throttling
- Story 16.14: Keep long listening sessions bounded and recoverable

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

Reuse the installed Epic 15 bar, manual queue, Back and compact browse navigation. Separate Playback source/selection settings from physical-device settings. Play something in 16.6 starts a fresh Radio; Resume retains logical-session identity/exclusions. Selection settings alone never replace an active session. Failed starts preserve recoverable existing state. Explain relationship transitions, fresh-center fallback, exhaustion, unsupported capabilities and quality/recovery states. Feedback and exports use the occurrence's source, not the browsed server.

## Cross-Story Dependencies and Release Gate

Deliver after Epic 15: 16.1 shared selection/settings → 16.2 bounded Radio → 16.3 relationships → 16.4 deduplication → 16.5 Radio gain → 16.6 final entry points → 16.7 reporting → 16.8 preferences → 16.9 local snapshot → 16.10 server playlists → 16.11 basket exports → 16.12 adaptation → 16.13 conditional sync protection → 16.14 sustained/installed validation. Preserve source/occurrence identity and existing album/Preview/Back semantics throughout. Reporting must account for replay without counting cursor movement as a completed listen.

Story 16.14 reuses packaging established in 15.17 and verifies the expanded workflow matrix on each shipping architecture, with compatible artifact/runtime versions, actual provider effects, resource measurements and recovery evidence. New dependencies and affected workflows need fresh checks; Epic 15 evidence does not certify Epic 16. Larger/provider/platform tasks may be split during preparation without dropping outcomes. No story implies automatic publication.
