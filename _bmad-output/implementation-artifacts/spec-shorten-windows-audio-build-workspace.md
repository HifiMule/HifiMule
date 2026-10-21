---
title: 'Shorten Windows audio build workspace'
type: 'bugfix'
created: '2026-10-21'
status: 'done'
route: 'one-shot'
---

# Shorten Windows audio build workspace

## Intent

**Problem:** After archive extraction succeeds, Windows audio provisioning fails during LAME's CMake compiler check. Building under the FFmpeg staging directory carries the release, target, PID, and UUID into every source and object path. CMake reports a 240-character object directory, and NMake cannot open the generated compiler dependency makefile.

**Approach:** Create a uniquely owned `hm-audio-` workspace under the system temporary directory for FFmpeg and dependency sources. Install into the existing staging prefix, retain cache locking and final runtime validation, and remove the source workspace in the existing finally block. Export the source builder so regression tests exercise its workspace allocation and cleanup directly. Both tests fail with the old staging-derived source directory and pass with the short workspace; they verify a representative nested CMake dependency path remains below 250 characters and cleanup occurs after extraction errors and invalid archives. The full script suite passes with one existing skip. Native Windows compiler execution remains for CI.

## Suggested Review Order

- Allocate short, isolated sources while preserving install staging and cleanup ownership.
  [windows-audio-runtime.mjs:214](../../scripts/windows-audio-runtime.mjs#L214)

- Reproduce staging path growth and check native path headroom plus failure cleanup.
  [windows-audio-runtime.test.mjs:89](../../scripts/tests/windows-audio-runtime.test.mjs#L89)
