# HifiMule — Project Overview

**Version:** 0.2.0 | **Generated:** 2026-05-07 | **Scan depth:** Exhaustive

---

## Purpose

HifiMule is a cross-platform desktop application that synchronizes music from a Jellyfin media server to legacy mass-storage audio players — primarily iPods running Rockbox firmware, but also any USB MSC device or MTP device.

The core problem it solves: Jellyfin manages your music library with rich metadata, but portable players like Rockbox iPods cannot connect to Jellyfin directly. HifiMule bridges this gap by letting users curate a "basket" of albums/playlists/artists and then copying the files to the device with correct paths, M3U playlists, and a manifest that tracks sync state. It also reads the Rockbox `.scrobbler.log` and reports back played tracks to Jellyfin.

---

## Core Principles

1. **Managed Sync Mode** — The device has a designated "managed" folder. HifiMule owns this folder completely; it adds and removes files autonomously to match the basket. Unmanaged folders are untouched.
2. **Jellyfin-First** — All library metadata (titles, album art, playlists) comes from Jellyfin. The app never maintains its own library database.
3. **Speed is King** — Delta sync: only copy what changed. Skipping files that are already present and byte-identical (via Jellyfin etag matching and metadata comparison) keeps syncs fast.
4. **Scrobble Bridge** — After each sync, parse the Rockbox `.scrobbler.log` and submit plays back to Jellyfin so listening history stays in sync.

---

## Architecture Overview

HifiMule is a **monorepo** containing two cooperating processes:

```
hifimule/
├── hifimule-daemon/     Rust backend — Axum JSON-RPC server, device I/O, sync engine
└── hifimule-ui/         Tauri 2 desktop shell — TypeScript + Vite + Shoelace
```

The UI is a thin shell. All business logic lives in the daemon. The UI communicates exclusively with the daemon via **JSON-RPC 2.0 over HTTP on `localhost:19140`**.

The Tauri shell is responsible for:
- Launching the daemon (as sidecar, Windows Service, or detecting a running instance)
- Proxying JSON-RPC calls from the WebView (to bypass browser mixed-content restrictions)
- Proxying Jellyfin image requests (to base64 data URLs)

---

## Technology Stack

| Component | Technology |
|-----------|-----------|
| Daemon language | Rust 1.93+ (MSRV 1.93.0) |
| Async runtime | Tokio (multi-thread) |
| HTTP server | Axum 0.8 |
| Database | SQLite via rusqlite (bundled) |
| Keyring | `keyring` crate (OS credential store) |
| System tray | `tray-icon` + `tao` |
| MTP (Windows) | WPD COM API via `windows-sys` |
| MTP (Unix) | libmtp FFI via `libc` |
| UI framework | Tauri 2 |
| Frontend language | TypeScript 5.6 |
| Build tool | Vite 6 |
| UI component library | Shoelace 2.19.1 |
| Packaging | Tauri bundler (DMG, .deb, .exe WiX/NSIS) |

---

## Key Features

### Sync
- **Delta sync**: computes adds, deletes, and ID-changes against the DeviceManifest before copying anything
- **Dirty-flag recovery**: if sync is interrupted, the manifest is marked dirty; on next connect the UI shows a Repair workflow to reconcile missing/orphaned files
- **Write-temp-rename atomicity**: MSC backend writes to a `.tmp` file then renames, preventing partial writes
- **FAT32/Rockbox path constraints**: paths are sanitized to ≤255 chars/component, ≤250 total (Windows MAX_PATH), with illegal characters replaced

### Multi-device
- Multiple devices can be connected simultaneously (stored in a `HashMap` keyed by mount path)
- The UI shows a "Device Hub" panel to switch between devices
- Each device has its own `DeviceManifest` (`.hifimule.json` at device root), basket, and sync settings

### Auto-fill
- Fills remaining device capacity with highest-priority tracks: favorites → most-played → newest
- Server-side sort and pagination stops as soon as capacity budget is exhausted
- Exclude list prevents manually-selected items from being double-counted

### Auto-sync on connect
- If enabled per device, the daemon triggers a full sync automatically when the device is plugged in (no UI required)

### Scrobbling
- Parses Rockbox `.scrobbler.log` (AudioScrobbler 1.1, tab-separated)
- Matches tracks to Jellyfin items by artist+title+duration
- Submits plays via `POST /Users/{userId}/PlayedItems/{itemId}`
- Deduplicates against `scrobble_history` SQLite table

### Transcoding
- Optional per-device transcoding profiles stored in `device-profiles.json`
- Uses Jellyfin `PlaybackInfo` API to negotiate the best stream URL with the server's transcoder
- Passthrough (no transcoding) is the default

---

## Persistent State

| Store | Location | Contents |
|-------|----------|----------|
| `DeviceManifest` | `<device-root>/.hifimule.json` | Sync state, basket, auto-fill settings, dirty flag |
| `config.json` | `%APPDATA%/HifiMule/` (Win) / `~/Library/Application Support/HifiMule/` (macOS) / `~/.local/share/HifiMule/` (Linux) | Jellyfin server URL, user ID |
| OS keyring | System credential store | Jellyfin access token |
| SQLite DB | Same app data dir as `config.json` | `devices` table (profile, auto-sync), `scrobble_history` |
| `device-profiles.json` | Same app data dir | Available transcoding profiles (seeded from embedded asset) |
| Browser `localStorage` | Tauri WebView | Basket state (session persistence) |

---

## Supported Platforms

| Platform | Support |
|----------|---------|
| macOS (10.15+) | Full — universal binary (x86_64 + arm64) |
| Windows 10/11 | Full — MSC + WPD MTP, Windows Service mode |
| Linux (Ubuntu 22.04+) | Full — MSC + libmtp, requires `libmtp-dev` |

---

## Project Status

Active development (v0.2.0). Core sync, multi-device, auto-fill, scrobbling, and manifest repair are all implemented and shipped.
