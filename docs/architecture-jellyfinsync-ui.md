# Architecture — JellyfinSync UI

_Generated: 2026-03-08 | Scan Level: Quick | Part: jellyfinsync-ui | Type: desktop_

## Executive Summary

The JellyfinSync UI is a Tauri 2 desktop application providing a graphical interface for managing media synchronization with Jellyfin servers. It features a TypeScript/Vite frontend using Shoelace web components, communicating with the companion daemon service via JSON-RPC over HTTP.

## Technology Stack

| Category | Technology | Version |
|----------|-----------|---------|
| Framework | Tauri | 2.x |
| Language (Frontend) | TypeScript | ~5.6.2 |
| Build Tool | Vite | ^6.0.3 |
| UI Library | Shoelace | ^2.19.1 |
| Tauri API | @tauri-apps/api | ^2 |
| Language (Backend) | Rust | Edition 2021 (MSRV 1.93.0) |
| Target | ES2020 / Chrome 105 (Windows) / Safari 13 |

## Architecture Pattern

**Tauri 2 desktop application** with:
- Vanilla TypeScript SPA (no framework — direct DOM manipulation)
- Shoelace web components for UI elements
- JSON-RPC client communicating with external daemon process
- Tauri Rust backend for native OS integration
- EventTarget-based state management

## Module Structure

### Frontend (TypeScript)

```
src/
├── main.ts              # App initialization, page routing, toast system
├── login.ts             # Login page — Jellyfin server authentication
├── library.ts           # Library browsing — views, items, device status
├── rpc.ts               # JSON-RPC 2.0 client (HTTP POST to daemon)
├── styles.css           # Global stylesheet
├── assets/              # Static assets (logos, SVGs)
├── components/
│   ├── MediaCard.ts     # Media item display card with details
│   ├── BasketSidebar.ts # Sync basket panel + sync execution + progress
│   ├── InitDeviceModal.ts   # Device initialization wizard
│   └── RepairModal.ts   # Manifest discrepancy detection and repair
└── state/
    └── basket.ts        # BasketStore — sync basket state management
```

### Tauri Backend (Rust)

```
src-tauri/
├── src/
│   ├── main.rs          # Tauri process entry point
│   └── lib.rs           # App setup, plugin registration, window management
├── tauri.conf.json      # App config (windows, bundle, security)
├── capabilities/
│   └── default.json     # Tauri permission capabilities
└── icons/               # App icons (multiple sizes/formats)
```

## UI Components

| Component | File | Description |
|-----------|------|-------------|
| `MediaCard` | `components/MediaCard.ts` | Displays media items with artwork, metadata, counts and sizes |
| `BasketSidebar` | `components/BasketSidebar.ts` | Sync basket panel — add/remove items, view storage, execute sync with progress tracking |
| `InitDeviceModal` | `components/InitDeviceModal.ts` | Device initialization wizard — configure sync folder and device profile |
| `RepairModal` | `components/RepairModal.ts` | Detects and repairs manifest discrepancies (prune orphans, relink moved files) |

## State Management

**Pattern:** EventTarget-based store (custom implementation)

### BasketStore (`state/basket.ts`)
- Extends `EventTarget` for event-driven updates
- Manages sync basket items (add, remove, toggle)
- Dual persistence: `localStorage` (immediate) + daemon manifest (via RPC)
- Hydrates item sizes from daemon API on load
- Dirty flag tracking for unsaved changes

## Application Windows

| Window | Size | Purpose |
|--------|------|---------|
| `main` | 1024x768 | Primary application window (starts hidden) |
| `splashscreen` | 400x500 | Transparent splash screen during startup |

## Pages / Views

| Page | File | Route Trigger |
|------|------|---------------|
| Login | `login.ts` | Shown when no credentials stored |
| Library | `library.ts` | Main view after authentication |

## Communication

All data fetching goes through `rpc.ts` which sends JSON-RPC 2.0 requests to the daemon at `http://127.0.0.1:19140/`. The UI does not communicate directly with the Jellyfin server — all Jellyfin API calls are proxied through the daemon.

## Build & Development

| Command | Description |
|---------|-------------|
| `npm run dev` | Vite dev server (port 1420) |
| `npm run build` | TypeScript compile + Vite build |
| `npm run tauri` | Tauri CLI (dev/build) |

### Tauri Configuration
- `withGlobalTauri: true` — Tauri API available globally
- `CSP: null` — No Content Security Policy restrictions
- Bundle targets: all platforms
- Frontend dist: `../dist` (Vite output)
