---
title: 'Fix reusable smoke workflow permissions'
type: 'bugfix'
created: '2026-09-20'
status: 'done'
baseline_commit: '7ed4b4ddbfe2db04fb84f164283cefb55bc40ba4'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The release workflow is rejected before execution because the reusable `smoke-test.yml` requests `contents: write`, while the `smoke-candidate` caller grants only `contents: read`. GitHub permits a called workflow to maintain or reduce caller token permissions, but never elevate them.

**Approach:** Make the reusable smoke workflow explicitly read-only and align both calling smoke jobs with the same least-privilege `contents: read` and `actions: read` contract. Add a regression assertion so future workflow edits cannot reintroduce write access or mismatched caller permissions.

## Boundaries & Constraints

**Always:** Preserve the release build job's `contents: write` access because the tag path creates a draft GitHub release. Preserve artifact download/upload behavior for candidate and release smoke tests. Keep permissions explicit at the reusable workflow and caller-job boundaries.

**Ask First:** Any change that removes release publication, changes workflow triggers, replaces artifact transport, or broadens token permissions beyond the identified read scopes.

**Never:** Grant `contents: write` to the candidate smoke caller merely to satisfy validation; remove candidate smoke testing; modify signing, build, packaging, or release-draft behavior; perform git commit, tag, or push operations.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Candidate smoke | Manual candidate build calls the reusable workflow with `contents: read` and `actions: read` | Workflow validates and can download candidate artifacts | Permission regression test fails if the callee requests write access |
| Tagged release smoke | Tag build calls the reusable workflow after draft-release creation | Smoke jobs can check out code, download release assets, and upload logs with read-only repository access | Release publication retains write access only in the separate build job |
| Future permission drift | A caller or reusable workflow is changed to request `contents: write` | Targeted contract test rejects the change | Test output identifies the unexpected permission contract |

</frozen-after-approval>

## Code Map

- `.github/workflows/release.yml` -- Defines the publishing matrix and the two jobs that invoke the reusable smoke workflow.
- `.github/workflows/smoke-test.yml` -- Reusable smoke workflow whose top-level token request currently conflicts with the candidate caller.
- `scripts/tests/release-contract.test.mjs` -- Existing release-workflow contract coverage and the appropriate location for a permission regression.

## Tasks & Acceptance

**Execution:**
- [x] `.github/workflows/smoke-test.yml` -- replace the unnecessary repository write permission with explicit read-only repository and Actions access.
- [x] `.github/workflows/release.yml` -- align both reusable-workflow caller jobs to the same read-only permission set while leaving the publishing job unchanged.
- [x] `scripts/tests/release-contract.test.mjs` -- assert that the reusable smoke workflow and both callers use the least-privilege contract and that only the release build job keeps `contents: write`.

**Acceptance Criteria:**
- Given a manual candidate dispatch, when GitHub validates the `smoke-candidate` reusable-workflow call, then the called workflow requests no permission stronger than the caller grants.
- Given a tag-triggered release, when the build publishes the draft release and invokes smoke testing, then publication retains `contents: write` while smoke testing uses read-only permissions.
- Given the targeted release contract test, when workflow permissions drift back to write access or caller/callee scopes diverge, then the test fails.

## Spec Change Log

## Verification

**Commands:**
- `node --test scripts/tests/release-contract.test.mjs` -- expected: all release contract tests pass, including the new permission regression.
- `npx prettier --check .github/workflows/release.yml .github/workflows/smoke-test.yml scripts/tests/release-contract.test.mjs` -- expected: all changed files conform to repository formatting.
- `git diff --check` -- expected: no whitespace errors.

## Suggested Review Order

**Permission boundary**

- Make the reusable smoke workflow request only the two required read scopes.
  [`smoke-test.yml:25`](../../.github/workflows/smoke-test.yml#L25)

- Align both callers while isolating release publication's required write access.
  [`release.yml:24`](../../.github/workflows/release.yml#L24)
  [`release.yml:354`](../../.github/workflows/release.yml#L354)

**Regression guard**

- Parse complete permission mappings so any added scope or write grant fails.
  [`release-contract.test.mjs:9`](../../scripts/tests/release-contract.test.mjs#L9)
  [`release-contract.test.mjs:77`](../../scripts/tests/release-contract.test.mjs#L77)

**Follow-up**

- Record inherited signing-secret exposure separately without expanding this fix.
  [`deferred-work.md:3`](deferred-work.md#L3)
