---
title: 'Autofill Device Playlist'
type: 'feature'
created: '2026-06-20'
status: 'done'
route: 'one-shot'
---

# Autofill Device Playlist

## Intent

**Problem:** Autofill sync copied tracks to the device but did not create a playlist containing the tracks chosen by autofill.

**Approach:** Reuse the existing device M3U generation path by adding a synthetic playlist named `Autofill` from the deduped autofill track set.

## Suggested Review Order

**Playlist Construction**

- Shared synthetic playlist helper uses the existing M3U data shape.
  [`rpc.rs:3598`](../../hifimule-daemon/src/rpc.rs#L3598)

- Provider autofill deltas append deduped autofill tracks to `Autofill`.
  [`rpc.rs:2723`](../../hifimule-daemon/src/rpc.rs#L2723)

- Multi-provider autofill reuses the dedup helper for ordered playlist tracks.
  [`rpc.rs:3574`](../../hifimule-daemon/src/rpc.rs#L3574)

- Legacy Jellyfin autofill path now emits the same synthetic playlist.
  [`rpc.rs:4683`](../../hifimule-daemon/src/rpc.rs#L4683)

**Sync Timing**

- Multi-server sync writes playlists after all provider groups copied files.
  [`rpc.rs:5117`](../../hifimule-daemon/src/rpc.rs#L5117)

**Verification**

- Existing dedup test now asserts playlist order and duplicate suppression.
  [`rpc.rs:8985`](../../hifimule-daemon/src/rpc.rs#L8985)
