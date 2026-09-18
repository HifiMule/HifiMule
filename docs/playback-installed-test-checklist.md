# Stories 15.4–15.6 installed playback checklist

Use a clean installed package for each row: Windows x64, Linux x64, macOS x64,
and macOS ARM64. Do not count a source-tree run or VM-only ARM64 run as installed
hardware evidence.

## Reported smoke evidence

On 2026-09-13, user testing confirmed audible AAC, ALAC, FLAC, M4A, Opus, WAV,
and MP3 playback with the latest changes on Windows, Linux, and macOS. The
explicit three-platform format matrix is recorded in
[`playback-evidence-cross-platform-formats-2026-09-13.json`](playback-evidence-cross-platform-formats-2026-09-13.json).
The report did not include every target's package hash, exact architecture,
provider/server version, loaded native-library paths, high-water measurements,
or every lifecycle scenario below, so it does not by itself complete the full
installed-evidence matrix.

The Windows build also produced a working installation package: installation
completed successfully and the installed application was usable. Together with
the playback report above, this confirms the Windows build → package → install →
playback smoke path.

The Windows x64 identity and native-module evidence is captured in
[`playback-evidence-windows-x64-2026-09-13.json`](playback-evidence-windows-x64-2026-09-13.json):
Windows 11 Professional 10.0.22631 x64, NSIS package SHA-256
`f6fafcbf8950580e9e3b8a4ab295894bbc9896fcc122a151d9935f53ddededd6`, and
all four required FFmpeg DLLs loaded from `%LOCALAPPDATA%\HifiMule`. Fields not
present in the supplied output remain explicitly `unverified` in that record.

A separate manual official-source result is captured in
[`playback-evidence-windows-x64-official-source-2026-09-13.json`](playback-evidence-windows-x64-official-source-2026-09-13.json).
After forcing English MSVC detection with `VSLANG=1033`, generating `.lib`
import libraries from FFmpeg's installed `.def` files, and installing LLVM for
the Clang runtime required by Rust bindgen, the HifiMule build and deployed build
succeeded. Subsequent explicit format reporting confirms AAC, ALAC, FLAC, M4A,
Opus, WAV and MP3 playback on Windows. The new package/runtime identity fields
remain explicitly unverified because they were not included in the report.

## Setup

1. Configure one Jellyfin server and one Subsonic/OpenSubsonic server containing
   WAV, FLAC, ALAC/M4A, MP3, AAC/M4A, Opus, AIF/AIFF, OGG/OGA, and WMA tracks.
2. Record package SHA-256, source revision, OS version/architecture, audio endpoint,
   server brand/version, and whether the machine has any system FFmpeg installed.
3. Start HifiMule and retain only the sanitized `audioRuntime` object returned by
   authenticated `daemon.health`. Never retain the owner token, request headers,
   authenticated URLs, or provider error bodies.

## Evidence collector

Run the collector from the matching source revision while the installed HifiMule
application and daemon are open. The package argument is the installer/package
that was installed; the install root is the application-owned directory that
contains the loaded private FFmpeg libraries. The tool reads the private daemon
descriptor only in memory, redacts home/profile prefixes, rejects secret-like
notes and remote URLs, and never writes the owner token.

macOS/Linux:

```bash
python3 scripts/playback-installed-evidence.py collect \
  --target macos-arm64 \
  --package <path-to-dmg> \
  --install-root /Applications/HifiMule.app \
  --output-device-kind physical \
  --desktop-session "macOS Aqua" \
  --provider-kind jellyfin \
  --provider-version <server-version> \
  --output docs/playback-evidence/macos-arm64.json
```

Windows PowerShell:

```powershell
py -3 scripts/playback-installed-evidence.py collect `
  --target windows-x64 `
  --package <path-to-installer.exe> `
  --install-root "$env:LOCALAPPDATA\HifiMule" `
  --output-device-kind physical `
  --desktop-session "Windows 11 Explorer" `
  --provider-kind jellyfin `
  --provider-version <server-version> `
  --output docs/playback-evidence/windows-x64.json
```

