# Playback adaptation evidence — policy v1

- Commit/build:
- OS/architecture:
- Provider and server version:
- Sanitized server identity:
- Network profile and monotonic phase schedule (ms):
- Available representations (`id`, codec/container, kb/s, quality tier):
- Selected representation at each occurrence boundary:
- Eligible samples (bytes, elapsed ms, bytes/s, compressed ms, PCM ms):
- Excluded samples and reason (startup/cache/range/partial/failure/short):
- Buffering events and durations:
- Compressed/PCM high-water bytes:
- Switch count and reasons:
- Recovery boundary/outcome:
- Generation/fence/reporting assertions:
- Environment limitations:

Never record URLs, headers, tokens, usernames, provider session IDs, or raw portable server IDs.

## Recorded deterministic review run — 2026-09-29

- Commit/build: working tree review of Story 16.12; Rust test profile with the verified FFmpeg 9 runtime contract.
- OS/architecture: macOS development host; installed Windows/Linux certification remains Story 16.14.
- Provider and server version: mocked OpenSubsonic `1.16.1`, Navidrome `0.64.0` capability response.
- Available representations: scoped original `flac-320kbps` plus verified `navidrome-mp3-192` fallback.
- Deterministic profiles: three independent complete-request slow/depleted samples select the 192 kb/s fallback; an unsustainable fallback retains the current representation and reports `noSustainableAlternative`; a sub-120-second burst cannot recover; a full 120-second recovery span can.
- Exclusions verified: startup, cache, range/seek, partial, failure, and short samples never contribute capacity.
- Bounds verified: 16 samples per scope, 64 scopes process-wide, 16 selected servers process-wide, and one pre-claim successor replacement within the 60-second preparation deadline.
- Fence/state assertions: selection remains provisional before handoff, and the fenced handoff carries the exact stable ID, codec, container, bitrate, and reason committed to the snapshot.
- Commands: targeted adaptation tests (7 passed) and the Navidrome alternative integration test (1 passed with local loopback permission).
- Environment limitations: no installed-platform soak, physical output interruption, or live remote-provider measurement was claimed; those remain Story 16.14 evidence work.
