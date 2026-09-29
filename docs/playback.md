# HifiMule Playback Guide

**Last Updated:** 2026-09-27 | **Version:** 0.15.0

## What playback adds

HifiMule is a desktop player for self-hosted music, audiobooks, and podcasts, and a sync tool for portable players. Browse a configured server, listen on the computer, organize what is next, then sync chosen media to a device when wanted.

Playback does not download a second library or turn the WebView into an audio engine. The daemon streams from the configured provider, decodes audio locally, and owns the listening session even when the main window is closed.

## Listening from the library

- **Play an album** to start its tracks in album order.
- **Play a playlist or Audiobookshelf Series/Collection** to start its resolved tracks in source order.
- **Play an Audiobookshelf book** as its ordered audio-file parts, or **play a podcast episode** from a scoped Podcasts library.
- **Preview a track** to audition it without discarding the current album or queue.
- **Add to queue** to place a track in the upcoming listening order.
- Use the floating playback bar from library screens, or open the **Playback** destination for the full queue and history view.

The main session and a preview are intentionally different. Starting a preview checkpoints the main session; a completed preview, an explicit return, or a safe restoration can resume the interrupted session. Starting ordinary playback replaces the listening context.

Audiobookshelf Book progress is owned by the player. The daemon verifies current whole-book timing and identity before writing progress; device sync does not update listening progress. Podcast episodes use a dedicated episode browse model and `playback.playEpisode` entry point. Direct MP3 and AAC/MP4 streams are seek candidates only after authenticated range, container, and decoder checks. HLS/transcode responses are outside the direct byte-stream path. See the [Audiobookshelf Implementation Map](./audiobookshelf-implementation.md) and [controlled integration contract](./audiobookshelf-integration-contract.md) for precise evidence limits.

## Controls and outputs

The playback bar provides pause/resume, previous/next, stop, retry, and a seek timeline when the stream supports seeking. It also shows preparation, buffering, recovery, and source/output errors without exposing provider credentials or authenticated stream URLs.

Choose an audio endpoint from the output picker. HifiMule persists that preference in `playback.json`; if the selected endpoint is unavailable or disconnected, playback pauses safely instead of silently switching to an unexpected speaker or headphone output. The output list can be refreshed, and the preference can be reset to choose a current endpoint again.

Operating-system media controls use the same daemon command path as the app controls, so they continue to work while the application window is closed.

## Queue and history

The playback destination separates the session into:

- **Current** — the active or paused occurrence and its transport state.
- **Upcoming** — tracks that can be moved up, moved down, or removed.
- **History** — already reached or completed tracks, retained as listening evidence rather than editable queue entries.

The UI refreshes the daemon snapshot after each change. This prevents a delayed browser action from replacing a newer queue, seek, or output decision.

## Architecture boundaries

| Layer | Responsibility |
|---|---|
| UI | Starts playback actions and renders safe, authoritative session state. |
| JSON-RPC | Carries `playback.*` commands through the Tauri proxy to the local daemon. |
| Daemon | Serializes playback ownership, resolves provider streams, persists sessions, and handles recovery. |
| FFmpeg + CPAL | Decodes/resamples audio and writes it to the selected system output. |
| Souvlaki | Connects native media controls to the same daemon-owned session. |
| Media provider | Supplies authenticated stream access and metadata; credentials stay inside the daemon. |

For exact RPC schemas and recovery guarantees, see [API Contracts — Daemon](./api-contracts-hifimule-daemon.md). For the broader process design, see [Daemon Architecture](./architecture-hifimule-daemon.md), [UI Architecture](./architecture-hifimule-ui.md), and [Integration Architecture](./integration-architecture.md).
# Track-boundary quality adaptation (policy v1)

Playback alternatives are daemon-private, provider-verified representations. Navidrome is the
first multi-representation path: a qualified original plus the existing OpenSubsonic `stream`
route at MP3 192 kb/s when the source exceeds that bitrate. Jellyfin and Audiobookshelf remain
single-representation until their server/version transcode contracts receive equivalent evidence.
Portable-device transcoding profiles never participate.

Policy v1 uses monotonic delivery time, a 60,000 ms observation window, at least three eligible
samples, 125% downgrade headroom, 175% recovery headroom, and a 60,000 ms recovery span. History
is scoped by portable server identity plus stable representation identity and is bounded to 16
samples per scope (120,000 ms maximum age). Explicit cache hits, startup/short samples (<250 ms),
range/seek reads, partial bodies, and failures do not establish capacity. Unknown capacity keeps
the current/highest verified representation and the existing 100 ms bounded startup fill.

Selection changes only while preparing a later queue occurrence. `SuccessorFence`, the one-slot
prepared-successor contract, the 60-second preparation deadline, and generation/control/queue
fences remain authoritative. A claimed successor is retained. Non-unity album gain deliberately
constrains the occurrence to its qualified original representation. No mid-track replacement,
automatic skip, output reroute, or synthetic reporting transition is permitted.
