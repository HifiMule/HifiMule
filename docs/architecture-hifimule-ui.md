# HifiMule UI — Architecture

**Part:** `hifimule-ui` | **Last Updated:** 2026-09-27 | **Scan depth:** Deep | **Workspace version:** 0.15.0

## Process structure

The desktop part combines a Tauri 2 Rust shell (`src-tauri/`) with a TypeScript 5.6/Vite 6 WebView (`src/`) and Shoelace components. `src/main.ts` routes splash, login, library, playback, and shutdown views. The UI renders daemon-owned state and sends actions through native commands. The daemon handles media-server credentials, playback, device I/O, and sync.

## Native lifecycle and IPC

`src-tauri/src/lib.rs` uses `hifimule-lifecycle` to resolve a private profile runtime, find or start a compatible daemon, and hold one UI instance lock. A second launch requests activation of the existing window through the lifecycle mailbox. The startup coordinator uses an epoch to discard late results from abandoned attempts. It reports `starting`, `ready`, `failed`, `stopping`, or `stopped` status with stable error codes.

The daemon listens on an available loopback port published in the private `owner.json` descriptor. The native `rpc_proxy` and `image_proxy` commands validate the observed owner and send the descriptor bearer token; they do not rely on a fixed port. `rpc_proxy` forwards JSON-RPC error `code`, `message`, and `data` to TypeScript. `image_proxy` returns authenticated provider artwork as a data URL. Additional commands include startup retry, UI close/reload, installed-smoke acknowledgements, and macOS launch-on-startup settings. See [Lifecycle Architecture](./architecture-hifimule-lifecycle.md) and [Integration Architecture](./integration-architecture.md).

## TypeScript RPC and state

`src/rpc.ts` wraps `invoke('rpc_proxy')`, defines typed server/browse/playback/destination DTOs, and increments a browse generation when the active server changes. A late response from the previous server raises `StaleBrowseResponse`; an unauthorized browse result emits the scoped reauthentication event. `src/state/playback.ts` projects authoritative daemon snapshots and coalesces reads. `src/state/basket.ts` manages selected device items and hydrates/persists them through manifest RPCs. `src/state/autoFill.ts` models pipeline settings; `src/state/mediaSyncSelection.ts` constructs Book, Podcast Show, and Episode basket entries.

## Server setup and library browser

`src/login.ts` probes servers and handles direct connection, Audiobookshelf discovery/choice/commit, and scoped reauthentication. The `audiobookshelfSetup.ts` helper validates provider and library-role choices. Each selected Audiobookshelf Books or Podcasts library appears as its own server in `ServerHub`.

`src/library.ts` displays only modes advertised by `browse.listModes`. Music views include artists, albums, tracks, playlists, genres, history, and favorites according to provider capabilities. Books views use Albums/Books, Authors, Series, and Collections; Podcasts use Shows and Recent Episodes with dedicated episode detail and play actions. `podcastRecents.ts` rejects stale pages and deduplicates by episode ID. Search uses bounded provider responses and can expose truncation. `MediaCard`, `TracksBrowseView`, and `PlaylistCurationView` render reusable browse and curation surfaces; provider-specific decisions come from typed metadata or capability flags.

## Playback and device destinations

`PlaybackControls` renders the floating transport bar, output picker, seek state, and recovery guidance. `PlaybackDestination` displays current, upcoming, and history occurrences. `AlbumPlayButton`, `PlaylistPlayButton`, `TrackPreviewButton`, and `TrackQueueButton` call daemon playback RPCs. The UI refreshes snapshots after mutations rather than maintaining a second player state. `DestinationHub` switches between the listening destination and connected devices.

`BasketSidebar` orchestrates selected device basket, capacity, auto-fill, sync status, and manifest repair entry points. `InitDeviceModal` and `RepairModal` cover setup and dirty-manifest reconciliation. Audiobookshelf role-specific folder settings and sync policy are sent to the daemon, which remains the source of truth for actual device writes.

Saved snapshots expose separate Add-to-basket and Replace-basket controls only for the selected configured physical destination. The target name and stable ID are visible before confirmation; Replace uses a target-named destructive prompt. `PlaybackSnapshots` fences planning and completion by snapshot, device, operation request generation, and component lifetime. Before planning it drains pending and dispatched legacy basket saves; after confirmed commit it hydrates `basketStore` only from the daemon-authoritative returned vector. Preflight limitations remain visible and never become optimistic local mutations.

## Build and tests

`package.json` provides `dev`, `build` (`tsc && vite build`), `preview`, and `tauri` scripts. The Tauri crate uses the shared workspace version. Run `rtk npm run build` from `hifimule-ui/` for the frontend and use the root build wrapper plus `scripts/prepare-sidecar.mjs` before packaging the desktop app. Node tests under `hifimule-ui/tests/` cover browse, playback, Audiobookshelf, and policy helpers; `scripts/tests/` covers UI and runtime contracts. See [Development Guide](./development-guide.md).
