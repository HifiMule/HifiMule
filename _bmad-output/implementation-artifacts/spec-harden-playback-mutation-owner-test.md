---
title: 'Harden playback mutation owner test'
type: 'bugfix'
created: '2026-09-17'
status: 'done'
route: 'oneshot'
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The dropped-caller playback mutation test does not prove its queued `Clear` completed and leaves its owner thread running after the test.

**Approach:** Add test-scoped cleanup that stops the owner even during unwinding, then assert the session state produced by the queued `Clear` after the mutation guard is released.

</frozen-after-approval>

## Implementation Notes

- Added a test-only RAII owner cleanup guard. It is declared before the database lock, ensuring lock guards release before `stop_and_join` during either normal completion or unwinding.
- The dropped-caller test now verifies the command's durable session-visible result: `Clear` advances the queue revision and leaves an idle, empty session after the mutation guard drains.

## Review Triage Log

- patch — The state assertions were vacuous on an already-empty queue. The test now seeds one occurrence and confirms `Clear` advances the revision and removes that state.
- patch — The neighboring mailbox test also left its playback owner running. It now uses the same test-scoped cleanup guard.
- defer — Both tests still poll `executing` for up to one second. Replacing that with a deterministic start signal requires a broader test-only owner-loop synchronization seam and is recorded in deferred work.
