---
title: 'Fix basket sync selection CI fixture'
type: 'bugfix'
created: '2026-10-21'
status: 'done'
route: 'one-shot'
---

# Fix basket sync selection CI fixture

## Intent

**Problem:** The extracted sync-start test harness lacks the capacity-estimation dependencies now used by `BasketSidebar`, causing both selection regression tests to throw before preparing a plan. Its expected Auto-Fill ceiling also assumes a stale slot size is authoritative.

**Approach:** Initialize an empty estimate cache, a stable capacity revision, and an asynchronous estimate refresh stub. Assert that an unestimated Auto-Fill selection leaves capacity resolution to sync preparation while preserving manual selections, server routing, exclusions, force mode, and plan discard checks. The focused selection tests, complete script test suite (one existing skip), and basket estimate, sync plan, and sync capacity suites pass.

## Suggested Review Order

- Supply the capacity dependencies used by the extracted sync-start method.
  [basket-sync-selection.test.mjs:39](../../scripts/tests/basket-sync-selection.test.mjs#L39)

- Keep manual selection and routing checks while expecting daemon-derived Auto-Fill capacity.
  [basket-sync-selection.test.mjs:65](../../scripts/tests/basket-sync-selection.test.mjs#L65)

- Verify pure Auto-Fill sends no stale slot ceiling or manual selection.
  [basket-sync-selection.test.mjs:83](../../scripts/tests/basket-sync-selection.test.mjs#L83)
