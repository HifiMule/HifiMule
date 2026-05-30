---
title: 'Fix Windows sync speed logging'
type: 'bugfix'
created: '2026-05-30'
status: 'done'
route: 'one-shot'
---

# Fix Windows Sync Speed Logging

## Intent

**Problem:** Sync speed logging could report `0.0MB/s` on Windows when fast download or write phases completed in less than one millisecond, because `Duration::as_millis()` truncated the elapsed time to zero.

**Approach:** Calculate transfer timing from `Duration::as_secs_f64()` and log fractional milliseconds for both legacy and provider sync paths, preserving the zero-duration guard for truly unmeasurable timings.

## Suggested Review Order

- [Timing helper](../../hifimule-daemon/src/sync.rs:53) -- confirm sub-millisecond durations keep non-zero speed math.
- [Legacy sync log](../../hifimule-daemon/src/sync.rs:1617) -- confirm download/write logging uses the helper output.
- [Provider sync log](../../hifimule-daemon/src/sync.rs:2227) -- confirm the provider path matches the legacy path.
- [Regression tests](../../hifimule-daemon/src/sync.rs:3018) -- confirm sub-millisecond and zero-duration cases are covered.
