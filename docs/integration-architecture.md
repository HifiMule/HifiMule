# HifiMule — Integration Architecture

**Last Updated:** 2026-09-27 | **Scan depth:** Deep | **Workspace version:** 0.15.0

## Runtime map

```text
WebView (TypeScript) → Tauri invoke → native Tauri shell
                                      │ hifimule-lifecycle: owner descriptor + token
                                      ▼
                       loopback JSON-RPC / image route
                                      ▼
                   Rust daemon: RPC, playback, sync, SQLite
                         ├─ MediaProvider → Jellyfin / Subsonic / Audiobookshelf
                         └─ DeviceIO → MSC filesystem / WPD or libmtp MTP
```

The two processes share `hifimule-lifecycle` as a Rust dependency, and the daemon shares `hifimule-i18n` with the UI's direct catalog import. The WebView calls `rpc_proxy` and `image_proxy`; the native shell performs HTTP on its behalf because the release WebView origin cannot directly fetch loopback HTTP as mixed content.

## Ownership before requests

The daemon holds `runtime/owner.lock` and publishes a private owner descriptor after readiness. It binds an available `127.0.0.1` port, rather than the historical fixed port. The descriptor contains the port, instance ID, protocol version, launch generation, and bearer token. The Tauri shell reads it, authenticates `daemon.health`, and binds its startup epoch to that owner. Every RPC and image request uses the verified owner and token. If the owner changes during a request, the UI rejects the stale result. Port `19140` is reserved only as a legacy-daemon collision check during startup.

The shared library also coordinates generation-bound launch tickets, explicit Quit fencing, private runtime files, and one UI per profile. A duplicate UI uses an activation mailbox to focus the existing window. See [Lifecycle Architecture](./architecture-hifimule-lifecycle.md).

## JSON-RPC and image flow

The native proxy posts JSON-RPC 2.0 requests to the descriptor port at `/` and returns the `result` value. Errors retain `code`, `message`, and optional `data`; the TypeScript wrapper turns them into `RpcError` and emits an unauthorized event for affected browse calls. `get_daemon_state` hydrates the UI's owner binding. The image proxy sends an authenticated `GET /jellyfin/image/:id` request and returns a data URL; the route name remains for compatibility while the daemon resolves current-provider artwork.

See [API Contracts](./api-contracts-hifimule-daemon.md) for method names and request shapes. Direct debugging calls require the current descriptor bearer token and should be made against its port. The [Development Guide](./development-guide.md) gives a safe local example.

## Provider and server identity

`ServerManager` loads SQLite server rows and lazily instantiates `MediaProvider` adapters. `server_config.id` is a machine-local UUID for DB, vault, and cache operations; `serverId` is deterministic and portable for basket, manifest, playback source, and sync routing. Jellyfin and Subsonic-compatible IDs derive from server identity or URL/user. Audiobookshelf includes selected upstream library ID and `audiobook`/`podcast` role, so Books and Podcasts are separate configured servers.

Jellyfin, Subsonic/OpenSubsonic/Navidrome, and Audiobookshelf traffic stays daemon-side. Authenticated URLs, credentials, and tokens do not enter the WebView. Provider capabilities decide browse modes and playlist write behavior. Audiobookshelf uses dedicated show/episode models alongside book-as-album mapping; see [Audiobookshelf Implementation Map](./audiobookshelf-implementation.md).

## Playback and sync data flow

For playback, the UI sends a source with portable server ID and track/episode ID. The daemon resolves the provider, obtains a playback description, verifies the media representation, decodes through FFmpeg, and sends samples to CPAL. The daemon owns queue state, output selection, continuity, and native media controls. Book progress is player-owned and does not flow through device sync.

For sync, the UI persists a basket in the selected device manifest. The daemon expands selections through the origin provider, calculates adds/deletes against existing manifest entries, stages media, writes through MSC/MTP `DeviceIO`, and updates the manifest. Media roles choose music, audiobook, or podcast paths. Per-server auto-fill configuration lives in the manifest while runtime history stays in SQLite. The device remains portable because IDs and manifest state are not tied to the machine-local server UUID.

## Error and test boundaries

Lifecycle failures use stable codes from `hifimule-lifecycle`; provider errors are mapped to JSON-RPC codes and safe UI messages. `hifimule-lifecycle/tests/contract.rs` covers cross-process ownership, while daemon provider and RPC tests cover authenticated discovery and routing. Audiobookshelf controlled-server observations and installed-app evidence have different scopes; consult the [integration contract](./audiobookshelf-integration-contract.md).
