# HifiMule — Source Tree Analysis

**Generated:** 2026-05-23 | **Scan depth:** Exhaustive

---

## Repository Structure

```
hifimule/
├── Cargo.toml                        Workspace root (members: daemon, workspace deps)
├── Cargo.lock
├── .github/
│   └── workflows/
│       ├── release.yml               Release CI (matrix: macOS universal, Ubuntu, Windows)
│       └── smoke-test.yml            Smoke test CI
├── scripts/
│   ├── prepare-sidecar.mjs           Build step: copies compiled daemon → ui/src-tauri/sidecars/
│   └── smoke-tests/                  Smoke test scripts
├── hifimule-daemon/              Rust backend (project part: "backend")
│   ├── Cargo.toml
│   ├── build.rs                      Windows: winresource (EXE icon); Unix: pkg_config libmtp
│   ├── assets/
│   │   └── device-profiles.json      Embedded default transcoding profiles
│   └── src/
│       ├── main.rs                   Entry point, DaemonState enum, Tokio runtime spawn
│       ├── rpc.rs                    Axum HTTP server, provider-aware RPC dispatch table, AppState
│       ├── api.rs                    JellyfinClient (reqwest), CredentialManager
│       ├── db.rs                     SQLite wrapper (rusqlite), devices + scrobble_history tables
│       ├── sync.rs                   Delta calculation, execute_sync, path construction, M3U gen
│       ├── auto_fill.rs              Auto-fill algorithm (paginated Jellyfin fetch, capacity truncation)
│       ├── scrobbler.rs              Rockbox .scrobbler.log parser, Jellyfin submission
│       ├── transcoding.rs            Device profiles loader (device-profiles.json)
│       ├── paths.rs                  get_app_data_dir() / get_device_profiles_path() (OS-aware)
│       ├── service.rs                Windows Service install/uninstall/run (windows-service crate)
│       ├── device_io.rs              DeviceIO trait, MscBackend, MtpBackend, MockMtpHandle
│       ├── domain/
│       │   ├── mod.rs                Provider-neutral domain module
│       │   └── models.rs             Library/Artist/Album/Song/Playlist/Genre/Change DTOs
│       ├── providers/
│       │   ├── mod.rs                MediaProvider trait, capabilities, provider factory
│       │   ├── jellyfin.rs           JellyfinProvider adapter over JellyfinClient
│       │   └── subsonic.rs           Subsonic/OpenSubsonic/Navidrome provider adapter
│       ├── device/
│       │   ├── mod.rs                DeviceManifest, DeviceManager, MSC/MTP observers, mount detection
│       │   ├── mtp.rs                WpdHandle (Windows WPD COM), LibmtpHandle (Unix FFI)
│       │   └── tests.rs              Device module integration tests
│       └── tests.rs                  Top-level integration tests
└── hifimule-ui/                  Tauri 2 desktop shell (project part: "desktop")
    ├── package.json                  npm/pnpm; deps: @tauri-apps/api ~2.10, shoelace ^2.19.1, vite ^6
    ├── tsconfig.json
    ├── vite.config.ts
    ├── index.html                    Main window HTML entry point
    ├── splashscreen.html             Splashscreen window HTML
    ├── src/
    │   ├── main.ts                   Entry — splash/main routing, daemon readiness polling
    │   ├── rpc.ts                    rpcCall() via Tauri invoke (rpc_proxy); getImageUrl() via image_proxy
    │   ├── login.ts                  Provider-neutral login form → server.probe/server.connect
    │   ├── library.ts                Provider-neutral browser (modes, hierarchy, pagination, favorites)
    │   ├── state/
    │   │   └── basket.ts             BasketStore singleton (EventTarget, localStorage + daemon sync)
    │   └── components/
    │       ├── BasketSidebar.ts      Main sidebar: basket, capacity bar, sync flow, device hub
    │       ├── MediaCard.ts          sl-card grid item with basket toggle + image loading
    │       ├── StatusBar.ts          Bottom status bar (daemon health, last RPC, device name)
    │       ├── InitDeviceModal.ts    New device initialization wizard (sl-dialog)
    │       └── RepairModal.ts        Dirty manifest repair (missing/orphaned file reconciliation)
    └── src-tauri/
        ├── tauri.conf.json           Window config (main + splashscreen), sidecar, WiX/NSIS bundle
        ├── capabilities/
        │   └── default.json          Tauri capability grants
        ├── Cargo.toml
        ├── icons/                    App icons (PNG, .icns, .ico)
        └── src/
            ├── lib.rs                Tauri setup: rpc_proxy, image_proxy, get_sidecar_status commands
            └── main.rs               Tauri entry point
```

