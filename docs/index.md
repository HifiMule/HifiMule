# HifiMule — Project Documentation Index

**Generated:** 2026-05-23 | **Last Updated:** 2026-09-27 | **Scan depth:** Deep | **Version:** 0.15.0 | **Deep-Dives:** 1

---

## Project Summary

- **Type:** Monorepo (Rust Cargo workspace) with 4 crates and 2 runtime processes
- **Primary Languages:** Rust, TypeScript
- **Architecture:** Two-process desktop app (daemon + Tauri 2 UI shell)
- **Communication:** Bearer-authenticated JSON-RPC 2.0 over an available loopback port published in the private lifecycle owner descriptor
- **Purpose:** Plays and synchronizes Jellyfin, Navidrome, Subsonic, OpenSubsonic, and Audiobookshelf libraries. Audiobookshelf Books and Podcasts have separate library roles, browse flows, direct playback, and device sync policies.

---

## Quick Reference

### `hifimule-daemon`

- **Type:** Backend service (Rust, Tokio async)
- **Tech Stack:** Axum 0.8, Tokio 1.49, rusqlite 0.38, reqwest 0.12, FFmpeg 9/CPAL, tray-icon, keyring, ChaCha20-Poly1305 vault; provider abstraction for Jellyfin/Subsonic/OpenSubsonic/Audiobookshelf; WPD (Windows MTP) / libmtp (Unix MTP)
- **Root:** [hifimule-daemon/](../hifimule-daemon/)
- **Entry Point:** [src/main.rs](../hifimule-daemon/src/main.rs)
- **RPC Endpoint:** `127.0.0.1:<owner descriptor port>`

### `hifimule-i18n`

- **Type:** Shared Rust library crate
- **Tech Stack:** Rust 2024 translation catalog consumed by daemon and UI-adjacent flows
- **Root:** [hifimule-i18n/](../hifimule-i18n/)

### `hifimule-lifecycle`

- **Type:** Shared Rust library crate
- **Role:** Daemon ownership, authenticated local discovery, startup and Quit fencing, and single-UI activation
- **Root:** [hifimule-lifecycle/](../hifimule-lifecycle/)
- **Entry Point:** [src/lib.rs](../hifimule-lifecycle/src/lib.rs)

### `hifimule-ui`

- **Type:** Desktop app (Tauri 2)
- **Tech Stack:** TypeScript 5.6, Vite 6, Shoelace 2.19.1 web components
- **Root:** [hifimule-ui/](../hifimule-ui/)
- **Entry Points:** [src/main.ts](../hifimule-ui/src/main.ts), [src-tauri/src/lib.rs](../hifimule-ui/src-tauri/src/lib.rs)

---

## Generated Documentation

### Core

- [Project Overview](./project-overview.md) — What, why, key features, persistent state, platforms
- [Source Tree Analysis](./source-tree-analysis.md) — Annotated critical source tree, module responsibilities, test coverage
- [Integration Architecture](./integration-architecture.md) — IPC protocol, provider abstraction, portable server identity, sync flow, UI state management, media-server API calls

### Architecture (per part)

- [Architecture — Daemon](./architecture-hifimule-daemon.md) — Process model, AppState, RPC server, MediaProvider layer, DeviceManager, sync engine, MTP backends, Windows Service
- [Architecture — UI](./architecture-hifimule-ui.md) — Tauri shell, daemon launch strategy, RPC layer, BasketStore, component lifecycle
- [Architecture — i18n](./architecture-hifimule-i18n.md) — Shared translation catalog and language fallback
- [Architecture — lifecycle](./architecture-hifimule-lifecycle.md) — Ownership locks, launch tickets, authenticated health, UI activation

### API & Data

- [API Contracts — Daemon](./api-contracts-hifimule-daemon.md) — RPC methods for server connection, provider-neutral browse, sync, device management, and legacy Jellyfin-compatible calls
- [Data Models — Daemon](./data-models-hifimule-daemon.md) — DeviceManifest, per-server AutoFillPipeline, provider-domain models, SyncedItem, BasketItem, SyncDelta, SyncOperation, DeviceMapping, ServerConfig
- [Playback Guide](./playback.md) — Desktop listening, queue and previews, outputs, native media controls, persistence, and boundaries
- [Audiobookshelf Implementation Map](./audiobookshelf-implementation.md) — Books and Podcasts setup, identity, browse, playback, sync, auto-fill, and evidence limits
- [Audiobookshelf Integration Contract](./audiobookshelf-integration-contract.md) — Dated controlled-server observations and fixture contract

