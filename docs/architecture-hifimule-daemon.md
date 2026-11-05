# HifiMule Daemon — Architecture

**Part:** `hifimule-daemon` | **Last Updated:** 2026-11-05 | **Scan depth:** Deep | **Workspace version:** 0.21.0

## Process model

`src/main.rs` owns the interactive daemon process, tray event loop, startup and Quit coordination, device observers, and auto-sync. A background OS thread runs the Tokio runtime (max 4 worker threads) so the native event loop can stay on the required main thread. Windows service behavior is in `service.rs`. `hifimule-lifecycle` establishes one daemon owner per app-data profile through an OS lock, validates generation-bound launch tickets, and publishes a private descriptor only after readiness. The daemon binds `127.0.0.1` on an available port; the descriptor supplies that port and a bearer token.

The current `DaemonState` variants are `Idle`, `Syncing`, `Scanning`, `DeviceFound`, `DeviceRecognized`, and `Error`. Explicit Quit (`daemon.quit` / `daemon.retryQuit`) stops new mutations, drains or cancels active work, checkpoints playback, and advances the durable launch generation so older launch attempts cannot revive the daemon. See [Lifecycle Architecture](./architecture-hifimule-lifecycle.md).

A second binary, `mediacheck` (`src/bin/mediacheck.rs`), is a packaged library audit tool that inspects audio files for format compatibility.

## Runtime state and RPC

`rpc.rs` (~19,000 lines) builds `AppState` around `ServerManager`, SQLite `Database`, `DeviceManager`, `SyncOperationManager`, daemon-owned `PlaybackSession`, state sender, caches, and pending Audiobookshelf setup choices. The dispatch match handles **~130 JSON-RPC methods** grouped by prefix:

| Prefix | Examples |
|--------|----------|
| `server.*` | `connect`, `probe`, `reauthenticate`, `logout`, `list`, `select`, `update`, `remove`, `audiobookshelf.discover/commit/cancelSetup` |
| `browse.*` | `listModes`, `listArtists`, `listAlbums`, `listPodcastShows`, `listRecentPodcastEpisodes`, `listTracks`, `search`, `listFavorites`, `listRecentlyPlayed` |
| `playback.*` | `getSession`, `control`, `seek`, `listOutputs`, `selectOutput`, `playAlbum/playPlaylist/playEpisode/previewTrack`, `getFeedback/setFeedback`, snapshots, exports |
| `device.*` / `device_profiles.*` | `list`, `select`, `initialize`, `update_manifest`, `music_scan_*`, `get/set/create/update/clone/constraints` |
| `sync.*` | `calculate_delta`, `prepare`, `plan_details`, `detect_changes`, `execute`, `cancel`, `setAutoFill` |
| `basket.*` / `autoFill.*` | `estimate`, `autoFill`, `setPipeline` |
| `manifest.*` | `get_basket`, `save_basket`, `get_discrepancies`, `prune`, `relink`, `clear_dirty` |
| `playlist.*` | `create`, `addItems`, `removeTracks`, `delete`, `rename`, `reorder` |
| `daemon.*` | `health`, `quit`, `retryQuit` |

Mutating methods acquire a `MutationGuard` from `SyncOperationManager` (shutdown fence). Heavy sync reads are spawned with admission control. Axum exposes JSON-RPC 2.0 at `POST /` and an authenticated provider-aware image route at `GET /jellyfin/image/:id`. Local bearer authentication compares the descriptor token and binds health to the owner instance. See [API Contracts](./api-contracts-hifimule-daemon.md).

## Provider boundary

`providers/mod.rs` defines the `MediaProvider` trait (async via `async_trait`), provider errors, capabilities, playback descriptions, sync media representations, book timing, library roles, feedback (0–5 ratings), and scrobbling. Consumers resolve the selected provider through `ServerManager` or a portable server ID. `domain/models.rs` holds neutral music DTOs plus dedicated `PodcastShow`, `PodcastEpisode`, and search models.

| Adapter | Role |
| --- | --- |
| `providers/jellyfin.rs` | Jellyfin catalog, playlists, change feed, artwork, streams, AAC/ALAC MP4 aliases |
| `providers/subsonic.rs` | Subsonic/OpenSubsonic/Navidrome catalog, signed requests, playlist and history capabilities |
| `providers/audiobookshelf.rs` | One selected Books or Podcasts library per configured server; authenticated discovery, catalog, grouping, search, direct playback, sync media, book progress, direct-copy MP3 |

## Rockbox tagcache (NEW)

`rockbox.rs` (~4,100 lines) is a reader/writer for the Rockbox `apps/tagcache.{c,h}` binary format — the on-device music database. It decodes/encodes `database_idx.tcd` and related files, with journal/backup/ownership sidecar files. `Session::open()` prepares a tagcache update from the device manifest; `Session::import_ratings()` syncs ratings both ways with Navidrome. Settings (`enabled`, `import_ratings`, `path_prefix`) are stored per server in `server_config`. See [Rockbox Database](./rockbox-database.md).

## Playback

`playback/` (~30 modules) owns the session, queue/history, persistence, direct HTTP source, FFmpeg decoding, CPAL/Pulse/WASAPI output routing, native media controls (Souvlaki), and seek qualification. UI actions go through `playback.*` RPC methods; the WebView is never an audio engine. Book playback uses ordered audio-file parts with `book_progress.rs`. Supported formats include MP3, FLAC, AAC/ALAC (M4A/MP4), Opus, Vorbis, WAV, and AC3. See [Playback Guide](./playback.md).

## Device and sync

`device/mod.rs` defines `DeviceManifest`, `BasketItem`, `SyncedItem`, `MediaRole`, and `DeviceManager`. The manifest at the device root (`.hifimule.json.zst`) is authoritative for MSC; MTP uses the local `manifests/<device_id>.json.zst` cache. `device/manifest_codec.rs` streams typed fields into column arrays compressed in a checksummed Zstd level 9 frame. `device_music_scan.rs` provides read-only music discovery on connected devices.

`folder_icons.rs` publishes album artwork as folder icons/thumbnails on mounted devices (KDE `.directory`, Windows `desktop.ini`, macOS `.VolumeIcon.icns`) with symlink-safe path validation and atomic staging.

`sync.rs` (~14,000 lines) expands basket selections, computes deltas, stages and transfers media, generates paths/playlists, and records results. `sync/artwork.rs` automates folder artwork during sync. `sync/protection.rs` implements the playback/sync coexistence policy. `auto_fill/` separates provider-bound candidate fetching from a pure pipeline engine. `transcoding.rs` + `transcoding/local.rs` manage device profiles and FFmpeg-based local transcoding.

## Database

`db.rs` defines the SQLite schema: `devices`, `server_config` (multi-server with portable `server_id`, ABS `provider_library_id/role`), `scrobble_history`, `autofill_history`, `autofill_rotation`, `autofill_pity`, `rockbox_ratings`, and `rockbox_rating_backfills`. Credentials live in a separate encrypted vault (`vault.rs`, ChaCha20-Poly1305 + secrecy).
