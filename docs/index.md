# HifiMule — Project Documentation Index

**Generated:** 2026-05-23 | **Last Updated:** 2026-11-05 | **Scan depth:** Deep | **Version:** 0.21.0 | **Deep-Dives:** 1

---

## Project Summary

- **Type:** Monorepo (Rust Cargo workspace) with 4 crates and 2 runtime processes
- **Primary Languages:** Rust 2024, TypeScript
- **Architecture:** Two-process desktop app (daemon + Tauri 2 UI shell)
- **Communication:** Bearer-authenticated JSON-RPC 2.0 (~130 methods) over an available loopback port published in the private lifecycle owner descriptor
- **Purpose:** Plays and synchronizes Jellyfin, Navidrome, Subsonic, OpenSubsonic, and Audiobookshelf libraries to portable devices (Rockbox iPods, DAPs, Garmin, MTP phones). Includes Rockbox tagcache sync with two-way rating sync, custom device profiles, and folder artwork automation.

---

## Quick Reference

### `hifimule-daemon`

- **Type:** Backend service (Rust 2024, Tokio async)
- **Tech Stack:** Axum 0.8, Tokio 1.54, rusqlite 0.41, reqwest 0.13, FFmpeg 9 (pinned prebuilt runtime)/CPAL 0.18, tray-icon, ChaCha20-Poly1305 vault; provider abstraction for Jellyfin/Subsonic/OpenSubsonic/Audiobookshelf; MSC/MTP sync; Rockbox tagcache; WPD (Windows MTP) / libmtp (Unix MTP)
- **Root:** [hifimule-daemon/](../hifimule-daemon/)
- **Entry Points:** [src/main.rs](../hifimule-daemon/src/main.rs), [src/bin/mediacheck.rs](../hifimule-daemon/src/bin/mediacheck.rs)
- **RPC Endpoint:** `127.0.0.1:<owner descriptor port>`

### `hifimule-i18n`

- **Type:** Shared Rust library crate
- **Tech Stack:** Rust 2024 translation catalog (en/fr/es/de, 1,061 keys per language)
- **Root:** [hifimule-i18n/](../hifimule-i18n/)

### `hifimule-lifecycle`

- **Type:** Shared Rust library crate
- **Role:** Daemon ownership via OS locks, authenticated local discovery, startup/Quit fencing, single-UI activation
- **Root:** [hifimule-lifecycle/](../hifimule-lifecycle/)
- **Entry Point:** [src/lib.rs](../hifimule-lifecycle/src/lib.rs)

### `hifimule-ui`

- **Type:** Desktop app (Tauri 2.12)
- **Tech Stack:** TypeScript 6/7, Vite 8, Shoelace 2.20 web components, 26 components, 9 state stores
- **Root:** [hifimule-ui/](../hifimule-ui/)
- **Entry Points:** [src/main.ts](../hifimule-ui/src/main.ts), [src-tauri/src/lib.rs](../hifimule-ui/src-tauri/src/lib.rs)

---

## Generated Documentation

### Core

- [Project Overview](./project-overview.md) — What, why, key features, persistent state, platforms
- [Source Tree Analysis](./source-tree-analysis.md) — Annotated critical source tree, module responsibilities, test coverage
- [Integration Architecture](./integration-architecture.md) — IPC protocol, provider abstraction, portable server identity, sync flow, UI state management

### Architecture (per part)

- [Architecture — Daemon](./architecture-hifimule-daemon.md) — Process model, AppState, ~130 RPC methods, MediaProvider layer, DeviceManager, sync engine, Rockbox tagcache, MTP backends
- [Architecture — UI](./architecture-hifimule-ui.md) — Tauri shell, StartupCoordinator, RPC layer, 26 components, 9 state stores
- [Architecture — i18n](./architecture-hifimule-i18n.md) — Shared translation catalog (4 languages, 1,061 keys) and language fallback
- [Architecture — lifecycle](./architecture-hifimule-lifecycle.md) — Ownership locks, launch tickets, authenticated health, UI activation

### API & Data

- [API Contracts — Daemon](./api-contracts-hifimule-daemon.md) — ~130 RPC methods for server connection, provider-neutral browse, sync, device management, playback, snapshots
- [Data Models — Daemon](./data-models-hifimule-daemon.md) — DeviceManifest, ServerConfig, rockbox_ratings, autofill tables, scrobble_history
- [Playback Guide](./playback.md) — Desktop listening, queue, previews, outputs, native media controls, format support (MP3/FLAC/AAC/AC3/Opus/WAV)
- [Audiobookshelf Implementation Map](./audiobookshelf-implementation.md) — Books and Podcasts setup, browse, playback, sync, auto-fill
- [Audiobookshelf Integration Contract](./audiobookshelf-integration-contract.md) — Controlled-server observations and fixture contract
- [Rockbox Database and Ratings](./rockbox-database.md) — Tagcache maintenance, Navidrome two-way rating sync, supported formats
- [Rockbox Tagcache Data Flow](./rockbox-tagcache-data-flow.md) — Data sources, device inventory, sync sequence

