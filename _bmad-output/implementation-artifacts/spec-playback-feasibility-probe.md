---
title: 'Cross-platform playback feasibility probe'
type: 'chore'
created: '2026-09-11'
status: 'done'
baseline_commit: '0285ccc'
context: []
---

<frozen-after-approval reason="user approved feasibility experiments in conversation">

## Intent

**Problem:** HifiMule has no measured proof that native Rust playback can satisfy gapless albums, lightweight background operation, and coexistence with sync across Windows, macOS, and Linux. The brainstorming established the desired experience but library documentation alone cannot validate it.

**Approach:** Build an isolated, repeatable native playback experiment and document its measured results alongside provider and metadata feasibility. Keep the main application unchanged. Use generated audio fixtures for deterministic decoder continuity checks, an optional native-output mode for device testing, and explicit platform checklists for hardware not available on this Mac.

## Boundaries & Constraints

**Always:** Target all three desktop OSes. Keep buffers bounded and audio callbacks free of blocking I/O. Report measured, simulated, and untested results separately. Use the existing auto-fill engine as the future reuse boundary. Keep the standalone experiment outside production dependencies and release packaging. Generated fixtures and build outputs are disposable and excluded from version control.

**Ask First:** Additional user input is needed only for unavailable server endpoints, hardware, or credentials when real-provider verification becomes necessary; do not read credential stores to discover them. No such input is needed to build offline fixtures and document untested cases.

**Never:** Claim gapless device output from decoder-only tests, claim Windows/Linux success from a Mac build, modify device files, initialize hardware, change system audio configuration, launch a real sync, or integrate production playback in this experiment. Do not silently substitute shared genre for strong artist links.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Decode sequence | Generated contiguous tracks | Deterministic decoded sample output and frame counts for comparison | Failed decode exits nonzero |
| Gapless metadata | Encoded versions of fixtures | Measure trimming and continuity against reference; distinguish lossy tolerances | Report unsupported formats explicitly |
| Native playback | Explicitly selected playable fixtures | Bounded decoded buffering and transport controls; output metrics | No-device/unsupported format exits clearly |
| Output loss | USB endpoint disappears | Stop or pause output without switching to another endpoint | Surface error; no automatic speaker fallback |
| Resource pressure | Background synthetic I/O load | Capture buffer underruns and wall time | Label synthetic load distinctly from real sync |
| Other OS | Windows/Linux unavailable locally | Provide repeatable build and hardware validation instructions | Mark not run |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/auto_fill/pipeline.rs` — pure selection core, byte/duration budgets, unqualified engine identities.
- `hifimule-daemon/src/main.rs` — daemon user-session event loop and separate UI launch.
- `hifimule-ui/src-tauri/src/lib.rs` — sidecar ownership and exit lifecycle.
- `hifimule-daemon/src/providers/` — streaming, annotations, playlists, and metadata adapters to assess.
- `experiments/playback-probe/` — new standalone Rust package with its own workspace boundary.

## Tasks & Acceptance

**Execution:**
- [x] `experiments/playback-probe/Cargo.toml`, `src/` — implement standalone decode/native audio probe, bounded buffering, and meaningful error-path checks.
- [x] `experiments/playback-probe/generate-fixtures.py` — create deterministic contiguous PCM fixtures and encoded variants using optional FFmpeg; no copyrighted source media.
- [x] `experiments/playback-probe/verify.py`, `stress.py` — compare decoder continuity and observe callbacks under synthetic I/O; report unsupported or failed variants explicitly.
- [x] `experiments/playback-probe/README.md`, `.gitignore` — repeatable commands, exclusions, platform requirements, and synthetic versus real workload distinction.
- [x] `_bmad-output/implementation-artifacts/playback-feasibility-results.md` — record actual measurements, source evidence, provider/metadata gaps, and unresolved hardware checks.
- [x] Run available Mac checks and adversarial review; correct actionable defects before handoff.

**Acceptance Criteria:**
- Given the isolated experiment, when building it, then production Cargo manifests and application behavior remain unchanged.
- Given contiguous generated files, when decoding them sequentially, then output can be compared to the unsplit reference with explicit codec-specific results.
- Given native playback, when data is temporarily unavailable, then the callback does not block and starvation is observable rather than hidden.
- Given no Tauri UI process, when native playback is exercised, then the probe's audio path does not depend on the WebView; OS media integration is separately marked tested or untested.
- Given this Mac-only environment, when results are written, then Windows/Linux runtime, actual USB unplug, real sync, and live-provider tests remain pending unless independently exercised.

## Spec Change Log

- Review clarification: native transport is provided by stdin pause/resume/stop; OS media controls remain explicitly pending. Standard stereo and a hard 60-second experiment deadline bound the probe. Preserve separate classification of decoder continuity, silent native callback operation, and untested physical/platform/provider behavior.

## Design Notes

This is one feasibility artifact, not delivery of the full playback feature. The probe may intentionally reject mismatched output formats to avoid mistaking an unvalidated resampler for production readiness. Keep finite source files and test parameters explicit. Media-control dependencies must remain user-session owned; investigate existing lifecycle separately from audio callback viability.

Synthetic PCM continuity is the deterministic baseline; lossy codecs require error statistics and endpoint alignment rather than byte equality. Silence during startup/end padding is not an underrun; count only unexpected starvation before decoder completion. A stalled decoder must eventually surface an error or finish, not leave a verification process hanging indefinitely.

## Verification

**Commands:**
- `rtk cargo test --manifest-path experiments/playback-probe/Cargo.toml` — unit tests pass.
- `rtk cargo build --manifest-path experiments/playback-probe/Cargo.toml` — native Mac build passes.
- `rtk proxy python3 experiments/playback-probe/generate-fixtures.py` — reproducible generated fixtures.
- `rtk proxy python3 experiments/playback-probe/verify.py` — machine-readable continuity measurements, with failures explicit.
- `rtk proxy python3 experiments/playback-probe/test_tools.py` — failure-reporting regression tests pass.

**Manual checks:** Native endpoint enumeration, optional quiet playback, keyboard controls, disconnect behavior, UI-independent lifetime, and playback alongside real sync must be individually classified by evidence.

## Suggested Review Order

- Start with measured results and limitations.
  [playback-feasibility-results.md:1](playback-feasibility-results.md#L1)
- Understand commands and platform validation boundaries.
  [README.md:1](../../experiments/playback-probe/README.md#L1)
- Inspect native decoding, buffering, and transport implementation.
  [main.rs:1](../../experiments/playback-probe/src/main.rs#L1)
- Inspect continuity assertions and explicit variant failures.
  [verify.py:1](../../experiments/playback-probe/verify.py#L1)
- Inspect fixture generation and synthetic I/O pressure.
  [generate-fixtures.py:1](../../experiments/playback-probe/generate-fixtures.py#L1)
  [stress.py:1](../../experiments/playback-probe/stress.py#L1)
- Review regression checks for failure-report integrity.
  [test_tools.py:1](../../experiments/playback-probe/test_tools.py#L1)
