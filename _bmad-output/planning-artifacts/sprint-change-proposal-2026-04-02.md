# Sprint Change Proposal — Lazy Basket Expansion (Auto-Fill Slot & Artist Entity)

**Date:** 2026-04-02
**Author:** Alexis (with SM agent)
**Status:** Approved (2026-04-02)

---

## 1. Issue Summary

**Problem Statement:** The current Auto-Fill and Artist basket behaviors both use an eager-expansion model that is slower, more complex, and less accurate than necessary.

- **Auto-Fill (Story 3.6 — done):** Toggling Auto-Fill immediately triggers a `basket.autoFill` RPC call to the daemon, which queries Jellyfin, ranks tracks by priority, and populates the basket with individual track cards. This requires debouncing, in-flight guards, and race-condition handling in `BasketSidebar.ts`. The basket then contains a "stale snapshot" of what was best at toggle-time, not at sync-time.

- **Artist basket items:** Adding an artist from the library browser eagerly expands to all individual tracks at add-time. If the artist releases new music in Jellyfin, those tracks are absent from the current basket and missed on the next sync.

**Discovery:** Identified during active use of Epic 3 / Story 3.6 implementation. The daemon already proves the correct pattern: [main.rs:503–526](../../jellyfinsync-daemon/src/main.rs) runs `run_auto_fill()` at sync-time for the auto-sync-on-connect path, not at basket-build time. Similarly, [rpc.rs:807–866](../../jellyfinsync-daemon/src/rpc.rs) already expands any container ID (album/playlist/artist) to children at `sync.start` time.

**Evidence:**
- `BasketSidebar.ts:149–335`: `autoFillInFlight`, `autoFillPendingRetrigger`, `autoFillDebounceTimer`, `scheduleAutoFill()` — all complexity caused by the eager-call model.
- `rpc.rs:807`: comment reads "Container items (playlist/album/artist) are expanded to individual tracks" — the lazy infrastructure already exists in `sync.start`; the UI side just bypasses it.

---

## 2. Impact Analysis

### Epic Impact

| Epic | Impact |
|------|--------|
| Epic 3 | Two new stories added (3.8, 3.9); Story 3.6 behavior superseded |
| Epic 4 | `sync.start` RPC handler gains optional `autoFill` param (small extension) |
| Epics 1, 2, 5, 6 | Unaffected |

### Story Impact

| Story | Change | Reason |
|-------|--------|--------|
| 3.6 (done) | Superseded by 3.8 — implementation to be replaced | Eager-expansion model is cumbersome and stale |
| 3.8 (new) | **Add** — Lazy Auto-Fill Virtual Slot | Auto-fill toggle inserts a virtual slot; expansion runs at sync time |
| 3.9 (new) | **Add** — Artist Entity Basket Item | Artist added as single entity card; expansion runs at sync time |

### Artifact Conflicts

| Artifact | Sections Affected |
|----------|------------------|
| PRD | FR29 description updated (lazy model); new FR34 added (artist entity basket) |
| Architecture | `basket.autoFill` RPC demoted to preview-only; `sync.start` gains `autoFill` param; `BasketItem` virtual slot concept documented |
| UX Spec | §5.3 Auto-Fill Components — virtual slot card replaces individual auto-filled track cards; artist entity card behavior documented |

### Technical Impact

- **`BasketSidebar.ts` simplification:** Remove `triggerAutoFill()`, `scheduleAutoFill()`, `autoFillInFlight`, `autoFillPendingRetrigger`, `autoFillDebounceTimer`, `isAutoFillLoading`, `basketStore.replaceAutoFilled()`. Toggle inserts a single virtual slot item instead.
- **`sync.start` RPC (rpc.rs):** Gains optional `autoFill: { enabled, maxBytes?, excludeItemIds[] }` param. If present, calls `run_auto_fill()` and merges results with explicit item list before executing sync.
- **Artist add-flow (UI):** On artist (+) click, store `{ type: 'MusicArtist', id: artistId, ... }` in `basketStore` — no child fetch at add-time. Daemon's existing expansion at `rpc.rs:831` handles the rest at sync time.
- **No new external dependencies.**