Use `linux-x64`, `macos-x64`, or `macos-arm64` as appropriate. For Linux,
provide the actual application-owned root shown by the installed daemon's
loaded module paths. The collector pauses before destructive scenarios so it
can retain buffer counters before Quit rotates the daemon instance. Relaunch
HifiMule when prompted; the tool reloads the new descriptor automatically.

Validate one result or the complete four-target directory:

```bash
python3 scripts/playback-installed-evidence.py validate docs/playback-evidence/macos-arm64.json
python3 scripts/playback-installed-evidence.py validate docs/playback-evidence
```

The directory validation succeeds only when Windows x64, Linux x64, macOS x64
and macOS ARM64 records are complete and passed. Failed or unavailable checks
remain explicit and keep the command non-zero.

## Required runs

The production policy under test is recorded in `hifimule-daemon/audio-runtime.json`:
64 KiB compressed chunks retained within an 8 MiB compressed window, a 500 ms
PCM target capped at 1 MiB, and a 100 ms startup fill. These are configured
bounds, not substitutes for the per-target observed high-water values below.

- Play every format from each applicable provider; record loading → active and
  audible output, then Pause, Resume, and Stop.
- Pause after measurable progress, quit normally, relaunch, confirm the session
  stays paused, and explicitly Resume from the restored position.
- Start a slow-loading track and immediately Play another track. Confirm only the
  second track becomes audible or publishes metadata.
- Close all UI windows while active, wait, reopen, and confirm playback continued
  without another Play command.
- Remove/invalidate the active endpoint. Confirm silence plus `OUTPUT_LOST`, no
  automatic endpoint migration, then explicitly Resume after selecting a valid
  selected endpoint.
- Quit while playing and while loading. Confirm audio stops before exit, sync
  cancellation is not delayed, no worker remains, and relaunch is paused.
- Repeat rapid replacement and a long/slow stream. Record
  `compressedHighWaterBytes`, `pcmHighWaterSamples`, underrun/timeout outcome,
  process RSS/native heap separately, and cleanup outcome.

## Native-library evidence

Record the actual loaded module paths from the installed daemon process, not only
pkg-config metadata or binary link declarations.

- macOS: `lsof -p <daemon-pid>` and retain only `libavcodec`, `libavformat`,
  `libavutil`, and `libswresample` rows.
- Linux: inspect `/proc/<daemon-pid>/maps` and retain only those four library rows.
- Windows PowerShell: `(Get-Process -Id <daemon-pid>).Modules` filtered to those
  four DLL names.

Each path must resolve inside the installed application/package. Record an
explicit failure if a developer path, Homebrew path, distro FFmpeg, or another
system location is loaded.

## Result record

For each target, save a sanitized JSON record containing: target, OS/architecture,
source revision, package hash, FFmpeg source hash/configuration, actual native
versions and loaded paths, endpoint/backend, provider/version, fixture, observed
transitions, audible outcome, decoded/consumed frames, compressed/PCM high-water,
timeout/underrun result, worker cleanup, and `passed` or `failed`. Keep unavailable
provider/hardware combinations as `unverified`; never infer them from another row.

## AIF, OGG, and WMA validation

The new format matrix must be checked separately on each installed target and
provider. The earlier 2026-09-13 smoke evidence does not cover these additions.

| Container / suffix | Audio variants | Local automated evidence | Installed evidence |
| --- | --- | --- | --- |
| AIF / AIFF | Big-endian PCM 16/24/32-bit | Synthetic decode, finite PCM, full duration, resume, >8 MiB metadata | Pending |
| OGG / OGA | Vorbis and Opus | Synthetic decode, finite PCM, full duration, resume, >8 MiB metadata | Pending |
| WMA / ASF | WMA v1 and v2, unencrypted | Synthetic decode, finite PCM, duration within one codec block, exact resume, >8 MiB metadata | Pending |
| WMA / ASF | WMA Pro and Lossless, unencrypted | Native decoders enabled; fixture validation pending | Pending |

