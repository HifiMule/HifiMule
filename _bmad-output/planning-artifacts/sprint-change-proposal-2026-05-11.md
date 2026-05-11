# Sprint Change Proposal — macOS Daemon Startup (launchd Agent)

**Date:** 2026-05-11
**Scope:** Minor
**Route to:** Developer agent for direct implementation

---

## 1. Issue Summary

On Windows, the MSI installer registers `hifimule-daemon.exe` as a startup application via `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (`startup-fragment.wxs`). This means the daemon starts automatically at user login, and auto-sync fires when a known device is connected even if the UI has never been opened.

On macOS, no equivalent mechanism exists. The daemon only runs when the UI is open and spawns it as a Tauri sidecar. Auto-sync on device-connect is therefore unavailable on macOS unless the user has the app running — breaking the cross-platform parity promise from the PRD.

This was a documented "Post-MVP" item inside Story 6.3 (macOS DMG installer) that was deferred and never picked up.

---

## 2. Impact Analysis

**Epic Impact:**
- Epic 6 (Packaging & Distribution) requires one new story (6.7). All other epics are unaffected.
- No completed stories need to be modified or rolled back.

**Story Impact:**
- New story 6.7 added to Epic 6 (backlog).
- Story 6.3 acceptance criteria remain valid as-written; 6.7 is additive scope.

**Artifact Conflicts:**
- **PRD FR21:** "Launch on Startup" toggling via launchd is stated but not yet delivered on macOS. Story 6.7 closes this gap.
- **PRD Business Success — Cross-Platform Parity:** Auto-sync without opening the app is a Windows-only feature today. Story 6.7 restores parity.
- **Architecture doc:** Missing documentation of the Windows startup application mechanism and the planned macOS launchd pattern. Updated in this proposal.
- **`hifimule-ui/src-tauri/src/lib.rs`:** Needs a macOS-specific `try_start_launchd_agent()` step and plist install logic (no behavioral change to Windows or Linux paths).
- **`sprint-status.yaml`:** New story entry added.

**Technical Impact:**
- Windows: zero changes.
- Linux: zero changes.
- macOS `lib.rs`: add plist write + `launchctl load` on first launch; add `#[cfg(target_os = "macos")]`-gated `settings.setLaunchOnStartup` RPC handler.
- The existing `RunEvent::Exit` kill block already only fires when `DaemonProcess` holds a live child — a launchd-owned daemon is never stored there, so no change is needed.

---

## 3. Recommended Approach

**Direct Adjustment** — add Story 6.7 within Epic 6.

No rollback required. MVP scope is unaffected (this has always been Post-MVP). Risk is low because all changes are `#[cfg(target_os = "macos")]`-gated and the Windows and Linux paths are completely untouched.

Effort estimate: **Low-Medium** (plist template, launchctl wiring, RPC handler, first-launch detection).

---

## 4. Detailed Change Proposals

### Change 1 — New Story 6.7: macOS Daemon as launchd User Agent (approved)

Added to `epics.md` after Story 6.6.

