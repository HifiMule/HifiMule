---
title: Fix playback CI runtime setup
type: bugfix
created: 2026-09-13
status: done
baseline_commit: 1ac5a98
context: []
---

## Intent

Restore the cross-platform playback evidence pipeline after the runtime verification changes. The supplied logs show missing macOS verification, Linux lock acquisition before parent creation, and Windows failures from host-dependent path operations in Linux simulations and prefix checks.

## Boundaries & Constraints

Preserve exact controlled-prefix ABI checks, host-compatible ABI checks, the daemon build guard, lock exclusivity, playback fixtures, and evidence failure reporting. Do not change runtime versions or publish changes.

## Code Map

- `scripts/playback-session-evidence.py`: currently executes raw Cargo commands.
- `scripts/build-daemon.mjs`: supported runtime-verifying Cargo wrapper.
- `scripts/linux-audio-runtime.mjs`: provisioning lock and Linux compiler preflight.
- `scripts/verify-audio-runtime.mjs`: host-native prefix containment.
- `scripts/tests/`: cross-platform script regression tests.

## Tasks & Acceptance

- [x] Route evidence test execution through the supported daemon wrapper; preserve command arguments and recorded exit codes. Add a mocked orchestration regression.
- [x] Create the Linux lock parent before acquiring the exclusive lock; add a fresh-directory regression without downloading FFmpeg.
- [x] Make simulated Linux paths independent of the host OS and prefix containment correct for native separators; test sibling/parent rejection.
- [x] Run script regressions and evidence orchestration tests; report native CI coverage limitations.

**Acceptance Criteria:**
- Given a macOS evidence run, when tests execute, then runtime verification precedes Cargo without manually granting the verification marker.
- Given a fresh Linux checkout, when the runtime lock is acquired, then its missing parent is created while the lock itself remains exclusive.
- Given Windows hosting Linux simulations, when preflight and wrapper tests execute, then Linux path semantics are retained.
- Given controlled runtime metadata outside the prefix, when verification runs, then it fails even for a sibling with a matching name prefix.
- Given a failing evidence subprocess, when reporting completes, then the JSON record retains the failure and the script returns a nonzero exit code.

## Verification

- `rtk proxy node --test scripts/tests/*.test.mjs`
- `rtk proxy python3 -m unittest discover -s scripts/tests -p 'test_playback_session_evidence.py'`
- `rtk git diff --check`

## Spec Change Log

Review fixed Cargo exit-code propagation through the Node CLI; a real child-process regression covers status 101.

## Results

39 Node tests, 2 Python orchestration tests, and 22 native macOS playback-session tests passed. Native Windows/Linux execution and the full GitHub matrix remain unverified.

## Suggested Review Order

- Route evidence commands through runtime verification
  [playback-session-evidence.py:85](../../scripts/playback-session-evidence.py#L85)

- Preserve failed child exit status
  [build-daemon.mjs:77](../../scripts/build-daemon.mjs#L77)

- Create fresh cache parents before exclusive locking
  [linux-audio-runtime.mjs:110](../../scripts/linux-audio-runtime.mjs#L110)

- Check native prefix containment
  [verify-audio-runtime.mjs:94](../../scripts/verify-audio-runtime.mjs#L94)

- Exercise CLI failure propagation
  [build-daemon.test.mjs:14](../../scripts/tests/build-daemon.test.mjs#L14)

- Verify evidence orchestration and reporting
  [test_playback_session_evidence.py:1](../../scripts/tests/test_playback_session_evidence.py#L1)

- Run orchestration regressions in CI
  [build.yml:41](../../.github/workflows/build.yml#L41)
