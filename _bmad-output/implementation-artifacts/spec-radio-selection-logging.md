---
title: 'Radio selection diagnostics in daemon.log'
type: 'feature'
created: '2026-10-02'
status: 'done'
route: 'one-shot'
---

# Radio selection diagnostics in daemon.log

## Intent

**Problem:** Radio decisions were difficult to debug because daemon.log did not explain candidate selection.

**Approach:** Use the existing daemon logger with a `[Radio]` prefix for initial source retrieval, seeded ordering, ranked track metadata, artist transition proposals, source-window rejection counts, preparation results, queue admission, discarded refills, cycle changes, and failure outcomes. Refill IDs, start tickets, and ranking pass IDs help follow decisions. Actual admission records include the track title, artist, source, queue position, and center. Provider URLs and error bodies are not logged; string metadata uses escaped Debug formatting. Candidate diagnostics show at most ten ranked results per selection pass. Selection policy is unchanged.

## Suggested Review Order

- Follow source retrieval, preparation, and refill outcomes.
  [playback_selection.rs:461](../../hifimule-daemon/src/rpc/playback_selection.rs#L461)

- Inspect ranking settings, recording-copy policy, and candidate metadata.
  [selection.rs:1070](../../hifimule-daemon/src/playback/selection.rs#L1070)

- Follow artist relation attempts and proposed transitions.
  [radio.rs:146](../../hifimule-daemon/src/playback/radio.rs#L146)

- Distinguish proposed tracks from durable queue admissions.
  [session.rs:2756](../../hifimule-daemon/src/playback/session.rs#L2756)

## Verification

- Radio-filtered daemon tests: 56 passed, with local mock HTTP server access enabled.
- Selection unit tests: passed using the controlled FFmpeg build wrapper.
- Formatting checked for the four changed Rust files; git diff whitespace check passed.
- Independent adversarial review: tightened candidate limits, ranking correlation, transition wording, source-failure diagnostics, and terminal refill outcomes. Retained the existing synchronous daemon logger and bounded metadata lookup rather than introducing a separate logging system.
