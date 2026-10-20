---
title: 'Automatically size sync staging and log effective tuning'
type: 'feature'
created: '2026-10-20'
status: 'done'
route: 'one-shot'
baseline_commit: '09a6f95'
---

# Automatically size sync staging and log effective tuning

## Intent

**Problem:** A fixed eight-track staging default requires manual tuning across different CPU counts. The execution-start diagnostic omitted the local encoding worker budget and did not distinguish automatic configuration from effective concurrency.

**Approach:** Default `staging_max_tracks` to `0`, meaning `max(1, available_parallelism - 1)`, using the same calculation as local encoding workers. Capture automatic parallelism once per execution and resolve both settings before logging or creating queues; preserve explicit staging limits `1`–`32` and encoding limits `1`–`64`. Missing or invalid fields default independently to automatic. Automatic counts follow the requested formula even above the explicit-setting maxima. Log all six effective tuning values plus both configured parallelism values at execution start. Existing saved explicit values, including `16`, remain explicit until changed to `0` or removed. The temporary-file byte budget is unchanged.

Tests cover zero acceptance, invalid fallback, CPU boundary counts, preserved overrides, automatic snapshot consistency, actual pipeline capacities and edits applying only to the next run. The pipeline test observed configured staging `0` resolve to `17`, explicit values `1` and `3` remain unchanged, and all six effective values appear in each startup log. Gated transfer fixtures explicitly request eight staging slots and two encoder jobs so their overlap requirements do not depend on host CPU count. The full suite passed 1,623 unit tests and five contract tests, with 14 ignored. Independent review found no material issues; formatting and diff checks passed.

## Suggested Review Order

- Resolve both automatic settings from one CPU snapshot while preserving explicit limits.
  [sync_performance.rs:11](../../hifimule-daemon/src/sync_performance.rs#L11)
  [sync_performance.rs:47](../../hifimule-daemon/src/sync_performance.rs#L47)

- Capture positive execution limits and report every effective performance value before transfer setup.
  [sync.rs:3274](../../hifimule-daemon/src/sync.rs#L3274)

- Check zero validation, invalid fallback and low/high CPU boundaries.
  [sync_performance.rs:123](../../hifimule-daemon/src/sync_performance.rs#L123)

- Verify actual pipeline caps and settings snapshots for automatic and explicit configurations.
  [sync.rs:9326](../../hifimule-daemon/src/sync.rs#L9326)

- Review configuration defaults, supported values and startup diagnostics.
  [sync-performance.md:13](../../docs/sync-performance.md#L13)
