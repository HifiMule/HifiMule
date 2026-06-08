---
title: 'Add German (de) Translation'
type: 'feature'
created: '2026-06-08'
status: 'done'
route: 'one-shot'
---

## Intent

**Problem:** HifiMule supported English, French, and Spanish but had no German locale, causing German-speaking users to always see English UI.

**Approach:** Add a full `"de"` section to `catalog.json` (240 keys), register `"de"` in `normalize_language`, and update tests accordingly.

## Suggested Review Order

1. [`hifimule-i18n/src/lib.rs:30`](../../hifimule-i18n/src/lib.rs) — `normalize_language`: verify `"de"` arm handles `de`, `de-DE`, `de-AT`, `de-CH` etc.
2. [`hifimule-i18n/catalog.json`](../../hifimule-i18n/catalog.json) — `"de"` section at end of file: spot-check placeholder variables (`{name}`, `{count}`, `{size}`, `{message}`, etc.)
3. [`hifimule-i18n/src/lib.rs:94`](../../hifimule-i18n/src/lib.rs) — updated + new tests: `falls_back_to_english_for_unknown_language` (now uses `"zz"`), `translates_german_keys`, `interpolates_german_values`
