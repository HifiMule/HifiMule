# Playback session lifetime and native controls proof

Date: 2026-09-11. Scope: optional `session-controls` feature in the standalone playback experiment. No production daemon, UI, installer or startup policy changed. All playback used generated 48 kHz stereo WAV fixtures at volume zero.

## Observed results

| Platform | Independent owner / close and reopen controller | Pause/resume | Native OS transport | Cleanup |
|---|---|---|---|---|
| macOS ARM64, CoreAudio | Passed twice after transport correction | Passed, position retained | Not yet verified | Owner exited; endpoint removed |
| Windows 11 ARM64 VM, WASAPI | Passed under interactive user Alexis | Passed, position retained | Windows SMTC Pause and Play passed | Owner exited; endpoint removed |
| Ubuntu ARM64 VM | Pending desktop access | Pending | MPRIS pending | Pending |

Physical keyboard routing remains untested on all platforms. Native OS command delivery is a separate claim from keyboard routing. Callback counters do not certify audible gaplessness or physical output quality.

## macOS evidence

Ignored raw captures: `experiments/playback-probe/results/session-repro-blocking` and `session-repro-final`. Final owner PID 60609 retained one session identity through both disposable controller windows. Consumed frames advanced from 6,656 after launcher exit to 150,016 after the second controller closed. Pause held at 151,040; resume advanced to 172,544. Zero underrun frames; wrong capability token rejected; explicit quit removed the endpoint and process.

An earlier run exposed a TCP reset while the owner remained healthy. Accepted sockets can inherit nonblocking mode on macOS; reads now explicitly use blocking mode with a bounded deadline. A delayed-request regression and two full lifecycle runs passed after this correction. Native macOS registration API calls returning success alone do not prove Control Center visibility or command delivery. UI automation was unable to complete that native check; it remains open.

## Windows evidence

Ignored raw capture: `experiments/playback-probe/results/session-windows`. Owner PID 2280 survived both controller windows closing. Pause retained frame 163,488; resume reached 184,608. An independent Windows test client discovered only the session with synthetic title `Generated silent continuity fixtures` and called the OS media-session API. Owner logs record native `Pause` and `Play`, both accepted, with corresponding status changes. Final output counted 227,328 consumed frames and zero underruns. Owner stderr was empty; explicit quit removed the endpoint and process.

The ARM64 executable uses the same pinned Rust dependencies and FFmpeg 9 shared runtime as the earlier Windows decoder proof. Portable official Python 3.14.6 ARM64 ran the harness. A temporary scheduled task launched it with the signed-in user's limited interactive token; task removal was verified after completion. This does not validate playback from a Windows system service.

## Architecture implications

The experiment supports a Rust playback owner with its own audio worker, native event loop and independently reconnectable controller. Keep native controls in the desktop user session. Integrate with the existing daemon main-thread tray loop rather than creating a second application event loop in production.

Production lifecycle work remains necessary: the UI currently kills a sidecar it spawned on application exit. An already running external daemon survives that exit. macOS has a user LaunchAgent; Windows installers use HKCU Run but retain a legacy service fallback; inspected Linux packaging has no equivalent user startup registration. These are integration requirements, not changes made by this experiment.

The proof deliberately omits queue persistence, seeking, network streams, Radio, memory benchmarks, device removal, sleep/wake, and physical media keys. The hidden native window still requires a desktop environment. Session mode has a finite duration and a three-minute safety watchdog; it is not a production background service.

## Regression and review

The existing six-format native decoder verifier passed after the session refactor: WAV, FLAC, ALAC, MP3, AAC and Opus all retained the expected 288,041 frames and existing error thresholds. Raw capture: `experiments/playback-probe/results/session-decode-regression.json`. Independent blind, edge-case and acceptance reviews identified controller-Q and test-harness evidence weaknesses; the implementation was corrected and focused re-review passed.

Review corrections added explicit owner exit-code capture through a detached test supervisor, complete endpoint-JSON readiness, clean controller-Q exit, native counter hold/advance checks and unconditional cleanup reporting. The supervised Mac rerun passed (`results/session-supervised-final`): owner PID 64884, exit code 0, zero underruns, owner and supervisor exited, endpoint removed. A deliberate supervisor child failure preserved exit code 7. Feature-enabled tests: 23 passing; Clippy passed.

A first Windows supervised rerun failed before publishing the owner PID; the harness correctly reported failure. No test processes remained after cleanup. Supervisor launch diagnostics are being added before rerunning; the earlier Windows native-control result remains valid but does not prove an owner exit code.

The final Windows supervised rerun passed (`results/session-windows-supervised-final`): owner PID 11172, exit code 0, zero underruns, endpoint removed, owner and supervisor exited. Native Pause held at 176,256 frames across both samples; native Play advanced from 183,936 to 199,776. Restoring explicit Windows detached-process creation for the owner resolved the supervisor-startup failure in this rerun. Supervisor stderr was empty. The reproducible independent OS client is retained under `experiments/playback-probe/windows-native-remote`.

Default Rust tests also passed (9). All review findings affecting this proof were patched or had their claimed unit-test scope narrowed; focused re-review reported no remaining findings. This is a checkpoint: Ubuntu runtime/MPRIS and macOS native transport delivery still require unlocked-desktop access. No production integration or physical-key certification is claimed.
