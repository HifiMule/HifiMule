---
title: 'Stabilize Linux release smoke rendering under Xvfb'
type: 'bugfix'
created: '2026-09-20'
status: 'in-review'
baseline_commit: '18ec7f7befb6700dd34d3c67e16f9103296fb1c7'
---

## Intent

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

**Problem:** The v0.15.0 Linux release smoke installs successfully and receives authenticated daemon health, but hifimule-ui aborts in Xlib `_XRead` before confirming hydration. The smoke script launches the installed GTK/WebKit application under Xvfb without an explicit headless rendering configuration. The exact triggering extension is not established by the supplied log.

**Approach:** Configure the Linux smoke process for X11 software rendering, disabling WebKit accelerated compositing in the Xvfb environment. Preserve actual installed-webview hydration and lifecycle assertions, and validate the mitigation against the existing release artifact on Ubuntu.

## Boundaries & Constraints

**Always:** Keep the mitigation scoped to the Linux smoke harness. Apply the same environment to initial, concurrent, reopened, and recovery UI processes. Continue requiring a fresh UI acknowledgment tied to the current daemon identity. Report Linux validation as pending until a real runner completes it.

**Ask First:** Changing application-wide rendering defaults, release artifacts, or published tags.

**Never:** Treat daemon health alone as a smoke pass, synthesize hydration markers, increase timeouts to conceal the crash, or disable the WebKit sandbox.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Headless UI | Installed deb under Xvfb | UI renders and acknowledges current daemon state | Missing acknowledgment still fails |
| Subsequent launches | Concurrent, reopened, or recovered UI | Same rendering environment and existing identity checks | Existing lifecycle failures retained |
| Graphics crash persists | Xlib abort despite software configuration | Job remains failed with diagnostic evidence | Obtain native backtrace before claiming root cause |

</frozen-after-approval>

## Code Map

- `scripts/smoke-tests/smoke-linux.sh` — owns Xvfb and every Linux installed UI launch.
- `scripts/smoke-tests/smoke-common.sh` — authenticated daemon polling and authoritative webview acknowledgment; shared with macOS.
- `.github/workflows/smoke-test.yml` — Ubuntu 22.04 runner and manual dispatch accepting an existing release tag.

## Tasks & Acceptance

**Execution:**
- [x] `scripts/smoke-tests/smoke-linux.sh` — export `GDK_BACKEND=x11`, `LIBGL_ALWAYS_SOFTWARE=1`, and `WEBKIT_DISABLE_COMPOSITING_MODE=1` before UI launch; explain their Xvfb scope and print only these non-secret settings.
- [x] `scripts/smoke-tests/smoke-linux.sh` — validate shell syntax and inspect all launch paths for inherited settings and retained failure assertions.
- [ ] `.github/workflows/smoke-test.yml` — use the existing manual dispatch on a ref containing the fix with `release_tag=v0.15.0` after the change is available remotely; avoid changing the release tag.

**Acceptance Criteria:**
- Given the Ubuntu smoke runner and v0.15.0 deb, when the corrected harness executes, then installation, real UI hydration, concurrent attachment, reopen, crash recovery, and uninstall all pass.
- Given an application that never emits a valid hydration acknowledgment, when the smoke executes, then it still fails even if daemon health succeeds.
- Given a normal packaged application launch outside the smoke harness, when it starts, then this change introduces no rendering override.

## Design Notes

The initial missing libmtp9/libxdo3 messages are recovered by `apt-get install -f`; the log confirms successful package configuration. The daemon responds after 25 seconds, then the UI aborts in Xlib. The accessibility-bus warning precedes this but is not established as fatal. This evidence supports investigating the graphical runtime rather than changing package dependencies again.

WebKit documents rendering failures mitigated by disabling compositing: https://bugs.webkit.org/show_bug.cgi?id=281279. That report supports a targeted experiment, not proof that this crash has the same cause. If the override fails, retain the failure and collect a native backtrace rather than stacking speculative workarounds.

The reusable workflow in the failed run came from v0.15.0. Re-running that original job would retain its original workflow revision; validation must use a ref containing the corrected script.

## Verification

**Commands:**
- `rtk proxy bash -n scripts/smoke-tests/smoke-linux.sh` — valid Bash syntax.
- `rtk git diff --check` — no whitespace errors.

**Integration:** Execute the existing release-tag smoke on Ubuntu 22.04. A macOS syntax check cannot verify Linux Xlib/WebKit behavior. Preserve logs and require all existing lifecycle evidence before reporting the issue fixed.

**Local results:** Bash syntax and `git diff --check` pass. Independent blind, edge-case, and acceptance reviews found no actionable change-caused issues. Implementation is complete; status remains `in-review` because Ubuntu integration is pending. No remote operations were performed.

## Suggested Review Order

- Smoke-scoped exports apply consistently to all four installed UI launches.
  [smoke-linux.sh:101](../../scripts/smoke-tests/smoke-linux.sh#L101)
