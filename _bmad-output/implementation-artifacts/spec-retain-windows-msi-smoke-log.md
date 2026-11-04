---
title: 'Retain Windows MSI smoke log'
type: 'chore'
created: '2026-11-04'
status: 'done'
route: 'one-shot'
---

# Retain Windows MSI smoke log

## Intent

**Problem:** The Windows smoke script generates a verbose MSI installation log, but CI uploads only the smoke summary. The 0.21.0 installation failure therefore lacks the action diagnostics needed to identify its cause.

**Approach:** Add the existing `hifimule-msi-install.log` to the Windows smoke artifact's explicit upload paths. Keep the existing `always()` condition so failures retain diagnostics, and name the upload step to reflect both kinds of evidence.

## Suggested Review Order

- Retain the verbose installer trace alongside the existing smoke summary, including failed runs.
  [smoke-test.yml:66](../../.github/workflows/smoke-test.yml#L66)