### UI

- [Component Inventory — UI](./component-inventory-hifimule-ui.md) — 26 components: ServerHub, BasketSidebar, DeviceProfileManager, PlaybackControls, DestinationHub, AutoFillPanel, etc.

### Development

- [Development Guide](./development-guide.md) — Prerequisites (Rust 1.99.1, Node 26.12.1), build commands, audio runtime provisioning, testing, RPC debugging
- [Deployment Guide](./deployment-guide.md) — CI workflows, packaging (DMG/DEB/AppImage/RPM/MSI), prebuilt FFmpeg runtime, release boundaries
- [Localization Guide](./localization.md) — Shared daemon/UI translations, adding a new language
- [Release Guide](./release-guide.md) — Release process, tagging, CI pipeline

### Feature docs

- [MediaCheck](./mediacheck.md) — Packaged library audit tool
- [Sharing Device Profiles](./sharing-device-profiles.md) — Device profile export/import
- [Dependency Upgrade 2026-10-26](./dependency-upgrade-2026-10-26.md) — Dependency version audit
- [Sync Performance](./sync-performance.md) — Sync performance benchmarks
- [Playback Feedback Contract](./playback-feedback-contract.md) — Rating/feedback protocol
- [Playback Sync Protection v1](./playback-sync-protection-v1.md) — Playback/sync coexistence scheduler contract

### Metadata

- [Project Parts (JSON)](./project-parts.json)
- [Project Scan Report (JSON)](./project-scan-report.json)

---

## Deep-Dive Documentation

- [Auto-Fill Deep-Dive](./deep-dive-autofill.md) — Per-`(device, serverId)` pipeline model, fast-path vs. configurable-engine routing — Generated 2026-06-15

---

## Existing Documentation (in repo root)

- [DEBUGGING.md](../DEBUGGING.md) — Daemon debugging: VS Code, CLI, JSON-RPC, tests
- [AGENTS.md](../AGENTS.md) — AI coding agent instructions
- [CLAUDE.md](../CLAUDE.md) — Claude Code instructions
- [README.md](../README.md) — Project readme
- [DESIGN.md](../DESIGN.md) — Design document

---

## Getting Started (Quick)

```bash
# Prerequisites: Rust 1.99.1, Node.js 26.12.1 + npm 12.2.1, platform audio/MTP libs

# Build daemon
rtk npm run build:daemon -- build -p hifimule-daemon

# Build & run UI in dev mode
cd hifimule-ui
rtk npm install
rtk npm run tauri dev

# Run all daemon tests
rtk npm run build:daemon -- test -p hifimule-daemon

# The daemon's port and bearer token are in <app-data>/runtime/owner.json
```

For full details see [Development Guide](./development-guide.md).

---

## Key Architecture Decisions

| Decision | Rationale |
|----------|-----------|
| Daemon as separate process | Auto-sync without UI open; survives UI crashes; Windows Service mode |
| Shared lifecycle ownership | OS locks establish one daemon + one UI per profile; private descriptor + bearer-authenticated health |
| JSON-RPC 2.0 over HTTP | Debuggable with curl; same protocol dev/prod; Tauri proxy handles mixed-content |
| Tauri `invoke` proxy for all RPC | Browser blocks fetch from `https://tauri.localhost` to `http://localhost` as mixed content |
| DeviceManifest on device | Manifest travels with device; no cloud dependency |
| SQLite for local state | Machine-local state that shouldn't live on the device |
| Multi-device via HashMap | Simultaneous connections; user selects focus device |
| Provider-domain layer | Browse, sync, cover art, scrobbling flow through `MediaProvider`; UI stays server-neutral |
| Rockbox tagcache sync | Build database on computer (fast) vs on device (slow); two-way rating sync with Navidrome |
| Custom device profiles | Per-device transcoding/folder/artwork defaults; editable via UI; stored in `device-profiles.json` |
| Folder artwork automation | Album art as folder icons improves device browsing UX |
| Portable server identity | Deterministic `server_id` survives remove/re-add and multi-machine use |
| Configurable auto-fill pipeline | Per-`(device, serverId)` config on device; runtime counters machine-local in SQLite |
| Legacy `jellyfin_*` RPC names retained | Existing UI calls continue to work while non-Jellyfin providers route through adapter |
