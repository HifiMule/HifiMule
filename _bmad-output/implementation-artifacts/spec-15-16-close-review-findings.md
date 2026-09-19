---
title: 'Close Story 15.16 review findings'
type: 'bugfix'
created: '2026-09-19'
status: 'done'
baseline_commit: 'c84ea89'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-15-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/15-16-compact-the-library-browse-mode-bar-with-icons-and-smaller-labels.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Story 15.16 leaves three actionable review defects: Grid/List focus can be lost when its retained group becomes hidden, clearing library content for an empty capability set can leave list/grid subscriptions alive, and preview-fixture setup can strand generated files after a partial failure.

**Approach:** Close all three findings with narrowly scoped lifecycle handling and focused regressions, preserving browse navigation ownership, stable controls, and the preview fixture’s production-module behavior.

## Boundaries & Constraints

**Always:** Preserve focus outside the browse bar; use the selected supported mode as the first focus fallback and the browse-bar container when no enabled mode is available. Tear down content listeners before replacing existing library DOM. Delete only files created by the current preview run and preserve the original setup error. Keep changes scoped to Story 15.16 seams and existing Node test conventions.

**Ask First:** Any change to browse-mode loading semantics, provider capability contracts, global keyboard behavior, or production build/runtime dependencies.

**Never:** Fix the two deferred pre-existing findings; recreate retained controls; add a test framework; delete a pre-existing preview file after an `EEXIST` failure; claim installed-platform or screen-reader verification.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Toggle hides for Tracks | Grid/List owns focus; selected Tracks button is enabled | Focus moves to Tracks; outside focus is untouched | Fall back to the browse-bar container if the selected control is unavailable |
| Toggle hides while loading or capabilities become empty | Grid/List owns focus; no enabled selected mode exists | Focus moves to the browse-bar container | Do not focus a disabled or detached control |
| Library reinitializes | Existing scroll and basket subscriptions are registered | Subscriptions are removed before the first content replacement | Capability-fetch errors must not retain stale subscriptions |
| Preview setup fails after one or more writes | Current run created temporary files; a later read/write fails | Current-run files are removed and setup fails nonzero | Preserve pre-existing conflicting files and surface the original error |

</frozen-after-approval>

## Code Map

- `hifimule-ui/src/library.ts` -- retained browse controls, focus reconciliation, content listener teardown, and library initialization.
- `scripts/tests/browse-mode-ui.test.mjs` -- production-module fake-DOM regression harness for reconciliation and initialization.
- `scripts/preview-browse-mode.mjs` -- disposable production-renderer fixture and generated-file lifecycle.
- `scripts/tests/preview-browse-mode.test.mjs` -- focused process-level setup-failure regressions.
- `_bmad-output/implementation-artifacts/15-16-compact-the-library-browse-mode-bar-with-icons-and-smaller-labels.md` -- authoritative story and review checklist.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-ui/src/library.ts` -- move focus out of Grid/List before its group is hidden, choosing the current enabled mode or browse-bar fallback without disturbing external focus.
- [x] `hifimule-ui/src/library.ts` -- tear down virtual-list, list-basket, and grid-basket listeners before initialization replaces library content.
- [x] `scripts/tests/browse-mode-ui.test.mjs` -- cover Tracks/loading/empty focus fallbacks, external-focus preservation, and teardown-before-replacement ordering.
- [x] `scripts/preview-browse-mode.mjs` -- centralize idempotent cleanup and wrap the complete generated-file setup sequence so partial failure cannot strand current-run files.
- [x] `scripts/tests/preview-browse-mode.test.mjs` -- inject early and later setup failures; prove created files are removed, conflicting pre-existing files survive, and the root error remains visible.

**Acceptance Criteria:**
- Given focus inside Grid/List, when that group becomes hidden, then focus lands on an enabled current-mode control or the browse-bar container and remains reachable.
- Given focus outside Grid/List, when the group becomes hidden, then focus does not move.
- Given registered list/grid listeners, when library initialization starts, then all handlers are removed before content replacement even if capability loading later fails or returns empty.
- Given preview setup fails after partial creation, when cleanup completes, then no file created by that run remains, no pre-existing file is deleted, and the initiating error is reported.
- Given the fixes, when focused Node tests and the frontend build run, then they pass without changing navigation requests, browse semantics, or dependency manifests.

## Spec Change Log

## Verification

**Commands:**
- `rtk node --test scripts/tests/browse-mode-ui.test.mjs scripts/tests/preview-browse-mode.test.mjs` -- expected: all focused regressions pass.
- `rtk node --test scripts/tests/*.test.mjs` -- expected: all frontend Node regressions pass.
- `rtk npm --prefix hifimule-ui run build` -- expected: TypeScript/Vite build succeeds with only documented existing warnings.
- `rtk git diff --check` -- expected: no whitespace errors.

**Results (2026-09-19):** 13 focused tests and 165 full Node tests passed; the production build and diff whitespace check passed. Vite emitted only the documented existing chunk/import warnings.

## Suggested Review Order

**Browse focus and lifecycle**

- Lead with the retained-control focus fallback and hidden-group transition.
  [`library.ts:702`](../../hifimule-ui/src/library.ts#L702)

- Confirm stale list and basket subscriptions are removed before content replacement.
  [`library.ts:2359`](../../hifimule-ui/src/library.ts#L2359)

**Preview fixture cleanup**

- Review exclusive creation, early path registration, and original-error preservation.
  [`preview-browse-mode.mjs:9`](../../scripts/preview-browse-mode.mjs#L9)

- Check idempotent generated-file cleanup across setup and server failures.
  [`preview-browse-mode.mjs:20`](../../scripts/preview-browse-mode.mjs#L20)

**Regression evidence**

- Verify focus fallback and teardown ordering across loading, Tracks, empty, and failure states.
  [`browse-mode-ui.test.mjs:104`](../../scripts/tests/browse-mode-ui.test.mjs#L104)

- Verify partial writes and pre-existing conflicts retain correct cleanup ownership.
  [`preview-browse-mode.test.mjs:81`](../../scripts/tests/preview-browse-mode.test.mjs#L81)

**Review records**

- Confirm all actionable Story 15.16 findings are checked closed.
  [`15-16-compact-the-library-browse-mode-bar-with-icons-and-smaller-labels.md:42`](./15-16-compact-the-library-browse-mode-bar-with-icons-and-smaller-labels.md#L42)

- Retain the two unrelated pre-existing findings for separate work.
  [`deferred-work.md:3`](./deferred-work.md#L3)
