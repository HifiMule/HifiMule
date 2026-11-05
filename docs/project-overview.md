# HifiMule — Project Overview

**Version:** 0.21.0 | **Generated:** 2026-05-23 | **Last Updated:** 2026-11-05 | **Scan depth:** Deep

---

## Purpose

HifiMule is a cross-platform desktop application for self-hosted music, audiobook, and podcast libraries. It plays media directly from a connected server and synchronizes selected items to legacy portable audio players — primarily iPods running Rockbox firmware, but also USB MSC and MTP devices, DAPs, and Garmin smartwatches.

The core problem it solves: modern servers such as Jellyfin, Navidrome, Subsonic, and OpenSubsonic manage music libraries with rich metadata, but portable players like Rockbox iPods cannot connect to them directly. HifiMule bridges this gap by letting users browse and listen to that library on their computer, manage a listening queue, then curate a "basket" of albums/playlists/artists and copy them to a device with correct paths, M3U playlists, and a manifest that tracks sync state. It also reads the Rockbox `.scrobbler.log` and reports played tracks back through the active provider when that provider supports scrobbling.

---

## Core Principles

1. **Managed Sync Mode** — The device has a designated "managed" folder. HifiMule owns this folder completely; it adds and removes files autonomously to match the basket. Unmanaged folders are untouched.
2. **Provider-Neutral Media Server Layer** — Library metadata, cover art, downloads, changes, scrobbling, and browse modes flow through the daemon's `MediaProvider` trait. Jellyfin, Subsonic, Navidrome, and OpenSubsonic adapters normalize their server-specific APIs into the same domain model.
3. **Speed is King** — Delta sync: only copy what changed. Skipping files that are already present and byte-identical (via provider version/metadata comparison) keeps syncs fast.
4. **Scrobble Bridge** — After each sync, parse the Rockbox `.scrobbler.log` and submit plays back through the active provider so listening history stays in sync.
5. **Listen, Then Sync** — Playback and device sync share the same provider-neutral library, so the desktop app is a music-library companion in the spirit of iTunes as well as a transfer tool.

---

## Architecture Overview

HifiMule is a **monorepo** containing two cooperating runtime processes and two shared Rust libraries:

```
hifimule/
├── hifimule-daemon/     Rust backend — Axum JSON-RPC server, device I/O, sync engine, Rockbox tagcache
├── hifimule-ui/         Tauri 2 desktop shell — TypeScript + Vite + Shoelace
├── hifimule-i18n/       Shared translation catalog crate (en/fr/es/de, 1,061 keys)
└── hifimule-lifecycle/  Shared process ownership and launch contract
```

The UI is a thin shell. All business logic lives in the daemon. The UI communicates through native Tauri commands with a bearer-authenticated daemon on an available loopback port published in the private lifecycle owner descriptor.

The daemon keeps legacy `jellyfin_*` RPC names for compatibility with older UI code, but active non-Jellyfin connections are routed through the provider layer. New browse features use explicit provider-neutral `browse.*` RPCs and render only the modes advertised by the active provider's capabilities.

The Tauri shell is responsible for:
- Coordinating daemon startup or attachment through the shared lifecycle contract
- Proxying JSON-RPC calls from the WebView (to bypass browser mixed-content restrictions)
- Proxying provider cover-art requests (to base64 data URLs)

---

## Technology Stack

| Component | Technology |
|-----------|-----------|
| Daemon language | Rust 2024 (MSRV 1.99.1) |
| Async runtime | Tokio (multi-thread, max 4 worker threads) |
| HTTP server | Axum 0.8 |
| Provider abstraction | `MediaProvider` trait + Jellyfin/Subsonic/OpenSubsonic/Audiobookshelf adapters |
| Audio playback | FFmpeg 9 decoding (pinned prebuilt runtime), CPAL 0.18 audio output, Souvlaki native media controls |
| Database | SQLite via rusqlite 0.41 (bundled) |
| Credential vault | ChaCha20-Poly1305 encrypted, machine-bound |
| System tray | `tray-icon` 0.26 + `tao` 0.37 |
| MTP (Windows) | WPD COM API via `windows-sys` 0.62 |
| MTP (Unix) | libmtp FFI via `libc` |
| Rockbox tagcache | Custom binary reader/writer for `apps/tagcache.{c,h}` format |
| UI framework | Tauri 2.12 |
| Frontend language | TypeScript 6/7 (dual: `@typescript/native` + `typescript`) |
| Build tool | Vite 8 |
| UI component library | Shoelace 2.20 |
| Shared localization | `hifimule-i18n` crate (English, French, Spanish, German) |
| Shared lifecycle | `hifimule-lifecycle` crate (OS locks, private descriptor, authenticated health, startup/Quit generation) |
| Packaging | Tauri bundler (DMG, .deb, AppImage, RPM, MSI) |

