---
title: Show where to find the Rockbox path prefix
type: docs
created: 2026-11-01
status: done
route: one-shot
---

# Show where to find the Rockbox path prefix

## Intent

**Problem:** The path-prefix help does not explain where the player displays its prefix.

**Approach:** Point to System / Rockbox Info in shared English, French, Spanish and German help and matching documentation. Preserve examples and prefix behavior. Relevant existing UI/localization tests and whitespace checks passed; independent review found no issues. No binary rebuild or live-device changes were needed for this text edit.

## Suggested Review Order

- Shared localized help reaches settings, profiles and device setup.
  [catalog.json:992](../../hifimule-i18n/catalog.json#L992)

- Document the same menu location.
  [rockbox-database.md:29](../../docs/rockbox-database.md#L29)
