---
title: 'Fix null originalBitrate for M4A/AAC files from Jellyfin'
type: 'bug-fix'
created: '2026-05-26'
status: 'done'
route: 'one-shot'
---

## Intent

**Problem:** When syncing M4A/AAC audio from a Jellyfin server, `originalBitrate` on the resulting `SyncedItem` is always `null`. This prevents the quality-upgrade re-sync logic (Story 4.11) from ever firing for M4A tracks, even when the server has a higher-bitrate version.

**Approach:** Jellyfin often leaves `MediaSource.Bitrate` null for M4A/AAC containers and only populates the bitrate in `MediaSource.MediaStreams[].BitRate` (stream `Type = "Audio"`). Added a `MediaStream` struct to `api.rs`, wired it into `MediaSource.media_streams`, and extended the three bitrate-extraction sites (`jellyfin_item_to_desired_item` in `rpc.rs`, its inline duplicate in `handle_sync_calculate_delta`, and the `to_desired_item` function in `main.rs`) to fall through to the audio stream's `BitRate` when the container-level field is absent. Added three regression unit tests covering: fallback-to-stream, container-takes-precedence, and no-audio-stream-returns-None.

## Suggested Review Order

1. [`hifimule-daemon/src/api.rs:69`](../../hifimule-daemon/src/api.rs) — `MediaStream` struct + `media_streams` field on `MediaSource`; verify serde renames match Jellyfin wire format (`"Type"`, `"BitRate"`)
2. [`hifimule-daemon/src/rpc.rs:1826`](../../hifimule-daemon/src/rpc.rs) — `jellyfin_item_to_desired_item` extraction chain with audio-stream fallback
3. [`hifimule-daemon/src/rpc.rs:2448`](../../hifimule-daemon/src/rpc.rs) — inline duplicate in `handle_sync_calculate_delta` (identical logic)
4. [`hifimule-daemon/src/main.rs:918`](../../hifimule-daemon/src/main.rs) — third extraction site in auto-sync `to_desired_item` fn
5. [`hifimule-daemon/src/rpc.rs:5776`](../../hifimule-daemon/src/rpc.rs) — three new regression tests (`test_jellyfin_item_original_bitrate_*`)
