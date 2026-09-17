---
title: 'Prime Linux Pulse playback before uncorking'
type: 'bugfix'
created: '2026-09-17'
status: 'done'
route: 'oneshot'
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Linux playback starts with audible dropouts because the Pulse/PipeWire stream is uncorked before it contains PCM. The existing 100 ms startup threshold leaves too little headroom for startup scheduling or source jitter.

**Approach:** Keep the Pulse stream corked while explicitly filling its 100 ms server buffer, retain a 200 ms local PCM reserve from a 300 ms initial fill, then uncork the already-primed stream.

</frozen-after-approval>

## Implementation Notes

- Added a Linux-only startup plan that waits for 300 ms of PCM, writes the negotiated Pulse server capacity while the stream remains corked, and keeps a 200 ms local reserve before uncorking.
- Kept Pulse buffer attributes and steady-state refill behavior unchanged; short decoded tracks can prime their available tail rather than waiting indefinitely.
- Added deterministic startup-plan sizing tests. The focused daemon playback suite passed through `npm run build:daemon -- test -p hifimule-daemon playback`.
- Review follow-up: priming now writes even when presentation timing is temporarily unavailable, targets negotiated `tlength` rather than `maxlength`, and remains full across partial writable windows.

## Review Triage Log

- high — Fixed: PCM writes no longer depend on an optional presentation cursor, preventing a permanently corked startup when timing is not yet available.
- medium — Fixed: the primer retains the negotiated target across partial writable windows instead of lowering its target from the first window.
- medium — Fixed: `PinnedStream` exposes negotiated `tlength`, which is now the Linux startup-prime target; `maxlength` remains the write safety cap and telemetry value.
- low — Addressed with deterministic `StartupPrimer` tests for partial writes and short completed tracks. A hardware-backed call-order test would duplicate the native Pulse integration path and is not suitable for the unit suite.
- low — Addressed: the finished short-track condition is unit-tested; the existing worker path still drains and completes after its PCM queue empties.