---

## 3. Recommended Approach

**Direct Adjustment** — add two stories to Epic 3, extend `sync.start` in Epic 4 scope, update three planning artifacts.

**Rationale:**
- The daemon already implements both patterns (lazy auto-fill in `main.rs`, container expansion in `rpc.rs`). The change is primarily removing eager logic from the UI, not adding new daemon capability.
- Story 3.8 and 3.9 are independently implementable and testable.
- No epic restructuring or MVP scope change required.

**Effort:** Low (mainly UI simplification; daemon change is a small `sync.start` extension)
**Risk:** Low — daemon patterns proven; UI change reduces complexity rather than adding it
**Timeline Impact:** Two new stories added to Epic 3 backlog; current sprint (Epic 6) unaffected

---

## 4. Detailed Change Proposals

### 4.1 New Story 3.8 — Lazy Auto-Fill Virtual Slot

**Story: [3.8] Lazy Auto-Fill Virtual Slot**
**Epic:** Epic 3 — The Curation Hub

*As a Convenience Seeker (Sarah),
I want to enable Auto-Fill with a single toggle and have the device fill with my best music at sync time,
So that I don't wait for a slow basket population and always get the freshest track selection when I actually sync.*

**Acceptance Criteria:**

*Enabling Auto-Fill*
- Given the basket sidebar is visible
- When I enable the "Auto-Fill" toggle
- Then a single "Auto-Fill Slot" card appears in the basket (not individual tracks)
- And the card shows the configured capacity target (e.g. "Fill remaining 12.4 GB" or the user-set max)
- And no Jellyfin API call is made at this point

*Basket display with mixed content*
- Given manual items and the Auto-Fill Slot are in the basket
- When I view the basket
- Then manual items appear as individual cards above the Auto-Fill Slot
- And the Auto-Fill Slot shows "Will fill ~X GB with top-priority tracks at sync time"
- And storage projection includes the slot's target bytes in the capacity bar

*Sync expansion*
- Given the basket contains the Auto-Fill Slot
- When I click "Start Sync"
- Then the daemon runs the priority algorithm (`run_auto_fill`) at the start of the sync job
- And expands the slot to real track IDs (favorites first, then play count, then newest)
- And excludes any track IDs already covered by manual basket items
- And the expanded track list is merged with manual items for the sync operation
- And the UI shows real-time progress exactly as today (files completed, current filename)

*Disabling Auto-Fill*
- Given Auto-Fill is enabled
- When I toggle it off
- Then the Auto-Fill Slot is removed from the basket immediately (no API call)

*Settings persistence (unchanged)*
- Auto-Fill preferences (enabled, maxBytes) continue to be persisted to the device manifest via `sync.setAutoFill`

**Technical Notes:**
- **Remove** from `BasketSidebar.ts`: `triggerAutoFill()`, `scheduleAutoFill()`, `autoFillInFlight`, `autoFillPendingRetrigger`, `autoFillDebounceTimer`, `isAutoFillLoading`, `basketStore.replaceAutoFilled()`
- **Add** to `BasketSidebar.ts`: toggle inserts a single `{ id: '__auto_fill_slot__', type: 'AutoFillSlot', maxBytes: N }` virtual item into `basketStore`
- **`basket.autoFill` RPC**: no longer called by UI for basket population; retained as optional preview/debug endpoint
- **`sync.start` RPC handler** ([rpc.rs](../../jellyfinsync-daemon/src/rpc.rs)): if request contains `autoFill: { enabled: true, maxBytes?, excludeItemIds[] }`, call `run_auto_fill()` and merge results with explicit item list before executing — mirrors existing daemon-initiated path at [main.rs:503](../../jellyfinsync-daemon/src/main.rs)
- **`sync.start` params** gain: `autoFill?: { enabled: boolean, maxBytes?: number, excludeItemIds: string[] }`
- Story 3.6 eager-population flow superseded

---

### 4.2 New Story 3.9 — Artist Entity Basket Item (Lazy Artist Expansion)

**Story: [3.9] Artist Entity Basket Item**
**Epic:** Epic 3 — The Curation Hub

