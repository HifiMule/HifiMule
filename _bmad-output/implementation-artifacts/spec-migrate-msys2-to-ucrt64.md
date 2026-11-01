---
title: 'Migrate MSYS2 setup to UCRT64'
type: 'chore'
created: '2026-11-01'
status: 'done'
route: 'one-shot'
---

# Migrate MSYS2 setup to UCRT64

## Intent

**Problem:** Both Windows GitHub Actions workflows inherit setup-msys2's deprecated MINGW64 default.

**Approach:** Explicitly select UCRT64 in build and release workflows and document the UCRT64 shell for local setup. The installed tools are shared MSYS packages; the existing MSVC compilation remains appropriate.

## Suggested Review Order

- Select UCRT64 for Windows build evidence.
  [build.yml:69](../../.github/workflows/build.yml#L69)
- Select the same environment for Windows release and candidate builds.
  [release.yml:113](../../.github/workflows/release.yml#L113)
- Align local prerequisites with CI and explain the shared build tools.
  [development-guide.md:56](../../docs/development-guide.md#L56)
