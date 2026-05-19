---
title: 'Rockbox Playback Log Scrobbling'
type: 'bugfix'
created: '2026-05-19'
status: 'done'
route: 'one-shot'
---

# Rockbox Playback Log Scrobbling

## Intent

**Problem:** Rockbox now records playback history in `.rockbox/playback.log` as `timestamp:elapsed_ms:duration_ms:path`, so HifiMule's old root `.scrobbler.log` reader no longer sees current scrobbles.

**Approach:** Prefer `.rockbox/playback.log`, keep `.scrobbler.log` as a compatibility fallback, and parse playback-log paths into artist, album, and title fields for the existing submission and dedup pipeline.

## Suggested Review Order

**Log discovery**

- Prefer the new Rockbox log path while keeping legacy fallback behavior.
  [`scrobbler.rs:185`](../../hifimule-daemon/src/scrobbler.rs#L185)

- The processing entry point now uses whichever supported log was found.
  [`scrobbler.rs:210`](../../hifimule-daemon/src/scrobbler.rs#L210)

**Playback parsing**

- New playback-log parser handles timestamp, elapsed, duration, and path metadata.
  [`scrobbler.rs:94`](../../hifimule-daemon/src/scrobbler.rs#L94)

- The generic parser detects playback-log lines before legacy tab-separated rows.
  [`scrobbler.rs:50`](../../hifimule-daemon/src/scrobbler.rs#L50)

**Tests**

- Parser coverage includes the supplied Rockbox path shape and millisecond timestamps.
  [`scrobbler.rs:425`](../../hifimule-daemon/src/scrobbler.rs#L425)

- Device processing proves `.rockbox/playback.log` wins over `.scrobbler.log`.
  [`scrobbler.rs:596`](../../hifimule-daemon/src/scrobbler.rs#L596)
