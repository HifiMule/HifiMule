---
title: 'Make release workflow contract tests newline-independent on Windows'
type: 'bugfix'
created: '2026-09-20'
status: 'done'
baseline_commit: 'a1ef88130a5dcf79bac1fa02ee7accedcf6c495f'
context:
  - '{project-root}/_bmad-output/planning-artifacts/project-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The `Playback evidence (windows-x64)` CI job fails in `scripts/tests/release-contract.test.mjs` even though the workflow permission blocks exist and are correct. The test helpers search only for LF line endings, while the Windows checkout presents the workflow text with CRLF endings, causing `permissionMap` to report that the top-level `permissions` block is missing.

**Approach:** Make the workflow-contract parsing helpers normalize platform line endings before matching structural YAML text, and add a regression test that exercises the same permission assertions with synthetic CRLF workflow content.

## Boundaries & Constraints

**Always:** Preserve the existing caller/callee least-privilege assertions; accept LF, CRLF, and legacy CR line endings consistently; keep the fix within the release-contract test code; verify the full JavaScript and Python evidence suites used by the CI step.

**Ask First:** Any change to workflow permissions, release behavior, signing behavior, or production/runtime code.

**Never:** Weaken or remove permission assertions; special-case Windows paths or runner names; alter Git configuration or repository-wide line-ending policy; mask genuine malformed or missing permission blocks.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|----------------------------|----------------|
| Linux/macOS checkout | Workflow text uses LF | Existing permission maps are parsed exactly | Missing/malformed blocks still fail assertions |
| Windows checkout | Workflow text uses CRLF | Caller and callee permission maps match the LF result | Missing/malformed blocks still fail assertions |
| Legacy newline input | Workflow text uses CR | Parser behavior remains equivalent after normalization | Missing/malformed blocks still fail assertions |

</frozen-after-approval>

## Code Map

- `scripts/tests/release-contract.test.mjs` -- Reads workflow YAML as text and contains the newline-sensitive `jobBlock` and `permissionMap` helpers plus the least-privilege contract test.
- `.github/workflows/release.yml` -- Defines release, `smoke-release`, and `smoke-candidate` job-level permissions consumed by the test; no behavior change is intended.
- `.github/workflows/smoke-test.yml` -- Defines reusable smoke-workflow top-level permissions consumed by the test; no behavior change is intended.

## Tasks & Acceptance

**Execution:**
- [x] `scripts/tests/release-contract.test.mjs` -- normalize workflow text line endings at the parser boundary and run both job-block and permission parsing against normalized text, so assertions are independent of checkout newline style.
- [x] `scripts/tests/release-contract.test.mjs` -- add explicit LF/CRLF/CR regression coverage for the smoke workflow and release job permission maps, including valid-first-then-malformed permission entries; distinguish legitimate block termination from invalid same-indentation permission entries so partial corruption cannot pass.

**Acceptance Criteria:**
- Given a Windows checkout with CRLF workflow files, when `node --test scripts/tests/*.test.mjs` runs, then the release workflow permission contract test passes without weakening its expected permission maps.
- Given workflow text with LF, CRLF, or CR separators, when the contract helpers inspect it, then they produce identical job blocks and permission maps.
- Given a permission block is genuinely absent or malformed, when the contract helper inspects it, then the test still fails with an actionable assertion.
- Given the repository's evidence test command runs, when all Node and Python suites complete, then both exit successfully.

## Spec Change Log

- 2026-09-20, review loop 1: All three reviewers found that the first implementation rejected a malformed first permission entry but silently accepted a malformed entry after a valid one. The regression task now requires valid-first-then-malformed coverage and explicit distinction between block termination and invalid same-indentation entries, avoiding partial permission maps that mask corruption. KEEP: parser-boundary normalization for LF/CRLF/CR, exact caller/callee least-privilege maps, unchanged workflow and production behavior, and actionable missing-block failures.

## Design Notes

Normalize only the workflow text passed to the structural helpers. This keeps YAML contract assertions readable and deterministic without imposing a repository-wide checkout policy or changing production workflows. The normalizer should canonicalize `\r\n` and lone `\r` to `\n` before any exact line matching.

## Verification

**Commands:**
- `rtk node --test scripts/tests/*.test.mjs` -- expected: all JavaScript contract tests pass.
- `rtk python -m unittest discover -s scripts/tests -p 'test_playback_session_evidence.py'` -- expected: playback session evidence tests pass.
- `rtk python -m unittest discover -s scripts/tests -p 'test_playback_installed_evidence.py'` -- expected: installed evidence tests pass after the adjacent signing change.

## Suggested Review Order

**Parser behavior**

- Canonicalize platform newlines before locating jobs or permission blocks.
  [`release-contract.test.mjs:9`](../../scripts/tests/release-contract.test.mjs#L9)

- Parse exact permissions while rejecting partial, duplicate, and tab-indented corruption.
  [`release-contract.test.mjs:16`](../../scripts/tests/release-contract.test.mjs#L16)

**Regression coverage**

- Exercise identical permission contracts under LF, CRLF, and legacy CR.
  [`release-contract.test.mjs:100`](../../scripts/tests/release-contract.test.mjs#L100)

- Preserve actionable failures for missing and malformed permission blocks.
  [`release-contract.test.mjs:121`](../../scripts/tests/release-contract.test.mjs#L121)
