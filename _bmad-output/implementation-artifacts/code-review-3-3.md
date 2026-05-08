# Code Review Findings: Story 3.3

**Story:** `3-3-high-confidence-storage-projection.md`
**Reviewer:** Antigravity (AI)
**Date:** 2026-02-15

## 📊 Summary
**Git vs Story Discrepancies:** 0 found (Perfect match)
**Issues Found:** 1 High, 1 Medium, 2 Low

## 🔴 CRITICAL ISSUES
- **[High] Logic Error**: `hifimule-daemon/src/api.rs:308` hardcodes `IncludeItemTypes=Audio` when recursively fetching child items for containers. Use of `MUSIC_ITEM_TYPES` const (which includes `MusicVideo`) is ignored here. If a user syncs a Playlist containing Music Videos, their size will be calculated as 0, leading to potential "Disk Full" errors—explicitly violating the story's core value proposition.

## 🟡 MEDIUM ISSUES
- **[Medium] Data Integrity/Migration**: Existing items in the `BasketStore` (saved in `localStorage`) will not have the `sizeBytes` field. `getTotalSizeBytes` handles this safely (`|| 0`), but users with existing baskets will see "0 MB" total size until they remove and re-add items. The `BasketStore` should ideally backfill this data or `MediaCard` should trigger a refresh for items with missing size data.

## 🟢 LOW ISSUES
- **[Low] Assumption**: `get_single_item_size` (`api.rs:385`) takes the size of the *first* `MediaSource`. If Jellyfin returns multiple sources (e.g., varying bitrates/formats), the sync logic might pick a different one than the size calculation assumes.
- **[Low] UX Ambiguity**: "10% free remaining" in AC is implemented as "Free space < 10% of Total Disk Space". This is a safe interpretation but technically aggressive for very large drives (e.g., 10% of 4TB is 400GB, which is a lot of "amber" space).

## 💡 Recommendations
1. **Fix Filtering**: Update `get_child_items_with_sizes` to use `MUSIC_ITEM_TYPES` or at least include `MusicVideo`.
2. **Fix Migration**: Add a `init()` or `hydrate()` method to `BasketStore` that checks for items with missing `sizeBytes` and calls `jellyfin_get_item_sizes` for them on startup.