Local evidence uses macOS ARM64 Homebrew FFmpeg 9.0.1 with the required ABI,
through the production bounded reader. It does not prove the reduced runtime
built for Windows/Linux: rebuild those runtimes from the updated manifest and
record configure flags, loaded libraries, and audible playback. No FFmpeg release
or dependency version changed. The native decoders add no external codec libraries.

Include uppercase suffixes, original-versus-alternative selection, restored
position, and long tracks beyond the 8 MiB retained window. Test malformed files
through the existing typed failure path; encrypted/DRM WMA is unsupported.
Fixture provenance and regeneration commands live in
[`generated-audio.source.md`](../hifimule-daemon/tests/fixtures/generated-audio.source.md).


## Story 15.5 output safety matrix

The earlier 2026-09-13 results do not validate this implementation. Collect fresh
Windows x64, Linux x64, macOS x64 and macOS ARM64 installed package records. Pass
`--output-device-kind physical` for hardware, or `virtual` for an explicitly
labeled virtual-device run. Virtual evidence does not satisfy physical speaker
safety. The collector records sanitized selected/pending/active identity hashes
and transport position before and after every scenario; it omits device names,
raw endpoint identifiers and media source metadata.

For each target:

- Switch between two outputs while playing, loading, buffering, paused, stopped,
  completed and idle. Verify occurrence/queue preservation and measure position
  delta, audible interruption and stop latency. Check differing rates/formats.
- Physically unplug headphones with built-in speakers available. Listen for any
  unexpected speaker playback. Replug and change the system default; playback
  must stay paused until explicit Resume. A healthy selected endpoint must stay
  selected when only the default changes.
- Restart with the saved endpoint absent. Verify unavailable state without name
  matching or fallback. Test duplicate friendly names, rename, a recreated device
  with a different identity, and a failed open.
- Exercise rapid A→B→C selection with Pause, Stop, a new track and Quit. Confirm
  only the latest admitted operation opens output, with no overlap or revived
  audio. A delayed retirement must block new audio while status/Stop/Quit remain
  responsive; retain and inspect shutdown blockers.
- Test sleep/wake, Linux server restart, and other-application audio concurrently.
  Record backend/library versions, server buffering, compressed/PCM high-water,
  measured physical destination and worker cleanup. Do not infer silent or shared
  behavior from compilation or mock adapters.

All four target rows remain unverified until installed physical evidence is
collected. A source-tree macOS build and a Pulse type-check probe cannot replace
those records.

## Story 15.6 native-control evidence

Collect `nativeEvidenceVersion: 1` independently from output evidence. Record the
OS, architecture, desktop session, command path, UI-open/closed state, actual
delivery, authoritative before/after playback state, daemon PID/instance,
generation, and state sequence. Never label an API command as a physical-key
observation.

For each installed target, exercise Play, Pause, Toggle and Stop through the
native API with the UI open and closed; use the tray Resume action while closed;
reopen the UI and verify it reflects the resulting state without replaying a
command. Replace a metadata-rich track with a sparse track and then clear the
session, confirming old fields disappear. Verify output-loss Play is rejected
without rerouting. Observe registration and metadata removal after Quit *before*
relaunch, then verify the new daemon instance registers once.

Physical media keys must be attempted separately with the UI open and closed.
Record observed delivery as a pass. If the desktop routes the key elsewhere or
does not deliver it, record `outcome: limitation`, `delivery: not-delivered`, and
a specific sanitized limitation; do not install a global keyboard hook or claim
API success as physical-key evidence.

### Native observation record fields

