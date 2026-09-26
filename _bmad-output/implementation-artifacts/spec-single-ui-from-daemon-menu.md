---
title: 'Reuse and focus the UI from the daemon menu'
type: 'bugfix'
created: '2026-09-26'
status: 'done'
baseline_commit: '168da58b1ce888130afbffbc726699725fd68eb4'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/15-1-close-and-reopen-the-ui-without-restarting-the-daemon.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Every daemon tray “Open UI” click starts a new Tauri UI process. Repeated clicks can leave multiple windows and independent UI sessions connected to the same daemon.

**Approach:** Enforce one desktop UI instance across all launch paths. A later launch should ask the running UI to restore and focus its existing window; after the UI has exited, the tray should launch a fresh UI. Preserve the daemon as an independent, long-lived process.

## Boundaries & Constraints

**Always:** Work on Windows, macOS, and Linux in packaged builds; keep rapid and concurrent launches from creating duplicate UI sessions; show and unminimize the main window when ready, or the splash window during startup; handle absent or closing windows without panic; keep daemon startup and quit behavior intact. Focus requests may be limited by the operating system or compositor, but the existing window must at least be shown where supported.

**Ask First:** Any design requiring a new always-on UI server, OS-specific window enumeration, or a change to the daemon's single-owner lifecycle.

**Never:** Treat daemon connectivity as proof that a UI is open, kill another UI process by PID, restart the daemon to open the UI, or change sync and playback state when focusing a window.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|----------------------------|----------------|
| Cold launch | No UI process; daemon tray clicked | One UI process opens and attaches to the daemon | Existing launch error remains visible in daemon logs |
| Repeat click | Main UI visible, hidden, or minimized | One UI session remains; main window is shown, restored, and focus is requested | Log window operation failure without starting another persistent UI |
| Startup click | UI still on splash screen | One UI session remains; splash is brought forward until main is ready | Missing window is safe and does not panic |
| Close and reopen | UI has fully exited; daemon stays running | New UI opens and reconnects to the same daemon | Existing startup failure and retry paths remain usable |
| Concurrent launch | Tray and direct OS/app launch overlap | Only one UI session survives | Loser exits cleanly; no extra daemon ownership attempt |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/main.rs` -- Tray “Open UI” handler currently calls `spawn()` for every click; debug uses `npm run tauri dev`, release uses the adjacent UI executable.
- `hifimule-ui/src-tauri/src/lib.rs` -- Tauri builder, splash/main lifecycle, and startup coordinator; place instance arbitration and window activation here.
- `hifimule-lifecycle/src/lib.rs` -- Private per-profile runtime paths and cross-process OS file locks; host the activation request contract.
- `hifimule-ui/src-tauri/tauri.conf.json` -- `main` starts hidden; `splashscreen` starts visible; application identifier defines instance identity.
- `hifimule-ui/src/main.ts` -- Shows main and closes splash after readiness; explicit close exits the UI.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-lifecycle/src/lib.rs` -- Add a per-profile lifetime UI lock and an atomic, private activation request mailbox. A loser requests activation and exits before UI/daemon coordination; concurrent requests may coalesce. Preserve existing runtime path and permissions rules.
- [x] `hifimule-ui/src-tauri/src/lib.rs` -- Arbitrate before constructing windows; the winner polls activation requests while alive, retries if no ready window exists, and shows/unminimizes/focuses the main or splash on the UI thread. A loser exits without initializing the UI or daemon. Handle startup errors through the existing failure path where possible.
- [x] `hifimule-daemon/src/main.rs` -- Request activation and launch the UI from the tray, including rapid clicks and development mode, without starting extra dev runners or coupling UI lifetime to daemon lifetime. Reap short-lived launcher children.
- [x] `scripts/smoke-tests/smoke-{linux,macos}.sh`, `scripts/smoke-tests/smoke-windows.ps1`, focused Rust tests -- Assert one UI survives concurrent launches and a prompt loser exit; make the macOS launch wait for the actual second process. Cover mailbox delivery, lock contention, splash selection, and reopen. Retain installed manual focus checks.

