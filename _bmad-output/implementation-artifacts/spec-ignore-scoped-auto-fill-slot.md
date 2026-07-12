---
title: 'Ignore Scoped Auto-Fill Slot Markers'
type: 'bugfix'
created: '2026-07-12'
status: 'done'
route: 'one-shot'
---

# Ignore Scoped Auto-Fill Slot Markers

## Intent

**Problem:** Provider auto-sync tried to resolve the virtual `__auto_fill_slot__:<serverId>` basket marker as a media item, causing Jellyfin to return HTTP 400.

**Approach:** Recognize bare and server-scoped auto-fill markers through one shared predicate, then exclude them before provider resolution and playlist creation.

## Suggested Review Order

**Marker contract**

- One predicate recognizes legacy, scoped, and safely malformed virtual markers.
  [`mod.rs:64`](../../hifimule-daemon/src/device/mod.rs#L64)

**Provider boundaries**

- Auto-sync removes virtual markers before favorite/normal item partitioning.
  [`main.rs:988`](../../hifimule-daemon/src/main.rs#L988)

- Playlist creation applies the same marker contract.
  [`rpc.rs:966`](../../hifimule-daemon/src/rpc.rs#L966)

**Regression coverage**

- Predicate boundaries protect scoped markers without matching near-prefix IDs.
  [`tests.rs:4`](../../hifimule-daemon/src/device/tests.rs#L4)

- Playlist regression covers both legacy and scoped marker forms.
  [`rpc.rs:12129`](../../hifimule-daemon/src/rpc.rs#L12129)
