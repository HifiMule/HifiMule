---
title: 'Fix Windows encoding dependency archive extraction'
type: 'bugfix'
created: '2026-10-21'
status: 'done'
route: 'one-shot'
---

# Fix Windows encoding dependency archive extraction

## Intent

**Problem:** Windows audio provisioning prepends MSYS2 tools to the dependency-build environment. GNU tar interprets a native archive path as a remote host and also rejects the native extraction destination. The first fix removed the archive drive prefix but left the destination incompatible, so LAME extraction still failed.

**Approach:** Select Windows' built-in `SystemRoot\\System32\\tar.exe` explicitly for both dependency and FFmpeg extraction, with the same executable checked during preflight. Respect case-insensitive SystemRoot keys and bypass MSYS2 PATH ordering. Keep dependency archive filenames local to their cache directory. Regression tests require the native executable, perform real extraction and license installation on both platform branches, and invoke the actual system tar when running on Windows. Compiler commands remain stubbed. The executable-selection regression failed before the fix and passes afterward; the full script suite passes with one skip. A complete Windows native build still requires CI.

## Suggested Review Order

- Resolve Windows' system tar independently of MSYS2 PATH order.
  [encoding-dependencies.mjs:11](../../scripts/encoding-dependencies.mjs#L11)

- Use the same native extractor for FFmpeg source extraction and preflight.
  [windows-audio-runtime.mjs:226](../../scripts/windows-audio-runtime.mjs#L226)

- Preserve native destination handling during dependency extraction.
  [encoding-dependencies.mjs:35](../../scripts/encoding-dependencies.mjs#L35)

- Check executable selection and perform real extraction with spaced paths on both platform branches.
  [encoding-audio-runtime.test.mjs:35](../../scripts/tests/encoding-audio-runtime.test.mjs#L35)
