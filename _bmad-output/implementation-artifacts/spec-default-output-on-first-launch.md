---
title: 'Select the concrete system default output on first launch'
type: 'bugfix'
created: '2026-09-16'
status: 'done'
baseline_commit: 'NO_VCS'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/15-5-choose-an-audio-output-and-recover-safely-from-disconnection.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** On a fresh installation, HifiMule presents no selected playback output even when the operating system has a concrete default output. Once a user chooses an output, its durable preference correctly restores on later launches. This makes initial playback need an unnecessary selection and is observed on Windows.

**Approach:** During first-run initialization only, resolve the currently available OS-default endpoint to its concrete, stable preference and durably select it. Continue treating every later startup as restoration of that exact saved endpoint: if it is absent, unavailable, or unidentifiable, remain unavailable and never substitute the current default.

## Boundaries & Constraints

**Always:** Persist a concrete endpoint identity, not a floating “system default” route; wait for discovery before making the first-run decision; use existing serialized selection/save machinery; preserve the selected/pending/active distinction; stay paused and do not open audio or auto-resume at startup; retain an unavailable saved identity without name/default fallback; cover Windows/macOS CPAL discovery behavior deterministically.

**Ask First:** Any change to output-config schema, a migration that overwrites an existing configuration, a UI copy/interaction redesign, or adding a user opt-out/reset preference.

**Never:** Automatically reroute after an explicit choice, output loss, a later default-device change, corrupt configuration, failed save, uncertain identity, or absent default; select a device by friendly name or enumeration order; start playback merely because initialization selected an endpoint.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Fresh launch | Missing config; discovery resolves one available `is_default` endpoint | Persist and publish that concrete endpoint as selected/available; no audio effect | Remain paused/idle |
| Later launch | Saved endpoint resolves | Restore exactly that endpoint | No default-follow behavior |
| Saved endpoint absent | Saved identity does not resolve; another endpoint is default | Publish saved identity unavailable | Require explicit user choice |
| Fresh discovery unavailable | Missing config; inventory cannot reliably resolve default | Remain unselected | Expose existing discovery status; retry through reconciliation |
| No default/ambiguous default | Missing config; zero or non-unique eligible default endpoints | Remain unselected | Require explicit user choice |
| Invalid config | Corrupt/future config | Preserve existing invalid-config flow | Never overwrite it with default |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/playback/session/output_selection.rs` -- loads durable output state, owns discovery reconciliation, and commits endpoint selections.
- `hifimule-daemon/src/playback/devices/mod.rs` -- CPAL discovery marks the concrete OS-default endpoint with `is_default` on Windows and macOS.
- `hifimule-daemon/src/playback/devices/worker.rs` -- asynchronous discovery inventory and sequence propagation.
- `hifimule-daemon/src/playback/config.rs` -- missing configuration is currently represented as `PlaybackConfig::default()` with no output.
- `_bmad-output/implementation-artifacts/15-5-choose-an-audio-output-and-recover-safely-from-disconnection.md` -- existing no-substitution identity and startup-safety contract that this change deliberately narrows only for a missing config.

## Tasks & Acceptance

**Execution:**

- [x] `hifimule-daemon/src/playback/session/output_selection.rs` -- distinguish a genuinely missing output configuration from an existing config whose output is null; on the first trustworthy discovery containing exactly one available concrete default, enqueue the same durable selection path used by an explicit selection, without an audio-open effect.
- [x] `hifimule-daemon/src/playback/session/output_selection.rs` -- add deterministic regression tests for fresh default initialization and no auto-play/effect; existing selection/restart/loss tests retain the missing/ambiguous/unavailable and saved-output invariants.
- [x] `hifimule-daemon/src/playback/config.rs` -- no config code change is required: `load` retains its missing-versus-explicit-null result while startup uses file presence solely to decide first-run initialization.
- [x] `docs/api-contracts-hifimule-daemon.md` -- document first-launch concrete-default initialization and the ongoing no-default-substitution guarantee.

**Acceptance Criteria:**

- Given no playback configuration and exactly one available concrete default endpoint, when startup discovery completes, then HifiMule selects and saves that endpoint without starting playback.
- Given that initialized endpoint was saved, when the application later starts, then it restores that identity rather than following a changed system default.
- Given a saved endpoint is absent, when another endpoint is currently default, then HifiMule shows the saved selection as unavailable and requires an explicit replacement choice.
- Given discovery cannot confidently provide one available default or configuration is invalid, when startup reconciles outputs, then HifiMule remains safely unselected or follows invalid-config recovery without writing a guessed preference.

## Spec Change Log

## Design Notes

The current `enable_outputs_with` branch sets `unselected` whenever `config::load` succeeds with `output: None`; reconciler logic only resolves a pre-existing selection. CPAL discovery already carries a stable preference and `is_default` marker for each endpoint. The implementation should add a one-shot first-run state marker, choose only the sole available default after discovery is resolvable, and use the existing preference worker so publish-after-commit, revision fencing, and save-failure handling remain authoritative. Do not treat an explicitly stored `{ output: null }` as consent to pick a default; it must remain unselected.

## Verification

**Commands:**

- `rtk cargo test -p hifimule-daemon playback::session::output_selection -- --test-threads=1` -- expected: targeted first-run and existing output-selection regressions pass.
- `rtk cargo test -p hifimule-daemon playback::config -- --test-threads=1` -- expected: missing, explicit-null, and invalid-config behavior pass.
- `rtk cargo fmt --all -- --check` -- expected: formatting passes.
- `rtk git diff --check` -- expected: no whitespace errors.

## Suggested Review Order

**Startup selection boundary**

- Preserve missing-versus-explicit-null intent from one authoritative read.
  [`output_selection.rs:54`](../../hifimule-daemon/src/playback/session/output_selection.rs#L54)

- Bootstrap only from a complete, unique, available concrete default endpoint.
  [`output_selection.rs:385`](../../hifimule-daemon/src/playback/session/output_selection.rs#L385)

**Persistence contract**

- Expose missing-file presence without changing existing configuration consumers.
  [`config.rs:53`](../../hifimule-daemon/src/playback/config.rs#L53)

- State the one-time default selection and permanent no-substitution policy.
  [`api-contracts-hifimule-daemon.md:838`](../../docs/api-contracts-hifimule-daemon.md#L838)

**Regression coverage**

- Verify a fresh default becomes durable but never opens audio.
  [`output_selection.rs:569`](../../hifimule-daemon/src/playback/session/output_selection.rs#L569)

- Verify missing and explicit-null configuration remain distinguishable.
  [`config.rs:235`](../../hifimule-daemon/src/playback/config.rs#L235)
