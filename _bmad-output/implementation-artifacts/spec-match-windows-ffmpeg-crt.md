---
title: 'Match FFmpeg and codec C runtimes on Windows'
type: 'bugfix'
created: '2026-10-21'
status: 'done'
route: 'one-shot'
---

# Match FFmpeg and codec C runtimes on Windows

## Intent

**Problem:** FFmpeg now passes its compiler sanity check but cannot link its LAME probe. The retained diagnostic shows LNK4098 runtime conflicts and unresolved DLL CRT imports such as `__imp_fseek`. Codec archives use CMake's `MultiThreadedDLL` runtime, while FFmpeg's compiler command does not select the DLL runtime.

**Approach:** Add `--extra-cflags=-MD` and `--extra-cxxflags=-MD` to the Windows configure flags in the runtime manifest. FFmpeg accumulates these extra compiler flags alongside the existing include path and applies them before library probes. Keeping the flags in the manifest also includes them in runtime receipts, invalidating cached builds without the runtime selection. The command regression fails before the fix and passes afterward for x64 and ARM64; a receipt regression rejects the prior flags. The targeted suites and full script suite pass with one existing full-suite skip. Native Windows compilation remains for CI.

## Suggested Review Order

- Pin the same DLL C runtime for FFmpeg that the codec builds already use.
  [audio-runtime.json:19](../../hifimule-daemon/audio-runtime.json#L19)

- Verify both Windows architectures receive the C and C++ runtime flags.
  [windows-audio-runtime.test.mjs:115](../../scripts/tests/windows-audio-runtime.test.mjs#L115)

- Reject cached runtime receipts that omit the matching runtime configuration.
  [windows-audio-runtime.test.mjs:222](../../scripts/tests/windows-audio-runtime.test.mjs#L222)
