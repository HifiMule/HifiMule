---
title: 'Library Item Navigation Loading Feedback'
slug: 'library-item-navigation-feedback'
created: '2026-03-01'
status: 'completed'
stepsCompleted: [1, 2, 3, 4]
tech_stack: ['TypeScript', 'Shoelace Web Components', 'Vite', 'Plain DOM (no framework)']
files_to_modify: ['jellysync-ui/src/library.ts', 'jellysync-ui/src/components/MediaCard.ts', 'jellysync-ui/src/styles.css']
code_patterns: ['DOM class toggling (classList.add/toggle)', 'Shoelace sl-spinner element injection', 'Absolute-positioned overlay on .card-image (position:relative + overflow:hidden)']
test_patterns: ['No test suite exists in jellysync-ui — manual verification only']
---

# Tech-Spec: Library Item Navigation Loading Feedback

**Created:** 2026-03-01

## Overview

### Problem Statement

When clicking an artist or playlist card in the library browser, there is no immediate visual feedback on the card. For `MusicArtist` items, nothing happens at all because the type is absent from the navigable `containerTypes` list. For `Playlist` and other navigable types, the card appears frozen until the entire container is replaced by a full-page spinner — leaving the user uncertain whether their click registered.

### Solution

(1) Add `MusicArtist` to the `containerTypes` array so artist cards navigate correctly. (2) When any card's navigation click fires, immediately apply an `is-navigating` CSS class to that specific card element and inject a spinner overlay into its `.card-image`. The class disables pointer events and dims the card. Because `loadItems(true)` replaces the entire container on completion, the overlay is torn down automatically with the DOM re-render — no cleanup code required.

### Scope

**In Scope:**
- Fix `MusicArtist` missing from `containerTypes` in `library.ts`
- Add `is-navigating` state (CSS class + spinner overlay injection) to the card element on navigation click, handled inside `MediaCard.create`'s existing click listener
- Add `.media-card.is-navigating` and `.nav-loading-overlay` CSS rules
- Applies to both `libraries` mode and `items` mode card clicks

**Out of Scope:**
- Loading feedback on breadcrumb navigation buttons
- Any changes to the basket toggle loading behaviour
- Any other item types or navigation paths

## Context for Development

### Codebase Patterns

- **UI stack**: TypeScript + Shoelace Web Components (`sl-card`, `sl-spinner`, `sl-icon-button`). No framework — plain DOM manipulation via `document.createElement`.
- **Loading pattern already established**: `toggleBtn.loading = true` on `sl-icon-button` (basket toggle in `MediaCard.ts:86`). The same imperative style applies here.
- **Overlay pattern already established**: `.selection-overlay` (`styles.css:242-259`) — `position: absolute; top:0; left:0; width:100%; height:100%; background: rgba(0,0,0,0.4); display:flex; justify-content:center; align-items:center; z-index:10`. The `.card-image` parent has `position:relative; overflow:hidden` — a perfect container for another absolute overlay.
- **Card state classes**: `is-selected`, `synced` toggled via `card.classList`. `is-navigating` follows the same naming convention.
- **DOM auto-teardown**: `loadItems(true)` sets `container.innerHTML = '<sl-spinner ...>'` synchronously at line 157. All card DOM — including any `is-navigating` overlay — is destroyed automatically. No cleanup required in `navigateToLibrary` or `navigateToItem`.
- **No UI tests**: `jellysync-ui` has no test suite. Verification is manual.

### Files to Reference

| File | Purpose |
| ---- | ------- |
| [jellysync-ui/src/library.ts](jellysync-ui/src/library.ts) | `containerTypes` (line 139), `navigateToItem`, `navigateToLibrary`, `loadItems`, `renderGrid` |
| [jellysync-ui/src/components/MediaCard.ts](jellysync-ui/src/components/MediaCard.ts) | `MediaCard.create` — card DOM construction and click event wiring (lines 68–74) |
| [jellysync-ui/src/styles.css](jellysync-ui/src/styles.css) | `.selection-overlay` (line 242), `.media-card` (line 220), `.media-card.synced` (line 336) |

### Technical Decisions

- **Where to apply the class**: Inside `MediaCard.create`'s existing click listener (`MediaCard.ts:68-74`), right before calling `onNavigate()`. The `card` element is already in scope there. This is cleaner than changing `renderGrid`'s closure or the `onNavigate` callback signature.
- **Spinner overlay**: Inject a `<div class="nav-loading-overlay">` containing an `<sl-spinner>` into `.card-image` at click time. DOM injection (vs CSS `::after`) is required because Shoelace's `<sl-spinner>` is a custom element that cannot be inserted via CSS pseudo-content.
- **No cleanup needed**: Navigation replaces `container.innerHTML`, destroying all card DOM automatically. No try/finally required.

## Implementation Plan

### Tasks

- [x] Task 1: Add `MusicArtist` to navigable container types
  - File: `jellysync-ui/src/library.ts`
  - Action: On line 139, add `'MusicArtist'` to the `containerTypes` array.
  - Before: `const containerTypes = ['MusicAlbum', 'Playlist', 'Folder', 'CollectionFolder', 'BoxSet', 'Series', 'Season'];`
  - After: `const containerTypes = ['MusicArtist', 'MusicAlbum', 'Playlist', 'Folder', 'CollectionFolder', 'BoxSet', 'Series', 'Season'];`
  - Notes: No other changes needed in this file. The existing `navigateToItem` call and `loadItems(true)` path handles the rest.

