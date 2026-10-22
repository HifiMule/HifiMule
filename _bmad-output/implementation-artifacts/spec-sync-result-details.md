---
title: 'Readable and selectable sync result details'
type: 'bugfix'
created: '2026-10-22'
status: 'done'
route: 'one-shot'
---

# Readable and selectable sync result details

## Intent

**Problem:** Sync warning and error lists were capped at 120px, with error rows truncated to one line. Reading or copying diagnostics was difficult.

**Approach:** Fill the available result-panel height with scrollable, wrapping messages. Add a Shoelace icon button that selects the entire list, including offscreen messages, for copying. Preserve footer controls and allow the result panel to scroll in very short windows. Localize the button in English, French, Spanish, and German.

## Suggested Review Order

1. [Result rendering and selection](../../hifimule-ui/src/components/BasketSidebar.ts) — warning/error states share native text selection.
2. [Result layout](../../hifimule-ui/src/styles.css) — flexible message height, wrapping, and constrained scrolling.
3. [Translations](../../hifimule-i18n/catalog.json) — accessible action label.
4. [Unit checks](../../hifimule-ui/tests/syncCompletion.test.mjs) and [browser checks](../../hifimule-ui/tests/sync-result-details.html) — build and 24 unit checks pass; browser fixture passes 23 layout/selection checks in tall, compact, and very short panels.

Independent review's short-window overflow concern was patched and verified in the browser.