Every `before` and `after` state requires `pid` (positive integer), `instanceId`,
`generationId` (UUID), `stateSequence` and `queueRevision` (decimal strings),
`playbackStatus`, `positionMs` (nonnegative integer), and `occurrenceId` (a
consistent anonymous occurrence label, or null for an empty session). Use actual
snapshot values; keep labels consistent across each comparison. Never copy
example values as observations. The collector rejects malformed JSON, non-object
input, unsafe fields and invalid state shapes, then asks for the same entry again
without losing previously entered observations.

Capture before and after snapshots during each action. API and menu transport
commands must advance the state sequence and preserve the occurrence and queue.
Stop must reach `stopped`, reset position to zero and rotate the generation.
For these additional observations, supply a `facts` object with the following
measured or directly observed fields (boolean values are JSON `true`/`false`):

| Observation | Required procedure and facts for a pass |
| --- | --- |
| `menu-resume-ui-closed` | Begin paused at a nonzero position; Resume to active and observe increasing position. `windowStayedClosed: true`, `audibleProgress: true`. |
| `ui-reopen-authoritative` | Pause natively before opening the UI so exact comparison is possible. State, position, generation and sequence remain unchanged. `commandReplayed: false`; `uiPositionMs` and `uiPlaybackStatus` match the daemon snapshot. |
| `metadata-cleared` | Observe rich metadata, replace with sparse metadata, then clear the session. `richFieldsObserved: true`, `sparseMissingFieldsCleared: true`, `emptySessionFieldsCleared: true`. Do not include private titles or artwork addresses. |
| `output-loss-rejected` | Capture paused/error state after output loss, attempt Play and verify frozen position. `playRejected: true`, `outputRerouted: false`, `audible: false`; `selectedOutputBefore` and `selectedOutputAfter` contain the same SHA-256 hash of the selected output ID. |
| `quit-deregistered-before-relaunch` | Retain the last snapshot before exit as `after` (there is no live RPC after exit). Separately observe OS state before relaunch: `registrationReleased: true`, `metadataCleared: true`, `observedBeforeRelaunch: true`, `processExited: true`. |
| `new-instance-reregistered` | Compare the old instance with the relaunched, paused instance; instance IDs must differ (OS PIDs can be reused). `registrationCount: 1` from the OS registration observation. |

Enter actual values for failed checks; the validator will retain them as failed
evidence. Use `{}` for facts on observations without additional required fields.
A physical-key routing limitation still requires before/after state evidence and
an explicit explanation; it never counts as API delivery or a physical-key pass.

### Optional native endpoint binding smoke

On macOS or Windows, this explicit test enumerates concrete native endpoints, opens and releases unstarted silent streams, and verifies an absent stable ID cannot fall back:

```sh
rtk proxy node scripts/build-daemon.mjs test -p hifimule-daemon native_concrete_endpoints -- --ignored --nocapture --test-threads=1
```

This source-build smoke is not an installed-package test and does not prove audible destination, unplug safety, coexistence or sleep/wake behavior. On 2026-09-16 it passed for two outputs on the macOS ARM64 development host; all installed physical matrix requirements above remain unverified.

### User-reported switching result

On 2026-09-16 the user reported that switching between physical devices works and requested continuing implementation. This is a passed manual switching observation; architecture, package hash, device pair and additional scenario details were not supplied. It does not certify the full installed matrix. Linux evidence additionally requires `pulseServerBufferMaxBytes` from daemon health, bounded to 38,400 bytes for the configured 48 kHz stereo float32 stream.

### Platform validation confirmation

All implementation and locally runnable automated checks are complete for story 15.5. The selected-endpoint WASAPI notification adapter also cross-compiles against the Windows ARM64 GNU Rust target; this does not substitute for a complete Windows package/runtime test. On 2026-09-16, the user confirmed that all remaining tests were complete and requested moving story 15.5 to review. This records user-reported completion of the platform validation; no additional per-target logs, package hashes or measurements were supplied in that confirmation. The optional Windows silent binding smoke now includes notification registration/unregistration.