*As a Ritualist (Arthur),
I want to add an artist to my basket as a single entity rather than a snapshot of their tracks,
So that any new albums or tracks added to that artist in Jellyfin are automatically included the next time I sync.*

**Acceptance Criteria:**

*Adding an artist*
- Given I am browsing the Artist view
- When I click (+) on an artist
- Then a single "Artist" card appears in the basket (not individual track cards)
- And the card shows: artist name, approximate track count, and estimated size (from artist entity metadata at add-time)
- And no per-track child fetch is triggered at add-time

*Basket display*
- Given an artist card is in the basket
- When I view it
- Then it shows "Artist · ~N tracks · ~X MB" (approximate)
- And storage projection uses this estimate for the capacity bar
- And the card has the same remove (×) interaction as any other basket item

*Sync expansion*
- Given the basket contains one or more artist cards
- When sync starts
- Then the daemon calls `get_child_items_with_sizes` for each artist ID to resolve current tracks (this already occurs at [rpc.rs:831](../../jellyfinsync-daemon/src/rpc.rs) for any container ID)
- And newly added tracks from that artist (since the basket was built) are included in the sync

*Mixed basket deduplication*
- Given artist cards and manually added albums/playlists are both in the basket
- Then duplicate tracks are deduplicated by the daemon at sync time via the existing manifest comparison logic

*Removing an artist*
- When I click (×) on an artist card
- Then the card is removed immediately; no individual track cleanup needed

**Technical Notes:**
- **UI — library browser**: on artist (+) click, store `{ id: artistId, type: 'MusicArtist', name, sizeBytes: artistTotalBytes, childCount }` in `basketStore` — use artist-level size from metadata, no child fetch
- **Daemon — `sync.start`**: no change required — `rpc.rs:807–866` already expands `MusicArtist` type container IDs via `get_child_items_with_sizes`
- **`BasketItem`**: `type: 'MusicArtist'` already valid in the interface; `sizeBytes` carries artist-level cumulative size
- Story 3.6's eager artist-track expansion at add-time is superseded by this story

---

### 4.3 PRD Updates

**Section: MVP Feature Set — Auto-Fill Sync Mode**

```
OLD:
- Auto-Fill Sync Mode: Intelligent device-filling that selects music by priority
  (favorites → play count → creation date) up to capacity or a user-defined limit.
  Can be mixed with manual basket selections.

NEW:
- Auto-Fill Sync Mode: Intelligent device-filling using a virtual basket slot.
  Enabling Auto-Fill places a single slot in the basket representing remaining
  capacity; the priority algorithm (favorites → play count → creation date) runs
  at sync time, not at basket-build time. Always uses the freshest library state.
  Can be mixed with manual basket selections.
```

**FR29 update:**

```
OLD:
FR29: The system can automatically select music to synchronize based on a priority
  algorithm (favorites first, then by play count, then by creation date) up to the
  device's available capacity or a user-defined size limit.

NEW:
FR29: The system can reserve capacity in the sync basket via a virtual Auto-Fill
  slot; at sync time the daemon expands the slot by running the priority algorithm
  (favorites first, then by play count, then by creation date) against the current
  Jellyfin library state, up to the device's available capacity or a user-defined
  size limit.
```

**New Functional Requirement:**

```
FR34: The system can add an artist to the sync basket as a single entity reference;
  at sync time the daemon resolves the artist to its current track list, ensuring
  tracks added to the artist after basket construction are automatically included.
```

**FR Coverage Map addition:**
```
FR34: Epic 3 — Artist Entity Basket Item (Story 3.9)
```

---

### 4.4 Architecture Updates

**`basket.autoFill` RPC (status change):**

```
OLD:
basket.autoFill — Configure and trigger auto-fill calculation.
  Params: { deviceId, maxBytes?, excludeItemIds[] }
  Response streams ranked item list progressively.
  Called by UI on toggle and basket change.

NEW:
basket.autoFill — Preview/debug endpoint for auto-fill calculation.
  Params: { deviceId, maxBytes?, excludeItemIds[] }
  Returns ranked item list (not used by primary sync flow).
  NOTE: UI no longer calls this to populate the basket.
  Auto-fill expansion happens inside sync.start when autoFill param is present.
```

