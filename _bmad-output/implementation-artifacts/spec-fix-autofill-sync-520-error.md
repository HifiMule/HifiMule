---
title: 'Fix auto-fill sync aborting on transient server errors (HTTP 5xx / transport failures)'
type: 'bugfix'
created: '2026-05-29'
status: 'done'
route: 'one-shot'
---

## Intent

**Problem:** When syncing with auto-fill enabled, any HTTP 5xx response (e.g. Cloudflare 520) or transport failure from the Jellyfin server during auto-fill track fetching immediately aborts the entire sync with no retry, giving the user a confusing "Auto-fill expansion failed at sync time" error.

**Approach:** Add a per-page retry loop (up to 3 total attempts, 1 s / 2 s backoff) around the paginated auto-fill HTTP request for both 5xx status codes and transient network-layer errors (`is_connect()` / `is_timeout()`).

## Suggested Review Order

1. [auto_fill.rs — retry loop + transport error handling](../../hifimule-daemon/src/auto_fill.rs) — core fix; check counter logic, retry guard, and `break expr.await?` semantics
