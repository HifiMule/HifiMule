---
title: 'Clean build warnings after dependency upgrades'
type: 'chore'
created: '2026-10-26'
status: 'done'
route: 'one-shot'
---

# Clean build warnings after dependency upgrades

## Intent

**Problem:** The dependency and toolchain upgrade exposed deprecated atomic calls and noisy dead-code warnings in the macOS build.

**Approach:** Replace all four `fetch_update` calls with the equivalent `try_update` API. Compile test helpers only for tests and Linux helpers only where needed. Keep five intentionally test-exercised policy variants and diagnostic fields with individual, documented dead-code expectations. Keep the transitive `block v0.1.6` future-incompatibility warning visible. Independent review found no actionable defects.

Validation on macOS: workspace check and compilation of every workspace test target passed through `npm run build:daemon`. All three `playback::output::queue_edit_tests` and both `tray_support::tests` passed. Three pre-existing test-only warnings remain in `api.rs` and `rpc.rs`. Linux and Windows builds were not run.

## Suggested Review Order

- Preserve atomic handoff behavior using the renamed standard-library method.
  [continuity.rs:48](../../hifimule-daemon/src/playback/continuity.rs#L48)
- Apply the same rename to Windows pause accounting.
  [audio.rs:525](../../hifimule-daemon/src/playback/audio.rs#L525)
- Keep Linux tray probing available to portable unit tests.
  [tray_support.rs:6](../../hifimule-ui/src-tauri/src/tray_support.rs#L6)
- Restrict convenience wrappers and persistence readers to their existing test callers.
  [device/mod.rs:1230](../../hifimule-daemon/src/device/mod.rs#L1230)
  [persistence.rs:1777](../../hifimule-daemon/src/playback/persistence.rs#L1777)
- Document exact dead-code expectations while preserving other compiler diagnostics.
  [adaptation.rs:32](../../hifimule-daemon/src/playback/adaptation.rs#L32)