```
### Story 6.7: macOS Daemon as launchd User Agent

As a Convenience Seeker (Sarah),
I want the HifiMule daemon to start automatically when I log in on macOS,
So that auto-sync fires when I connect my device even if I haven't opened the app.

**Acceptance Criteria:**

**Given** HifiMule is installed to /Applications on macOS
**When** the UI is launched for the first time (or after an upgrade where the
  plist is absent)
**Then** the UI writes a launchd user agent `.plist` to
  `~/Library/LaunchAgents/com.hifimule.daemon.plist`, templated with the
  resolved absolute path to the bundled daemon binary.
**And** the UI runs `launchctl load ~/Library/LaunchAgents/com.hifimule.daemon.plist`
  so the agent is active immediately.
**And** subsequent user logins start the daemon automatically with no UI
  interaction required.

**Given** the UI launches and the daemon is already running (started by launchd)
**When** the UI performs its health-check on port 19140
**Then** the UI attaches to the running daemon (status = "startup") without
  spawning a sidecar.
**And** when the UI window is closed or exits, the daemon is NOT killed
  (only a sidecar-spawned child held in `DaemonProcess` is killed on exit).

**Given** the user toggles "Launch on Startup" OFF in Settings
**When** the UI calls the `settings.setLaunchOnStartup(false)` RPC
**Then** the UI backend runs `launchctl unload` on the plist.
**And** the daemon continues running for the current session.
**When** the user toggles "Launch on Startup" ON
**Then** the UI backend reinstalls and `launchctl load`s the plist.

**Technical Notes:**
- `.plist` template is embedded in `lib.rs` as a string constant; the
  `{DAEMON_PATH}` placeholder is replaced at runtime with the path found
  by scanning `current_exe().parent()` for an entry whose name starts with
  `"hifimule-daemon-"` (same scan used for quarantine clearance in
  spec-fix-macos-daemon-launch).
- LaunchAgents dir is created with `create_dir_all` if absent.
- `launchctl` invocations use `std::process::Command` — no elevated privileges.
- `RunEvent::Exit` kill block in `lib.rs`: already gated on
  `daemon_proc.take()` returning `Some` — no change needed; a launchd-owned
  daemon was never stored in `DaemonProcess`, so it won't be killed.
- New RPC `settings.setLaunchOnStartup(enabled: bool)` handled in the UI
  Rust backend (not the daemon), gated `#[cfg(target_os = "macos")]`.
- `service.rs` and Windows logic are untouched.

**Status:** backlog
```

---

### Change 2 — Architecture Document: Daemon Lifecycle Section (approved)

Add four bullet points to the "Packaging & Distribution" section of `architecture.md`:

```
- **Daemon Lifecycle — Windows:** The WiX installer registers
  `hifimule-daemon.exe` as a startup application via
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
  (`startup-fragment.wxs`). The daemon starts in the user's interactive
  session at login, giving it full access to the system tray and OS keyring.
  The UI health-checks port 19140 on launch; if the daemon is already running,
  no sidecar is spawned and the exit handler does not kill it.
- **Daemon Lifecycle — macOS:** On first launch, the UI writes a launchd
  user agent `.plist` to `~/Library/LaunchAgents/com.hifimule.daemon.plist`
  and loads it via `launchctl load`. The daemon then starts automatically at
  each login in the user's session. The UI health-checks port 19140; if
  already running (launchd-owned), no sidecar is spawned and the exit handler
  does not kill the process. The UI RPC `settings.setLaunchOnStartup(bool)`
  calls `launchctl load/unload` to toggle the agent.
- **Daemon Lifecycle — Linux:** Sidecar model only. systemd user-unit support
  is deferred (Story 6.4 note preserved).
- **Note — Windows Service:** `service.rs` contains Windows Service scaffolding
  (`--install-service` / `--service` flags) but is not used by the production
  installer. The startup application model is sufficient and keeps the daemon
  in the user session where tray and keyring access work correctly.
```

---

### Change 3 — Epics Document: Story 6.7 (approved, covered by Change 1 above)

No separate entry needed — the full story text is in Change 1.

---

## 5. Implementation Handoff

**Scope classification:** Minor

**Route to:** Developer agent for direct implementation.

**Files to modify:**
- `hifimule-ui/src-tauri/src/lib.rs` — plist write, `launchctl load`, `try_start_launchd_agent()`, `settings.setLaunchOnStartup` RPC handler (all `#[cfg(target_os = "macos")]`)
- `_bmad-output/planning-artifacts/epics.md` — add Story 6.7
- `_bmad-output/planning-artifacts/architecture.md` — add Daemon Lifecycle bullets
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — add story 6.7 entry

**Success criteria:**
- On a fresh macOS install, first UI launch creates `~/Library/LaunchAgents/com.hifimule.daemon.plist` and loads the agent.
- On subsequent UI launches, health-check passes before any sidecar spawn is attempted.
- Plugging in a known auto-sync device with the UI closed triggers a sync and sends an OS notification.
- "Launch on Startup" toggle in Settings correctly loads/unloads the plist.
- Windows and Linux build and behavior are completely unchanged.
