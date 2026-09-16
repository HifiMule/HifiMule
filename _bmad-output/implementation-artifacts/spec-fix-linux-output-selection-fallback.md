---
title: 'Allow a last-resort Linux shared output selection'
type: 'bugfix'
created: '2026-09-16'
status: 'done'
baseline_commit: '3ff269bfcdd9adb9ed5f44e30970774432fd5741'
context:
  - '{project-root}/docs/api-contracts-hifimule-daemon.md'
  - '{project-root}/_bmad-output/implementation-artifacts/15-5-choose-an-audio-output-and-recover-safely-from-disconnection.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** On Linux, a physical PipeWire/PulseAudio sink with multiple ports is marked unavailable even when it is the only output and its active port is known. The UI consequently disables the default system output and reports it cannot safely be used in shared mode, although it can support shared Pulse playback.

**Approach:** Retain the conservative compatibility preference, but permit one explicit last-resort physical default sink when no normally compatible output is available. Apply the same decision at discovery, selection, and stream-opening boundaries so the UI cannot offer an output that the backend rejects.

## Boundaries & Constraints

**Always:** Keep the output explicitly pinned to its named Pulse sink; preserve stable identity checks, selection persistence, and existing non-Linux behavior. Prefer fully certified shared outputs whenever any are available. Only relax the port-topology gate for the discovered default physical sink and only when it is the sole selectable fallback. Surface the fallback as a caution in the UI rather than falsely claiming certified speaker-safety.

**Ask First:** Expanding fallback selection beyond the system default, allowing unknown/ambiguous identities, or removing the safety warning requires user approval.

**Never:** Auto-switch an existing selected output to the fallback; change system routing or port configuration; treat virtual output as physical-routing evidence; use a UI-only bypass; alter macOS or Windows discovery behavior.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Certified output exists | At least one normally compatible output and a multi-port physical default | Certified output remains selectable; multi-port default remains unavailable | Explain its unsupported status |
| Last-resort default | No normally compatible outputs; one stable, physical multi-port default with active port | Default is selectable and opens through Pulse shared mode with a caution | Stream validates the same fallback eligibility |
| Uncertain fallback | Default identity is ambiguous, absent, inactive, or discovery is incomplete/fails | No fallback is exposed or persisted | Preserve structured output-unavailable/identity error |
| Reopen after selection | Fallback saved then inventory refreshes | Restore only if the same named sink and identity properties still resolve | Never reroute by name/default role |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/playback/devices/pulse.rs` -- Pulse/PipeWire sink discovery and availability classification.
- `hifimule-daemon/src/playback/devices/pulse_route.rs` -- shared routing safety policy used in discovery and open-time validation.
- `hifimule-daemon/src/playback/devices/pulse_stream.rs` -- validates the actual opened sink before uncorking audio.
- `hifimule-daemon/src/playback/session/output_selection.rs` -- first-run/default selection and persistent selection policy.
- `hifimule-ui/src/components/PlaybackControls.ts` -- disabled options and localized fallback caution.
- `hifimule-i18n/catalog.json` -- four-locale UI copy.
- `scripts/tests/playback-ui.test.mjs` -- selector behavior regression coverage.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/playback/devices/pulse_route.rs` -- model and test certified versus last-resort routing eligibility -- ensure a multi-port default is allowed only after proving no certified output exists.
- [x] `hifimule-daemon/src/playback/devices/pulse.rs`, `hifimule-daemon/src/playback/devices/pulse_stream.rs`, `hifimule-daemon/src/playback/session/output_selection.rs` -- carry the narrowly scoped fallback classification through discovery, default selection, persistence, and open-time verification -- prevent mismatched UI/backend admission.
- [x] `hifimule-ui/src/components/PlaybackControls.ts`, `hifimule-i18n/catalog.json`, `scripts/tests/playback-ui.test.mjs` -- show the fallback as selectable with an accurate caution and verify disabled/certified cases -- preserve accessibility and locale parity.

**Acceptance Criteria:**
- Given the reported PipeWire sink (two ports, a known active port, and no certified alternatives), when Outputs refreshes, then the default is selectable and shared playback opens against that named sink.
- Given a certified output exists, when the same multi-port default is listed, then it remains unavailable and certified output selection is unaffected.
- Given the fallback's identity or active route cannot be verified at open time, when playback is requested, then playback fails without default-role rerouting or audible output elsewhere.
- Given a fallback selection is saved, when the daemon restarts, then it restores only to the same resolvable endpoint and starts paused.

## Spec Change Log

## Design Notes

The reported default sink is stable (`device.bus_path`, `device.string`, and hardware properties), has two configured ports, and exposes a concrete active port. The old binary policy treats `port_count != 1` as inherently incompatible. This change treats that topology as a last-resort, best-effort shared route rather than proof of incompatibility, while keeping the policy strict whenever a certified alternative exists.

## Verification

**Commands:**
- `rtk proxy node scripts/build-daemon.mjs test -p hifimule-daemon playback:: -- --test-threads=1` -- expected: new policy regressions and existing playback tests pass.
- `rtk proxy node --test scripts/tests/playback-ui.test.mjs` -- expected: fallback option and unavailable-option behavior pass.
- `rtk npm run build --prefix hifimule-ui` -- expected: UI compiles successfully.
- `rtk cargo fmt --all -- --check` -- expected: formatting passes.
- `rtk git diff --check` -- expected: no whitespace errors.

**Manual checks:**
- On the supplied Linux PipeWire session, verify the fallback is selectable only with no certified outputs, play alongside another desktop application, and confirm the audible destination remains the selected output. Exercise a port/default change and verify HifiMule does not silently reroute.

## Suggested Review Order

**Fallback admission**

- Defines the certified-versus-last-resort decision and rejects incomplete inventories.
  [`pulse_route.rs:17`](../../hifimule-daemon/src/playback/devices/pulse_route.rs#L17)

- Applies that decision only to stable default multi-port Pulse sinks.
  [`pulse.rs:147`](../../hifimule-daemon/src/playback/devices/pulse.rs#L147)

**Open-time safety**

- Revalidates multi-port fallback identity and admission within the original deadline.
  [`pulse_stream.rs:112`](../../hifimule-daemon/src/playback/devices/pulse_stream.rs#L112)

**User-facing behavior**

- Labels a selectable fallback as best-effort shared routing.
  [`PlaybackControls.ts:144`](../../hifimule-ui/src/components/PlaybackControls.ts#L144)

- Keeps the warning translated in every supported locale.
  [`catalog.json:10`](../../hifimule-i18n/catalog.json#L10)

**Regression coverage**

- Covers incomplete inventory rejection and the selectable UI fallback warning.
  [`pulse_route.rs:75`](../../hifimule-daemon/src/playback/devices/pulse_route.rs#L75)
  [`playback-ui.test.mjs:189`](../../scripts/tests/playback-ui.test.mjs#L189)
