---
title: 'Fix genres navigation slow initial load'
type: 'bugfix'
created: '2026-05-22'
status: 'done'
route: 'one-shot'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The genres navigation view has a very long initial load time because `handle_browse_list_genres` ignores the pagination params sent by the UI and enriches cover art for *all* genres in the library before responding — causing N+1 HTTP round-trips to the media server.

**Approach:** Apply `browse_pagination()` (already used by all other list handlers) to slice the provider result to the requested page, then enrich art only for that page. Total genres count is computed before the slice so the UI's load-more pagination works correctly.

</frozen-after-approval>

## Suggested Review Order

1. [`hifimule-daemon/src/rpc.rs:643`](../../hifimule-daemon/src/rpc.rs) — `handle_browse_list_genres`: added `browse_pagination`, `total` before slice, slice before enrichment

## Spec Change Log

