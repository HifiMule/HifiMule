# Playback session lifetime and native controls proof

Date: 2026-09-11. Scope: optional `session-controls` feature in the standalone playback experiment. No production daemon, UI, installer or startup policy changed. All playback used generated 48 kHz stereo WAV fixtures at volume zero.

## Results

| Platform | Independent owner; controller close/reopen | Native controls | Shutdown |
|---|---|---|---|
| macOS ARM64, CoreAudio | Passed | User-operated media key delivered native Toggle; counters paused and resumed | Supervised lifecycle exit code 0; separate native-key run reached deadline and removed endpoint |
| Windows 11 ARM64 VM, WASAPI | Passed | Windows SMTC Pause/Play and counter effects passed | Exit code 0; owner/supervisor exited; endpoint removed |
| Ubuntu ARM64 VM, PipeWire route | Passed | MPRIS Pause/Play and counter effects passed | Exit code 0; owner/supervisor exited; endpoint removed; Wayland warning below |

All measured runs had zero underrun frames. Physical Windows/Linux keyboard routing was not tested; their native OS transport APIs were tested directly. Callback counters do not certify audible gaplessness or physical output quality. These are short generated-fixture proofs, not long-running reliability or memory benchmarks.

## macOS evidence

Raw captures: `experiments/playback-probe/results/session-supervised-final` and `session-macos-media-key`. The supervised lifecycle run retained owner PID 64884 through both controller windows, paused/resumed with position retained, rejected an incorrect token, and recorded exit code 0. The detached test supervisor also exited and the endpoint was removed.

In the separate user-operated media-key run, owner PID 69477 logged four accepted native Toggle events. Status sampling held exactly 2,125,824 frames while paused, then observed the native resume at 2,126,848 and continued advancement to 2,578,432. Final output counted 2,578,944 frames, zero underruns, and a deadline shutdown. Stderr was empty, the endpoint was removed, and the process exited. That run did not use the exit-code supervisor; exit-code verification comes from the separate lifecycle run. Control Center UI presentation was not inspected.

An earlier controller run exposed a TCP reset while the owner stayed healthy. Explicit blocking mode with a bounded read deadline resolved the accepted-socket behavior on macOS. A delayed-request regression and repeated full lifecycle runs passed afterward.

## Windows evidence

Raw final capture: `experiments/playback-probe/results/session-windows-supervised-final`. Owner PID 11172 survived both disposable controller windows closing. Native Pause held at 176,256 frames; native Play advanced from 183,936 to 199,776. Owner exit code was 0; owner and supervisor exited and the endpoint was removed. Supervisor stderr was empty.

The independent Windows helper discovers only the synthetic title `Generated silent continuity fixtures`, then calls GlobalSystemMediaTransportControlsSessionManager session APIs. Owner logs identify the resulting native Pause and Play events. Reproducible helper sources are retained in `experiments/playback-probe/windows-native-remote`.

The ARM64 probe used the pinned Rust dependencies and FFmpeg 9 shared runtime from the earlier decoder proof. Official portable Python 3.14.6 ARM64 ran the harness. A temporary scheduled task used the signed-in user's limited interactive token. Task removal and absence of test processes were verified. This does not validate playback from a system service.

The first supervised Windows attempt stalled before owner PID publication. Explicit detached-process flags on the owner, captured supervisor diagnostics and a bounded 15-second startup wait were added; the rerun passed. The failure was retained in `results/session-windows-supervised`.

## Ubuntu evidence

Raw capture: `experiments/playback-probe/results/session-linux/capture`. Owner PID 19291 retained session identity `a77c631580c669d7` after launcher exit and both controller windows closing. MPRIS Pause held at 162,496 frames; Play advanced from 168,640 to 185,024. Zero underruns, exit code 0, owner/supervisor exit and endpoint removal passed.

Rust/Cargo 1.93.1 built the vendored dependencies offline. D-Bus was 1.16.2; `ldd` confirmed private FFmpeg 9 libavcodec/libavformat 63 and libavutil 61. CPAL used explicit device `pipewire` in the signed-in desktop session. All 23 feature-enabled tests passed. The initial build needed libsystemd development metadata in addition to libdbus development files; extracting both official Ubuntu packages into the temporary workspace resolved this without a system-wide install.

Wayland printed a teardown warning about proxies still attached to its event queue. Although the process exited successfully and released its endpoint, native-window teardown deserves investigation before production integration. The proof does not claim warning-free Wayland cleanup.

## Regression and review

Default Rust tests: 9 passed. Feature-enabled tests: 23 passed on macOS and Ubuntu. Mac Clippy passed. Six-format native decoding regression passed for WAV, FLAC, ALAC, MP3, AAC and Opus: all retained the expected 288,041 frames and existing error thresholds (`results/session-decode-regression.json`).

Independent blind, edge-case and acceptance reviews identified controller-Q exit behavior and harness evidence gaps. Corrections added recorded owner exit codes, valid endpoint-JSON readiness, explicit runtime checks, native counter hold/advance checks, and guaranteed report writing during cleanup failures. A deliberately failing supervisor child preserved exit code 7. Focused re-review found no remaining actionable issues within the bounded proof.

## Integration implications and limits

The results support a Rust playback owner with a separate audio worker, native event loop and reconnectable controllers on macOS, Windows and Linux. Native controls belong in the desktop user session. Production integration should reuse the daemon's existing main-thread tray loop.

Production lifecycle work remains: the UI kills a sidecar it spawned on application exit; an already running external daemon survives. macOS uses a user LaunchAgent. Windows installers use HKCU Run but retain a legacy service fallback. Inspected Linux packaging has no equivalent user startup registration. This experiment changes none of those policies.

Queue persistence, seeking, network streaming, Radio, device removal, sleep/wake, memory measurements, physical Windows/Linux media keys, and production UI integration remain outside this proof. The hidden native window requires a desktop environment. Finite sessions and a three-minute watchdog are experimental bounds, not a production background-service design.
