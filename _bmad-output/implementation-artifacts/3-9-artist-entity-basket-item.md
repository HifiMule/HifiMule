# Story 3.9: Artist Entity Basket Item

Status: ready-for-dev

## Story

As a Ritualist (Arthur),
I want to add an artist to my basket as a single entity rather than a snapshot of their tracks,
so that any new albums or tracks added to that artist in Jellyfin are automatically included the next time I sync.

## Acceptance Criteria

1. **Artist (+) click inserts a single "Artist" card (no per-track expansion):**
   - Given I am browsing the Artist view
   - When I click (+) on an artist
   - Then a single "Artist" card appears in the basket (not individual track cards)
   - And the card shows: artist name, approximate track count, and estimated size (from artist entity metadata at add-time)
   - And no per-track child fetch is triggered at add-time (daemon handles expansion at sync time)

2. **Artist card display in basket:**
   - Given an artist card is in the basket
   - When I view it
   - Then it shows "Artist · ~N tracks · ~X MB" (approximate)
   - And storage projection uses this estimate for the capacity bar
   - And the card has the same remove (×) interaction as any other basket item

3. **Daemon expands artist to current tracks at sync time:**
   - Given the basket contains one or more artist cards
   - When sync starts
   - Then the daemon calls `get_child_items_with_sizes` for each artist ID to resolve current tracks (already occurs at `rpc.rs:862` for any container ID)
   - And newly added tracks from that artist (since the basket was built) are included in the sync

4. **Duplicate deduplication:**
   - Given artist cards and manually added albums/playlists are both in the basket
   - Then duplicate tracks are deduplicated by the daemon at sync time via the existing `seen_ids` HashSet

5. **Remove interaction:**
   - When I click (×) on an artist card
   - Then the card is removed immediately; no individual track cleanup needed

## Tasks / Subtasks

