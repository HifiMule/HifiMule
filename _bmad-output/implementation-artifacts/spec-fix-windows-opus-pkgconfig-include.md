---
title: 'Fix Windows Opus pkg-config include directory'
type: 'bugfix'
created: '2026-10-21'
status: 'done'
route: 'one-shot'
---

# Fix Windows Opus pkg-config include directory

## Intent

**Problem:** FFmpeg now links LAME successfully but cannot compile the Opus detection probe. Our Windows `opus.pc` points at `include/`, while CMake installs Opus headers under `include/opus/` and FFmpeg includes `opus_multistream.h` directly.

**Approach:** Emit `Cflags: -I${includedir}/opus` for Opus, matching its pinned upstream `opus.pc.in`. Keep the include root for the other packages. Expand the existing build fixture to extract all four pinned Windows dependency layouts and check the generated include flags for all five package files. The regression fails with the old Opus flags and passes after the fix. A source audit confirms the remaining include roots, MSVC library names, and Vorbis static dependency chain match the pinned upstream sources. The targeted runtime tests and full script suite pass, with one existing skip in the full suite. Native Windows compilation remains for CI.

## Suggested Review Order

- Match Opus's generated include flags to its installed header subdirectory.
  [encoding-dependencies.mjs:64](../../scripts/encoding-dependencies.mjs#L64)

- Exercise all Windows dependency archives and verify every generated package's include flags.
  [encoding-audio-runtime.test.mjs:40](../../scripts/tests/encoding-audio-runtime.test.mjs#L40)
