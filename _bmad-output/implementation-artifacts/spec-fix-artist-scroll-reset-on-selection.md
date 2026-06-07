---
title: 'Fix artist scroll reset on selection in PlaylistCurationView'
type: 'bugfix'
created: '2026-06-07'
status: 'done'
route: 'one-shot'
---

## Intent

**Problem:** In the playlist curation editor, selecting an artist triggers a full `innerHTML` re-render which resets the artist panel's scroll position to zero, causing the selected artist to scroll out of view.

**Approach:** After `render()` rebuilds the DOM, query the selected artist row from within `#curation-artist-panel` and call `scrollIntoView({ block: 'nearest' })` to restore visibility without jarring full-page scrolls.

## Suggested Review Order

- [`hifimule-ui/src/components/PlaylistCurationView.ts:255`](../../hifimule-ui/src/components/PlaylistCurationView.ts) — the 4-line fix at the end of `render()`, after `bindEvents()`
