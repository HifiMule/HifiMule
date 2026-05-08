# Sprint Change Proposal — Post-Testing UX & Engine Improvements

**Date:** 2026-03-28
**Author:** Alexis (with SM agent)
**Status:** Approved (2026-03-28)

---

## 1. Issue Summary

**Problem Statement:** Post-implementation testing of Epic 3 and Epic 4 surfaced four improvement areas: the artist view becomes slow and stateless on large catalogs; the sync progress display gives no time estimate; the auto-fill basket model is cumbersome (slow to toggle, clutters the basket with stale tracks); and playlist syncs produce no native playlist file on the device.

**Discovery:** Direct testing by Alexis following Story 3.6 (Auto-Fill) reaching `review` status.

**Evidence:**
- Artist view: scroll position lost on back-navigation; every return triggers a full re-fetch; no way to jump to a letter or search by name on large libraries.
- Sync progress: percentage and filename visible but no ETA, leaving users uncertain whether to wait or leave.
- Auto-fill: enabling the toggle triggers a full Jellyfin query + progressive stream; disabling clears all those items; the basket fills with potentially hundreds of individual tracks that may be stale by sync time.
- Playlist sync: tracks land on device but no `.m3u` file is written, so DAPs and Rockbox cannot load the playlist natively.

---

## 2. Impact Analysis

### Epic Impact

| Epic | Impact |
|------|--------|
| Epic 3 | Story 3.6 reworked (significant); new Story 3.7 added |
| Epic 4 | New Stories 4.6 and 4.7 added |
| Epics 1, 2, 5, 6 | Unaffected |

### Story Impact

| Story | Change | Reason |
|-------|--------|--------|
| 3.6 Auto-Fill | **Rework** — virtual reservation model replaces basket population | Current model is slow to toggle and uses stale selections |
| 3.7 (new) | **Add** — artist view cache, scroll state, alpha-jump bar, search box | Performance and navigation gap on large catalogs |
| 4.6 (new) | **Add** — sync progress ETA | Missing user feedback during sync |
| 4.7 (new) | **Add** — playlist M3U file generation | Playlists not usable natively on DAP/Rockbox without .m3u |

### Artifact Conflicts

| Artifact | Sections Affected |
|----------|------------------|
| PRD | FR29 (auto-fill algorithm runs at sync time, not basket population) |
| Architecture | `basket.autoFill` IPC removed; `sync.start` payload updated; `device.setBasket` IPC added; `on_sync_progress` event schema extended; `.hifimule.json` manifest schema updated (basket block + playlists array + autoSyncOnConnect promotion) |
| UX Spec | §5.3 Auto-Fill Components (remove Auto Badge, Priority Reason Tags; update toggle description; add reservation slot component); §5.1 Foundation Components (artist view: alpha-jump bar, search box, scroll restoration); §5.2 Custom Components (Sync Basket: ETA label) |
| Story 2.3 tech notes | `autoSyncOnConnect` now at device root of manifest, not inside `autoFill` block |
| Story 4.5 tech notes | `on_sync_progress` payload updated with `bytesTransferred` and `totalBytes` |
| Stories 3.1, 3.6 tech notes | `basket.autoFill` RPC removed; `sync.start` payload updated |

### Technical Impact

- **IPC contract changes:** `sync.start` payload restructured; `basket.autoFill` removed; `device.setBasket` added; `on_sync_progress` schema extended. All additive or scoped to the changed stories.
- **Manifest schema change:** Breaking change to `.hifimule.json` `autoFill` block — requires migration logic for existing managed devices (split `autoSyncOnConnect` out, introduce `basket` block).
- **No daemon memory impact:** Auto-fill reservation is pure UI state until sync runs.
- **No new external dependencies.**

---

## 3. Recommended Approach

**Direct Adjustment** — modify Story 3.6 in place and add three new stories to Epics 3 and 4 within the existing plan.

**Rationale:**
- No epic restructuring needed; all changes slot naturally into in-progress epics.
- Story 3.6 is in `review` and has not been merged; rework cost is low.
- Stories 3.7, 4.6, 4.7 are self-contained additions with no dependency conflicts.
- Manifest migration (autoFill → basket schema) is a one-time forward migration scoped to Story 3.6 implementation.