- [ ] Task 1: Update `BasketSidebar.ts` — add artist card rendering (AC: #1, #2, #5)
  - [ ] 1.1 In `renderItem()`, add check before the generic card path: `if (item.type === 'MusicArtist') return this.renderArtistCard(item);`
  - [ ] 1.2 Add `private renderArtistCard(item: BasketItem): string` — returns HTML with `.basket-item-artist` class, artist icon, name, "~N tracks · ~X MB" meta, and `×` remove button (same `remove-item-btn` pattern as other cards)

- [ ] Task 2: Add CSS for `.basket-item-artist` in `jellyfinsync-ui/src/styles.css` (AC: #2)
  - [ ] 2.1 Add `.basket-item-artist` class with a distinct artist-style (e.g. `sl-icon name="music-note-list"` or `"person-fill"`, subtle tint to distinguish from albums/playlists) — closely mirror the `.basket-item-auto-fill-slot` pattern for consistency

## Dev Notes

### Architecture Overview — What's Already Done (No Code Needed)

This story is **UI-only**. All infrastructure is already in place:

| Component | Status | Why |
|-----------|--------|-----|
| `MediaCard.ts` basket toggle | ✅ Already works | Calls `jellyfin_get_item_counts` + `jellyfin_get_item_sizes` with artist ID; stores `type: 'MusicArtist'`. Both RPCs handle artist IDs correctly (see below). |
| `basket.ts` store | ✅ Already works | `BasketItem.type` accepts any string; `getManualItemIds()`/`getManualSizeBytes()` exclude only `AUTO_FILL_SLOT_ID`. |
| `rpc.rs` sync expansion | ✅ Already works | `handle_sync_calculate_delta` at line 856 calls `get_child_items_with_sizes` for any non-Audio/MusicVideo item, including `MusicArtist`. |
| `api.rs` size calculation | ✅ Already works | `CONTAINER_TYPES` (line 8) includes `"MusicArtist"` — `get_item_sizes` correctly sums all child tracks recursively. |

**Only change needed:** `BasketSidebar.ts` `renderItem()` — currently shows generic "N tracks • MusicArtist" label. Need to display "Artist · ~N tracks · ~X MB" as per AC.

### Existing Code to Reuse — DO NOT Reinvent

| What | Where | How to Reuse |
|------|-------|-------------|
| `renderAutoFillSlotCard()` | `BasketSidebar.ts:982` | Mirror its structure for `renderArtistCard()` — same `.basket-item-card` wrapper, icon div, info div, remove button |
| `remove-item-btn` pattern | `BasketSidebar.ts:995, 1026` | Use identical `<sl-icon-button name="x" class="remove-item-btn" data-id="${item.id}" label="Remove">` — existing click handler at line ~740 already covers it |
| `formatSize(bytes)` | `BasketSidebar.ts:52` | Already in scope — use for "~X MB" display |
| `.basket-item-auto-fill-slot` CSS | `styles.css` | Mirror its structure for `.basket-item-artist` (dashed vs solid border, different tint) |
| `loadBasketImages()` | `BasketSidebar.ts:1032` | Artist images already load via `getImageUrl(id)` which calls `/Items/{id}/Images/Primary` — Jellyfin has artist photos. **Skip the `.basket-item-image` div for artist card** if you want an icon instead, OR include the image div to show artist photo (both are valid). |

### `renderArtistCard()` Implementation Guide

```typescript
private renderArtistCard(item: BasketItem): string {
    return `
        <div class="basket-item-card basket-item-artist" data-id="${this.escapeHtml(item.id)}">
            <div class="basket-item-artist-icon">
                <sl-icon name="person-fill"></sl-icon>
            </div>
            <div class="basket-item-info">
                <div class="basket-item-name">${this.escapeHtml(item.name)}</div>
                <div class="basket-item-meta">
                    Artist · ~${item.childCount} tracks · ~${formatSize(item.sizeBytes)}
                </div>
            </div>
            <sl-icon-button name="x" class="remove-item-btn" data-id="${this.escapeHtml(item.id)}" label="Remove"></sl-icon-button>
        </div>
    `;
}
```

Note: `item.childCount` comes from `jellyfin_get_item_counts` (`recursive_item_count` field). `item.sizeBytes` comes from `jellyfin_get_item_sizes` (recursive sum of all track bytes). Both are set at add-time by `MediaCard.ts` line 116–131.

### CSS for `.basket-item-artist`

```css
.basket-item-artist {
    /* Distinct from regular cards and auto-fill slot */
}
.basket-item-artist-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 48px;
    height: 48px;
    flex-shrink: 0;
    color: var(--sl-color-neutral-500);
    font-size: 1.5rem;
}
```

Pattern: follow the `.basket-item-auto-fill-slot` / `.basket-item-auto-fill-icon` pair as a template.

### How Artist Size & Count Are Populated (MediaCard.ts already does this)

When user clicks (+) on an artist card in `MediaCard.ts:96`:

```typescript
// Already exists in MediaCard.ts lines 116-131
const [metadata, sizeData] = await Promise.all([
    rpcCall('jellyfin_get_item_counts', { itemIds: [item.Id] }),
    rpcCall('jellyfin_get_item_sizes', { itemIds: [item.Id] }),
]);
basketStore.add({
    id: item.Id,
    name: item.Name,
    type: item.Type,           // 'MusicArtist'
    childCount: info.recursiveItemCount,   // artist's total track count
    sizeBytes: sizeInfo.totalSizeBytes,    // sum of all tracks' bytes
    ...
});
```

`jellyfin_get_item_sizes` → `get_single_item_size` → detects `MusicArtist` in `CONTAINER_TYPES` (`api.rs:8`) → `get_child_items_with_sizes(artistId)` with `Recursive=true&IncludeItemTypes=Audio,MusicVideo` → sums MediaSources sizes. This is ONE server-side recursive query, NOT per-track card expansion.

### How Daemon Expands Artist at Sync Time (rpc.rs already does this)

In `handle_sync_calculate_delta` (`rpc.rs:851-894`):

```rust
if is_downloadable_item_type(&item.item_type) {
    // Audio/MusicVideo → add directly
} else {
    // MusicAlbum, Playlist, MusicArtist → expand via get_child_items_with_sizes
    // get_child_items_with_sizes uses Recursive=true, so artist → all tracks
    let children = client.get_child_items_with_sizes(..., &item.id).await?;
    for child in children {
        if is_downloadable_item_type(&child.item_type) {
            results.push(Ok(to_desired_item(child)));
        }
    }
}
```

The `MusicArtist` ID flows into this path automatically. "Newly added tracks" are picked up because the daemon fetches the current Jellyfin state at sync time, not the cached state from when the basket was built. ✓

### Critical Constraints

- **No changes to MediaCard.ts** — the (+) button already works for artists. Do NOT add artist-specific logic there.
- **No changes to basket.ts** — the store is type-agnostic. Do NOT filter by type.
- **No changes to rpc.rs or api.rs** — `CONTAINER_TYPES` already includes `MusicArtist`, expansion already works.
- **Backwards compat**: Old manifests without artist items continue to work as-is. New manifests save `type: 'MusicArtist'` items correctly via existing `manifest_save_basket` flow.
- **`loadBasketImages()`**: Artist items already get their image loaded via `getImageUrl(id)` → `/Items/{id}/Images/Primary`. If using an icon instead of an image div in `renderArtistCard()`, just skip the `.basket-item-image` div — `loadBasketImages()` will simply not find the selector for artist cards and do nothing.

### Previous Story Learnings (from Story 3.8)

- **`remove-item-btn` event handler**: The existing handler at `BasketSidebar.ts:~740` fires for ALL `.remove-item-btn` clicks including the × on artist cards. No special handling needed.
- **`basketStore.add()` Map semantics**: Adding an artist item with the same ID twice overwrites the entry — this matches the expected "toggle adds" behavior via `MediaCard.ts:176` (`basketStore.toggle()`).
- **Storage projection**: `basketStore.getTotalSizeBytes()` already includes ALL items including artists (it sums `sizeBytes` for everything). The artist's estimated `sizeBytes` contributes to the capacity bar automatically.
- **TypeScript strictness**: Escape HTML for all user-controlled strings (`item.id`, `item.name`). Existing `escapeHtml()` at line 1043 — use it consistently in `renderArtistCard()`.

### Project Structure Notes

- UI changes only: `jellyfinsync-ui/src/components/BasketSidebar.ts` and `jellyfinsync-ui/src/styles.css`
- No new files needed
- No Rust changes

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 3.9 — full acceptance criteria and technical notes]
- [Source: jellyfinsync-ui/src/components/MediaCard.ts:96-141 — basket toggle handler that already handles artists]
- [Source: jellyfinsync-ui/src/components/BasketSidebar.ts:1000-1028 — renderItem() to modify]
- [Source: jellyfinsync-ui/src/components/BasketSidebar.ts:982-998 — renderAutoFillSlotCard() to mirror]
- [Source: jellyfinsync-ui/src/components/BasketSidebar.ts:52 — formatSize() utility]
- [Source: jellyfinsync-daemon/src/api.rs:8 — CONTAINER_TYPES includes MusicArtist]
- [Source: jellyfinsync-daemon/src/api.rs:421-491 — get_item_sizes / get_single_item_size handles MusicArtist]
- [Source: jellyfinsync-daemon/src/rpc.rs:851-894 — handle_sync_calculate_delta container expansion]

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

### File List
