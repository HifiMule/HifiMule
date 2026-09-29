# Story 16.14 evidence index

Contract: [`16-14-soak-contract.md`](16-14-soak-contract.md), version 1.

No installed acceptance run has been supplied for the current source revision. The rows below are
deliberately blockers; controlled source tests cannot promote them.

| Artifact row | Install/upgrade | Sustained album/Radio | Physical coexistence | Epic 16.1–16.13 workflow/a11y | Result |
|---|---|---|---|---|---|
| Windows x64 MSI | NOT RUN | NOT RUN | NOT RUN | NOT RUN | BLOCKED |
| Windows x64 NSIS | User-installed 0.16.1; clean-install/upgrade verification NOT RUN | NOT RUN (two-minute Radio preflight only) | User observed no audible problem during sync; matched throughput/physical trace NOT RUN | NOT RUN | BLOCKED |
| Linux x64 deb | NOT RUN | NOT RUN | NOT RUN | NOT RUN | BLOCKED |
| Linux x64 AppImage | NOT RUN (clean launch required) | NOT RUN | NOT RUN | NOT RUN | BLOCKED |
| macOS x64 DMG | NOT RUN | NOT RUN | NOT RUN | NOT RUN | BLOCKED |
| macOS ARM64 DMG | Isolated clean launch PASS; install/upgrade NOT RUN | NOT RUN | NOT RUN | Controlled regressions PASS; installed workflow/a11y NOT RUN | BLOCKED |

Every AC 1–9 and every Story 16.1–16.13 scenario requires a row-local raw JSON record validated by
`scripts/playback-long-session-evidence.py`. A capability that does not apply must include a specific
row-local reason; it cannot inherit another row's observation. Physical media-key delivery is distinct
from API invocation. Material captures require an immutable non-secret URI, SHA-256 and retention policy.

Open blocker: matching immutable packages, installed Windows/Linux/macOS hosts, configured providers,
physical audio outputs and a representative physical sync device are unavailable in this checkout.

## Windows x64 local source check — 2026-09-29

- Current source is version 0.16.1; the only local Windows MSI is version 0.16.0 and cannot certify this revision.
- Local regressions passed: 47 Python evidence tests, 284 Node tests (6 skipped), 1,350 daemon tests
  (7 intentionally ignored), 5 Audiobookshelf contract tests, 7 i18n tests, and TypeScript type checking.
- The Node suite initially failed because a destination-selection test mock lacked the current basket
  context method; the harness was corrected and the complete suite passed.
- At this earlier check, all-target Clippy was unverified because its retry required the controlled
  FFmpeg source cache, which was unavailable locally, and the restricted host could not fetch it.
  A later check is recorded below.
- At this earlier source-check stage, no current-revision installed run, physical output/device
  observation, provider fixture, sustained soak, or Windows sleep/wake scenario had been completed.

## Windows x64 NSIS installed continuation — 2026-09-29

- User installed `HifiMule_0.16.1_x64-setup.exe` (SHA-256
  `2827237927621AC2D729F57E7CA3AF6B63E971AB8E5CF4073B4CA38BAFC58565`). The running
  daemon executable SHA-256 `D5B70D0B0BFFE2317746B7EECBEB558843E7608287054FDFD47C4B592415DBA6`
  matches the current release build. The daemon answered authenticated health with a matching
  owner instance; FFmpeg bindings were 9.0.0, the controlled runtime manifest was 9.0.2, and
  all four loaded libraries came from the installed private directory with matching ABI versions.
- The installed daemon reported four configured servers, one connected device, seven audio
  outputs, a selected physical output, and an active Radio session. The user directly observed
  no audible problem during sync. This observation does not establish the matched 5% throughput
  gate or an uninterrupted sustained run.
- Normal all-target Clippy completed successfully with existing warnings. The Windows NSIS
  [blocked evidence record](16-14-windows-x64-nsis.json) passes the version-1 evidence validator;
  its decision remains `BLOCKED`.
- The sanitized [two-minute Radio preflight](16-14-windows-nsis-radio-preflight.jsonl) has SHA-256
  `BC35CFFCFA619D93565F770BED9CACD7D6B819CDB06D34AC5570C13E8BE1B0BC`: 24 samples,
  zero collection errors, 5.01–5.23-second spacing, RSS 53.3–54.4 MiB, handles at most 468,
  threads at most 34, compressed per-source high water at most 1,834,657 bytes and aggregate
  high water at most 2,153,789 bytes. Playback reported `paused` in 15 samples and `playing`
  in nine; the user confirmed the pause was intentional. This local mutable record is diagnostic
  only and cannot satisfy the frozen warm-up, duration, repetition, fault or immutable-URI gates.
- An isolated installed-UI smoke attempt did not publish a hydration marker within 45 seconds;
  this diagnostic attempt failed and needs diagnosis before a clean-launch result. The normal
  installed daemon stayed running.
  MSI and all Linux/macOS rows remain separate blockers.
- An eight-hour Radio resource sampler started at 2026-09-29 23:06:10 Europe/Paris and is
  writing five-second sanitized samples to an ignored local workspace file. Its planned stop is
  2026-09-30 07:11 Europe/Paris. The first 19 samples had no collection error. The collector
  does not inject the frozen fault schedule or prove audible continuity, so this run cannot by
  itself satisfy the full installed acceptance contract.

## macOS ARM64 controlled package check — 2026-09-29

- Host: macOS 27.0 ARM64; package `HifiMule_0.16.1_aarch64.dmg`, SHA-256
  `4588403fc9e18f6e273708ab189be5f913a70647f04cd4108a1b3402f3c78355`.
- The read-only DMG verified, the copied app passed strict deep code-signature verification, and both
  UI and daemon executables were ARM64. The isolated app clean-launched and accepted orderly Quit.
- The running daemon loaded all four FFmpeg libraries from the app's private `bundled-libs` directory.
  Health reported bindings 9.0.0, packaged FFmpeg 9.0.2 ABI versions, CPAL 0.18.2 and CoreAudio.
- Automated results: 39 macOS/release/runtime Node tests, 47 evidence Python tests, 1,355 daemon tests
  plus five Audiobookshelf contract tests passed; eight daemon tests were intentionally ignored.
- No audio or USB device appeared in the host inventory and no configured provider fixture was supplied.
  Therefore audible output, sustained album/Radio profiles, sleep/wake, physical sync coexistence,
  install/upgrade, accessibility and the Epic 16 installed workflow matrix remain `NOT RUN`/`BLOCKED`.
