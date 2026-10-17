---
title: 'Log audiobook metadata changes'
type: 'feature'
created: '2026-10-17'
status: 'done'
route: 'one-shot'
---

# Log audiobook metadata changes

## Intent

**Problem:** Audiobook tag corrections during sync are invisible in daemon logs, making their impact difficult to inspect.

**Approach:** After a successful staged-file rewrite, log the book and part plus changed metadata fields as old-to-new values, or explicitly report no changes. Escape control characters and truncate long display values. Include ID3 recording dates and all MP4 string values so changes remain visible.

## Suggested Review Order

1. [Tag snapshots and logging](../../hifimule-daemon/src/sync_tags.rs) — successful rewrite boundaries, before/after values, and regression coverage for dates and multiple artists.
2. [Audiobookshelf documentation](../../docs/audiobookshelf-implementation.md) — staged-file logs versus subsequent device transfers.
