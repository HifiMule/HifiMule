---
title: 'Fix profile-history and playback-selection test regressions'
type: 'bugfix'
created: '2026-10-24'
status: 'done'
route: 'one-shot'
---

# Fix profile-history and playback-selection test regressions

## Intent

**Problem:** Auto-sync regression tests expected pre-migration profile IDs and exercised completion without its execution audio fingerprint; results depended on the user's profile store. Playback-selection script tests omitted the numeric validation dependency and assumed the seed remained a number-type input.

**Approach:** Consolidate ID/fingerprint recording in the shared production completion helper. Exercise it using isolated legacy stores migrated into canonical system IDs, preserving failure/cancellation and changed-selection assertions. Load the real numeric validator in the script test DOM harness, locate the seed by its label, and verify invalid seeds cannot save before correction to zero. Full Rust and combined script/UI JavaScript suites pass; independent review found no actionable defects.

## Suggested Review Order

- Record execution identity and audio fingerprint through one completion entry point.
  [main.rs:1825](../../hifimule-daemon/src/main.rs#L1825)

- Isolate migration fixtures and preserve canonical history, recovery, and alias assertions.
  [tests.rs:5](../../hifimule-daemon/src/tests.rs#L5)

- Load actual numeric validation into the playback-selection test DOM.
  [playback-selection-ui.test.mjs:45](../../scripts/tests/playback-selection-ui.test.mjs#L45)

- Verify invalid seed drafts never save, while corrected zero remains valid.
  [playback-selection-ui.test.mjs:188](../../scripts/tests/playback-selection-ui.test.mjs#L188)

