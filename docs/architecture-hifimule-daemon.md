# HifiMule Daemon — Architecture

**Part:** `hifimule-daemon` | **Last Updated:** 2026-09-27 | **Scan depth:** Deep | **Workspace version:** 0.15.0

## Process model

`src/main.rs` owns the interactive daemon process, tray event loop, startup and Quit coordination, device observers, and auto-sync. A background OS thread runs the Tokio runtime so the native event loop can stay on the required main thread. Windows service behavior is in `service.rs`. `hifimule-lifecycle` establishes one daemon owner per app-data profile through an OS lock, validates generation-bound launch tickets, and publishes a private descriptor only after readiness. The daemon binds `127.0.0.1` on an available port; the descriptor supplies that port and a bearer token. Port `19140` is checked only to reject a concurrently running legacy daemon or another occupant.

The current `DaemonState` variants are `Idle`, `Syncing`, `Scanning`, `DeviceFound`, `DeviceRecognized`, and `Error`. Explicit Quit stops new mutations, drains or cancels active work, checkpoints playback, and advances the durable launch generation so older launch attempts cannot revive the daemon. See [Lifecycle Architecture](./architecture-hifimule-lifecycle.md).

## Runtime state and RPC

`rpc.rs` builds `AppState` around `ServerManager`, SQLite `Database`, `DeviceManager`, `SyncOperationManager`, daemon-owned `PlaybackSession`, state sender, caches, and pending Audiobookshelf setup choices. The manager loads server records from SQLite and connects provider instances lazily. The legacy `JellyfinClient` remains for compatibility paths. Axum exposes JSON-RPC 2.0 at `POST /` and an authenticated provider-aware image route at `GET /jellyfin/image/:id`. Local bearer authentication compares the descriptor token and binds health to the owner instance.

The dispatch table contains server setup and management, browse/search, playback, device and destination, sync, manifest, auto-fill, playlist, and legacy Jellyfin-compatible methods. The current method list and selected request shapes are in [API Contracts](./api-contracts-hifimule-daemon.md). The native Tauri proxy forwards structured JSON-RPC errors, including `code` and `data`.

## Provider boundary

`providers/mod.rs` defines `MediaProvider`, provider errors, capabilities, playback descriptions, sync media representations, book timing, and library roles. Consumers resolve the selected provider through `ServerManager` or a portable server ID. `domain/models.rs` holds neutral music DTOs plus dedicated `PodcastShow`, `PodcastEpisode`, and search models.

| Adapter | Role |
| --- | --- |
| `providers/jellyfin.rs` | Jellyfin catalog, playlists, change feed, artwork and streams |
| `providers/subsonic.rs` | Subsonic/OpenSubsonic/Navidrome catalog, signed requests, playlist and history capabilities |
| `providers/audiobookshelf.rs` | One selected Books or Podcasts library per configured server; authenticated discovery, catalog, grouping, search, direct playback, sync media, and book progress |

Capabilities decide the modes surfaced by the UI. The Audiobookshelf provider keeps upstream IDs scoped to its selected library and rejects cross-library or changed-role results. The [Audiobookshelf Implementation Map](./audiobookshelf-implementation.md) traces this adapter end to end.

## Playback

`playback/` owns the session, queue/history, persistence, direct HTTP source, FFmpeg decoding, CPAL output routing, native media controls, and seek qualification. UI actions go through `playback.*` RPC methods and the daemon's command service; the WebView is never an audio engine. Book playback uses ordered audio-file parts, verified whole-book timing, and `book_progress.rs` for player-owned progress. Direct MP3 and AAC/MP4 are seek candidates only after transport and decoder qualification; an HLS response is not fed to the byte-stream decoder. See [Playback Guide](./playback.md).

## Device and sync

`device/mod.rs` defines `DeviceManifest`, `BasketItem`, `SyncedItem`, `MediaRole`, and `DeviceManager`. The manifest at the device root (`.hifimule.json`) is authoritative for managed paths, basket, synced files, auto-fill settings, and repair state. `music`, `audiobook`, and `podcast` roles can use separate configured paths; absent role overrides fall back to the primary managed path. The manager tracks multiple connected MSC/MTP devices and a selected destination. Platform MTP implementations live in `device/mtp.rs`; `device_io.rs` abstracts MSC and MTP writes.

`sync.rs` expands basket selections, computes a delta against manifest state, stages and transfers media, generates paths and playlists, and records durable results. Book parts and podcast episodes keep role and portable server identity in the plan. Non-music transfers ask the provider for a compatible direct media representation. Transfer staging uses bounded memory rather than holding a full downloaded file in RAM. Interrupted syncs mark the manifest dirty for repair. `auto_fill/` separates provider-bound candidate fetching from a pure pipeline engine; per-server configuration travels on the device, while rotation/history counters live in SQLite.

Playback/sync coexistence uses a daemon-internal, playback-owner-published snapshot. It is fenced by session and generation and contains only portable server/representation identity, monotonic evidence age, bounded sample summary, classification, and sanitized reason. `sync/protection.rs` converts that snapshot into a cancellable admission delay before future provider resolution/request work. It never changes the two-track/2 GiB staging bounds, holds a staging permit while waiting, or interrupts the single verified writer, manifest transaction, cleanup, or playlist work. Attributable risk affects only the matching server producer; stale, healthy, unknown, and device-writer-only conditions preserve the normal scheduler. See [policy v1](./playback-sync-protection-v1.md).

## Persistent data and trust

`db.rs` initializes and migrates SQLite tables for devices, scrobbles, server configuration, auto-fill runtime history, and book continuity. Server rows contain machine-local UUIDs plus deterministic portable IDs; Audiobookshelf rows additionally retain immutable upstream library ID and role. `api.rs` and `vault.rs` keep media-server credentials in native storage, outside the UI. `hifimule-i18n` provides daemon messages from the shared catalog.

## Build and verification

The root audio-runtime wrapper is required before daemon compilation because FFmpeg native libraries must be prepared before Cargo dependency build scripts. Use `rtk npm run build:daemon -- check -p hifimule-daemon` and `rtk npm run build:daemon -- test -p hifimule-daemon`. Provider tests, `tests/audiobookshelf_contract.rs`, sync/device tests, and installed smoke scripts cover separate boundaries. See [Development Guide](./development-guide.md) for platform prerequisites and commands.
# Playback adaptation ownership

`playback/adaptation.rs` owns the pure bounded policy and scoped history. `http_source.rs` captures
sanitized byte/time facts; `streaming.rs` retains compressed-buffer ownership; `audio.rs` invokes
the decision only in the existing successor preparation path. The CPAL/Pulse callback performs no
adaptation work, allocation, I/O, estimator locking, persistence, or provider access. Existing
generation and `SuccessorFence` admission remains the final authority.
