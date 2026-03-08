# JellyfinSync — Project Documentation Index

_Generated: 2026-03-08 | Scan Level: Quick | Workflow Version: 1.2.0_

## Project Overview

- **Type:** Monorepo (Rust Cargo workspace) with 2 parts
- **Primary Languages:** Rust, TypeScript
- **Architecture:** Two-process desktop app (daemon + Tauri UI)
- **Communication:** JSON-RPC 2.0 over local HTTP

## Quick Reference

### jellyfinsync-daemon

- **Type:** Backend service (Rust)
- **Tech Stack:** Tokio, Axum, rusqlite, reqwest, tray-icon
- **Root:** `jellyfinsync-daemon/`
- **Entry Point:** `src/main.rs`

### jellyfinsync-ui

- **Type:** Desktop app (Tauri 2)
- **Tech Stack:** TypeScript, Vite, Shoelace web components
- **Root:** `jellyfinsync-ui/`
- **Entry Points:** `src/main.ts`, `src-tauri/src/main.rs`

## Generated Documentation

### Core

- [Project Overview](./project-overview.md)
- [Source Tree Analysis](./source-tree-analysis.md)
- [Integration Architecture](./integration-architecture.md)

### Architecture (per part)

- [Architecture — Daemon](./architecture-jellyfinsync-daemon.md)
- [Architecture — UI](./architecture-jellyfinsync-ui.md)

### API & Data

- [API Contracts — Daemon](./api-contracts-jellyfinsync-daemon.md)
- [Data Models — Daemon](./data-models-jellyfinsync-daemon.md)

### UI

- [Component Inventory — UI](./component-inventory-jellyfinsync-ui.md)

### Development

- [Development Guide](./development-guide.md)

### Metadata

- [Project Parts (JSON)](./project-parts.json)

## Existing Documentation

- [DEBUGGING.md](../DEBUGGING.md) — Daemon debugging guide (VS Code, CLI, JSON-RPC, tests)
- [CLAUDE.md](../CLAUDE.md) — AI coding instructions and RTK configuration

## Getting Started

1. **Prerequisites:** Install Rust (MSRV 1.93.0) and Node.js
2. **Setup:** Run `npm install` at root and `cd jellyfinsync-ui && npm install`
3. **Build:** Run `npm run build` to build both parts
4. **Develop:** Run daemon (`cargo run -p jellyfinsync-daemon`) and UI (`cd jellyfinsync-ui && npm run tauri dev`) in separate terminals
5. **Test:** Run `cargo test` for all workspace tests

For detailed instructions, see the [Development Guide](./development-guide.md).