Story 15.5 review refinements: Linux physical outputs with multiple or unknown ports are unavailable because sink pinning cannot prevent automatic headphone-to-speaker port changes. Use a concrete single-port output for physical acceptance; virtual outputs remain explicitly labeled and cannot certify downstream routing. Test rejected analog multi-port routes separately. Evidence requires complete sanitized selected/pending/active descriptors (or explicit null roles), output status/revision/error and transport/position state. The switching scenario must capture different selected identity hashes before and after; absent-startup must capture an unavailable saved output with no active stream.
## Story 15.7 seek evidence

Seek acceptance uses `seekEvidenceVersion: 1`. Each enabled row records OS and
architecture, package revision, backend, provider and server version,
representation/container/codec, operation and playback identity, queue revision,
requested target, prior committed cursor, independently measured decoded
landing, absolute error, transport before/after, compressed and PCM peaks, and
audible outcome. The decoded error must be at most 50 ms. Native API and physical
control observations are separate fields.

Required installed rows are Windows x64, Linux x64, macOS x64 and macOS ARM64
for each enabled Jellyfin original combination: PCM-in-WAV, AAC/ALAC-in-M4A,
Opus-in-Ogg (`ogg`, `oga` or `opus` suffix), MP3 and FLAC. A disabled
provider/format row must include a
reason and ordinary-playback result. Existing Stories 15.4–15.6 records remain
historical evidence and cannot satisfy seek acceptance. At story implementation
time, installed runs unavailable on the current host remain explicitly
`unverified`; source compilation or an ordinary playback smoke does not promote
them.

Boundary/race runs cover zero, exact duration, forward/backward, paused,
repeated/superseded, failure, Stop, replacement, output loss/switch and Quit.
The collector rejects no-op forward/backward success, request-equals-result
without an independent oracle, changed identity/queue, pending/failed targets
shown as committed, contradictory duplicate matrix rows and missing or
unsupported evidence versions.


Review follow-up (2026-09-18): the user reported Jellyfin PCM-WAV seeking works
on both macOS and Linux test systems. These are qualitative field results; the
architecture, PCM depth and numeric landing observations needed for formal
installed rows were not supplied. The user also confirmed that all three first
compressed-batch combinations work on macOS: Jellyfin original AAC-in-M4A,
ALAC-in-M4A and Opus-in-Ogg. These compressed results are also qualitative;
deterministic decoder tests alone do not supply the missing installed metrics.
The second candidate batch adds runtime-verified Jellyfin original MP3 and FLAC
for installed testing. MP3 uses bounded decoder pre-roll to reconstruct its bit
reservoir before target trimming.
Field follow-up: Jellyfin FLAC seeking works on macOS. MP3 initially remained
unavailable because sequential probing did not expose FFmpeg stream duration,
despite Jellyfin providing the displayed duration; MP3 now accepts that positive
provider duration when stream duration is unavailable and still verifies the
opened MP3 codec/container plus the decoded seek landing.
The validator now rejects an entirely disabled matrix as Story 15.7 acceptance,
unchanged actual/oracle cursors behind different requested targets, wrong-direction
landings, target error above 50 ms, and transport-intent mismatches. This safe
interim gate does **not** satisfy the mandatory usable-seek acceptance criterion.
Record the required installed observations before marking exact
provider/codec/backend/OS/architecture combinations qualified for release.

### Provisional field observations

- 2026-09-18 — macOS, Jellyfin, WAV: user reports that seeking works after the
  runtime qualification fix. This confirms that the control becomes available
  and produces a usable seek on one real setup. Architecture, PCM depth,
  requested/landed positions, numeric error, transport before/after, backend,
  buffer peaks and package revision were not supplied, so this observation is
  not yet an AC9-qualified installed row.