**Acceptance Criteria:**
- Given a running UI, when “Open UI” is selected repeatedly, then one UI session remains and its current window is shown with a focus request.
- Given the UI has exited and the daemon is still running, when “Open UI” is selected, then one new UI opens and attaches to that daemon.
- Given simultaneous tray and direct launches, when both reach UI startup, then one instance survives and only that instance initializes the UI lifecycle.

## Spec Change Log

- 2026-09-26 review loop 3 remediation: A losing UI now waits briefly for an activation acknowledgment or the profile lock to release; if the previous owner exits before handling the request, the loser acquires ownership and opens the UI. The owner acknowledges successful window activation and baselines an older mailbox request before startup. Runtime path and UI-lock failures enter the existing splash failure/retry flow. Development tray clicks probe the UI lock, reuse an existing UI, and reopen through the adjacent debug binary if the npm runner outlives its UI. Nonzero launcher exits are logged. Cross-process tests cover acknowledgment and lock takeover.

- 2026-09-26 review loop 2 implementation: Removed plugin forwarding. The UI acquires its private profile lock before building Tauri windows; a loser writes an activation request and returns. The winner polls the mailbox, retries until a splash or hydrated main window is available, then attempts show/unminimize/focus on the UI thread. Tray clicks write before launching, and installed smoke scripts check that the second UI exits; macOS waits through `open -W -n`. A running development runner reuses the mailbox instead of starting another Vite server. If that runner remains alive after its UI has exited, stop the runner before opening a fresh development UI. Installed platform activation remains unverified.

- 2026-09-26 review remediation: Added a private `ui.lock` held for the UI lifetime, with a losing setup exiting before daemon coordination. Added cross-process lock coverage and splash/main selection tests; installed smoke scripts now require the second UI to exit and the original to survive. Development tray clicks reuse one `tauri dev` runner and signal the built UI only after the UI lock is held. If the runner remains alive without a UI lock, a repeat click logs that startup is still pending; a stuck development runner must be stopped before another is launched. Installed focus checks remain pending.

- 2026-09-26 review loop 1: Tauri plugin 2.4.5 has a macOS socket-bind and Windows mutex/window startup gap, so plugin-only arbitration can initialize two UIs. Require a per-profile OS lock before UI coordination and have a losing process exit; this avoids duplicate sessions even when notification is unavailable. The existing concurrent-launch smoke scripts also expect a second UI to stay alive, so update that assertion. KEEP the plugin for normal activation, main-thread show/unminimize/focus, daemon child reaping, and development-runner coalescing. Prefer splash over an unhydrated hidden main.
- 2026-09-26 review loop 2: The plugin endpoint owner can differ from the OS-lock winner in the same startup race, leaving the surviving UI unreachable for later activation. Replace plugin forwarding with a private per-profile activation mailbox: every second launch requests activation before exiting, and the lock winner polls and retries until a window is ready. This avoids a permanent no-op after the race and also avoids the plugin's global endpoint conflicting with isolated profiles. macOS smoke must wait for the second app launch, not pass on a pre-launch PID snapshot. KEEP the lifetime lock, main-thread window operations, splash-before-main selection, daemon child reaping and development-runner coalescing.

- 2026-09-26: Implemented single-instance arbitration and window activation, coalesced development tray launches, and added window-selection tests. Installed platform checks remain pending.

## Design Notes

Use the existing private runtime directory for both the UI lifetime lock and a small atomic activation request. The lock determines the winner before windows or daemon coordination; a loser posts a request and exits. The winner polls on a bounded cadence and schedules window operations on Tauri's main thread. This is profile-scoped and avoids a second server or platform window enumeration. A request arriving before a window exists stays pending until a window can be shown. OS foreground focus remains best effort.

