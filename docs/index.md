# HifiMule — Project Documentation Index

**Generated:** 2026-05-23 | **Last Updated:** 2026-06-15 | **Scan depth:** Exhaustive | **Version:** 0.6.1 | **Deep-Dives:** 1

---

## Project Summary

- **Type:** Monorepo (Rust Cargo workspace) with 2 parts
- **Primary Languages:** Rust, TypeScript
- **Architecture:** Two-process desktop app (daemon + Tauri 2 UI shell)
- **Communication:** JSON-RPC 2.0 over local HTTP on `localhost:19140`
- **Purpose:** Synchronizes Jellyfin, Navidrome, Subsonic, and OpenSubsonic music libraries to legacy portable audio players (Rockbox iPods, USB/MTP devices)

---

## Quick Reference

### `hifimule-daemon`

- **Type:** Backend service (Rust, Tokio async)
- **Tech Stack:** Axum 0.8, rusqlite, reqwest, tray-icon, keyring; provider abstraction for Jellyfin/Subsonic/OpenSubsonic; WPD (Windows MTP) / libmtp (Unix MTP)
- **Root:** [hifimule-daemon/](../hifimule-daemon/)
- **Entry Point:** [src/main.rs](../hifimule-daemon/src/main.rs)
- **RPC Port:** 19140

### `hifimule-ui`

- **Type:** Desktop app (Tauri 2)
- **Tech Stack:** TypeScript 5.6, Vite 6, Shoelace 2.19.1 web components
- **Root:** [hifimule-ui/](../hifimule-ui/)
- **Entry Points:** [src/main.ts](../hifimule-ui/src/main.ts), [src-tauri/src/lib.rs](../hifimule-ui/src-tauri/src/lib.rs)

---

## Generated Documentation

### Core

- [Project Overview](./project-overview.md) — What, why, key features, persistent state, platforms
- [Source Tree Analysis](./source-tree-analysis.md) — Full file tree, module responsibilities, test coverage
- [Integration Architecture](./integration-architecture.md) — IPC protocol, provider abstraction, sync flow, UI state management, media-server API calls

### Architecture (per part)

- [Architecture — Daemon](./architecture-hifimule-daemon.md) — Process model, AppState, RPC server, MediaProvider layer, DeviceManager, sync engine, MTP backends, Windows Service
- [Architecture — UI](./architecture-hifimule-ui.md) — Tauri shell, daemon launch strategy, RPC layer, BasketStore, component lifecycle

### API & Data

- [API Contracts — Daemon](./api-contracts-hifimule-daemon.md) — RPC methods for server connection, provider-neutral browse, sync, device management, and legacy Jellyfin-compatible calls
- [Data Models — Daemon](./data-models-hifimule-daemon.md) — DeviceManifest, provider-domain models, SyncedItem, BasketItem, SyncDelta, SyncOperation, DeviceMapping

### UI

- [Component Inventory — UI](./component-inventory-hifimule-ui.md) — TypeScript components and provider-neutral browse UI: BasketSidebar, MediaCard, StatusBar, InitDeviceModal, RepairModal, library.ts, basket.ts

### Development

- [Development Guide](./development-guide.md) — Prerequisites, build commands, testing, RPC debugging, common issues
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
# Prerequisites: Rust stable, Node.js LTS, pnpm, platform MTP libs (see development-guide.md)

# Build daemon
rtk cargo build -p hifimule-daemon

# Build & run UI in dev mode
cd hifimule-ui
rtk pnpm install
rtk pnpm tauri dev

# Run all daemon tests
rtk cargo test -p hifimule-daemon

# Direct RPC call (daemon running on :19140)
curl -s -X POST http://localhost:19140 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"get_daemon_state","params":{},"id":1}'
```

For full details see [Development Guide](./development-guide.md).

---

## Key Architecture Decisions

| Decision | Rationale |
|----------|-----------|
| Daemon runs as separate process | Enables auto-sync without UI open; survives UI crashes; Windows Service mode |
| JSON-RPC 2.0 over HTTP (not IPC pipe) | Debuggable with curl; same protocol in dev and prod; Tauri proxy handles mixed-content |
| Tauri `invoke` proxy for all RPC calls | Browser blocks fetch from `https://tauri.localhost` to `http://localhost` as mixed content |
| DeviceManifest on device (not in DB) | Manifest travels with the device; no sync across reinstalls; no cloud dependency |
| SQLite for device profiles and scrobble history | Only machine-local state that shouldn't live on the device itself |
| Multi-device via HashMap (not single active) | Enables simultaneous connections; user explicitly selects focus device |
| Auto-fill slot as virtual basket item | UI renders it like a real item; daemon expands it at sync time; never persisted |
| Provider-domain layer | Browse, sync, cover art, changes, scrobbling, and transcoding flow through `MediaProvider`; UI remains server-neutral |
| Legacy `jellyfin_*` RPC names retained | Existing UI calls continue to work while active non-Jellyfin providers are routed through the provider adapter |
