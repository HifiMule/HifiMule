---
title: 'Fix Jellyfin playlist rename UserData validation'
type: 'bugfix'
created: '2026-06-08'
status: 'done'
route: 'one-shot'
context:
  - '{project-root}/_bmad-output/planning-artifacts/project-context.md'
---

# Fix Jellyfin playlist rename UserData validation

## Intent

**Problem:** Renaming a Jellyfin playlist failed with HTTP 400 because HifiMule posted the fetched item DTO back to `/Items/{id}` with `UserData`, and newer Jellyfin servers reject that read-only user metadata when required server fields such as `Key` are missing.

**Approach:** Keep the existing provider abstraction and rename flow, but sanitize the fetched item before update serialization so user-scoped metadata is omitted from the POST body.

## Suggested Review Order

- Start with the request-body sanitization that removes Jellyfin read metadata before update.
  [`api.rs:1629`](../../hifimule-daemon/src/api.rs#L1629)

- Confirm the regression test reproduces a playlist containing `UserData`.
  [`api.rs:2202`](../../hifimule-daemon/src/api.rs#L2202)

- Verify the expected POST body contains the renamed item without `UserData`.
  [`api.rs:2226`](../../hifimule-daemon/src/api.rs#L2226)