- [x] Task 2: Add CSS rules for `is-navigating` state and loading overlay
  - File: `jellysync-ui/src/styles.css`
  - Action: After the `.media-card.synced` block (line 338), insert the following two new rule blocks:
  ```css
  /* Navigation loading state */
  .media-card.is-navigating {
    pointer-events: none;
    opacity: 0.75;
  }

  .nav-loading-overlay {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    justify-content: center;
    align-items: center;
    z-index: 20;
  }

  .nav-loading-overlay sl-spinner {
    font-size: 2rem;
    --track-color: rgba(255, 255, 255, 0.2);
    --indicator-color: white;
  }
  ```
  - Notes: `z-index: 20` places the overlay above the `synced-badge` (no explicit z-index) and `selection-overlay` (z-index: 10). `.card-image` already has `position: relative; overflow: hidden` so the absolute overlay clips correctly.

- [x] Task 3: Inject loading state into the card on navigation click
  - File: `jellysync-ui/src/components/MediaCard.ts`
  - Action: In the `card.addEventListener('click', ...)` handler (lines 68–74), add the `is-navigating` class and overlay injection **inside the `if (!isButton)` branch, before `onNavigate()` is called**.
  - Before:
  ```ts
  card.addEventListener('click', (e) => {
      const path = e.composedPath();
      const isButton = path.some(el => (el as HTMLElement).classList?.contains('basket-toggle-btn'));
      if (!isButton) {
          onNavigate();
      }
  });
  ```
  - After:
  ```ts
  card.addEventListener('click', (e) => {
      const path = e.composedPath();
      const isButton = path.some(el => (el as HTMLElement).classList?.contains('basket-toggle-btn'));
      if (!isButton) {
          card.classList.add('is-navigating');
          const cardImage = card.querySelector('.card-image');
          if (cardImage) {
              const overlay = document.createElement('div');
              overlay.className = 'nav-loading-overlay';
              const spinner = document.createElement('sl-spinner');
              overlay.appendChild(spinner);
              cardImage.appendChild(overlay);
          }
          onNavigate();
      }
  });
  ```
  - Notes: `onNavigate()` is not awaited here — it is a void callback defined as `() => navigateToLibrary(...)` or `() => navigateToItem(...)`, both of which are async. The loading state persists until `loadItems(true)` replaces `container.innerHTML`, which destroys all card DOM. No cleanup is needed.

### Acceptance Criteria

- [x] AC 1: Given the library is showing artists (items view, `MusicArtist` type), when the user clicks an artist card, then the card immediately shows a centered white spinner over a dark overlay on the card image, the card opacity drops to 0.75, and the view subsequently navigates to that artist's content.

- [x] AC 2: Given the library is showing playlists (items view, `Playlist` type), when the user clicks a playlist card, then the same spinner overlay and dimming appear immediately on that card, and the view navigates into the playlist.

- [x] AC 3: Given any navigable card in the `libraries` or `items` view, when the user clicks it once, then `pointer-events: none` is applied to the card, so a second rapid click before navigation completes does not trigger a duplicate navigation.

- [x] AC 4: Given a card is in `is-navigating` state and navigation completes (success or error), then no orphaned overlay remains visible — the entire container is replaced by either the new grid or the error state.

- [x] AC 5: Given a card with a basket toggle button is visible, when the user clicks the basket toggle button, then the `is-navigating` class is NOT applied to the card (the `isButton` guard in the click handler prevents it).

- [x] AC 6: Given the user clicks an artist card while already viewing artist items, then the breadcrumb stack updates correctly and the artist's albums/songs are displayed (verifies the `MusicArtist` fix is end-to-end functional).

## Additional Context

### Dependencies

None — no new libraries or packages required.

### Testing Strategy

No automated test suite exists for `jellysync-ui`. Manual verification only:
- Run the app, browse to a Music library, click an Artist card → card dims with spinner overlay, then navigates into the artist's albums.
- Click a Playlist card → same loading feedback appears, then navigates.
- Click a library root card (libraries view) → same loading feedback.
- Fast double-click should not trigger duplicate navigations (`pointer-events: none` prevents it).
- On navigation error (`rpcCall` throws), the full spinner/error state renders normally — no orphaned overlay (card DOM is replaced regardless).

### Notes

- The `onNavigate` callback is a fire-and-forget void function. If a future refactor makes it awaitable, the loading state would still work correctly since the DOM teardown on `loadItems(true)` is independent of the callback's return value.
- If breadcrumb navigation (also calls `loadItems(true)`) later gets the same feedback treatment, the `loadItems` function is the right place to add it — not in this spec's scope.

## Review Notes

- Adversarial review completed
- Findings: 12 total, 5 fixed, 7 skipped
- Resolution approach: auto-fix
- Fixed: F2 (keyboard guard), F3 (double-click guard), F4 (10s hang timeout), F7 (opacity stacking), F8 (z-index comment), F9 (containerTypes comment)
- Skipped: F1/F5/F6 (undecided), F10/F11/F12 (noise)
