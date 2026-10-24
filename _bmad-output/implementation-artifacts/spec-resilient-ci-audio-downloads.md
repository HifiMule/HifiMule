---
title: 'Make CI audio source downloads resilient'
type: 'bugfix'
created: '2026-10-24'
status: 'done'
route: 'one-shot'
---

# Make CI audio source downloads resilient

## Intent

**Problem:** CI source downloads failed on curl TLS error 35 because the existing default retry policy did not cover it. Other FFmpeg download paths had the same policy.

**Approach:** Use curl's built-in retry handling for all errors, with five retries and exponential backoff. Limit connection establishment to 30 seconds, each transfer to 300 seconds, and the retry window to 600 seconds. An attempt started within the retry window can finish afterward. Preserve file output, SHA-256 verification, existing cache behavior, and Windows signature verification.

Verification: the new real-curl regression failed before the fix and passed afterward. It exercises an interrupted response, verified cache reuse, and checksum rejection with temporary-file cleanup. The full JavaScript script and UI regression suite passed with zero failures and ten platform skips. A hosted CI build was not rerun.

## Suggested Review Order

- Retry TLS and network errors while preserving verified codec archives.
  [encoding-dependencies.mjs:30](../../scripts/encoding-dependencies.mjs#L30)
- Apply the same bounded policy to Windows source, signature, and key downloads.
  [windows-audio-runtime.mjs:295](../../scripts/windows-audio-runtime.mjs#L295)
- Cover Linux FFmpeg source downloads.
  [linux-audio-runtime.mjs:142](../../scripts/linux-audio-runtime.mjs#L142)
- Cover macOS FFmpeg source downloads.
  [macos-audio-runtime.mjs:116](../../scripts/macos-audio-runtime.mjs#L116)
- Exercise real curl recovery, cache reuse, and checksum rejection using a local server.
  [audio-download-retry.test.mjs:11](../../scripts/tests/audio-download-retry.test.mjs#L11)
