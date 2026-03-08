# JellyfinSync — Project Overview

_Generated: 2026-03-08 | Scan Level: Quick_

## What is JellyfinSync?

JellyfinSync is a desktop application for synchronizing media from a Jellyfin media server to a local device. It consists of a background daemon service and a Tauri-based desktop UI, enabling users to browse their Jellyfin library, select media to sync, and manage offline copies on their device.

## Key Features

- **Jellyfin Library Browsing** — Browse views, collections, and media items from a Jellyfin server
- **Selective Media Sync** — Add items to a sync basket and download them to a local device
- **Delta Sync** — Calculate differences between local and remote, only transfer what's needed
- **Resumable Transfers** — Sync operations can be resumed after interruption
- **Playback Scrobbling** — Track media playback history
- **Device Management** — Initialize devices, manage storage, configure sync profiles
- **Manifest Repair** — Detect and fix discrepancies between local files and sync manifest
- **System Tray** — Daemon runs in the background with tray icon status indicators

## Tech Stack Summary

| Component | Technology |
|-----------|-----------|
| Daemon | Rust (Tokio, Axum, rusqlite, reqwest) |
| UI Frontend | TypeScript, Vite, Shoelace web components |
| UI Framework | Tauri 2 |
| Database | SQLite (embedded) |
| Credentials | OS keyring |
| Communication | JSON-RPC 2.0 over HTTP (localhost) |

## Architecture

- **Repository Type:** Monorepo (Rust Cargo workspace)
- **Architecture Style:** Two-process desktop app (daemon + UI)
- **Communication:** JSON-RPC 2.0 over local HTTP (`127.0.0.1:19140`)

### Parts

| Part | Type | Path | Description |
|------|------|------|-------------|
| jellyfinsync-daemon | Backend service | `jellyfinsync-daemon/` | Background daemon with system tray, local API, Jellyfin client, SQLite storage |
| jellyfinsync-ui | Desktop app | `jellyfinsync-ui/` | Tauri 2 desktop UI with TypeScript frontend |

## Quick Start

### Prerequisites
- Rust (Edition 2021, MSRV 1.93.0)
- Node.js with npm
- Tauri 2 CLI

### Build
```bash
npm run build          # Build entire project (UI + daemon)
npm run build:ui       # Build UI only
npm run build:daemon   # Build daemon only
```

### Development
```bash
# Terminal 1: Run daemon
cargo run -p jellyfinsync-daemon

# Terminal 2: Run UI in dev mode
cd jellyfinsync-ui && npm run dev
```

### Test
```bash
cargo test -p jellyfinsync-daemon    # Daemon tests
cargo test                           # All workspace tests
```