**Effort:** Low–Medium
**Risk:** Low — no architectural style change, no new technology, IPC changes are scoped
**Timeline Impact:** Minimal — 3.6 returns to backlog; three new stories added to backlog

---

## 4. Detailed Change Proposals

### 4.1 Story 3.6 — Auto-Fill Sync Mode (Reworked)

**Story: [3.6] Auto-Fill Sync Mode — Virtual Reservation**

```
OLD: Basket populates with individual ranked tracks from basket.autoFill
     stream. Auto Badge and Priority Reason Tags displayed per item.

NEW: Enabling the Auto-Fill toggle adds a single "Auto-Fill Reservation"
     slot to the basket instantly (no RPC call). The slot displays
     reserved bytes (device free space minus manual selections). Actual
     track selection runs daemon-side at sync time using the priority
     algorithm (favorites → play count → creation date).
```

**Acceptance Criteria (new):**

*Enabling Auto-Fill*
- When Auto-Fill toggle is enabled → a single reservation slot appears immediately showing "Auto-Fill · X GB reserved". No RPC call made. Storage Projection bar fills to full capacity.
- Reservation size = device free space − manually added items.
- When Max Fill Size slider is adjusted → reservation slot and projection bar update instantly. No RPC call.
- When Auto-Fill toggle is disabled → reservation slot removed instantly. No RPC call.

*Sync Execution*
- `sync.start` payload includes `{ autoFill: { maxBytes: N, excludeItemIds: [...] } }` instead of a flat track ID list.
- Daemon runs priority algorithm live at sync start; deduplicates against manual items.
- Sync follows same differential algorithm, buffered IO, and manifest update logic as standard sync.

*Basket Persistence in Manifest*
- On basket change (item add/remove, auto-fill toggle, slider adjust) → daemon writes basket state to `.hifimule.json` via `device.setBasket` RPC using Write-Temp-Rename pattern.
- On known device reconnect → UI restores basket from manifest (manual items + auto-fill reservation slot if enabled). No auto-fill track resolution on restore.
- Manual basket clear → writes `{ "manualItemIds": [], "autoFill": { "enabled": false, "maxBytes": null } }`.

**Removed from Story 3.6:**
- "Auto" badge on individual tracks
- Priority Reason Tags (★ Favorite, ▶ 47 plays, "New")
- Progressive item stream from `basket.autoFill`
- Basket populating with individual auto-selected tracks

**IPC Changes:**

```
REMOVED:  basket.autoFill
ADDED:    device.setBasket
            params: { deviceId, manualItemIds: string[],
                      autoFill: { enabled, maxBytes } }

UPDATED:  sync.start payload
  OLD: { deviceId, itemIds: string[] }
  NEW: { deviceId, manualItemIds: string[],
         autoFill?: { maxBytes: number, excludeItemIds: string[] } }
```

**Manifest Schema Change:**

```json
OLD:
{
  "autoFill": { "enabled": true, "maxBytes": null, "autoSyncOnConnect": true }
}

NEW:
{
  "autoSyncOnConnect": true,
  "basket": {
    "manualItemIds": [],
    "autoFill": { "enabled": false, "maxBytes": null }
  }
}
```

*Migration note:* On first read of an existing manifest with old `autoFill` shape, daemon migrates in-place: promotes `autoSyncOnConnect` to device root, initialises `basket` block with `enabled: false`.

---

### 4.2 New Story 3.7 — Artist View Cache, Scroll State & Quick Navigation

**Story: [3.7] Artist View — Cache, Scroll State & Quick Navigation**

*Cache & Scroll*
- Artist/album list results cached in-memory (TypeScript Map, keyed by RPC method + params hash).
- Back-navigation restores scroll position without a fetch.
- Cache TTL: 5 minutes. Invalidated on `sync.complete` or manual refresh.
- On stale cache: display cached content immediately, refresh in background.

*Alphabetical Jump Bar*
- A–Z + # index bar visible alongside the artist list.
- Clicking a letter scrolls instantly to the first matching artist (client-side).
- Letters with no matching artist are visually dimmed.

*Search Box*
- Search box at top of artist view.
- Real-time client-side filter over cached results (case-insensitive, debounced 150ms). No RPC call.
- Clearing search restores full list at previous scroll position.
- Search box disabled with loading indicator until cache is populated.

