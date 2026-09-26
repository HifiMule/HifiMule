# HifiMule — Source Tree Analysis

**Generated:** 2026-05-23 | **Last Updated:** 2026-09-27 | **Scan depth:** Deep

---

## Repository Structure

```
hifimule/
├── Cargo.toml                        Workspace root (members: i18n, lifecycle, daemon, UI Tauri crate)
├── Cargo.lock
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
│       ├── db.rs                     SQLite: devices, servers with library scope, scrobbles, auto-fill and book continuity
│       ├── sync.rs                   Role-aware delta/execution, staged transfer, paths, M3U generation
│       ├── playback/               Daemon-owned session, decoder, outputs, book progress, native controls
│       ├── auto_fill/
│       │   ├── mod.rs                Auto-fill facade, legacy fill path, provider-routed fill wiring
│       │   ├── fetch.rs              Async pool materialization from MediaProvider for pipeline fills
│       │   └── pipeline.rs           Pure configurable pipeline engine and tests
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
│       │   ├── subsonic.rs           Subsonic/OpenSubsonic/Navidrome provider adapter
│       │   └── audiobookshelf.rs     Books and Podcasts scoped provider; browse, direct media, progress
│       ├── device/
│       │   ├── mod.rs                DeviceManifest, DeviceManager, MSC/MTP observers, mount detection
│       │   ├── mtp.rs                WpdHandle (Windows WPD COM), LibmtpHandle (Unix FFI)
│       │   └── tests.rs              Device module integration tests
│       └── tests.rs                  Top-level integration tests
├── hifimule-i18n/                Shared localization crate
│   ├── Cargo.toml
│   ├── catalog.json                 Shared Rust/TypeScript translation source
│   └── src/                      Translation catalog and lookup helpers
├── hifimule-lifecycle/           Shared process contract (project part: "library")
│   ├── src/lib.rs                  Owner/UI locks, private runtime metadata, health, launch fencing
│   ├── src/bin/lifecycle-owner-probe.rs Cross-process test helper
│   └── tests/contract.rs           Lifecycle contract integration tests
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
    │   ├── audiobookshelfSetup.ts    Library-role choice validation for Audiobookshelf setup
    │   ├── podcastRecents.ts         Recent episode page dedupe/stale-request handling
    │   ├── i18n.ts                   UI translation lookup backed by shared catalog assets
    │   ├── serverIdentity.ts         Server label/icon formatting helpers
    │   ├── library.ts                Provider-neutral browser including Books, Authors, Shows, Episodes, Series/Collections
    │   ├── state/
    │   │   ├── basket.ts             BasketStore singleton (EventTarget, localStorage + daemon sync)
    │   │   ├── playback.ts           Playback snapshot store
    │   │   ├── autoFill.ts           Typed auto-fill configuration
    │   │   └── mediaSyncSelection.ts Book and Podcast basket item construction
    │   └── components/
    │       ├── ServerHub.ts          Multi-server switch/add/edit/remove/logout control
    │       ├── BasketSidebar.ts      Main sidebar: basket, capacity bar, sync flow, device hub
    │       ├── AutoFillPanel.ts      Configurable per-server auto-fill pipeline UI
    │       ├── PlaylistCurationView.ts Playlist edit/rename/delete/add/reorder view
    │       ├── TracksBrowseView.ts   Tracks-first browse mode with panels, A-Z strips, bulk actions
    │       ├── MediaCard.ts          sl-card grid item with basket toggle + image loading
    │       ├── PlaybackControls.ts   Session transport and seek controls
    │       ├── PlaybackDestination.ts Queue/history destination
    │       ├── DestinationHub.ts     Playback/device destination switcher
    │       ├── InitDeviceModal.ts    New device initialization wizard (sl-dialog)
    │       └── RepairModal.ts        Dirty manifest repair (missing/orphaned file reconciliation)
    └── src-tauri/
        ├── tauri.conf.json           Window config (main + splashscreen), sidecar, WiX/NSIS bundle
        ├── capabilities/
        │   └── default.json          Tauri capability grants
        ├── Cargo.toml
        ├── icons/                    App icons (PNG, .icns, .ico)
        └── src/
            ├── lib.rs                Tauri setup: lifecycle coordination, rpc_proxy, image_proxy, window activation
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
| `keyring 3.6` | OS credential store |
| `reqwest` | Jellyfin/Subsonic/OpenSubsonic/Audiobookshelf HTTP clients |
| `hifimule-lifecycle` | Owner discovery, startup and Quit coordination |
| `ffmpeg-next`, `cpal`, `souvlaki` | Playback decoding, audio output, and native media controls |
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

| Module | Responsibility |
|--------|----------------|
| `main.rs` | Entry point, CLI flags, tray, lifecycle owner, auto-sync orchestration |
| `rpc.rs` | Axum server, AppState, authenticated provider-aware JSON-RPC dispatch, browse/playback/sync handlers |
| `api.rs` | `JellyfinClient` and `CredentialManager` |
| `db.rs` | SQLite CRUD and migrations for devices, server library scope, scrobbles, auto-fill, and book continuity |
| `sync.rs` | Role-aware delta and execution, bounded staging, path construction, M3U generation, cancellation |
| `playback/*` | Daemon-owned session, FFmpeg decoding/streaming, CPAL outputs, queue/history, seeking, book progress, native controls |
| `auto_fill/*` | Provider fetch and pure pipeline selection, including podcast retention policy |
| `domain/models.rs` | Provider-neutral music plus Podcast show/episode DTOs |
| `providers/mod.rs` | `MediaProvider` trait, capabilities, book timing, playback and sync representation models |
| `providers/audiobookshelf.rs` | Scoped Books/Podcasts adapter, authenticated catalog/search/grouping, direct playback and progress |
| `providers/jellyfin.rs` / `subsonic.rs` | Music provider adapters |
| `device/mod.rs` | `DeviceManifest`, media roles and folders, multi-device manager and observers |
| `device_io.rs` / `device/mtp.rs` | MSC/MTP device I/O and platform MTP backends |

### Entry Points

- **Interactive mode** (`run_interactive`): spawns Tokio runtime + tray icon event loop on main thread
- **Service mode** (`--service`): Windows Service dispatcher
- **Install/Uninstall** (`--install-service` / `--uninstall-service`): Windows SCM helpers
- **Auto-sync test** (`--auto-sync`): headless auto-sync trigger (for testing)

### Daemon Process Architecture

```
Main thread (macOS: event loop; Windows: main)
  └─ start_daemon_core() ──► Background OS thread ──► Tokio multi-thread runtime
                                                          ├─ Axum HTTP server (descriptor-published loopback port)
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
| `@tauri-apps/plugin-opener` | Open URLs |
| `@shoelace-style/shoelace ^2.19.1` | Web components (buttons, cards, dialogs, etc.) |
| `vite ^6` | Dev server + production bundler |
| `typescript ~5.6` | Type checking |

### Module Responsibilities

| File | Responsibility |
|------|----------------|
| `main.ts` | DOMContentLoaded handler; splash vs main window routing; daemon readiness poll |
| `rpc.ts` | `rpcCall(method, params)`, `getImageUrl(id)`, provider-neutral browse DTOs and wrappers |
| `login.ts` / `audiobookshelfSetup.ts` | Server probing, Audiobookshelf discover/choice/commit, reauthentication |
| `library.ts` / `podcastRecents.ts` | Capability-driven music, Books, and Podcasts browse, search, paging and navigation |
| `state/basket.ts` | `BasketStore` singleton: in-memory Map + localStorage + 1s debounced daemon save |
| `state/playback.ts` | `PlaybackStore`: authoritative session snapshot, connection freshness, read coalescing, and subscriber lifecycle |
| `state/mediaSyncSelection.ts` | Book, Podcast Show, and Episode basket entries |
| `components/ServerHub.ts` | Multi-server chip/menu: switch, add, edit identity, remove, logout; reconciles legacy basket server IDs |
| `components/BasketSidebar.ts` | Main sidebar: basket list, capacity bar, sync flow, auto-fill, device hub, folder info |
| `components/MediaCard.ts` | Grid card: cover art, role-aware basket/playback actions, navigation click |
| `components/AlbumPlayButton.ts` / `TrackPreviewButton.ts` / `TrackQueueButton.ts` | Starts ordered album playback, auditions a track, or adds a track to the playback queue |
| `components/PlaybackControls.ts` | Floating transport bar: playback state, seeking, output selection, recovery guidance, and surface switch |
| `components/PlaybackDestination.ts` | Dedicated listening view with current playback plus paged upcoming/history queues and upcoming-item edits |
| `components/PlaylistCurationView.ts` | Playlist editor: rename/delete, artist/album filters, add/remove/reorder tracks |
| `components/TracksBrowseView.ts` | Tracks mode: artist/album/track panels, paginated loading, A-Z strips, multi-select |
| `components/AutoFillPanel.ts` | Builder UI for per-server auto-fill pipeline settings and live preview |
| `components/DestinationHub.ts` | Playback or selected device destination switcher |
| `components/InitDeviceModal.ts` | New device setup wizard (name, icon, folder, transcoding profile) |
| `components/RepairModal.ts` | Manifest repair: missing vs orphaned file comparison, prune/relink operations |

### Tauri Shell

The Rust-side `src-tauri/src/lib.rs` coordinates startup through `hifimule-lifecycle` and exposes native commands including:

| Command | Description |
|---------|-------------|
| `rpc_proxy(method, params)` | Bearer-authenticated JSON-RPC to the verified owner; forwards structured error code/data |
| `image_proxy(id, maxHeight?, quality?)` | Authenticated cover fetch from the verified daemon, returned as a data URL |
| `get_sidecar_status()` / `retry_daemon_startup()` | Lifecycle status and retry for the splash screen |
| `close_ui()` / `reload_main_window()` | UI process/window control |
| `report_ui_ready()` / `report_shutdown_rendered()` | Installed smoke-test acknowledgements |
| `settings_set_launch_on_startup()` | macOS launch agent setting |

### Daemon Launch Strategy (in order)

1. Resolve the private app-data runtime and check the owner descriptor against an authenticated `daemon.health` response.
2. If no compatible owner is running, create a generation-bound launch ticket and start the daemon through the platform path, then validate the new owner.
3. Keep the UI bound to that observed owner and reject a replacement owner during an in-flight request. A second UI launch activates the existing window through the lifecycle mailbox.

### Window Configuration

| Window | Size | Behavior |
|--------|------|----------|
| `splashscreen` | 400×500 | Transparent, no decorations, always-on-top; shows while daemon is starting |
| `main` | 1024×768 | Initially hidden; shown once daemon responds |

---

## Part 3: `hifimule-i18n`

`catalog.json` is the shared translation source for English, French, Spanish, and German. `src/lib.rs` embeds it for the daemon and handles locale detection, fallback, and interpolation; `hifimule-ui/src/i18n.ts` imports the same JSON through the UI build alias. The Rust crate's unit tests check supported languages and playback-key completeness. See [i18n Architecture](./architecture-hifimule-i18n.md) and [Localization Guide](./localization.md).

## Part 4: `hifimule-lifecycle`

`src/lib.rs` defines private runtime files, daemon and UI OS locks, owner descriptor, authenticated health, launch tickets, Quit generation, and UI activation mailbox. `src/bin/lifecycle-owner-probe.rs` drives cross-process tests in `tests/contract.rs`. Both the daemon and the Tauri Rust shell depend on the crate. See [Lifecycle Architecture](./architecture-hifimule-lifecycle.md).

## CI / Release Pipeline

`.github/workflows/build.yml`, `release.yml`, and `smoke-test.yml` are present. They build and package platform artifacts and run smoke checks; see [Release Guide](./release-guide.md) and [Development Guide](./development-guide.md).

---

## Test Coverage

- `hifimule-daemon/src/api.rs` — Comprehensive mockito-based integration tests for Jellyfin API calls
- `hifimule-daemon/src/providers/*.rs` — Jellyfin/Subsonic/Audiobookshelf adapter tests; Audiobookshelf fixtures and offline contract tests are under `hifimule-daemon/tests/fixtures/audiobookshelf/`
- `hifimule-daemon/src/db.rs` — In-memory SQLite tests for all CRUD operations and migrations
- `hifimule-daemon/src/auto_fill/*` — Unit tests for legacy fill, fetch/routing, and pure pipeline stages
- `hifimule-daemon/src/sync.rs` — Delta calculation tests
- `hifimule-daemon/src/device/tests.rs` — Device module tests
- `hifimule-daemon/src/tests.rs` — Top-level integration tests
- `hifimule-lifecycle/tests/contract.rs` — Cross-process owner and UI activation contract
- `hifimule-ui/tests/audiobookshelfBrowse.test.mjs`, `podcastRecents.test.mjs`, `story17-8Policy.test.mjs`, and `story17-8Selection.test.mjs` — Audiobookshelf browse, recent paging, and sync policy checks
- `hifimule-ui/tests/playback-bar.html` and `scripts/tests/playback-ui.test.mjs` — browser-style playback-bar and UI-contract coverage
