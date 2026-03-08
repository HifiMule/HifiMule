# JellyfinSync — Source Tree Analysis

_Generated: 2026-03-08 | Scan Level: Quick_

## Repository Structure

**Type:** Monorepo (Rust Cargo workspace with 2 members)

```
JellyfinSync/
├── Cargo.toml                          # Workspace manifest (2 members)
├── package.json                        # Root dev tooling (ESLint, Prettier, Jest, build scripts)
├── CLAUDE.md                           # AI coding instructions
├── DEBUGGING.md                        # Daemon debugging guide
├── .vscode/
│   └── launch.json                     # VS Code debug configurations
├── .gitignore
│
├── jellyfinsync-daemon/                # ── Part: Backend Daemon ──
│   ├── Cargo.toml                      # Daemon crate manifest
│   ├── assets/                         # System tray icons
│   │   ├── icon.png                    # Default tray icon
│   │   ├── icon_error.png              # Error state tray icon
│   │   └── icon_syncing.png            # Syncing state tray icon
│   └── src/
│       ├── main.rs                     # ★ Entry point — tray icon, event loop, server startup
│       ├── rpc.rs                      # JSON-RPC 2.0 handler (24 methods) + image proxy
│       ├── api.rs                      # Jellyfin HTTP API client + credential manager
│       ├── db.rs                       # SQLite database (devices, scrobble_history)
│       ├── sync.rs                     # Media sync engine (delta calculation, file transfer)
│       ├── scrobbler.rs                # Playback scrobbling/history tracking
│       ├── paths.rs                    # Platform-specific path utilities
│       ├── tests.rs                    # Integration tests
│       └── device/
│           ├── mod.rs                  # Device management (storage, folders, initialization)
│           └── tests.rs               # Device module tests
│
├── jellyfinsync-ui/                    # ── Part: Desktop UI (Tauri 2) ──
│   ├── package.json                    # UI npm dependencies (Tauri, Vite, Shoelace, TS)
│   ├── tsconfig.json                   # TypeScript configuration (ES2020, strict)
│   ├── vite.config.ts                  # Vite build config (Tauri-optimized)
│   ├── index.html                      # Main HTML entry
│   ├── splashscreen.html               # Splash screen during startup
│   ├── .env                            # Environment variables
│   ├── src/
│   │   ├── main.ts                     # ★ Entry point — app initialization, routing
│   │   ├── login.ts                    # Login page logic
│   │   ├── library.ts                  # Library browsing (views, items, status)
│   │   ├── rpc.ts                      # JSON-RPC client → daemon API
│   │   ├── styles.css                  # Global styles
│   │   ├── assets/                     # Static assets (logos, SVGs)
│   │   ├── components/
│   │   │   ├── MediaCard.ts            # Media item display card
│   │   │   ├── BasketSidebar.ts        # Sync basket panel + sync execution
│   │   │   ├── InitDeviceModal.ts      # Device initialization wizard
│   │   │   └── RepairModal.ts          # Manifest discrepancy repair
│   │   └── state/
│   │       └── basket.ts              # BasketStore — EventTarget-based state management
│   └── src-tauri/
│       ├── Cargo.toml                  # Tauri Rust crate
│       ├── build.rs                    # Tauri build script
│       ├── tauri.conf.json             # Tauri app config (windows, bundle, security)
│       ├── capabilities/
│       │   └── default.json            # Tauri permission capabilities
│       ├── gen/schemas/                # Auto-generated Tauri schemas
│       ├── icons/                      # App icons (multiple sizes + formats)
│       └── src/
│           ├── main.rs                 # ★ Tauri entry point
│           └── lib.rs                  # Tauri app setup (plugins, window management)
│
└── docs/                               # Generated project documentation
    └── project-scan-report.json        # Workflow state file
```

## Critical Folders Summary

| Folder | Part | Purpose | Key Files |
|--------|------|---------|-----------|
| `jellyfinsync-daemon/src/` | daemon | Core daemon business logic | `main.rs`, `rpc.rs`, `api.rs`, `sync.rs` |
| `jellyfinsync-daemon/src/device/` | daemon | Device management subsystem | `mod.rs` |
| `jellyfinsync-ui/src/` | ui | TypeScript frontend application | `main.ts`, `library.ts`, `rpc.ts` |
| `jellyfinsync-ui/src/components/` | ui | UI component classes | `MediaCard.ts`, `BasketSidebar.ts` |
| `jellyfinsync-ui/src/state/` | ui | Client-side state management | `basket.ts` |
| `jellyfinsync-ui/src-tauri/` | ui | Tauri Rust backend for UI | `lib.rs`, `tauri.conf.json` |

## Entry Points

| Entry Point | Part | Description |
|-------------|------|-------------|
| `jellyfinsync-daemon/src/main.rs` | daemon | Daemon process — tray icon, event loop, HTTP server |
| `jellyfinsync-ui/src/main.ts` | ui | Frontend app initialization and page routing |
| `jellyfinsync-ui/src-tauri/src/main.rs` | ui | Tauri process entry (delegates to lib.rs) |

## Integration Points

The UI communicates with the daemon via **JSON-RPC 2.0 over HTTP** on `127.0.0.1:19140`:
- `jellyfinsync-ui/src/rpc.ts` → HTTP POST → `jellyfinsync-daemon/src/rpc.rs`
- Image proxy: `jellyfinsync-ui` → GET `/jellyfin/image/{id}` → daemon proxies from Jellyfin server