---

## Key Features

### Sync
- **Delta sync**: computes adds, deletes, and ID-changes against the DeviceManifest before copying anything
- **Provider-aware change detection**: `sync_detect_changes` uses provider change feeds where available and Subsonic/OpenSubsonic album-level fallbacks where needed
- **Dirty-flag recovery**: if sync is interrupted, the manifest is marked dirty; on next connect the UI shows a Repair workflow to reconcile missing/orphaned files
- **Write-temp-rename atomicity**: MSC backend writes to a `.tmp` file then renames, preventing partial writes
- **FAT32/Rockbox path constraints**: paths are sanitized to ≤255 chars/component, ≤250 total (Windows MAX_PATH), with illegal characters replaced
- **Folder artwork**: automatically publishes album art as folder icons/thumbnails on connected devices (KDE, Windows, macOS)

### Rockbox tagcache (NEW in 0.20+)
- Builds and updates the Rockbox on-device database (`database_idx.tcd` and related files) on the computer as part of media sync — much faster than building it on the player
- Incremental updates reuse existing local metadata and skip unchanged database files
- Two-way rating sync with Navidrome: reads device ratings into the tagcache and pushes ratings back to the server
- See [Rockbox Database](./rockbox-database.md) and [Tagcache Data Flow](./rockbox-tagcache-data-flow.md)

### Desktop playback
- **Play from the library**: start a whole album or preview an individual track without first copying it to a portable device
- **Listening queue**: add tracks to the queue, inspect listening history and upcoming tracks, then reorder or remove upcoming entries
- **Transport controls**: pause/resume, previous/next, stop, seek, retry failed preparation, and return from a preview to the interrupted session
- **Output selection**: choose an available audio output; the selected endpoint is retained safely and playback recovers predictably when an output disappears
- **Native controls**: operating-system media controls continue to control the daemon-owned session while the main window is closed
- **Format support**: MP3, FLAC, AAC/ALAC (M4A/MP4), Opus, Vorbis, WAV, and AC3 (added in 0.20)

### Device profiles (NEW in 0.20+)
- Custom device transcoding profiles managed via `DeviceProfileManager` UI component
- Built-in profiles for Rockbox players, Garmin smartwatches, Sony Walkman, generic MP3 players, car USB sticks, audiobook/podcast devices
- Per-profile settings: music folder, playlist folder, audiobook/podcast paths, folder artwork defaults, transcoding format
- Numeric constraint validation with inline feedback
- Profiles stored in `device-profiles.json` and editable via `device_profiles.*` RPCs

### Multi-device
- Multiple devices can be connected simultaneously (stored in a `HashMap` keyed by mount path)
- The UI shows a "Destination Hub" panel to switch between playback and devices
- Each device has its own `DeviceManifest` (`.hifimule.json.zst` at device root; authoritative local cache for MTP), basket, and sync settings

### Auto-fill
- Auto-fill config is stored per `(device, portable serverId)` in the device manifest, with legacy `{ enabled, maxBytes }` blocks migrated when possible
- The configurable `AutoFillPipeline` supports filter, source blend, unit, ordering, memory, budget, fallback, quality, rarity, pity, context, and promotion stages
- Runtime auto-fill history is machine-local in SQLite (`autofill_history`, `autofill_rotation`, `autofill_pity`), not stored on the device
- Provider-routed preview and sync-time fill use the active `MediaProvider`; legacy Jellyfin priority fill remains as a compatibility path

### Multi-server and playlist editing
- Users can configure several servers, switch the active server in `ServerHub`, and keep basket items tagged by portable server identity
- The daemon lazily connects provider instances on first use and caches them by machine-local server UUID
- Playlist creation, rename, delete, add/remove tracks, and reorder are routed through provider capabilities for Jellyfin and Subsonic-compatible servers

