---
title: 'Fix Preview audio remaining in Loading'
type: 'bugfix'
created: '2026-09-18'
status: 'done'
baseline_commit: 'b7cb2c7e4b266b0dc24df7d731ecd36f7f3862d9'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-15-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/15-11-preview-a-full-track-without-losing-the-main-listening-session.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Clicking Preview successfully admits an audition and shows Loading, but the selected track never produces audio or reaches Active. Return to session still works, proving the preserved-session transition is committed while preview output authorization is lost during device opening.

**Approach:** Preserve the shared audio gate from the authoritative active transport rather than the suspended main session when an output opens. Add regression coverage for previews both with and without a main session, while retaining all existing output-loss, generation-fencing and return behavior.

## Boundaries & Constraints

**Always:** Keep the daemon owner authoritative; compute output authorization from the current generation's active transport; preserve output selection safety, exact generation matching, and the existing callback-safe atomic gate; cover main-present and no-main auditions; keep Return and ordinary main playback unchanged.

**Ask First:** Any change that alters the public RPC/snapshot contract, Preview persistence schema, output-selection policy, or audio backend lifecycle beyond correcting active-transport gate projection.

**Never:** Start audio in the UI, bypass the selected-output checks, force the gate open for paused/error auditions, auto-resume after output loss, add a second audio owner, or discard the existing uncommitted Story 15.11 work.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|----------------------------|----------------|
| Preview over main | Main is paused for preservation; active preview is Buffering; selected output opens for preview generation | Output gate remains open, decoded preview can advance, and owner can publish Active | Stale generation or unavailable output remains rejected |
| Standalone preview | Canonical main is Idle; active preview is Buffering | Output gate remains open despite idle main and preview begins | Missing/failed output produces existing recoverable preview error |
| Paused preview | Active preview is Paused when output opening completes | Output gate stays closed and no samples advance | Explicit Resume remains required |
| Main playback | No preview; main is Buffering or Playing | Existing output-open authorization is unchanged | Existing output-loss behavior remains unchanged |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/playback/session/output_selection.rs` -- `output_opened` currently derives the shared gate only from canonical main state, ignoring the preview overlay.
- `hifimule-daemon/src/playback/session.rs` -- owns `ActivePreview`, active-state projection, Preview admission, generation/epoch fences and output gate.
- `hifimule-daemon/src/playback/audio.rs` -- opens the selected output and consumes the shared gate in the audio callback; must remain callback-safe.
- `hifimule-daemon/src/playback/commands.rs` -- dispatches the admitted Preview snapshot through provider resolution and the common audio-start path.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/playback/session/output_selection.rs` -- add a failing regression for output opening during main-present and standalone Preview, plus paused-preview protection.
- [x] `hifimule-daemon/src/playback/session/output_selection.rs` -- authorize the output gate from the active transport state for the current generation instead of canonical main state alone.
- [x] `hifimule-daemon/src/playback/session.rs` -- expose or reuse the narrow active-state helper needed by the sibling output-selection module without widening public API.
- [x] `_bmad-output/implementation-artifacts/15-11-preview-a-full-track-without-losing-the-main-listening-session.md` -- record the discovered runtime defect, fix and verification in the active story handoff.

**Acceptance Criteria:**
- Given an admitted Preview in Buffering state, when its selected output reports opened for the current generation, then the shared output gate remains true even though canonical main state is Paused or Idle.
- Given a paused or failed Preview, when output-open state is reconciled, then the output gate remains false until an explicit valid resume.
- Given no active Preview, when output opening completes, then main-session authorization behaves exactly as before.
- Given an obsolete generation or an output mismatch/loss, when late work reports opened, then it cannot authorize audio or mutate current Preview state.
- Given the fix, when focused playback tests and the full daemon suite run through the controlled runtime wrapper, then no regression fails.

## Spec Change Log

## Design Notes

`output_opened` is the last authorization boundary before the backend can deliver samples. The correct source of truth is the already-fenced active transport projection: Preview state when an overlay exists, otherwise canonical main state. This keeps one atomic gate and avoids special-casing the callback or dispatch layer.

## Verification

**Commands:**
- `rtk npm run build:daemon -- test -p hifimule-daemon playback::session::output_selection::tests` -- expected: output-selection and Preview gate regressions pass.
- `rtk npm run build:daemon -- test -p hifimule-daemon playback::` -- expected: all controlled playback regressions pass, aside from documented diagnostic ignores.
- `rtk npm run build:daemon -- test -p hifimule-daemon` -- expected: full daemon suite passes, aside from documented diagnostic ignores.
- `rtk cargo fmt --all -- --check` -- expected: no formatting diff.
- `rtk git diff --check` -- expected: no whitespace errors.

## Suggested Review Order

**Output authorization boundary**

- Project gate state from the active transport after the selected output opens.
  [`output_selection.rs:136`](../../hifimule-daemon/src/playback/session/output_selection.rs#L136)

**Regression coverage**

- Reproduce Preview-over-main and prove output opening preserves its Buffering authorization.
  [`output_selection.rs:737`](../../hifimule-daemon/src/playback/session/output_selection.rs#L737)

- Cover standalone Preview and the paused-Preview closed-gate invariant.
  [`output_selection.rs:772`](../../hifimule-daemon/src/playback/session/output_selection.rs#L772)

**Handoff and follow-up**

- Record the runtime defect, corrected boundary, and final controlled-suite evidence.
  [`15-11-preview-a-full-track-without-losing-the-main-listening-session.md:232`](15-11-preview-a-full-track-without-losing-the-main-listening-session.md#L232)

- Preserve broader Story 15.11 findings for separate, explicitly scoped follow-up.
  [`deferred-work.md:3`](deferred-work.md#L3)
