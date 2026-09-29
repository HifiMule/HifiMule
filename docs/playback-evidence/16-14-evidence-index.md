# Story 16.14 evidence index

Contract: [`16-14-soak-contract.md`](16-14-soak-contract.md), version 1.

No installed acceptance run has been supplied for the current source revision. The rows below are
deliberately blockers; controlled source tests cannot promote them.

| Artifact row | Install/upgrade | Sustained album/Radio | Physical coexistence | Epic 16.1–16.13 workflow/a11y | Result |
|---|---|---|---|---|---|
| Windows x64 MSI | NOT RUN | NOT RUN | NOT RUN | NOT RUN | BLOCKED |
| Windows x64 NSIS | NOT RUN | NOT RUN | NOT RUN | NOT RUN | BLOCKED |
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
