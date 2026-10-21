---
title: 'Fix Windows UI test paths'
type: 'bugfix'
created: '2026-10-21'
status: 'done'
route: 'one-shot'
---

# Fix Windows UI test paths

## Intent

**Problem:** Two UI tests pass URL pathnames such as `/D:/a/HifiMule/...` to esbuild, preventing the Windows CI regression step from loading their entry points.

**Approach:** Use Node's `fileURLToPath` in both tests to produce native filesystem paths and decode escaped characters. The six affected tests, full Node regression suite, and two Python evidence tests pass locally on macOS. An explicit Windows path conversion check passes; a Windows CI rerun remains necessary. Independent review found no actionable regressions.

## Suggested Review Order

- Convert the Radio settings entry point to a native filesystem path.
  [radioDefaults.test.mjs:7](../../hifimule-ui/tests/radioDefaults.test.mjs#L7)

- Apply the same conversion to the server-switch RPC entry point.
  [serverSwitchBrowse.test.mjs:7](../../hifimule-ui/tests/serverSwitchBrowse.test.mjs#L7)