### UI

- [Component Inventory — UI](./component-inventory-hifimule-ui.md) — TypeScript components and provider-neutral browse UI: ServerHub, BasketSidebar, MediaCard, PlaylistCurationView, TracksBrowseView, AutoFillPanel, InitDeviceModal, RepairModal, library.ts, basket.ts

### Development

- [Development Guide](./development-guide.md) — Prerequisites, build commands, testing, RPC debugging, common issues
- [Build and Deployment Guide](./deployment-guide.md) — CI workflows, packaging, runtime staging, installation and release boundaries
- [Localization Guide](./localization.md) — Shared daemon/UI translations and how to add a new language
- [Release Guide](./release-guide.md) — Release process, tagging, CI pipeline

### Metadata

- [Project Parts (JSON)](./project-parts.json)
- [Project Scan Report (JSON)](./project-scan-report.json)

---

## Deep-Dive Documentation

Detailed analysis of specific areas:

- [Auto-Fill Deep-Dive](./deep-dive-autofill.md) — Per-`(device, serverId)` pipeline model & manifest persistence, fast-path vs. configurable-engine routing, the pure `run_pipeline` engine, discovery/memory stages (rotation tiers, rarity, pity, context), quality & promotion modifiers, machine-local DB counters, and the `AutoFillPanel` builder UI — Generated 2026-06-15

---

## Existing Documentation (in repo root)

- [DEBUGGING.md](../DEBUGGING.md) — Daemon debugging: VS Code, CLI, JSON-RPC, tests
- [AGENTS.md](../AGENTS.md) — AI coding agent instructions
- [CLAUDE.md](../CLAUDE.md) — Claude Code instructions and RTK configuration
- [README.md](../README.md) — Project readme

---

## Getting Started (Quick)

```bash
# Prerequisites: Rust 1.93+, Node.js LTS, platform audio/MTP libs (see development-guide.md)

# Build daemon
rtk npm run build:daemon -- build -p hifimule-daemon

# Build & run UI in dev mode
cd hifimule-ui
rtk npm install
rtk npm run tauri dev

# Run all daemon tests
rtk npm run build:daemon -- test -p hifimule-daemon

# The daemon's current port and bearer token are in the private
# <app-data>/runtime/owner.json descriptor. See development-guide.md.
```

For full details see [Development Guide](./development-guide.md).

---

## Key Architecture Decisions

| Decision | Rationale |
|----------|-----------|
| Daemon runs as separate process | Enables auto-sync without UI open; survives UI crashes; Windows Service mode |
| Shared lifecycle ownership | OS locks establish one daemon and one UI per app-data profile; private descriptor and bearer-authenticated health bind the UI to the owner |
| JSON-RPC 2.0 over HTTP (not IPC pipe) | Debuggable with curl; same protocol in dev and prod; Tauri proxy handles mixed-content |
| Tauri `invoke` proxy for all RPC calls | Browser blocks fetch from `https://tauri.localhost` to `http://localhost` as mixed content |
| DeviceManifest on device (not in DB) | Manifest travels with the device; no sync across reinstalls; no cloud dependency |
| SQLite for device profiles and scrobble history | Only machine-local state that shouldn't live on the device itself |
| Multi-device via HashMap (not single active) | Enables simultaneous connections; user explicitly selects focus device |
| Auto-fill slot as virtual basket item | UI renders it like a real item; daemon expands it at sync time; never persisted |
| Provider-domain layer | Browse, sync, cover art, changes, scrobbling, and transcoding flow through `MediaProvider`; UI remains server-neutral |
| Audiobookshelf library scope | Each Books or Podcasts library is a distinct configured server, with portable identity derived from URL, user, immutable library ID, and role |
| Portable server identity | Device manifests and basket items use deterministic `server_id` values so synced devices survive remove/re-add and multi-machine use |
| Configurable auto-fill pipeline | Per-`(device, portable serverId)` manifest config stays portable; runtime history, rotation, and pity counters stay machine-local in SQLite |
| Legacy `jellyfin_*` RPC names retained | Existing UI calls continue to work while active non-Jellyfin providers are routed through the provider adapter |
