---
title: 'Localize device initialization and music import'
type: 'bugfix'
created: '2026-10-16'
status: 'done'
route: 'one-shot'
---

# Localize device initialization and music import

## Intent

**Problem:** Device initialization rendered hardcoded English labels, buttons, status messages, and profile descriptions despite the selected language. The music import step retained the English initialization title.

**Approach:** Use the shared catalog for the initialization form and a dedicated import title. Add French, Spanish, and German translations alongside English, reuse existing common labels, and translate stock profile metadata while preserving customer overrides, IDs, and folder defaults. UI build, nine existing scan tests, seven i18n tests, catalog parity checks, and four-language rendering/profile smoke checks passed. Independent review found one issue with custom profile metadata; patched before completion.

## Suggested Review Order

- Follow the localized setup form and import title.
  [InitDeviceModal.ts:55](../../hifimule-ui/src/components/InitDeviceModal.ts#L55)
- Check preservation of customized profile text.
  [InitDeviceModal.ts:430](../../hifimule-ui/src/components/InitDeviceModal.ts#L430)
- Review the new strings in all four languages.
  [catalog.json:3](../../hifimule-i18n/catalog.json#L3)