### Audiobookshelf Books and Podcasts
- Discovery authenticates in the daemon, offers accessible libraries, and commits one Books or Podcasts library as a separate scoped server
- Books browse as albums and ordered audio-file parts, with Authors, Series, and Collections; podcasts browse as Shows, Recent Episodes, and individual episodes
- Direct book and episode playback uses the daemon session. Book progress is player-owned and verified against current part timing before write-back
- Basket and manifest keep media roles. Optional `audiobookPath` and `podcastPath` keep device folders distinct; role-aware auto-fill includes podcast retention settings
- Direct-copy MP3 for Audiobookshelf (added in 0.20)
- See the [Audiobookshelf Implementation Map](./audiobookshelf-implementation.md) for code paths and current boundaries

### Provider-Neutral Browse
- Server probing detects Jellyfin, Subsonic, OpenSubsonic-compatible, and Audiobookshelf servers before login
- Browse modes are capability-driven: artists, albums, playlists, genres, recently added, frequently played, recently played, and favorites are shown only when the active provider supports them
- Navidrome/OpenSubsonic history modes use server-provided newest/frequent/recent ordering; classic Subsonic hides unsupported history modes instead of synthesizing misleading data
- Favorites are browsed hierarchically as favorite artists, albums, and tracks while preserving basket semantics for direct favorites and scoped favorite groups
- Audiobookshelf modes are scoped by selected library role; Books and Podcasts have distinct models and UI flows

### Auto-sync on connect
- If enabled per device, the daemon triggers a full sync automatically when the device is plugged in (no UI required)

### Scrobbling
- Parses Rockbox `.scrobbler.log` (AudioScrobbler 1.1, tab-separated)
- Matches tracks to provider items by artist+title+duration where possible
- Submits plays through the active provider (`PlayedItems` for Jellyfin, `scrobble` for Subsonic/OpenSubsonic)
- Deduplicates against `scrobble_history` SQLite table

### Transcoding
- Optional per-device transcoding profiles stored in `device-profiles.json`
- Local transcoding uses FFmpeg for on-daemon format conversion; server-side transcoding uses the provider's `PlaybackInfo`/stream URLs
- Passthrough (no transcoding) is the default
- `mediacheck` CLI tool audits packaged libraries for format compatibility

---

## Persistent State

| Store | Location | Contents |
|-------|----------|----------|
| `DeviceManifest` | `<device-root>/.hifimule.json.zst`; authoritative `manifests/<device_id>.json.zst` cache for MTP | Lossless compressed sync state, basket, per-server auto-fill pipeline settings, dirty flag, folder artwork records |
| `config.json` | App data dir (Win: `%APPDATA%/HifiMule/`, macOS: `~/Library/Application Support/HifiMule/`, Linux: `~/.local/share/HifiMule/`) | Legacy Jellyfin URL and user ID |
| OS keyring / encrypted vault | Machine-bound ChaCha20-Poly1305 vault in app data dir | Access token or password-derived provider secret |
| SQLite DB | Same app data dir | `devices`, `scrobble_history`, `server_config`, `autofill_history`, `autofill_rotation`, `autofill_pity`, `rockbox_ratings`, `rockbox_rating_backfills` |
| `device-profiles.json` | Same app data dir | Available transcoding/device profiles (seeded from embedded asset) |
| `playback.json` | Same app data dir | Selected audio-output preference and playback configuration |
| `runtime/` | Same app data dir | Private lifecycle locks, owner descriptor, launch generation/tickets, UI activation mailbox |
| Browser `localStorage` | Tauri WebView | Basket state (session persistence) |

---

## Supported Platforms

| Platform | Support |
|----------|---------|
| macOS (10.15+) | Full — universal binary (x86_64 + arm64) |
| Windows 10/11 | Full — MSC + WPD MTP, Windows Service mode, UCRT64 build |
| Linux (Ubuntu 22.04+) | Full — MSC + libmtp, DEB/AppImage/RPM packages |

---

## Project Status

Active development (v0.21.0). Core sync, desktop playback, multi-device, multi-server routing, playlist editing, configurable auto-fill, scrobbling, manifest repair, MTP hardening, provider-neutral browse, shared localization and lifecycle, Rockbox tagcache sync with two-way rating sync, custom device profiles, folder artwork, AC3/AAC/ALAC playback, and Jellyfin/Subsonic/OpenSubsonic/Audiobookshelf support are implemented.
