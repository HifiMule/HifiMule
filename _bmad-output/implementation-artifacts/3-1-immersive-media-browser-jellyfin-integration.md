# Story 3.1: Immersive Media Browser (Jellyfin Integration)

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a **Ritualist (Arthur)**,
I want to **browse my Jellyfin playlists and albums with high-quality artwork**,
so that **I can enjoy the curation process as I do on the server.**

## Acceptance Criteria

1.  **Vibrant Hub Layout**: The main UI MUST display the "Vibrant Hub" 70/30 split layout, with the Library Browser occupying the left 70% and a placeholder/collapsible Basket sidebar on the right 30%. (AC: #1)
2.  **Jellyfin Library Integration**: The browser MUST fetch and display a grid of **Albums** and **Playlists** directly from the connected Jellyfin server using the authenticated session from Story 2.1. (AC: #2)
3.  **High-Quality Artwork**: Items MUST display primary image tags (folder/cover art). Images MUST use lazy-loading and proper aspect ratios (1:1 for albums, 16:9 or 1:1 for playlists). (AC: #3)
4.  **Synced Status Indicators**: Items that already exist on the connected mass storage device (verified via `.jellysync.json` manifest) MUST display a distinct "Synced" badge or overlay. (AC: #4)
5.  **Navigation & Pagination**: Users MUST be able to navigate deeper (e.g., Album List -> Album Details -> Track List) and handle large libraries via pagination or infinite scroll. (AC: #5)

## Tasks / Subtasks

- [ ] **T1: Daemon - Library & Status API** (AC: #2, #4)
    - [ ] Implement `jellyfin_get_views` and `jellyfin_get_items` in `jellysync-daemon` to fetch libraries, albums, and playlists.
    - [ ] Implement `jellyfin_get_item_details` to retrieve track lists for a container.
    - [ ] Implement `sync_get_device_status_map` RPC: Returns a list/set of Jellyfin Item IDs that currently exist in the active device manifest.
- [ ] **T2: UI - Layout Skeleton** (AC: #1)
    - [ ] Create `jellysync-ui/src/pages/library.html` (or component) implementing the 70/30 grid layout.
    - [ ] Integrate **Shoelace** split panel (`<sl-split-panel>`) or CSS Grid for the "Vibrant Hub" structure.
- [ ] **T3: UI - Media Grid & Navigation** (AC: #2, #5)
    - [ ] Create `MediaGrid` component: Responsive grid layout for cards.
    - [ ] Implement data fetching from Daemon RPC (`jellyfin_get_items`) with pagination support.
    - [ ] Implement navigation state (breadcrumbing) to drill down from Library -> Album -> Tracks.
- [ ] **T4: UI - Album/Playlist Card Component** (AC: #3)
    - [ ] Create `MediaCard` web component using `<sl-card>`.
    - [ ] Implement image loading using Jellyfin's `/Items/{id}/Images/Primary` endpoint (proxied or direct if CORS allowed, else use Tauri asset protocol if caching locally).
    - [ ] Add loading skeleton state using `<sl-skeleton>`.
- [ ] **T5: UI - Sync Status Integration** (AC: #4)
    - [ ] Fetch device status map on load.
    - [ ] Apply "Synced" visual indicator (e.g., Green check badge or opacity fade) to `MediaCard` if ID exists in the map.

## Dev Notes

-   **Architecture Patterns:**
    -   **IPC:** Use `get_daemon_state` and new `jellyfin_*` RPC methods.
    -   **State Management:** The UI should maintain the current view state (current folder/parent ID).
    -   **Performance:** Do NOT fetch all tracks for all albums. Fetch on demand.
-   **Technical Specifics (Tauri v2 & Shoelace):**
    -   **Images:** Use the specific Jellyfin Image API headers if authentication is required on image requests. If CORS is an issue, consider a Daemon Proxy command `jellyfin_proxy_image` or use Tauri's `fetch` API which bypasses CORS.
    -   **Web Components:** Ensure Shoelace assets are correctly configured in `tauri.conf.json` or copied to `assets` folder.
-   **Security:**
    -   Ensure the Jellyfin Token is NOT exposed in the UI logs.
-   **Source tree components to touch:**
    -   `jellysync-daemon/src/jellyfin/api.rs`: [NEW] Library browsing logic.
    -   `jellysync-daemon/src/rpc.rs`: Expose new methods.
    -   `jellysync-ui/src/components/MediaCard.ts`: [NEW] Card component.
    -   `jellysync-ui/src/pages/Library.ts`: [NEW] Main view logic.

### Project Structure Notes

-   Keep Shoelace components encapsulated.
-   Ensure `jellysync-ui` connects to the Daemon RPC port defined in `.env`.

### References

-   [Story 2.1 (Auth)](file:///c:/Workspaces/JellyfinSync/_bmad-output/implementation-artifacts/2-1-secure-jellyfin-server-link.md)
-   [UX Design - Visual Theme](file:///c:/Workspaces/JellyfinSync/_bmad-output/planning-artifacts/ux-design-specification.md#L49)
-   [Jellyfin API - Items](https://api.jellyfin.org/#tag/Items/operation/GetItems)

## Dev Agent Record

### Agent Model Used
Antigravity (Workflow Engine)

### Debug Log References

### Completion Notes List

### File List
