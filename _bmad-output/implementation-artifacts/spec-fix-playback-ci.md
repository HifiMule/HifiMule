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

## Follow-up: Linux and Windows provisioning

The next CI run passed macOS but exposed an unsupported FFmpeg 9 configure flag and loss of the Windows Path value when spreading process.env into a plain object. Removed --disable-postproc, added NASM to Linux x64 preflight and CI/developer prerequisites, and consolidated Windows path aliases before prepending FFmpeg for daemon, sidecar, and CI environment exports.

Validation: 42 Node regressions and 2 Python orchestration tests pass. Verified the cached FFmpeg archive against the manifest SHA-256 and successfully ran its configure script with the corrected manifest flags on macOS ARM64; the enabled libraries and decoders match the intended subset. Native Linux and Windows builds still require CI confirmation.

## Follow-up: exact source ABI metadata

Linux successfully built FFmpeg but rejected its libraries because the manifest expected micro version 100. Verified the pinned archive SHA-256 and read all four library version headers: avcodec=63.1.101, avformat=63.1.101, avutil=61.1.101, swresample=7.1.101. Updated the exact manifest values and verifier fixtures, retaining rejection of nonmatching versions. Also added libasound2-dev to both Linux workflows, developer prerequisites, and preflight based on the local alsa-sys build script requirement.

Validation: all four source versions equal the corrected manifest; 43 Node regression tests pass; git diff --check passes. Full native Linux CI remains pending.

## Follow-up: Linux daemon native linking

UTM guest inspection confirmed the installed daemon requested FFmpeg 8 SONAMEs (62/62/60/6), despite FFmpeg 9 libraries being bundled. Pinning FFMPEG_DIR alone reproduced the wrong linkage in a native rebuild. The root build script must emit the controlled library directory before system directories from libmtp/native dependencies. Linux entry points now supply the verified prefix consistently, and packaging validates the daemons actual DT_NEEDED majors before accepting it. Added stale-prefix and wrong-linkage regression coverage; 44 Node tests pass. Native rebuild verification follows.

Native ARM64 verification completed: the first rebuild with FFMPEG_DIR alone still linked FFmpeg 8; adding the private search path before libmtp fixed all four DT_NEEDED majors. Installed the corrected daemon in the UTM Linux guest with /usr/bin/hifimule-daemon.before-abi-fix as backup. ldd confirms all four FFmpeg libraries resolve from /usr/lib/HifiMule/bundled-libs. Existing running process was left intact pending user tray quit/reopen.
