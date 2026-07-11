---
title: 'Fix Auto-Sync Playlist And Autofill'
type: 'bugfix'
created: '2026-07-06'
status: 'done'
route: 'one-shot'
---

# Fix Auto-Sync Playlist And Autofill

## Intent

**Problem:** Auto-sync with a playlist/basket plus auto-fill could resolve only the playlist items, leaving remaining device capacity unused; playlist-only sync work could also be skipped when no files needed copying.

**Approach:** Reuse one auto-fill merge helper in daemon auto-sync, run Jellyfin auto-fill after non-empty basket resolution, generate the synthetic `Autofill` playlist, and treat playlist-only deltas as sync work.

## Suggested Review Order

**Jellyfin Auto-Sync Fill**

- Non-empty basket auto-sync now fills remaining budget after playlist/basket resolution.
  [`main.rs:751`](../../hifimule-daemon/src/main.rs#L751)

**Auto-Fill Merge**

- Shared helper dedups against manual tracks, appends desired items, and emits `Autofill`.
  [`main.rs:996`](../../hifimule-daemon/src/main.rs#L996)

**Playlist-Only Work**

- Auto-sync no longer exits when the only pending work is playlist generation.
  [`main.rs:1038`](../../hifimule-daemon/src/main.rs#L1038)

**Provider Auto-Sync**

- Provider auto-sync reuses the same helper so provider fills also get `Autofill`.
  [`main.rs:1360`](../../hifimule-daemon/src/main.rs#L1360)

**Verification**

- Helper test covers deduped fill tracks and synthetic playlist generation.
  [`main.rs:1062`](../../hifimule-daemon/src/main.rs#L1062)
- Regression test covers playlist-only auto-sync work.
  [`main.rs:1094`](../../hifimule-daemon/src/main.rs#L1094)
