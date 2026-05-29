# HifiMule 0.8.4

Release date: 2026-05-29

## Highlights

- **Correct sync large playlists**: When syncing a large playlist or genre, we get a "error decoding response body" message.

---

## Bug Fixes

### When syncing a large playlist or genre, we get a "error decoding response body" message.

Axum 0.8 defaults to a 2MB request body limit on the Json extractor. When starting a sync with 8000 songs, sync_execute sends the full SyncDelta (all add items + playlist tracks) back to the daemon as JSON-RPC params, which comes to ~3MB. Axum rejected it silently with a 413 before the handler ran — no daemon log, no handler log. The 413 response body isn't valid JSON, so response.json().await in the Tauri proxy failed with "error decoding response body".
