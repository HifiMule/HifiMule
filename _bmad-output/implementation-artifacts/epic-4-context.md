# Epic 4 Context: The Sync Engine & Self-Healing Core

<!-- Compiled from planning artifacts. Edit freely. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Build the performant, atomic sync logic that moves files from a media server to a connected device safely and correctly. This epic covers the full sync pipeline: device IO abstraction (MSC and MTP), differential manifest comparison, buffered streaming, legacy path sanitization, dirty-manifest resume, playlist M3U generation, transcoding negotiation, provider-neutral compatibility enforcement, and idempotent cleanup. Together these stories make sync reliable enough that a mid-sync disconnect or an incompatible format never corrupts a device or silently produces wrong output.

## Stories

- Story 4.0: Device IO Abstraction Layer
- Story 4.1: Differential Sync Algorithm (Manifest Comparison)
- Story 4.2: Atomic Buffered-IO Streaming
- Story 4.3: Legacy Hardware Constraints (Path & Char Validation)
- Story 4.4: Self-Healing "Dirty Manifest" Resume
- Story 4.5: "Start Sync" UI-to-Engine & Daemon-Initiated Trigger
- Story 4.6: Sync Progress — Time Remaining Estimation
- Story 4.7: Playlist M3U File Generation
- Story 4.8: Transcoding Handshake via Device Profiles
- Story 4.9: Provider-Neutral Transcoding, Compatibility, and Extension Verification
- Story 4.10: Idempotent Managed File Deletion on USB
- Story 4.11: (new)
- Story 4.12: (new)

## Requirements & Constraints

- Sync computes a delta (adds + deletes) against `.hifimule.json` before touching any file. Only necessary changes are made to preserve hardware longevity.
- All file writes use the Write-Temp-Rename pattern plus `sync_all()` (MSC). MTP has no rename; use a `.dirty` marker object written before overwrite, removed after — providing crash detection without native atomicity.
- The manifest is updated atomically after each file completes. Partial syncs leave the manifest in "Dirty" state; reconnect triggers resume from the last successful file.
- Streaming is direct from the media server into the device buffer — no intermediate local disk writes.
- Filenames and paths must be sanitized to fit legacy hardware limits (FAT32, Rockbox 255-char path limit) before writing; mappings are logged in the manifest.
- Unmanaged user files on the device must never be deleted or modified.
- Missing-file deletes during cleanup are treated as success (idempotent); genuine IO errors must surface.
- Transcoding profile selection is a hard device constraint. If the required transcoding cannot be negotiated with the provider, the track is skipped and excluded from the manifest — never written in an incompatible format.
- The manifest (`providerItemId` / `jellyfin_id`) is the source of truth for which tracks are present and under which profile they were written. `transcoding_profile_dirty = true` triggers delete+re-add for affected files on next sync.
- `sync.start` responds immediately with a `jobId`; progress is delivered via `on_sync_progress` events. The daemon auto-triggers sync for known devices with `auto_sync_on_connect` enabled.
- Sync must complete manifest audit and reach "ready to sync" in under 5 seconds. Throughput is bounded only by device write speed or network bandwidth.
- Retry network interruptions during streaming for at least 3 cycles before failing.
- Mid-sync disconnect must not leave the device unmountable; the dirty marker and Dirty manifest flag enable safe recovery.

## Technical Decisions

**DeviceIO trait (mandatory):** All device file operations go through `Arc<dyn DeviceIO>`. Direct `std::fs` calls targeting a device path are forbidden outside `MscBackend`. Backends:
- `MscBackend { root: PathBuf }` — wraps `std::fs`
- `MtpBackend { device: MtpHandle }` — WPD (Windows `windows-rs`) or `libmtp` (Linux/macOS `libmtp-rs`)

`DeviceManager` instantiates the correct backend at detection time and passes it to all downstream callers (sync engine, manifest handler, scrobble reader).

**Dirty-marker on reconnect:** If a `.dirty` marker object is present on an MTP device at connect time, `on_device_dirty` fires — same as the MSC dirty-manifest path.

**execute_sync() signature:** `execute_sync(..., device_io: Arc<dyn DeviceIO>, transcoding_profile: Option<serde_json::Value>)`. Both callers (`rpc.rs` `sync.start` and `main.rs` `run_auto_sync`) load these from the device manifest and pass them through.

**Transcoding:** Jellyfin — `POST /Items/{id}/PlaybackInfo` with the `DeviceProfile` payload; `TranscodingUrl` takes precedence over direct download. Subsonic — `stream.view?format=mp3&maxBitRate=<kbps>` (kbps, not bps); encapsulated in `SubsonicProvider::download_url()`. No Subsonic stream URL is constructed outside `providers/subsonic.rs`. Passthrough profile (`transcoding_profile_id = null`) skips negotiation and uses the standard download path unchanged.

**Incompatible format handling:** If the provider returns passthrough content whose type is incompatible with the active profile, the item is skipped and not added to the manifest. The safe fallback is omission, never passthrough.

**Playlist M3U generation:** Written to `manifest.playlistPath` (fallback: `managed_paths[0]`). Format: `#EXTM3U` header, `#EXTINF:<seconds>,<Artist> - <Title>` per track, relative paths with forward slashes. Only regenerated when `trackIds` hash changes; deleted from device when playlist leaves the basket. Atomicity via Write-Temp-Rename + `sync_all()`.

**Manifest fields relevant to this epic:** `synced_items` (keyed by `providerItemId`/`jellyfin_id`), `playlists` (array of `PlaylistManifestEntry`), `transcoding_profile_id`, `last_synced_transcoding_profile_id`, `transcoding_profile_dirty`, `playlist_path`, `server_id`. All new fields use `#[serde(default)]` for backward compatibility.

**device-profiles.json:** Seeded from `include_bytes!` embedded asset on first daemon start. User-editable post-install. `passthrough` profile (`deviceProfile: null`) explicitly disables transcoding.

**ETA calculation (Story 4.6):** UI-side only in `BasketSidebar.ts`. Formula: `bytes_remaining / avg_bytes_per_second`; cumulative average since `startedAt`. Displayed after 2 samples with non-zero bytes transferred.

**IPC:** `sync.start` → immediate `{ jobId }` + `on_sync_progress` events. Progress payload includes `bytesTransferred` and `totalBytes` (pre-computed at sync start).

## Cross-Story Dependencies

- 4.0 (DeviceIO) must land first — all subsequent stories (4.1–4.10) depend on `Arc<dyn DeviceIO>` being the only path to device files.
- 4.1 (differential algorithm) feeds the `SyncDelta` that 4.2 (streaming), 4.7 (playlists), and 4.10 (cleanup) consume.
- 4.8 (transcoding handshake) and 4.9 (compatibility enforcement) are layered — 4.9 adds skip logic on top of the URL-negotiation path 4.8 establishes.
- 4.4 (dirty resume) depends on the dirty-marker mechanism introduced by 4.0 for MTP and the existing MSC dirty-manifest flag.
- Stories 4.11 and 4.12 (new) extend this epic; they may depend on completed stories above — check their individual spec files for dependencies.
- Epic 5 (scrobble bridge) uses `device_io.read_file(".scrobbler.log")` — depends on 4.0 being stable.
