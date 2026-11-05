# HifiMule UI — Architecture

**Part:** `hifimule-ui` | **Last Updated:** 2026-11-05 | **Scan depth:** Deep | **Workspace version:** 0.21.0

## Process structure

The desktop part combines a Tauri 2 Rust shell (`src-tauri/`) with a TypeScript/Vite WebView (`src/`) and Shoelace 2.20 web components. No React/Vue/Svelte — components are classes that build DOM via template strings with Shoelace elements. `src/main.ts` routes splash, login, library, playback, and shutdown views. The UI renders daemon-owned state and sends actions through native commands. The daemon handles media-server credentials, playback, device I/O, and sync.

## Native lifecycle and IPC

`src-tauri/src/lib.rs` uses `hifimule-lifecycle` to resolve a private profile runtime, find or start a compatible daemon, and hold one UI instance lock. A `StartupCoordinator` with epoch-based state discards late results from abandoned attempts. On macOS, daemon spawn uses a LaunchAgent; elsewhere it spawns directly. The daemon listens on an available loopback port published in the private `owner.json` descriptor. The native `rpc_proxy` and `image_proxy` commands validate the observed owner and send the descriptor bearer token. See [Lifecycle Architecture](./architecture-hifimule-lifecycle.md).

## TypeScript RPC and state

`src/rpc.ts` (~889 lines) wraps `invoke('rpc_proxy')`, defines typed DTOs for server/browse/playback/destination, and uses a browse generation counter to discard stale responses. `ERR_UNAUTHORIZED = -8` triggers a scoped re-auth flow. State modules in `src/state/` are framework-free singleton stores (`EventTarget`-based):

| Module | Purpose |
|--------|---------|
| `basket.ts` | Sync basket with localStorage + daemon sync (generation/hash locks), auto-fill slots |
| `playback.ts` | Playback snapshot store with BigInt sequence guards |
| `autoFill.ts` | Pure type mirror of daemon's `AutoFillPipeline` serde contract |
| `mediaSyncSelection.ts` | Book/Podcast basket item factories |
| `syncCapacity.ts` | Size math for planned vs actual selection |
| `basketEstimate.ts` | Cached per-basket size estimates |
| `libraryCache.ts` | Page cache + playlist write capability flag |
| `queue.ts` | Track queue helpers |
| `snapshotSaves.ts` | Snapshot save state |

## Components

26 components in `src/components/`:

| Component | Purpose |
|-----------|---------|
| `ServerHub` | Header bar: multi-server switch/add/edit/remove |
| `BasketSidebar` (~2,500 lines) | Main sync panel: basket, capacity, auto-fill, device chooser, repair |
| `DeviceProfileManager` (NEW) | Modal CRUD for device transcoding profiles via `device_profiles.*` RPCs |
| `PlaybackControls` (~950 lines) | Now-playing bar: transport, output picker, seek, feedback |
| `DestinationHub` | Switch between playback instance and physical devices |
| `PlaybackDestination` | Current/upcoming/history occurrences |
| `PlaybackSelectionSettings` | Playback configuration |
| `PlaybackSnapshots` | Listening snapshots browser with export controls |
| `AutoFillPanel` (~85 KB) | Auto-fill pipeline editor UI |
| `InitDeviceModal` | First-time device setup wizard |
| `RepairModal` | Dirty-manifest reconciliation |
| `library.ts` (~3,250 lines) | Main browse surface — virtualized grid, breadcrumbs, pagination |
| `MediaCard`, `TracksBrowseView`, `PlaylistCurationView` | Reusable browse/curation surfaces |
| Small button factories | `AlbumPlayButton`, `TrackPlayButton`, `TrackPreviewButton`, `TrackQueueButton`, `RadioPlayButton` |

## Build and tests

`package.json` scripts: `dev` (vite), `build` (`tsc && vite build`), `preview`, `tauri`. Dependencies: Shoelace ^2.20, Tauri API ~2.12, Vite ^8. Node tests in `hifimule-ui/tests/` and `scripts/tests/`. See [Development Guide](./development-guide.md).
