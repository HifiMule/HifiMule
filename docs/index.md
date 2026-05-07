# JellyfinSync — Project Documentation Index

**Generated:** 2026-05-07 | **Scan depth:** Exhaustive | **Version:** 0.2.0

---

## Project Summary

- **Type:** Monorepo (Rust Cargo workspace) with 2 parts
- **Primary Languages:** Rust, TypeScript
- **Architecture:** Two-process desktop app (daemon + Tauri 2 UI shell)
- **Communication:** JSON-RPC 2.0 over local HTTP on `localhost:19140`
- **Purpose:** Synchronizes Jellyfin music libraries to legacy portable audio players (Rockbox iPods, USB/MTP devices)

---

## Quick Reference

### `jellyfinsync-daemon`

- **Type:** Backend service (Rust, Tokio async)
- **Tech Stack:** Axum 0.8, rusqlite, reqwest, tray-icon, keyring; WPD (Windows MTP) / libmtp (Unix MTP)
- **Root:** [jellyfinsync-daemon/](../jellyfinsync-daemon/)
- **Entry Point:** [src/main.rs](../jellyfinsync-daemon/src/main.rs)
- **RPC Port:** 19140

### `jellyfinsync-ui`

- **Type:** Desktop app (Tauri 2)
- **Tech Stack:** TypeScript 5.6, Vite 6, Shoelace 2.19.1 web components
- **Root:** [jellyfinsync-ui/](../jellyfinsync-ui/)
- **Entry Points:** [src/main.ts](../jellyfinsync-ui/src/main.ts), [src-tauri/src/lib.rs](../jellyfinsync-ui/src-tauri/src/lib.rs)

---

## Generated Documentation

### Core

- [Project Overview](./project-overview.md) — What, why, key features, persistent state, platforms
- [Source Tree Analysis](./source-tree-analysis.md) — Full file tree, module responsibilities, test coverage
- [Integration Architecture](./integration-architecture.md) — IPC protocol, sync flow, UI state management, Jellyfin API calls

### Architecture (per part)

- [Architecture — Daemon](./architecture-jellyfinsync-daemon.md) — Process model, AppState, RPC server, DeviceManager, sync engine, MTP backends, Windows Service
- [Architecture — UI](./architecture-jellyfinsync-ui.md) — Tauri shell, daemon launch strategy, RPC layer, BasketStore, component lifecycle

### API & Data

- [API Contracts — Daemon](./api-contracts-jellyfinsync-daemon.md) — All 34 RPC methods with params, return types, and error codes
- [Data Models — Daemon](./data-models-jellyfinsync-daemon.md) — DeviceManifest, SyncedItem, BasketItem, SyncDelta, SyncOperation, JellyfinItem, DeviceMapping

### UI

- [Component Inventory — UI](./component-inventory-jellyfinsync-ui.md) — All TypeScript components: BasketSidebar, MediaCard, StatusBar, InitDeviceModal, RepairModal, library.ts, basket.ts

### Development

- [Development Guide](./development-guide.md) — Prerequisites, build commands, testing, RPC debugging, common issues
- [Release Guide](./release-guide.md) — Release process, tagging, CI pipeline

### Metadata

- [Project Parts (JSON)](./project-parts.json)
- [Project Scan Report (JSON)](./project-scan-report.json)

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
cargo build -p jellyfinsync-daemon

# Build & run UI in dev mode
cd jellyfinsync-ui
pnpm install
pnpm tauri dev

# Run all daemon tests
cargo test -p jellyfinsync-daemon

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
