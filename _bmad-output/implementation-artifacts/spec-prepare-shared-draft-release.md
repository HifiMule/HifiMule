---
title: 'Prepare one shared draft release before platform builds'
type: 'bugfix'
created: '2026-09-20'
status: 'done'
baseline_commit: 'bc10cb3'
context: []
---

<frozen-after-approval reason="human-owned intent — approved in conversation">

## Intent

**Problem:** Linux and ARM macOS failed creating a draft release, while Windows created it later and subsequent uploads succeeded. Every matrix job independently searches for or creates the same draft, delaying discovery of creation failures until packaging has completed. Common token permissions provide no evidence of a Windows-specific capability; the precise cause of the observed GitHub 403 remains unconfirmed.

**Approach:** Prepare one draft before platform builds, validate the existing tag, and pass the resulting release ID to every Tauri action branch. Serialize release runs for the same tag and report actionable creation errors early. Preserve manual candidate builds and the existing draft-only publishing boundary.

## Boundaries & Constraints

**Always:** Use the existing GITHUB_TOKEN with contents: write. Validate that the remote tag resolves to the triggering commit, including annotated tags. Keep platform builds parallel after initialization. Reuse an existing matching draft without replacing its notes. Preserve all signing, bundle verification, artifact upload and smoke paths. Surface permission failures without claiming that serialization fixes GitHub authorization. Use environment or structured parameters for event values, never executable interpolation.

**Ask First:** Introducing a new credential or publishing an existing draft.

**Never:** Publish, delete releases, move/create tags, add a PAT, or change application behavior. Do not retry a persistent authorization denial as though it were a build failure. Do not let a candidate dispatch create or modify a release.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Fresh release | Tag exists at triggering commit; draft absent | Create one draft and return numeric ID | Fail before builds if API denies creation |
| Existing draft / rerun | Matching draft exists | Reuse its ID and preserve notes | Reject published release |
| Annotated tag | One or more tag objects point to commit | Dereference and compare commit | Bound traversal and reject noncommit targets |
| Missing or moved tag | Ref missing or commit differs | No create request and no matrix build | Explain expected and observed identity |
| Creation ambiguity | Another actor creates matching draft before create returns error | Re-list once and reuse matching draft if present | Otherwise retain original status and request ID |
| Candidate dispatch | Explicit candidate ref/version | Run normal candidate matrix without release API mutation | Initialization must not skip dependent matrix |
| Multiple same-tag runs | Overlapping release workflow runs | Serialize by tag without canceling active release | Candidate concurrency remains independent |

</frozen-after-approval>

## Code Map

- `.github/workflows/release.yml` — four matrix rows, three push-only Tauri action branches and manual candidate paths.
- `scripts/prepare-draft-release.cjs` — new GitHub-script-compatible module receiving injected github/context/core objects for testable initialization.
- `scripts/tests/prepare-draft-release.test.mjs` — mocked API behavioral tests without network writes.
- `scripts/tests/release-contract.test.mjs` — workflow permissions, candidate and release wiring checks.
- `scripts/tests/release-runtime-contract.test.mjs` — existing distribution signing and runtime regression checks.
- `docs/release-guide.md` — release operator instructions and diagnostics.

## Tasks & Acceptance

**Execution:**
- [x] `.github/workflows/release.yml` and `scripts/prepare-draft-release.cjs` — initialize draft once, validate tag identity, output shared ID and pin push checkout to event commit; preserve candidate branch and add concurrency.
- [x] `scripts/tests/prepare-draft-release.test.mjs` — exercise the I/O matrix using injected API doubles, including creation failures and preserved notes.
- [x] `scripts/tests/release-contract.test.mjs` — enforce shared ID use, preparation gating and unchanged candidate behavior, updating exact permission count.
- [x] `docs/release-guide.md` — explain preparation, reruns, serialization, and limits of the 403 diagnosis.

**Acceptance Criteria:**
- Given a tag push, when preparation succeeds, then every platform action receives the same nonempty release ID and builds the event commit.
- Given failed preparation, when the workflow schedules dependents, then packaging and smoke jobs do not run.
- Given a candidate dispatch, when release preparation is bypassed at step level, then all four candidate builds and candidate smoke remain reachable without release creation.
- Given local validation, when focused tests and workflow syntax checks run, then they pass without GitHub mutations or native rebuilds.

## Spec Change Log

- Review identified that rerunning only failed jobs retains the original preparation output. Added read-only validation of the saved release ID and current tag immediately before each platform packaging action, with regression tests for published, retagged, deleted and invalid releases. This preserves one-time creation and closes the retry bypass.

## Design Notes

The user approved the concrete four-part proposal before this specification was recorded; no second approval is needed. A preparation job can run successfully as a no-op for candidate events, avoiding GitHub's skipped-needs behavior. Tag push builds use the immutable event SHA rather than the mutable ref name. The helper uses the existing Octokit client from actions/github-script so it needs no new package installation. Error output includes only operation, HTTP status, request ID, tag and revision context, not tokens or request headers. A single re-list after creation errors recovers an externally created draft without blindly retrying authorization failures. Concurrency prevents overlapping active runs for the same tag but does not promise an unlimited queue of pending runs.

## Verification

**Commands:**
- `rtk proxy node --test scripts/tests/prepare-draft-release.test.mjs scripts/tests/release-contract.test.mjs scripts/tests/release-runtime-contract.test.mjs` — all behavioral and workflow contract checks pass.
- `rtk git diff --check` — no whitespace errors.
- Validate YAML and embedded workflow expressions with available actionlint tooling; record any unavailable checks explicitly.

Live cross-platform packaging remains a CI verification step after the change is pushed; local tests cannot reproduce GitHub token authorization.

## Results

- 41 focused tests passed (23 helper behaviors, 9 release contracts, 9 runtime/signing contracts).
- Release YAML parsed; dependency graph and both embedded GitHub scripts validated.
- `git diff --check` passed. `actionlint` is not installed, so its expression/schema checks were not run.
- Blind, edge-case and acceptance reviews completed; rerun validation finding fixed and re-reviewed.
- No remote mutation or full native build performed. GitHub authorization and packaging remain live CI checks.

## Suggested Review Order

- Inspect preparation, dependency gating and concurrency.
  [release.yml:22](../../.github/workflows/release.yml#L22)
- Follow tag validation, draft reuse and read-only retry checks.
  [prepare-draft-release.cjs:1](../../scripts/prepare-draft-release.cjs#L1)
- Inspect regression coverage for API failures and candidate isolation.
  [prepare-draft-release.test.mjs:1](../../scripts/tests/prepare-draft-release.test.mjs#L1)
- Review operator guidance and live-verification limits.
  [release-guide.md:72](../../docs/release-guide.md#L72)
