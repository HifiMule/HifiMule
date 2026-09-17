---
title: 'Fix output RPC replay test'
type: 'bugfix'
created: '2026-09-17'
status: 'done'
route: 'one-shot'
---

# Fix output RPC replay test

## Intent

**Problem:** The output selection RPC test assumed revision 1 while its default-device fixture also triggered automatic initialization, allowing revision 2.

**Approach:** Use a non-default endpoint to isolate explicit selection. Assert selection advances the prior revision exactly once and replay preserves the accepted revision. Independent review found no actionable issues. Verified with `npm run build:daemon -- test -p hifimule-daemon`: 833 passed, 6 ignored. Execution required access to macOS SystemConfiguration outside the sandbox.

## Suggested Review Order

1. Isolate explicit selection from automatic default initialization.
   [rpc.rs:7664](../../hifimule-daemon/src/rpc.rs#L7664)
2. Check selection increments once and command replay preserves the revision.
   [rpc.rs:7710](../../hifimule-daemon/src/rpc.rs#L7710)
