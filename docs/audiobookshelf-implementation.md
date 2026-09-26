# Audiobookshelf implementation map

**Last Updated:** 2026-09-27 | **Scan depth:** Deep | **Workspace version:** 0.15.0

This page describes the implementation in the current source tree. The [Audiobookshelf integration contract](./audiobookshelf-integration-contract.md) records dated, controlled upstream observations and earlier feature gates; its historical "later story" wording does not describe current feature availability.

## Where the feature lives

| Concern | Current code |
| --- | --- |
| Discovery, scoped provider, catalog, search, grouping, playback, sync media, progress | `hifimule-daemon/src/providers/audiobookshelf.rs` |
| Provider contract and book timing | `hifimule-daemon/src/providers/mod.rs` |
| Server records and portable identity | `hifimule-daemon/src/db.rs`, `hifimule-daemon/src/server_manager.rs` |
| JSON-RPC setup, browse, playback, selection, sync | `hifimule-daemon/src/rpc.rs` |
| Book continuity | `hifimule-daemon/src/playback/book_progress.rs` and `playback/` |
| Manifest roles and device paths | `hifimule-daemon/src/device/mod.rs`, `hifimule-daemon/src/sync.rs` |
| Login, library browser, typed RPC, basket selection | `hifimule-ui/src/login.ts`, `library.ts`, `rpc.ts`, `state/mediaSyncSelection.ts` |
| Offline contract and UI tests | `hifimule-daemon/tests/audiobookshelf_contract.rs`, `hifimule-ui/tests/audiobookshelfBrowse.test.mjs`, `story17-8Policy.test.mjs`, `story17-8Selection.test.mjs` |

## Connection and identity

`server.audiobookshelf.discover` authenticates in the daemon and returns temporary, opaque library choices with `audiobook` or `podcast` roles. `server.audiobookshelf.commit` consumes one setup and choice, stores the selected upstream library ID and role in `server_config`, persists the credential through `CredentialManager`, and creates a scoped provider. Cancel discards a pending setup. Each selected Books or Podcasts library is a separate configured server. Reauthentication is scoped to that server.

`derive_audiobookshelf_server_id` hashes normalized URL, username, immutable upstream library ID, and role into a portable ID. The machine-local server UUID remains the database/vault/provider-cache key. Provider item IDs are opaque and encode the selected library plus upstream item, media, and part/episode identity; display titles and paths are never identity. The provider rejects items that leave the selected library or change role.

## Browse and search

Books map to the provider-neutral album/track model: an audiobook is an album and each ordered audio file is a track. Authors are the artist-equivalent, with narrator credits exposed as presentation metadata. The UI labels Books and Authors for an audiobook server and can show Series and Collections as ordered, playlist-like groups. The provider fetches the selected library's catalog and grouping endpoints, validates membership, and preserves source order.

Podcasts use dedicated `PodcastShow` and `PodcastEpisode` models and RPCs. The UI has a Shows tab and a Recent Episodes tab. The recent page helper discards stale requests, deduplicates episode IDs, and uses `sourceCount` when the upstream page contains entries that cannot be mapped. Both book and podcast search use bounded upstream requests and report possible truncation rather than claiming complete results. `browse.listModes` advertises only capabilities for the selected library role.

## Playback and progress

Book parts and podcast episodes play through the daemon-owned playback session. `playback.playAlbum` handles a book's ordered parts; `playback.playEpisode` validates the selected podcast role and episode before issuing a track operation. `playback.playPlaylist` can play an ordered grouping. The provider negotiates direct media and validates authenticated range delivery. The player gates seeking on the actual opened container/codec and verified range behavior; the controlled contract supports direct MP3 and AAC/MP4 candidates. HLS/transcode is not an accepted byte-stream representation for this path.

`playback/book_progress.rs` holds player-private whole-book timing and continuity. It verifies the current book identity and part timing before writing progress to Audiobookshelf; progress is not part of ordinary device sync. Podcast episode playback uses the provider playback path without treating an episode as a generic `Song` browse item. See [Playback Guide](./playback.md) and the controlled contract for evidence limits.

## Device sync and auto-fill

The device manifest records `MediaRole` (`music`, `audiobook`, `podcast`) on synced items. Optional `audiobookPath` and `podcastPath` route those roles to separate managed folders; each falls back to the primary managed path. The basket supports `Book`, `PodcastShow`, and `PodcastEpisode` selections. Sync planning expands them to real parts/episodes, checks direct media representation for non-music items, and retains media role in the delta. Podcast filenames include show and publication-date context when available. The transfer path stages media with bounded memory before device writes.

Auto-fill remains per `(device, portable serverId)` in the manifest. Audiobookshelf Book and Podcast selections use role-aware candidates; podcast configuration includes a `podcastRetention` policy with recent-count and show selection. The daemon's auto-sync path loads the selected server's saved pipeline. Consult [Auto-Fill Deep-Dive](./deep-dive-autofill.md) for the general pipeline; that older page predates Audiobookshelf and should be read with this page for role-specific behavior.

## Evidence and limits

The provider has offline DTO, authentication, identity, catalog, search, playback, and sync tests; the UI has Audiobookshelf browse and policy tests. The controlled [integration contract](./audiobookshelf-integration-contract.md) records Audiobookshelf v2.36.1 observations and distinguishes transport shape from installed-app playback evidence. Its direct-media seek qualification does not by itself prove installed playback on every platform or server version.

## Useful starting points for changes

- New server setup or credentials: inspect `handle_audiobookshelf_discover` and `handle_audiobookshelf_commit` in `rpc.rs`, then `AudiobookshelfProvider::discover` and `scope_to`.
- Browse shape: change provider models and mapping first, then typed responses in `hifimule-ui/src/rpc.ts`, then `library.ts`.
- Playback: preserve session and progress ownership; verify direct representation before altering decoder or seek capability.
- Sync: preserve role, stable ID, managed folder, and manifest compatibility across `rpc.rs`, `sync.rs`, and `device/mod.rs`.
