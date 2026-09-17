---
title: 'Increase M4A startup buffering on Linux'
type: 'bugfix'
created: '2026-09-17'
status: 'done'
route: 'oneshot'
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** AAC M4A files with tail-located MP4 metadata can decode in bursts at startup, exhausting Linux’s current 300 ms startup reserve and causing audible stutter.

**Approach:** Use a 1.2-second local reserve and 1.5-second PCM capacity only for M4A/MP4 playback on Linux; preserve the existing startup profile for other formats and steady-state output latency.

</frozen-after-approval>

## Implementation Notes

- M4A/MP4 decoder hints now use a 1,200 ms local PCM startup reserve and 1,500 ms queue capacity; all other formats retain the 200 ms reserve and 500 ms capacity.
- Verified with `npm run build:daemon -- test -p hifimule-daemon playback::audio::pulse_output` (6 passed).
- The larger profile derives from the selected representation container (including common M4A aliases) rather than relying solely on a filename hint; capacity is guaranteed to accommodate the requested initial fill within the existing bounded PCM limit.

## Review Triage Log

- high — Fixed: M4A MIME aliases now select the MP4 startup profile through representation metadata.
- medium — Fixed: extensionless provider URLs retain the MP4 profile because classification occurs before the lossy decoder-hint fallback.
- medium — Fixed: startup planning clamps reserves to the bounded PCM capacity and queue construction is at least the resulting initial-fill size.
