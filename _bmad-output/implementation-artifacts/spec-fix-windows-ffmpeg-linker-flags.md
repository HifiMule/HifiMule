---
title: 'Use native Windows FFmpeg linker flags and retain compiler diagnostics'
type: 'bugfix'
created: '2026-10-21'
status: 'done'
route: 'one-shot'
---

# Use native Windows FFmpeg linker flags and retain compiler diagnostics

## Intent

**Problem:** All pinned codec dependencies compile, but FFmpeg's compiler sanity check fails. The Windows builder supplies GNU `-L` syntax through `--extra-ldflags`. In the pinned configure script, this option is appended while the linker flag filter is still `echo`, before MSVC detection, and is never translated afterward. Microsoft link.exe warns with LNK4044 for the unrecognized option, which configure treats as failure. Cleanup also deletes the compiler's actual diagnostic log.

**Approach:** Pass native `-libpath:D:/.../lib` syntax with forward slashes. Extract configure/build execution into a directly testable helper that retains the final compiler probe outside the source workspace and prints its last 200 lines on failure. Omit configure's initial environment dump, preserve the original subprocess error, and retain existing cleanup behavior. A local reproduction using parsing and linking helpers from the pinned FFmpeg archive confirmed the GNU flag fails and the native flag passes with a simulated Microsoft linker. Regression tests cover x64/ARM64 command flags, diagnostic persistence after source cleanup, omitted environment values, and failure without a log. The full script suite passes with one existing skip. A complete native Windows build remains for CI.

## Suggested Review Order

- Supply native linker flags before configure's compiler detection can translate them.
  [windows-audio-runtime.mjs:214](../../scripts/windows-audio-runtime.mjs#L214)

- Preserve the last compiler probe without copying the initial environment dump.
  [windows-audio-runtime.mjs:229](../../scripts/windows-audio-runtime.mjs#L229)

- Verify native flags, retained compiler errors, and the original exception.
  [windows-audio-runtime.test.mjs:119](../../scripts/tests/windows-audio-runtime.test.mjs#L119)
