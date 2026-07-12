---
title: 'Fix Autofill Track Number Filenames'
type: 'bugfix'
created: '2026-07-12'
status: 'done'
route: 'one-shot'
---

# Fix Autofill Track Number Filenames

## Intent

**Problem:** Tracks selected through autofill lost their provider track number before sync path generation, so filenames fell back to `00 - ...` while album and playlist sync kept the correct number.

**Approach:** Carry `track_number` through `AutoFillItem` and reuse it in every autofill-to-`DesiredItem` conversion path.

## Suggested Review Order

- [../../hifimule-daemon/src/auto_fill/mod.rs](../../hifimule-daemon/src/auto_fill/mod.rs) -- confirm autofill selections now retain provider `track_number`.
- [../../hifimule-daemon/src/rpc.rs](../../hifimule-daemon/src/rpc.rs) -- confirm RPC autofill paths pass `track_number` into sync desired items.
- [../../hifimule-daemon/src/main.rs](../../hifimule-daemon/src/main.rs) -- confirm auto-sync autofill path keeps the same field.
- [../../hifimule-daemon/src/auto_fill/pipeline.rs](../../hifimule-daemon/src/auto_fill/pipeline.rs) -- confirm pipeline-generated autofill items preserve `Song.track_number`.
