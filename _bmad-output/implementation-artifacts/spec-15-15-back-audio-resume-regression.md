---
title: 'Restore audible playback after Back'
type: 'bugfix'
created: '2026-09-19'
status: 'complete'
baseline_commit: '3efa4529c56c94a009d5ed02a64562a97b957c9a'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/15-15-restart-the-current-track-or-return-to-the-previous-track.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** On macOS, an accepted Back command updates the authoritative position or current occurrence but produces silence even though playback intent remains active. Pipeline retirement closes the session-wide output gate after Back has authorized it, and the replacement pipeline inherits that closed gate.

**Approach:** Make successful Back preparation explicitly resume the newly installed pipeline when the accepted command carried audible intent. Keep paused and output-inhibited Back silent, and fence the resume by the accepted generation and control epoch.

## Boundaries & Constraints

**Always:** Preserve the daemon-owned 3,000 ms navigation rule, persistence-first Back transition, generation/epoch fencing, selected-output policy, Preview isolation, paused intent, and output-loss inhibition. Only publish Back committed after preparation and any required pipeline resume succeed for the current generation.

**Ask First:** Any change to the public RPC/snapshot schema, queue or attempt persistence, native capability semantics, or platform adapter code.

**Never:** Re-enable audio from UI state, bypass output selection, auto-resume a paused or inhibited session, target a stale pipeline, add a macOS-only workaround, or weaken command deduplication.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|----------------------------|----------------|
| Audible restart | Playing after 3,000 ms | Current occurrence restarts at zero and replacement pipeline becomes audible | Stale generation/epoch performs no resume |
| Audible previous | Playing at or before 3,000 ms with predecessor | Previous occurrence becomes current at zero and plays | Preparation failure remains recoverable and silent |
| Paused Back | Paused current or preview | Navigation/restart prepares at zero while gate remains closed | Explicit Resume remains required |
| Output inhibited | Output loss/unavailable during Back | Accepted cursor remains recoverable without sound | Existing output error is preserved |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/playback/commands.rs` -- dispatches Back preparation and completion.
- `hifimule-daemon/src/playback/audio.rs` -- owns installed pipeline reuse/resume and the shared output gate.
- `hifimule-daemon/src/playback/commands_tests.rs` -- production command-service regression coverage.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/playback/audio.rs` -- make generation-fenced `resume_existing` reopen the reusable pipeline gate as part of resumption.
- [x] `hifimule-daemon/src/playback/commands.rs` -- consume `back_audible` after successful preparation and resume only the matching replacement pipeline before Back completion.
- [x] `hifimule-daemon/src/playback/audio.rs` and `hifimule-daemon/src/playback/commands_tests.rs` -- add regression coverage for audible and paused/inhibited Back behavior.

**Acceptance Criteria:**
- Given playing main playback, when Back restarts or selects the predecessor and preparation succeeds, then audible playback begins at zero on the selected occurrence.
- Given paused or output-inhibited playback, when Back succeeds, then the selected occurrence remains silent until an authorized Resume.
- Given a superseding command between Back admission and preparation completion, when the stale Back worker finishes, then it cannot open the gate or publish success for the newer pipeline.

## Spec Change Log

## Verification

**Commands:**
- `rtk proxy env HIFIMULE_FFMPEG_RUNTIME_VERIFIED=ffmpeg-9-runtime-verified-v2 cargo test -p hifimule-daemon playback::commands::tests::` -- Back and shared command behavior pass.
- `rtk proxy env HIFIMULE_FFMPEG_RUNTIME_VERIFIED=ffmpeg-9-runtime-verified-v2 cargo test -p hifimule-daemon playback::audio::tests::` -- pipeline gate and retirement regressions pass.
- `rtk proxy env HIFIMULE_FFMPEG_RUNTIME_VERIFIED=ffmpeg-9-runtime-verified-v2 cargo test -p hifimule-daemon playback::` -- broader playback suite passes.
- `rtk proxy env HIFIMULE_FFMPEG_RUNTIME_VERIFIED=ffmpeg-9-runtime-verified-v2 cargo test --workspace` -- passes: daemon 1,038 passed/6 intentional ignores plus all workspace suites.
- `rtk node --test scripts/tests/*.test.mjs` -- 152/152 pass.
- `rtk npm --prefix hifimule-ui run build` -- passes with the existing chunk/import warnings.
- `rtk cargo fmt --all -- --check` -- formatting passes.

**Manual checks (if no CLI):**
- On macOS while playing, verify Back after 3 seconds restarts audibly and Back near the beginning selects and audibly starts the predecessor; verify paused Back remains silent.