**`sync.start` RPC (param addition):**

```
OLD:
sync.start
  Params: { devicePath: string, itemIds: string[] }

NEW:
sync.start
  Params: {
    devicePath: string,
    itemIds: string[],
    autoFill?: {
      enabled: boolean,
      maxBytes?: number,
      excludeItemIds: string[]
    }
  }
  If autoFill.enabled: daemon calls run_auto_fill() and merges results with
  itemIds before executing. Mirrors daemon-initiated auto-sync path (main.rs:503).
```

**BasketItem — virtual slot types (new concept):**

```
Virtual basket items are UI-only markers stored in the basket that represent
deferred expansion. They are passed to sync.start as metadata params, not as
itemIds. Two virtual types:

  AutoFillSlot: { id: '__auto_fill_slot__', type: 'AutoFillSlot', maxBytes: N }
    → passed as sync.start autoFill param; not included in itemIds

  MusicArtist: { id: artistJellyfinId, type: 'MusicArtist', ... }
    → passed as a regular itemId; daemon's existing container expansion handles it
```

---

### 4.5 UX Spec Updates

**Section 5.3 — Auto-Fill Components**

```
OLD:
- Auto Badge: Distinct visual indicator on auto-filled items using a muted accent
  color to differentiate from manually added (+) items.
- Priority Reason Tags: Small inline labels on auto-filled items showing selection
  reason (★ Favorite, ▶ 47 plays, "New").

NEW:
- Auto-Fill Slot Card: A single card in the basket (replacing individual track cards)
  showing the configured capacity target: "Will fill ~X GB with top-priority tracks
  at sync time". Rendered with a distinct dashed border to signal deferred content.
- Auto Badge and Priority Reason Tags: Removed from basket (no longer applicable;
  individual auto-filled tracks are not shown until sync runs).
- Artist Entity Card: Artist basket items render identically to album cards —
  single card showing "Artist · ~N tracks · ~X MB". No "Auto" badge needed.
  The ~N track count and ~X size are estimates based on metadata at add-time.
```

---

## 5. Implementation Handoff

### Change Scope: Minor

All changes are within existing epics. No new epics. Epic 6 (current sprint) unaffected.

| Recipient | Responsibility |
|-----------|---------------|
| **Dev** | Implement Story 3.8: remove eager auto-fill logic from `BasketSidebar.ts`; insert virtual slot on toggle; extend `sync.start` RPC with `autoFill` param |
| **Dev** | Implement Story 3.9: change artist (+) action to store `MusicArtist` entity in basket; no daemon change required |
| **SM / Architect** | Update `epics.md`: add Stories 3.8 and 3.9 to Epic 3 |
| **SM / Architect** | Update `prd.md`: revise FR29; add FR34; update FR coverage map; update MVP Auto-Fill description |
| **Architect** | Update `architecture.md`: `basket.autoFill` RPC status; `sync.start` param addition; virtual slot concept |
| **SM** | Update `ux-design-specification.md`: §5.3 virtual slot card and artist entity card |
| **SM** | Update `sprint-status.yaml`: add `3-8` and `3-9` as `backlog` under epic-3 |

### Success Criteria

- [ ] Story 3.8: enabling Auto-Fill toggle shows single slot card with no API call; disabling removes slot instantly
- [ ] Story 3.8: sync with Auto-Fill slot active runs `run_auto_fill()` at sync start and merges tracks with manual items
- [ ] Story 3.8: `BasketSidebar.ts` contains no `autoFillInFlight`, `autoFillDebounceTimer`, or `replaceAutoFilled` references
- [ ] Story 3.9: artist (+) adds single entity card to basket with no child fetch
- [ ] Story 3.9: syncing with artist entity card includes any new tracks added to that artist since basket was built
- [ ] PRD: FR29 describes lazy-slot model; FR34 present with coverage map entry
- [ ] Architecture: `basket.autoFill` marked preview-only; `sync.start` params updated; virtual slot concept documented
- [ ] UX Spec: §5.3 describes virtual slot card and artist entity card; Auto Badge / Priority Reason Tag notes updated
