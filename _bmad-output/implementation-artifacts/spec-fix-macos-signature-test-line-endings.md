---
title: 'Fix macOS signature contract test line endings'
type: 'bugfix'
created: '2026-09-20'
status: 'done'
route: 'one-shot'
---

# Fix macOS signature contract test line endings

## Intent

**Problem:** The macOS signature-ordering test extracted normalized LF steps but searched for them in raw CRLF workflow text on Windows. Both offsets were -1, causing the ordering assertion to fail.

**Approach:** Normalize the workflow before extraction and offset comparison. Run the entire signature contract against LF, CRLF, and CR variants of the actual workflow. Reproduced the original CRLF failure (-1/-1); normalized positions preserve the intended ordering. Focused tests pass; independent review found no actionable defects. Packaging behavior is unchanged.

## Suggested Review Order

- Compare offsets in normalized workflow text and cover all three line endings.
  [release-runtime-contract.test.mjs:129](../../scripts/tests/release-runtime-contract.test.mjs#L129)
