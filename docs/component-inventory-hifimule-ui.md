# Component Inventory — HifiMule UI

**Last Updated:** 2026-09-27 | **Scan depth:** Deep | **Workspace version:** 0.15.0

## Entry, views, and shared state

| File | Responsibility |
| --- | --- |
| `src/main.ts` | Route splash, login, library, playback destination, and shutdown; coordinate daemon readiness and window display |
| `src/login.ts` | Add server and reauthentication forms; Audiobookshelf discovery and per-library choice |
| `src/audiobookshelfSetup.ts` | Validate Audiobookshelf provider choice and library role data |
| `src/library.ts` | Capability-driven browse, search, navigation, card rendering, and role-specific Books/Podcasts surfaces |
| `src/podcastRecents.ts` | Reject stale Recent Episodes pages, deduplicate by ID, advance paging offset |
| `src/rpc.ts` | Typed JSON-RPC wrappers and DTOs; Tauri invoke proxy; stale-server response fencing |
| `src/state/basket.ts` | Device basket store and manifest hydration/persistence |
| `src/state/mediaSyncSelection.ts` | Build Book, Podcast Show, and Podcast Episode basket items with size estimates |
| `src/state/playback.ts` | Authoritative daemon playback snapshot projection and read coalescing |
| `src/state/autoFill.ts` | Auto-fill pipeline types and UI state |
| `src/i18n.ts` | Shared catalog lookup, language selection, document language |

## UI components

| Component | Role and reuse |
| --- | --- |
| `ServerHub.ts` | List, select, add, rename, reauthenticate, and remove servers; displays Audiobookshelf library role |
| `DestinationHub.ts` | Switch between desktop playback and connected device destinations |
| `BasketSidebar.ts` | Orchestrate selected device basket, capacity, sync preview/execution, role-aware settings, auto-fill, and repair entry |
| `MediaCard.ts` | Reusable cover/title/card with browse navigation and context-dependent basket or playback actions |
| `TracksBrowseView.ts` | Paginated tracks-first browse with artist/album/track panels and bulk selection |
| `PlaylistCurationView.ts` | Edit provider-supported playlists; Audiobookshelf Series/Collections are browse groupings rather than writable playlists |
| `AutoFillPanel.ts` | Edit per-server pipeline configuration, including Books/Podcasts policy controls |
| `InitDeviceModal.ts` | Initialize device identity, folders, and profile |
| `RepairModal.ts` | Reconcile missing/orphaned manifest entries after interrupted sync |
| `PlaybackControls.ts` | Floating transport, seek state, output picker, and recovery guidance |
| `PlaybackDestination.ts` | Current playback with paged upcoming and history sections |
| `AlbumPlayButton.ts` | Start ordered album or book-part playback |
| `PlaylistPlayButton.ts` | Start ordered playback of a playlist or grouping |
| `TrackPreviewButton.ts` | Audition a track without replacing the main queue |
| `TrackQueueButton.ts` | Add a track to the daemon-owned upcoming queue |

## Interaction patterns

The UI uses Shoelace web components for cards, buttons, dialogs, inputs, and alerts, with styling in `src/styles.css`. Components create or update DOM elements directly and use typed RPC helpers rather than a frontend framework store. The daemon is authoritative for playback, sync, device, and server state. `rpc.ts` rejects browse results from a previous selected server. Provider capabilities decide visible modes and edit controls.

Audiobookshelf Books use book/author labels and presentation credits; Podcasts use show, episode, and Recent Episodes views. The UI constructs role-aware basket entries but the daemon validates library identity, media representation, and actual device paths. See [Audiobookshelf Implementation Map](./audiobookshelf-implementation.md), [UI Architecture](./architecture-hifimule-ui.md), and [API Contracts](./api-contracts-hifimule-daemon.md).

## Tests

`hifimule-ui/tests/` covers browse and server switching, Audiobookshelf Book/Podcast views, recent episode paging, and role-specific sync policy. `scripts/tests/` covers playback and broader UI/runtime contracts. Run frontend type/build checks through `rtk npm run build` from `hifimule-ui/`.
