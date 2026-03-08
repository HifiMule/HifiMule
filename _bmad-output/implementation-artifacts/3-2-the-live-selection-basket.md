# Story 3.2: The Live Selection Basket

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a **Convenience Seeker (Sarah)**,
I want to **click items and have them "collect" in a sidebar**,
so that **I can see exactly what I'm about to sync without committing yet.**

## Acceptance Criteria

1.  **Selection Interaction**: Clicking the `(+)` button (or defined action area) on an Album or Playlist in the Library Grid MUST add it to the "Sync Basket". Clicking it again MUST remove it. (AC: #1)
2.  **Visual Feedback (Intent Overlay)**: Selected items in the grid MUST display a distinct visual state (e.g., a thick border, overlay, or "Selected" badge) to indicate they are in the basket. (AC: #2)
3.  **Basket Sidebar**: The right-hand sidebar (30% of layout) MUST populate with a list of selected items. Each item MUST show its thumbnail and title. (AC: #1)
4.  **Track Count Preview**: The "Intent Overlay" or the Basket Item itself MUST display the track count (e.g., "+12 Tracks") to give immediate feedback on the scope of the selection. (AC: #2)
5.  **State Persistence**: The selection state SHOULD be maintained while navigating between views (e.g., navigating into an album and back shouldn't clear the basket).

## Tasks / Subtasks

- [x] **T1: Daemon - Item Intelligence** (AC: #4)
    - [x] Implement `jellyfin_get_item_counts(ids: Vec<String>)` or extend `jellyfin_get_item_details` to return lightweight metadata (track count, total size) for a list of items.
    - [x] Ensure this logic handles recursion (e.g., counting tracks in a Playlist).
- [x] **T2: UI - Basket State Management** (AC: #1, #5)
    - [x] Create `BasketStore` (class or module) to manage the Set of selected Item IDs and their metadata.
    - [x] Implement reactive state updates: Notification/Event when basket changes to update UI components.
    - [x] Ensure state persists across `library.ts` view navigation changes.
- [x] **T3: UI - Basket Sidebar Component** (AC: #3)
    - [x] Implement `BasketSidebar.ts` component (or extend existing placeholder).
    - [x] Render a list of `BasketItem` cards (compact view).
    - [x] Add "Remove" (X) button to each basket item.
- [x] **T4: UI - Grid Interaction & Overlay** (AC: #1, #2, #4)
    - [x] Update `MediaCard.ts`: Add `(+)` / `(-)` toggle button overlay.
    - [x] Bind click events to `BasketStore.toggle(item)`.
    - [x] Implement "Selected" visual state (CSS class `.is-selected` with styling).
    - [x] Fetch and display "Track Count" on selection (using T1 RPC).

## Dev Notes

-   **Architecture Patterns:**
    -   **State Management:** Use a simple centralized Store pattern (singleton or exported module) for `BasketStore`. Do NOT rely on DOM state alone.
    -   **IPC:** Minimize RPC chatter. When selecting an item, fetch its details (count/size) once and cache it in the Store.
    -   **UX/Components:** Re-use `MediaCard` visual language but adapted for the Sidebar (compact list style).
-   **Technical Specifics (Typescript & Shoelace):**
    -   **Reactivity:** Since we aren't using a heavy framework like React, use a simple Pub/Sub or EventTarget pattern for the `BasketStore` so the Sidebar and Grid update automatically when state changes.
    -   **Styling:** Ensure the "Selected" state in the grid is high-contrast and obvious (e.g., border color matching branding `#EBB334`).
-   **Learnings from Story 3.1:**
    -   **Navigation:** Ensure the "Add to Basket" action does NOT conflict with the "Navigate to Album" action. Use a specific button or handle click (add) vs double-click (navigate), or separate click zones. *Decision: Use a dedicated (+) button overlay on hover/focus.*
    -   **Image Proxy:** Continue using the correctly implemented image proxy from Story 3.1.
-   **Source tree components to touch:**
    - `jellyfinsync-daemon/src/rpc.rs`: [MODIFY] Add item count helper.
    - `jellyfinsync-ui/src/state/basket.ts`: [NEW] State management.
    - `jellyfinsync-ui/src/components/BasketSidebar.ts`: [NEW] Sidebar logic.
    - `jellyfinsync-ui/src/components/MediaCard.ts`: [NEW] Add selection overlay.
    - `jellyfinsync-ui/src/library.ts`: [NOTE] Integrate basket sidebar.

### Project Structure Notes

-   Create `jellyfinsync-ui/src/state/` directory for `basket.ts` if it doesn't exist, to keep state logic separate from UI components.

### References

-   [Story 3.1 (Library)](file:///c:/Workspaces/JellyfinSync/_bmad-output/implementation-artifacts/3-1-immersive-media-browser-jellyfin-integration.md)
-   [UX Design - Basket Layout](file:///c:/Workspaces/JellyfinSync/_bmad-output/planning-artifacts/ux-design-specification.md#L58-L59)

## Dev Agent Record

### Agent Model Used

Antigravity

### Debug Log References

### Completion Notes List

### File List