---

## Part 1: `hifimule-daemon`

### Language & Runtime
- **Rust** (MSRV 1.93.0), async via **Tokio** multi-thread runtime
- The Tokio runtime is spawned in a **background OS thread** (`start_daemon_core()`) so that macOS can own the main thread for the system tray event loop

### Key Dependencies

| Crate | Role |
|-------|------|
| `tokio` | Async runtime |
| `axum 0.8` | HTTP/JSON-RPC server |
| `rusqlite` (bundled) | SQLite database |
| `keyring 2.3` | OS credential store |
| `reqwest` | Jellyfin/Subsonic/OpenSubsonic HTTP clients |
| `serde / serde_json` | Serialization |
| `uuid` | Device ID generation |
| `tray-icon` + `tao` | System tray (cross-platform) |
| `notify-rust` | OS desktop notifications (sync complete) |
| `windows-sys` + `windows` | WPD MTP COM API (Windows only) |
| `libc` | libmtp FFI (Unix only) |
| `windows-service` | Windows Service integration (Windows only) |
| `bytes` | HTTP body streaming |
| `futures` | `join_all` for concurrent Jellyfin fetches |
| `async-trait` | Async trait objects (`DeviceIO`, `MediaProvider`) |
| `anyhow` | Error handling |
| `winresource` (build-dep) | Windows EXE icon embedding |
| `pkg-config` (build-dep) | libmtp detection on Unix |

### Module Responsibilities

| Module | LOC | Responsibility |
|--------|-----|----------------|
| `main.rs` | ~900 | Entry point, CLI flags, `DaemonState` enum, tray, auto-sync orchestration |
| `rpc.rs` | ~6900 | Axum server setup, AppState, CORS, provider-aware RPC dispatch, all handlers and tests |
| `api.rs` | ~1500 | `JellyfinClient`: Jellyfin API calls; `CredentialManager`: config.json + keyring secret map |
| `db.rs` | ~450 | SQLite CRUD: `devices`, `scrobble_history`, `server_config`; runtime migrations |
| `sync.rs` | ~4100 | `calculate_delta`, provider-aware `execute_sync`, path sanitization, M3U generation, warnings |
| `auto_fill.rs` | 325 | Priority-sorted capacity fill; `run_auto_fill`, `rank_and_truncate` |
| `scrobbler.rs` | 577 | Rockbox log parse, Jellyfin match, `process_device_scrobbles` |
| `transcoding.rs` | 142 | `DeviceProfileEntry`, `load_profiles`, `find_device_profile` |
| `paths.rs` | 52 | OS-appropriate app data dir resolution |
| `service.rs` | 232 | Windows Service lifecycle (install, uninstall, SCM handler) |
| `device_io.rs` | 504 | `DeviceIO` async trait; `MscBackend` (filesystem); `MtpBackend` (blocking wrapper) |
| `domain/models.rs` | ~230 | Provider-neutral library, artist, album, song, playlist, genre, change, and unit conversion models |
| `providers/mod.rs` | ~450 | `MediaProvider` trait, capabilities, server probing, provider factory, error taxonomy |
| `providers/jellyfin.rs` | ~1500 | Jellyfin adapter mapping Jellyfin DTOs to provider-domain models |
| `providers/subsonic.rs` | ~2400 | Subsonic/OpenSubsonic/Navidrome adapter, signed URLs, history/favorites/genre support |
| `device/mod.rs` | ~1800 | `DeviceManifest`, `DeviceManager` (multi-device), MTP cache fields, mount detection, observers |
| `device/mtp.rs` | ~1600 | `WpdHandle` (WPD COM), `LibmtpHandle` (libmtp FFI) |

### Entry Points

- **Interactive mode** (`run_interactive`): spawns Tokio runtime + tray icon event loop on main thread
- **Service mode** (`--service`): Windows Service dispatcher
- **Install/Uninstall** (`--install-service` / `--uninstall-service`): Windows SCM helpers
- **Auto-sync test** (`--auto-sync`): headless auto-sync trigger (for testing)

### Daemon Process Architecture

```
Main thread (macOS: event loop; Windows: main)
  └─ start_daemon_core() ──► Background OS thread ──► Tokio multi-thread runtime
                                                          ├─ Axum HTTP server (port 19140)
                                                          ├─ MSC device observer (polling, 1s)
                                                          ├─ MTP device observer (polling, 3s)
                                                          └─ Auto-sync trigger (on connect)
```

