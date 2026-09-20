---
title: 'Make playback shutdown saturation synchronization deterministic'
type: 'bugfix'
created: '2026-09-20'
status: 'done'
route: 'one-shot'
---

# Make playback shutdown saturation synchronization deterministic

## Intent

**Problem:** Linux playback evidence fails in `saturated_mailbox_cannot_block_shutdown_or_commit_queued_work_after_snapshot` at the assertion `owner did not begin the command`. The test holds the inner-state mutex while waiting for executing=true. The owner may need that mutex for maintenance before receiving a command, so the test can deadlock until its deadline. The supplied CI log records 88 passing session tests and this one failure.

**Approach:** Introduce a per-session, one-shot handshake under cfg(test), acknowledging Apply dequeue and pausing before the session-state lock and shutdown fence check. The test waits for acknowledgement, admits all 64 queued commands, explicitly verifies overflow is rejected, starts shutdown, and releases the owner after the ingress fence is visible. Every admitted reply must still be DAEMON_STOPPED and queue revision must remain zero. The resume sender disconnects during unwinding before OwnerThreadCleanup joins the owner.

All instrumentation is excluded from production builds. This is a correction to test synchronization, not a change to playback shutdown behavior or evidence pass/fail handling.

Review rejected an earlier events-mutex approach because owner_loop also locks events after an idle receive timeout. The final handshake cannot pause before successful dequeue. Independent review found no remaining actionable defects.

## Suggested Review Order

- Define the test-only handshake and its per-session ownership.
  [session.rs:42](../../hifimule-daemon/src/playback/session.rs#L42)
- Acknowledge dequeue before locking state and checking the shutdown fence.
  [session.rs:1356](../../hifimule-daemon/src/playback/session.rs#L1356)
- Preserve saturation, overflow, shutdown rejection, and unchanged durable revision assertions.
  [session.rs:5354](../../hifimule-daemon/src/playback/session.rs#L5354)


## Verification

- All 89 playback session tests pass on macOS ARM64.
- The final handshake test passes 200 exact-test runs across eight concurrent processes.
- All nine commands in `scripts/playback-session-evidence.py` pass (every command exit code is zero). The first sandboxed run could not bind localhost RPC sockets; rerunning with local socket permission passed. Evidence is recorded at `/private/tmp/hifimule-playback-fixed-evidence.json` with the source diff hash.
- `node scripts/build-daemon.mjs check -p hifimule-daemon` passes with existing warnings, confirming the non-test build excludes the handshake.
- `git diff --check` passes. Native Linux CI remains the cross-platform confirmation; no workflow was rerun remotely.
