---
title: 'Start the packaged Linux daemon at desktop login'
type: 'bugfix'
ticket: ''
created: '2026-10-03'
status: 'built'
baseline_revision: 'dd2013a34a21becbf0cf56e2975e4936c912833f'
route: 'oneshot'
route_source: 'auto'
risk: 'medium'
review: 'quick'
review_source: 'pinned'
lenses_ran: ['quick']
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The Fedora RPM installs the daemon but never registers it for desktop login. Linux's unused startup setter also returns success without doing anything, so launching the UI is currently necessary to start the daemon.

**Approach:** Package a shared XDG autostart desktop entry in RPM and DEB that directly launches the installed daemon in the signed-in user's desktop session. Preserve daemon ownership arbitration, one-shot startup and Quit behavior; do not add a restart supervisor or open the UI at login. Document per-user opt-out and verification. Portable AppImage startup remains manual because its mounted sidecar path is temporary. Preserve existing Fedora package metadata and other platforms.

</frozen-after-approval>

## Implementation Notes

- One-shot route: fewer than 100 implementation lines across desktop entry, Linux packaging configuration, existing platform regression tests and deployment guide. Medium risk because startup happens automatically in each desktop session.
- Research confirmed no frontend caller of `settings_set_launch_on_startup`; implementing that setter alone would not fix default package startup. The new registration belongs to the package, and package removal removes it. A user-local entry of the same filename with `Hidden=true` takes priority and survives upgrades.
- The daemon's direct launch already creates a bounded lifecycle ticket and participates in ownership election; launch it without frozen attempt arguments. Keep its existing runtime resolution and lifecycle code intact.
- The authorized pre-existing packaging edits were committed externally before this plan's baseline was captured; the working tree was clean at implementation start. Preserve that current baseline.
- Regression reproduced the missing RPM registration before the fix. After adding the shared entry and both package mappings, the combined packaging/release contract suite passed 31/31; `desktop-file-validate` passed.
- Native Tauri rebundling on Fedora 44 produced both RPM and DEB successfully from the existing release executables. Extracted packages each contain the exact desktop source and executable `/usr/bin/hifimule-daemon`. Tauri emitted an existing binary bundle-marker warning (`__TAURI_BUNDLE_TYPE` not found); no updater changes are in scope.
- Desktop dispatch via `gio launch` using the extracted RPM's entry with only executable paths rebased to the extraction root started an authenticated daemon in a temporary profile. A second dispatch retained the same instance/PID. This is a session-dispatch check, not a real logout/login certification, and no package was globally installed.
- The native shutdown diagnostic received accepted Quit but observed existing `SHUTDOWN_TIMEOUT`/late cleanup rather than timely removal of ownership. Record this separately from startup. Temporary diagnostic launchers were stopped; cleanup was restricted to the exact extracted daemon executable and isolated profile. No claim of successful native Quit qualification.

## Plan Change Log

## Review Triage Log

- Quick review: one low finding, patched. The deployment guide implied audio/keyring readiness ordering, which an XDG entry does not guarantee. Reworded to graphical-session initialization with access to the user's session services; no readiness guarantee remains. No deferred findings.
- Verification exposed a pre-existing shutdown limitation; deferred separately because this configuration change does not alter teardown. Startup and duplicate-owner checks passed, but full logout/login and timely native Quit remain unverified.

## Verification

- Add and first run a failing platform regression proving RPM/DEB registration is missing; then validate entry fields, source availability and portable AppImage isolation.
- Run `rtk node --test --test-isolation=none scripts/tests/tauri-platform-config.test.mjs scripts/tests/release-contract.test.mjs scripts/tests/release-runtime-contract.test.mjs`.
- Validate desktop syntax using `desktop-file-validate` if available. Build RPM/DEB from existing release executable when possible and inspect the actual archived autostart entry and executable path.
- Exercise an isolated daemon launch using desktop-entry dispatch when the host desktop/runtime permits; record any environmental limit. A real logout/login test cannot be inferred from a package content check.