---

## Part 2: `hifimule-ui`

### Language & Build
- **TypeScript 5.6**, compiled by **tsc**, bundled by **Vite 6**
- Tauri 2 provides the native window shell and the Rust-side command handlers

### Key Dependencies

| Package | Role |
|---------|------|
| `@tauri-apps/api ~2.10` | Window management, `invoke()` IPC |
| `@tauri-apps/plugin-shell` | Sidecar spawning, process events |
| `@tauri-apps/plugin-opener` | Open URLs |
| `@shoelace-style/shoelace ^2.19.1` | Web components (buttons, cards, dialogs, etc.) |
| `vite ^6` | Dev server + production bundler |
| `typescript ~5.6` | Type checking |

### Module Responsibilities

| File | Responsibility |
|------|----------------|
| `main.ts` | DOMContentLoaded handler; splash vs main window routing; daemon readiness poll |
| `rpc.ts` | `rpcCall(method, params)`, `getImageUrl(id)`, provider-neutral browse DTOs and wrappers |
| `login.ts` | Media-server connection form, debounced `server.probe`, `server.connect` submission |
| `library.ts` | Provider-neutral browser: capability-driven modes, hierarchy, paginated items, quick-nav, favorite tree |
| `state/basket.ts` | `BasketStore` singleton: in-memory Map + localStorage + 1s debounced daemon save |
| `components/BasketSidebar.ts` | Main sidebar: basket list, capacity bar, sync flow, auto-fill, device hub, folder info |
| `components/MediaCard.ts` | Grid card: cover art (via image_proxy), basket toggle, navigation click |
| `components/StatusBar.ts` | Bottom bar: daemon connection status (3s poll), last RPC, device name |
| `components/InitDeviceModal.ts` | New device setup wizard (name, icon, folder, transcoding profile) |
| `components/RepairModal.ts` | Manifest repair: missing vs orphaned file comparison, prune/relink operations |

### Tauri Shell

The Rust-side `src-tauri/src/lib.rs` exposes three Tauri commands:

| Command | Description |
|---------|-------------|
| `rpc_proxy(method, params)` | Forwards JSON-RPC 2.0 call to daemon on port 19140; extracts `result` or surfaces `error.message` |
| `image_proxy(id, maxHeight?, quality?)` | Fetches image from daemon, re-encodes as `data:<type>;base64,...` |
| `get_sidecar_status()` | Returns daemon launch status string for the splash screen |

### Daemon Launch Strategy (in order)

1. **Health check** — `POST http://127.0.0.1:19140` with `get_daemon_state`; if successful, daemon is already running (startup app or previous instance)
2. **Windows Service** (Windows only) — `sc start hifimule-daemon`; verify with health check
3. **Sidecar spawn** — `app.shell().sidecar("hifimule-daemon").spawn()`; monitors stdout/stderr; kills on `RunEvent::Exit`

### Window Configuration

| Window | Size | Behavior |
|--------|------|----------|
| `splashscreen` | 400×500 | Transparent, no decorations, always-on-top; shows while daemon is starting |
| `main` | 1024×768 | Initially hidden; shown once daemon responds |

---

## CI / Release Pipeline

- **Trigger**: push of `v*` tags
- **Matrix**: macOS (universal — aarch64 + x86_64), Ubuntu 22.04, Windows
- **macOS**: builds universal binary; requires merging arm64+x86_64 libmtp dylibs via Homebrew
- **Ubuntu**: installs `libmtp-dev`, `libwebkit2gtk-4.1-dev`, etc.
- **Windows**: builds MSC + WPD variant
- **Artifacts**: Tauri bundles (`.dmg`, `.deb`, `.exe`) uploaded to GitHub Release via `tauri-action`

---

## Test Coverage

- `hifimule-daemon/src/api.rs` — Comprehensive mockito-based integration tests for Jellyfin API calls
- `hifimule-daemon/src/providers/*.rs` — Provider adapter tests for Jellyfin/Subsonic/OpenSubsonic mapping, capabilities, changes, and error sanitization
- `hifimule-daemon/src/db.rs` — In-memory SQLite tests for all CRUD operations and migrations
- `hifimule-daemon/src/auto_fill.rs` — Unit tests for `rank_and_truncate` (capacity, negatives, zero-size, break semantics)
- `hifimule-daemon/src/sync.rs` — Delta calculation tests
- `hifimule-daemon/src/device/tests.rs` — Device module tests
- `hifimule-daemon/src/tests.rs` — Top-level integration tests
- No UI tests (Tauri/DOM testing not wired up)
