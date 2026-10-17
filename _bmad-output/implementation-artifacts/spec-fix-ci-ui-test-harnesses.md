---
title: 'Fix CI UI test harnesses'
type: 'bugfix'
created: '2026-10-17'
status: 'done'
route: 'one-shot'
---

# Fix CI UI test harnesses

## Intent

**Problem:** Script regressions in CI use browser mocks and layout assertions that predate the UI accessibility and loading improvements. Playback construction fails because the document mock lacks event methods; running the full script suite exposes related stale assumptions.

**Approach:** Update test boundaries to support document events, DOM connectivity, custom elements, precise selectors, and extracted library modules. Align selection and layout assertions with the current production behavior. Verify visibility listener removal directly. Production code is unchanged.

## Suggested Review Order

- Document events and listener cleanup cover the reported construction failure.
  [playback-ui.test.mjs](../../scripts/tests/playback-ui.test.mjs)
- Load real extracted status and cache modules in the browse harness.
  [browse-mode-ui.test.mjs](../../scripts/tests/browse-mode-ui.test.mjs)
- Model connected menu nodes so reveal and keyboard activation run.
  [context-menu-ui.test.mjs](../../scripts/tests/context-menu-ui.test.mjs)
- Resolve actual descendant selectors and retain the compact reading floor.
  [destination-ui.test.mjs](../../scripts/tests/destination-ui.test.mjs)
- Support the radio custom-element wrapper and distinguish error notifications from progress.
  [library-radio-ui.test.mjs](../../scripts/tests/library-radio-ui.test.mjs)
