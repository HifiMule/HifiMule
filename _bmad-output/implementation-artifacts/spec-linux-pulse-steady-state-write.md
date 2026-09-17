---
title: 'Keep Linux Pulse PCM writes independent of timing metadata'
type: 'bugfix'
created: '2026-09-17'
status: 'done'
route: 'oneshot'
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Linux playback can still cut out after startup because the steady-state Pulse write loop waits for optional presentation timing metadata before it writes decoded PCM.

**Approach:** Always submit writable PCM to Pulse; use timing metadata only for position accounting, exactly as the startup priming path does.

</frozen-after-approval>

## Implementation Notes

- Steady-state PCM rendering and writing no longer depend on optional Pulse timing metadata; timing remains an input to presentation accounting only.
- Added a local submission cursor so PCM emitted before timing becomes available is included in the presentation ledger when timing later resumes.
- `npm run build:daemon -- test -p hifimule-daemon playback::audio::pulse_output` passed: 5 tests. The broader `playback` filter remains blocked by two pre-existing decoder tests whose ALAC fixture files are absent.

## Review Triage Log

- high — Fixed: missing timing metadata no longer prevents a writable PCM buffer from being submitted to Pulse.
- medium — Fixed: the local submission cursor accounts for audio written without timing metadata and continues monotonically when a native cursor later appears.
- low — Fixed with a deterministic regression test covering an unavailable cursor followed by timing recovery; a native Pulse mock is unnecessary because the accounting seam is pure and the write is now unconditional.
