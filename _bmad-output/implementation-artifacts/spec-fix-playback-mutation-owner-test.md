---
title: 'Fix playback mutation owner test synchronization'
type: 'bugfix'
created: '2026-09-17'
status: 'done'
route: 'oneshot'
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The playback session test that verifies a dropped caller retains its admitted mutation guard can time out before the owner begins the command.

**Approach:** Block the command's database write after the owner has dequeued it, rather than locking the session state needed by the owner before dequeueing.

</frozen-after-approval>

## Implementation Notes

- Replaced the test's `inner` mutex guard with the database connection guard. The owner can now dequeue and mark the command as executing before its `Clear` write blocks; the admitted `MutationGuard` remains owned by the command until the write is released.

## Review Triage Log

- defer — The pre-existing test does not explicitly stop its playback owner thread. This small synchronization correction did not cause the missing teardown; addressing it consistently requires auditing the session test lifecycle.
- defer — The test intentionally proves `MutationGuard` ownership survives a dropped reply and releases after the owner completes. Adding a separate assertion that `Clear` committed would broaden the test from guard lifetime to command-result validation.