## Verification

**Commands:**
- `rtk cargo check -p hifimule-ui` -- UI compiles with the mailbox and lifetime lock.
- `rtk cargo test -p hifimule-ui --lib` -- Focused lifecycle tests pass.
- `rtk git diff --check` -- No whitespace errors.

**Manual checks:**
- On Windows, macOS, and Linux installed builds, click the tray item several times during splash, main, and minimized states; verify a single UI process and expected window activation. Close the UI fully and verify tray reopen keeps the same daemon PID. Include rapid clicks and one direct OS launch while UI is open.

## Suggested Review Order

**Launch and ownership**

- Tray requests activation before launching, then avoids redundant development runners.
  [main.rs:1204](../../hifimule-daemon/src/main.rs#L1204)

- The profile lock chooses one UI before Tauri creates windows.
  [lib.rs:883](../../hifimule-ui/src-tauri/src/lib.rs#L883)

- Private runtime files carry activation requests and acknowledgment across processes.
  [lib.rs:569](../../hifimule-lifecycle/src/lib.rs#L569)

**Window activation and handoff**

- The surviving UI polls requests and retries until a window is ready.
  [lib.rs:838](../../hifimule-ui/src-tauri/src/lib.rs#L838)

- Main-thread activation restores the ready main window or startup splash.
  [lib.rs:797](../../hifimule-ui/src-tauri/src/lib.rs#L797)

- A launch during UI exit can take ownership after the old process releases its lock.
  [lib.rs:647](../../hifimule-lifecycle/src/lib.rs#L647)

**Verification**

- Cross-process tests check mailbox delivery, acknowledgment, and lock takeover.
  [contract.rs:42](../../hifimule-lifecycle/tests/contract.rs#L42)

- Installed smoke checks expect one surviving UI and wait for the second macOS app.
  [smoke-macos.sh:87](../../scripts/smoke-tests/smoke-macos.sh#L87)

**Local implementation checks (2026-09-26):** `rtk cargo check -p hifimule-ui`, `rtk cargo test -p hifimule-ui --lib` (10 passed), `rtk cargo clippy -p hifimule-ui --lib -- -D warnings`, `rtk npm run build:daemon -- check -p hifimule-daemon`, `rtk cargo fmt --all -- --check`, and `rtk git diff --check` passed. The daemon build uses its required FFmpeg verification wrapper. Installed Windows, macOS, and Linux activation behavior has not yet been exercised; OS foreground policies can still decline a focus request.

**Review remediation checks (2026-09-26):** `rtk cargo test -p hifimule-lifecycle` passed 14 tests with local loopback access; the unprivileged run was blocked by the sandbox when an existing health test bound `127.0.0.1`. UI check and 10 UI library tests passed. UI/lifecycle Clippy with warnings denied, daemon check via its FFmpeg verification wrapper, Bash syntax, PowerShell parser, Rust formatting, and diff whitespace checks passed. Installed platform behavior is still unverified.

**Mailbox implementation checks (2026-09-26):** `rtk cargo test -p hifimule-lifecycle` passed 16 tests with local loopback access, including cross-process UI lock/request delivery and simultaneous mailbox writes. UI check and 10 UI library tests passed. Daemon check via its FFmpeg verification wrapper, UI/lifecycle Clippy with warnings denied, Bash syntax, PowerShell parser, Rust formatting, and diff whitespace checks passed. Installed Windows, macOS, and Linux activation behavior is pending.

**Review loop 3 checks (2026-09-26):** `rtk cargo test -p hifimule-lifecycle` passed 18 tests with local loopback access, including cross-process acknowledgment and takeover. `rtk cargo test -p hifimule-ui --lib` passed 10 tests. UI check, daemon check via the FFmpeg verification wrapper, UI/lifecycle Clippy with warnings denied, Bash syntax, PowerShell parser, formatting, and diff whitespace checks passed. Installed platform activation remains unverified.