*Technical Notes:* All three features operate on the same cached dataset — no additional RPC traffic. Alphabetical bar built client-side from cached names. Component: vertical `<sl-button-group>` or custom sticky element. No daemon changes required.

---

### 4.3 New Story 4.6 — Sync Progress ETA

**Story: [4.6] Sync Progress — Time Remaining Estimation**

- After ≥ 2 `on_sync_progress` samples: display ETA as rolling average of last 5 samples (`bytes_remaining / avg_bytes_per_second`).
- Format: ≥ 60s → "~N min left"; < 60s → "~N sec left"; < 10s → "Almost done…"
- Before 2 samples: display "Calculating…"
- ETA displayed below progress bar in Sync Basket sidebar and tray tooltip.
- On sync complete: replaced by "Sync Complete".

**IPC Change (`on_sync_progress` event — additive):**

```
OLD: { jobId, filesCompleted, totalFiles, percentage, currentFilename }
NEW: { jobId, filesCompleted, totalFiles, percentage, currentFilename,
       bytesTransferred, totalBytes }
```

*ETA calculation is UI-side; daemon only adds the two byte-count fields.*

---

### 4.4 New Story 4.7 — Playlist M3U File Generation

**Story: [4.7] Playlist Sync — M3U File Generation**

- When a Jellyfin playlist is in the sync basket and sync runs → write a `.m3u` file to the root of the managed sync folder.
- Filename: sanitized playlist name + `.m3u` (uses existing Story 4.3 sanitization; truncated if over hardware path limit).
- Format: extended M3U (`#EXTM3U`, `#EXTINF:<seconds>,<Artist> - <Title>`, relative path per track).
- Track paths relative to `.m3u` location.
- Duration sourced from Jellyfin `RunTimeTicks` (÷ 10,000,000 = seconds). No additional API calls — uses already-fetched sync job metadata.
- Differential sync: `.m3u` not rewritten if playlist unchanged (manifest-tracked). Regenerated if tracks change. Deleted if playlist removed from basket (managed cleanup only).
- Write pattern: Write-Temp-Rename (atomic).

**Manifest addition:**

```json
"playlists": [
  {
    "jellyfinId": "abc123",
    "filename": "Running Mixes.m3u",
    "trackCount": 24,
    "lastModified": "2026-03-28T..."
  }
]
```

---

## 5. Implementation Handoff

### Change Scope: Minor

All code changes are scoped within existing epics. Story 3.6 returns to backlog for rework; three new stories added to backlog.

| Recipient | Responsibility |
|-----------|---------------|
| **Dev** | Rework Story 3.6: virtual reservation model, `device.setBasket` IPC, `sync.start` payload, manifest migration, basket restore on reconnect |
| **Dev** | Implement Story 3.7: UI-layer cache, scroll restoration, alpha-jump bar, search box |
| **Dev** | Implement Story 4.6: `bytesTransferred`/`totalBytes` fields in `on_sync_progress`; ETA calculation in UI |
| **Dev** | Implement Story 4.7: M3U generation in sync engine, manifest `playlists` tracking |
| **Architect** | Update architecture doc: IPC table (`basket.autoFill` removed, `device.setBasket` added, `sync.start` updated, `on_sync_progress` schema); manifest schema diagram; note manifest migration |
| **SM/Dev** | Update story tech notes: 2.3 (`autoSyncOnConnect` location), 3.1 (cache context), 4.5 (`on_sync_progress` payload) |

### Success Criteria

- [ ] Story 3.6 reworked: auto-fill toggle is instant (no RPC on toggle), basket persisted in manifest, sync.start carries autoFill reservation
- [ ] Story 3.6: manifest migration runs cleanly on existing managed devices
- [ ] Story 3.7: artist view cache eliminates re-fetch on back-navigation; alpha-jump and search work client-side
- [ ] Story 4.6: ETA displayed during sync with correct formatting; "Calculating…" shown on first 2 samples
- [ ] Story 4.7: `.m3u` files generated, updated, and deleted correctly with differential sync; Rockbox loads playlist natively
- [ ] `basket.autoFill` RPC removed from daemon; no regressions on existing IPC paths
