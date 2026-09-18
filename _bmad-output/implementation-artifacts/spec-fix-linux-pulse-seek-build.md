---
title: 'Fix Linux PulseAudio seek build'
type: 'bugfix'
created: '2026-09-18'
status: 'done'
route: 'oneshot'
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The Linux PulseAudio output path still accepts a boolean seek capability after the decoder API changed to require the typed seek mechanism, causing release builds to fail.

**Approach:** Preserve and forward the optional typed seek mechanism through the PulseAudio output boundary, derive the local boolean only for Pulse-specific status logic, and platform-gate imports used solely by non-Linux output.

</frozen-after-approval>

## Implementation Notes

- Updated the Linux PulseAudio boundary to carry `Option<PlaybackSeekMechanism>` through to the decoder. Pulse-specific event handling still derives its local boolean from that option.
- Gated decoder and CPAL stream imports that are used only by non-Linux output, eliminating Linux-only unused-import warnings.
- Added a Linux CI daemon check after controlled runtime provisioning, so cfg-gated PulseAudio type errors fail pull requests.

## Review Triage Log

- patch — Linux CI provisioned PulseAudio and FFmpeg dependencies without compiling the daemon. It now runs the controlled daemon check.
- defer — A direct behavior test that confirms the typed seek mechanism reaches the decoder needs a new testable output-boundary seam; compilation coverage now catches the reported type mismatch.
