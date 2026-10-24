---
title: 'Fix the Windows FFmpeg AC-3 build'
type: 'bugfix'
created: '2026-10-24'
status: 'done'
route: 'one-shot'
---

# Fix the Windows FFmpeg AC-3 build

## Intent

**Problem:** The pinned FFmpeg source declares static E-AC-3 helpers in `ac3dec.h`, but includes their definitions in `ac3dec_float.c` only with `CONFIG_EAC3_DECODER`. Building the AC-3 decoder alone fails with MSVC C2129. The trailing missing `stdbit.h` diagnostic is an optional configure probe, not the build failure.

**Approach:** Enable the companion E-AC-3 decoder through the Windows configure flags. The existing receipt comparison invalidates cached runtimes with the old flags. Preserve the pinned source archive, signature verification, and other platforms' flags.

## Suggested Review Order

- Include the helper definitions required by MSVC without modifying upstream source.
  [audio-runtime.json:19](../../hifimule-daemon/audio-runtime.json#L19)
- Check both Windows architectures receive the companion decoder flag.
  [windows-audio-runtime.test.mjs:130](../../scripts/tests/windows-audio-runtime.test.mjs#L130)
