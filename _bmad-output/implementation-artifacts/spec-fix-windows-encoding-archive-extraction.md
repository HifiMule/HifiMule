---
title: 'Fix Windows encoding dependency archive extraction'
type: 'bugfix'
created: '2026-10-21'
status: 'done'
route: 'one-shot'
---

# Fix Windows encoding dependency archive extraction

## Intent

**Problem:** Windows audio provisioning prepends MSYS2 tools to the dependency-build environment. GNU tar interprets the drive colon in an absolute archive filename as a remote host, so extracting LAME fails with `Cannot connect to D: resolve failed` even though downloads and FFmpeg signature verification succeed.

**Approach:** Extract dependency archives from their cache directory using the pinned archive filename and an absolute destination. This preserves archive hash verification and avoids implementation-specific tar flags. Regression tests invoke the real builder against a checksum-pinned local archive, perform real extraction, and verify license installation for both Windows and Unix branches while stubbing compiler commands. Both tests failed before the fix and pass afterward; the full script suite passes with one skip. A complete Windows native build still requires CI.

## Suggested Review Order

- Keep archive filenames local to tar while preserving the extraction destination.
  [encoding-dependencies.mjs:31](../../scripts/encoding-dependencies.mjs#L31)

- Exercise the builder with a real archive, spaced paths, and both platform branches.
  [encoding-audio-runtime.test.mjs:28](../../scripts/tests/encoding-audio-runtime.test.mjs#L28)
