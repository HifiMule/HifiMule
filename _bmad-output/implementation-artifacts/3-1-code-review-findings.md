# 🔥 CODE REVIEW FINDINGS: Story 3.1

**Story:** `3-1-immersive-media-browser-jellyfin-integration.md`
**Reviewer:** Antigravity (Adversarial Reviewer)
**Agitation Level:** MAX

## Summary
The implementation is a facade. It looks like it works on the surface (if code is read quickly), but it fails completely at runtime. **Images won't load, you can't click anything, and you can't change libraries.**

| Category | Count | Status |
| :--- | :--- | :--- |
| 🔴 **CRITICAL** | 2 | **MUST FIX** |
| 🟡 **HIGH** | 2 | **MUST FIX** |
| 🔵 **MEDIUM** | 2 | **SHOULD FIX** |
| 🟢 **LOW** | 0 | - |

## 🔴 CRITICAL ISSUES

### 1. 💀 Broken Image Loading (No Proxy)
**Location:** `jellysync-ui/src/library.ts` lines 91-94
**Problem:** The code constructs image URLs as `/api/Items/{id}/...`. This assumes the frontend server (Vite/Tauri) proxies `/api` to the Jellyfin server. **IT DOES NOT.** `vite.config.ts` has no proxy config, and the Daemon (Axum) has no route for `/api/`.
**Result:** **All images will 404.** The UI will look broken.
**Fix:** Implement a proper image proxy route in `rpc.rs` (using Axum) that streams the image from Jellyfin, OR use a custom Tauri command to fetch image blobs.

### 2. 🧱 Interaction Dead End (No Navigation)
**Location:** `jellysync-ui/src/library.ts` line 81 (`createMediaCard`)
**Problem:** The `MediaCard` is created, but **there is no `onclick` event listener.**
**Result:** The user sees albums but **cannot click them.** They are stuck on the root page forever.
**Fix:** Add click handler to `createMediaCard` that calls `renderMediaGrid(containerId, item.Id)`.

## 🟡 HIGH ISSUES

### 3. 🔍 Hardcoded Library View (Missing Selection)
**Location:** `jellysync-ui/src/library.ts` line 190 (`initLibraryView`)
**Problem:** The code hardcodes `renderMediaGrid(..., 'MusicAlbum,Playlist')`. It completely ignores `fetchViews()`.
**Result:** Users verify their "Music" library but cannot access "Podcasts", "Audiobooks", or other libraries. This violates **AC #2** ("fetch and display a grid of Albums and Playlists...").
**Fix:** Implement a "Library Selector" (e.g., a dropdown or initial grid of Views) before showing items.

### 4. 📉 Pagination Missing (Data Cap)
**Location:** `jellysync-ui/src/library.ts` line 140
**Problem:** Hardcoded `limit: 50` in `renderMediaGrid`. The UI displays "Showing 50 of X" (line 163) but provides **zero mechanism** to load the rest.
**Result:** Users can only see their first 50 albums. For large libraries, this is unusable.
**Fix:** Add a "Load More" button or meaningful pagination controls.

## 🔵 MEDIUM ISSUES

### 5. 👻 Sync Status is a Lie (Placeholder)
**Location:** `jellysync-daemon/src/rpc.rs` line 391
**Problem:** `sync_get_device_status_map` returns an empty list `[]`.
**Result:** The "Synced" badge will **never appear**, even if items are synced. While the story notes this manifest work is for Epic 4, we should at least verify if we can make it work for the simple case or confirm this is intended deferral.
**Fix:** Accept as deferred or implement basic file existence check if possible.

### 6. ⚠️ Hardcoded RPC Port
**Location:** `jellysync-ui/src/library.ts` line 3
**Problem:** `RPC_PORT` fallback is hardcoded.
**Result:** If `.env` is missing, it might mismatch the daemon.
**Fix:** Ensure robustness or runtime configuration.

---

## 🛠️ Recommended Action

I recommend **Option 1: Fix them automatically**.
I can fix the Critical and High issues right now:
1.  **Add `proxy_image` endpoint** to `rpc.rs` (Axum) to tunnel Jellyfin images.
2.  **Add navigation logic** to `library.ts` (click to drill down, breadcrumbs to go back).
3.  **Add Library Selection** (start with Views, then click to Items).
4.  **Add "Load More" button** for pagination.
