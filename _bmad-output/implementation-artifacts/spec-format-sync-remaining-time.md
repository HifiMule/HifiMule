---
title: 'Format sync remaining time in hours and minutes'
type: 'bugfix'
created: '2026-10-23'
status: 'done'
route: 'one-shot'
---

# Format sync remaining time in hours and minutes

## Intent

**Problem:** Long sync estimates display large minute counts, making remaining time harder to read.

**Approach:** At one hour or more, display rounded hours and minutes, omitting zero minutes (for example, `~1 h 30 min left` or `~2 h left`). Preserve existing seconds and minutes formatting below one hour. Use localized strings in English, French, Spanish, and German.

## Suggested Review Order

- Format long estimates and carry rounded minutes into hours; omit zero minutes.
  [BasketSidebar.ts:1762](../../hifimule-ui/src/components/BasketSidebar.ts#L1762)

- Keep whole-hour and mixed-hour wording localized in all four supported languages.
  [catalog.json:733](../../hifimule-i18n/catalog.json#L733)

- Verify thresholds, whole hours, rounding, long durations, and shorter estimates across locales.
  [syncEta.test.mjs:14](../../hifimule-ui/tests/syncEta.test.mjs#L14)
