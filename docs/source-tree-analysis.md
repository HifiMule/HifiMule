# HifiMule — Source Tree Analysis

**Generated:** 2026-05-23 | **Last Updated:** 2026-11-05 | **Scan depth:** Deep | **Version:** 0.21.0

---

## Repository Structure

```
hifimule/
├── Cargo.toml                        Workspace root (members: i18n, lifecycle, daemon, UI Tauri crate)
├── Cargo.lock
├── .nvmrc                            Node 26.12.1
├── scripts/
│   ├── build-daemon.mjs              Cargo wrapper with per-platform FFmpeg/audio runtime provisioning
│   ├── run-tauri.mjs                 Tauri dev/build wrapper
│   ├── prebuilt-audio-runtime.mjs    Pinned prebuilt FFmpeg runtime consumption
│   ├── windows-audio-runtime.mjs     Windows UCRT64 audio runtime staging
│   ├── linux-audio-runtime.mjs       Linux audio runtime provisioning
│   ├── macos-audio-runtime.mjs       macOS audio runtime provisioning
│   ├── verify-audio-runtime.mjs      Runtime verification
│   ├── prepare-sidecar.mjs           Copies compiled daemon → ui/src-tauri/sidecars/
│   ├── prepare-draft-release.cjs     Release draft preparation
│   └── smoke-tests/                  Smoke test scripts
├── .github/workflows/
│   ├── build.yml                     CI: playback evidence matrix (Win/Linux/macOS x64+arm64)
│   ├── release.yml                   Release: macOS x64/arm64, Linux x64, Windows x64 MSI
│   └── smoke-test.yml                Reusable smoke test workflow
├── changelogs/                       Per-version changelogs (0.10.0 through 0.21.0)
├── hifimule-daemon/              Rust backend (project part: "backend")
│   ├── Cargo.toml                    Two bins: hifimule-daemon + mediacheck
│   ├── assets/device-profiles.json   Embedded default device/transcoding profiles
│   └── src/
│       ├── main.rs                   Entry point, DaemonState, Tokio runtime, tray event loop
│       ├── rpc.rs (~19k lines)       Axum JSON-RPC server, ~130-method dispatch, AppState
│       ├── api.rs                    JellyfinClient (reqwest), CredentialManager
│       ├── db.rs                     SQLite: devices, server_config, scrobble_history, autofill_*, rockbox_ratings
│       ├── vault.rs                  ChaCha20-Poly1305 encrypted credential vault
│       ├── sync.rs (~14k lines)      Role-aware delta/execution, staged transfer, paths, M3U, folder artwork
│       ├── sync/                     sync/artwork.rs, sync/protection.rs (playback coexistence policy)
│       ├── rockbox.rs (~4k lines)    Rockbox tagcache binary reader/writer, rating sync
│       ├── playback/ (~30 modules)   Session, decoder, outputs, book progress, native controls, snapshots
│       ├── auto_fill/                mod.rs, fetch.rs, pipeline.rs (configurable pipeline engine)
│       ├── scrobbler.rs              Rockbox .scrobbler.log parser, provider submission
│       ├── transcoding.rs + transcoding/local.rs  Device profiles + FFmpeg local transcode
│       ├── folder_icons.rs           Folder artwork publisher (KDE/Windows/macOS)
│       ├── device_music_scan.rs      Read-only device music discovery
│       ├── device/                   DeviceManifest, DeviceManager, MTP (mtp.rs), manifest_codec.rs
│       ├── device_io.rs              DeviceIO trait, MSC/MTP backends
│       ├── domain/models.rs          Provider-neutral DTOs
│       ├── providers/                MediaProvider trait + jellyfin/subsonic/audiobookshelf adapters
│       ├── server_manager.rs         Multi-server management, lazy provider connections
│       ├── paths.rs                  OS-aware app data paths
│       ├── service.rs                Windows Service (cfg(windows))
│       ├── notifications.rs          OS notifications
│       ├── source_audio.rs           Audio format detection
│       ├── bin/mediacheck.rs         Packaged library audit tool
│       └── rpc/                      playback_export, playback_feedback, playback_selection, tests
├── hifimule-i18n/                Shared localization crate
│   ├── Cargo.toml
│   ├── catalog.json                 4 languages × 1,061 keys (en/fr/es/de)
│   └── src/lib.rs                  Translation lookup, language normalization, fallback
├── hifimule-lifecycle/           Shared process contract
│   ├── src/lib.rs (~1,600 lines)    Owner/UI locks, descriptor, health, launch tickets, activation
│   └── src/bin/lifecycle-owner-probe.rs
└── hifimule-ui/                  Tauri 2 desktop shell (project part: "desktop")
    ├── package.json                  Shoelace ^2.20, Tauri ~2.12, Vite ^8, TS 6/7 dual
    ├── src/
    │   ├── main.ts                   App bootstrap, window/lifecycle orchestration
    │   ├── rpc.ts (~889 lines)       RPC client via Tauri rpc_proxy
    │   ├── library.ts (~3,250 lines) Virtualized browse grid
    │   ├── login.ts                  Server probe/connect, ABS setup
    │   ├── state/                    basket, playback, autoFill, mediaSyncSelection, syncCapacity, etc.
    │   └── components/ (26 files)    ServerHub, BasketSidebar, DeviceProfileManager, PlaybackControls, etc.
    └── src-tauri/
        ├── Cargo.toml                tauri 2.12, hifimule-lifecycle path dep
        └── src/lib.rs (~1,460 lines) StartupCoordinator, daemon spawn, rpc_proxy
```

---

## Critical Folders

| Folder | Purpose |
|--------|---------|
| `hifimule-daemon/src/rpc.rs` | Single JSON-RPC dispatch — all ~130 methods in one match block |
| `hifimule-daemon/src/sync.rs` | Sync engine — delta computation, transfer, playlist generation |
| `hifimule-daemon/src/rockbox.rs` | Rockbox tagcache binary format — the on-device database |
| `hifimule-daemon/src/providers/` | MediaProvider trait + 3 server adapters |
| `hifimule-daemon/src/playback/` | Daemon-owned playback session, decoder, outputs |
| `hifimule-ui/src/library.ts` | Main browse UI — largest frontend file |
| `hifimule-ui/src/components/` | 26 Shoelace-based UI components |
| `hifimule-ui/src/state/` | 9 framework-free state stores |
| `hifimule-lifecycle/src/lib.rs` | Cross-process ownership contract |
| `scripts/` | Build, runtime provisioning, release automation |
| `.github/workflows/` | CI build matrix, release pipeline, smoke tests |
