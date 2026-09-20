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

## Follow-up: Reopen Hydration Timeout

The subsequent user-supplied run installed a different deb hash (`912b3d6eb100d054cbafab3ae9d7c2a71e6e84836293c22424b85ebc8a928bc2`). Initial and concurrent UI hydration passed; reopening failed to acknowledge within 30 seconds. This proves progress past the previous crash in that run, but cannot isolate the rendering change from the rebuilt artifact.

The concurrent launch and reopen timestamps are 26.699 seconds apart, including the harness's one-second close sleep. Together with the initial 25-second daemon readiness delay, this suggests a native startup delay. D-Bus timeout and WebKit teardown overlap are hypotheses, not established causes.

Confirmed harness gaps: Xvfb's DISPLAY was not propagated to D-Bus activation; no private session bus was provisioned; UI termination was not awaited; secondary UI cleanup was missing on early failure; hydration failures did not identify the launch stage or report process status.

Follow-up implementation:
- [x] `scripts/smoke-tests/smoke-linux.sh` — use one private D-Bus session for the full lifecycle, propagate display/rendering settings to activated services, await both owned UI PIDs before reopen, and report stage-specific process diagnostics without command arguments.
- [x] `.github/workflows/smoke-test.yml` — explicitly install D-Bus runtime tools.
- [x] `scripts/smoke-tests/test-linux-lifecycle.py` — exercise shutdown ordering, failure diagnostics, and session setup with controlled mocks (7 tests pass).
- [ ] Run the updated harness on Ubuntu; preserve the existing 30-second hydration requirement.

Keep the original smoke-only rendering settings and all authoritative hydration/identity gates. Leave unrelated README edits untouched. No application rendering defaults, credentials, release artifacts, or tags are changed.

Validation: 7 Linux harness tests, 3 shared UI evidence tests, and 4 macOS harness tests passed; Bash syntax and diff whitespace checks passed. Three independent reviewers found one cleanup issue, patched by sending SIGKILL only to recorded UI PIDs on the bounded close timeout while retaining failure. No other actionable findings. Actual Ubuntu reopen validation remains pending.

Follow-up review stops:
- [smoke-linux.sh:18](../../scripts/smoke-tests/smoke-linux.sh#L18) — private bus shared across every lifecycle phase.
- [smoke-linux.sh:90](../../scripts/smoke-tests/smoke-linux.sh#L90) — bounded shutdown before reopening and stage-specific diagnostics.
- [test-linux-lifecycle.py:1](../../scripts/smoke-tests/test-linux-lifecycle.py#L1) — mocked failure and ordering regressions.

## Follow-up: Initial UI Disappears

The latest supplied run activates the private session portals successfully and reports daemon health after one second. Initial UI hydration still fails; the process diagnostic prints no process row. This suggests the recorded UI exited, but its exit status was not captured. Neither the accessibility warning nor PipeWire connection warning establishes the cause. No further graphics/session override is justified by this log.

Diagnostics now retain each recorded child exit status (including successful exit), label possible signal encodings, and preserve failure. Linux passes the expected launch PID into the shared hydration poll so it fails promptly when that process disappears and cannot accept another UI's acknowledgment. macOS callers retain the existing marker-based behavior. Capture daemon identity before the first hydration gate so failure cleanup can terminate the known daemon.

Local validation: nine Linux helper tests, five shared acknowledgment tests, and four macOS harness tests pass. Bash syntax passes. These checks validate the diagnostic changes, not the Linux application startup.

Reproduction is blocked on obtaining the failing installer: the public v0.15.0 deb URL returned HTTP 404; local Docker is available. The supplied [job](https://github.com/HifiMule/HifiMule/actions/runs/35517051715/job/106097984998) confirms revision `910ee17`; the available browser is signed out and cannot expand job logs. No matching deb is present in Downloads. Three independent reviewers found no actionable diagnostic-change defects. Actual application exit cause and Ubuntu smoke success remain unverified.

## Follow-up: Exact Installer and Xlib Initialization

The user supplied the failing installer and full log. SHA256 `e27262cb7209c263e72a308bdfa90f7be4fbc9aee044f6e7297136812100f237` matches the job. The earlier artifact-access blocker is resolved.

Reproduction uses Ubuntu 22.04 amd64 with WebKitGTK 2.50.4, in a disposable Docker container on the ARM development machine through QEMU. This is useful comparative evidence, not native GitHub runner validation. The unchanged installer intermittently exits during initial, concurrent, reopened, or recovery UI hydration. A diagnostic preload captures GTK `_exit(1)` through `libX11::_XReply` and `XGetWindowProperty`, with `errno=11` but `xcb_connection_has_error=0`. The latter means a physical connection failure is not established. Adding Openbox did not help. Window-sizing calls dispatch through Tauri's main-thread message mechanism; no app sizing thread violation was found.

The original trace records no `XInitThreads` call. Tao 0.34.5 starts an X11 input worker after GTK initialization, and its explicit `XInitThreads` belongs to a separate optional API unused by Tauri. Calling `XInitThreads` before application startup passed three complete smoke runs with tracing; restoring the original startup failed again at concurrent hydration. A minimal initialization-only preload then passed five additional complete runs without diagnostic hooks: eight successful full smoke runs, 32 real UI hydration acknowledgments, with early initialization. The original-startup control failed between the two batches.

The original smoke-rendering mitigation remains smoke-scoped. The repeated user request to fix the persistent failure now requires a separate Linux application startup correction: initialize Xlib threading at the beginning of `hifimule_ui_lib::run`, before Tauri/GTK or application worker creation. This changes neither rendering defaults nor the existing release artifact/tag. The previous rendering-only hypothesis is insufficient; retain its history above rather than presenting it as the root cause.

Implementation: `hifimule-ui/src-tauri/src/lib.rs` links the X11 library already required by GTK on Linux, calls `XInitThreads`, and reports a clear fatal error if initialization fails. Other platforms compile out the call. Xlib documents that initialization must precede all other Xlib calls: https://www.x.org/releases/current/doc/libX11/libX11/libX11.html#Using_Xlib_with_Threads.

Acceptance: a newly built Linux installer must pass the existing native Ubuntu smoke, including all four real hydration gates. The unchanged installer plus diagnostic/preload experiments are evidence for this correction, not verification of the rebuilt Rust binary. Keep native integration pending until that run completes.

Validation for the startup correction:
- cargo check -p hifimule-ui --lib --locked passed on macOS.
- cargo test -p hifimule-ui --lib --locked passed all seven tests on macOS.
- Extracted Linux initialization code compiled to metadata for aarch64-unknown-linux-gnu with Rust 2024; this validates the Linux branch syntax/types, not a Linux package link/build.
- rustfmt --check and git diff --check passed.
- Independent blind, edge-case, and acceptance reviewers reported no actionable defects.
- Comparative Linux logs and the minimal C preload source are preserved at /tmp/hifimule-linux-smoke-evidence. No preload is shipped or added to the smoke harness.
- A rebuilt native Linux candidate remains required. Dispatch the existing Release workflow with candidate_ref set to the new commit SHA and candidate_version=0.15.0; its candidate smoke consumes freshly built artifacts without altering the published release tag.

## Suggested Review Order

- Initialize Xlib threading before Tauri or GTK starts.
  [lib.rs:752](../../hifimule-ui/src-tauri/src/lib.rs#L752)
- Preserve actual UI hydration as the acceptance gate.
  [smoke-linux.sh:125](../../scripts/smoke-tests/smoke-linux.sh#L125)
