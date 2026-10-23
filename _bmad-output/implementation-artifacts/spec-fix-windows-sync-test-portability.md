---
title: Fix Windows sync test portability
type: bugfix
created: 2026-10-23
status: done
route: one-shot
baseline_commit: e0a02af
---

# Fix Windows sync test portability

## Intent

**Problem:** Two daemon sync tests fail on Windows because they assume Unix canonicalization failure timing and native path separators matching portable manifest paths.

**Approach:** Assert preservation of the underlying filesystem error instead of a particular failure-stage message. Normalize the expected relative path to the manifest's forward slashes. Keep existing ownership, error, byte-preservation, retry, force and cleanup assertions; change no production behavior.

Verification: before the fixes, the music test group reported 33 passed and the two reported failures. After the fixes, all 35 passed. The full daemon suite passed with 1,668 tests, zero failures, 12 ignored and no exclusions; all five contract integration tests passed. Independent read-only review found no actionable issues. Logs: `target/windows-sync-tests-before.log`, `target/windows-sync-tests-after.log`, `target/windows-sync-full-suite.log`.

The local commit attempt failed because configured GPG signing could not connect to keyboxd. Signing settings were preserved; the verified changes remain staged.

## Suggested Review Order

- Preserve filesystem failure evidence across OS-specific canonicalization and parent-creation behavior.
  [sync.rs:12634](../../hifimule-daemon/src/sync.rs#L12634)

- Compare collision ownership using the manifest's portable path convention.
  [sync.rs:12846](../../hifimule-daemon/src/sync.rs#L12846)
