# Sprint Change Proposal — 2026-03-15

## Section 1: Issue Summary

### Change A: Release Mode Communication Fix
**Problem:** Tauri v2 serves the frontend from `https://tauri.localhost` in release builds. Direct `fetch()` calls from the webview to `http://localhost:19140` (the daemon) were blocked by browser mixed-content and CORS policies, making the application non-functional in production.

**Discovery:** Identified during Story 6.1 (Tauri Bundler / Sidecar Packaging) when testing release builds.

**Resolution (already implemented):**
- All RPC calls now route through Tauri `invoke('rpc_proxy')` in the UI Rust backend
- Image fetches route through `invoke('image_proxy')`, returning base64 data URLs
- Daemon CORS config updated to allow `https://tauri.localhost` origin
- File-based logging added to both daemon (`daemon.log`) and UI (`ui.log`) for release mode diagnostics

### Change B: Daemon as OS-Native Service (Post-MVP)
**Problem:** The current sidecar model ties the daemon lifecycle to the UI — the daemon only runs while the UI is open and is killed on exit. For a sync tool that should respond to device connections at any time, this is limiting.

**Requirement:** `jellyfinsync-daemon` should be installable as an OS-native background service (Windows Service, systemd user unit, launchd agent) so it runs independently of the UI, surviving reboots and user logoffs. The UI should connect to the running service rather than spawning a process.

## Section 2: Impact Analysis

### Epic Impact

| Epic | Change A Impact | Change B Impact |
|------|----------------|----------------|
| Epic 1 (Foundation) | None — sidecar model unchanged | Future: UI connects to service instead of spawning sidecar |
| Epic 2 (Connection) | None — daemon RPC port unchanged | None |
| Epic 3 (Curation Hub) | None — proxy is transparent to UI components | None |
| Epic 4 (Sync Engine) | None | None |
| Epic 5 (Ecosystem) | None | Minor — always-running service improves device detection |
| Epic 6 (Packaging) | None — fix enables release builds to work | Stories 6.2–6.4 gain post-MVP service registration criteria |

### Artifact Conflicts

| Artifact | Updates Needed |
|----------|---------------|
| **Architecture doc** | Added: Release Mode Proxy pattern, Tauri Commands, Logging & Diagnostics |
| **Integration Architecture doc** | Updated: RPC and Image Proxy sections with dev vs release paths |
| **PRD** | Updated: FR20 and FR21 clarified with MVP (sidecar) vs Post-MVP (service) |
| **Epics** | Updated: Stories 6.2, 6.3, 6.4 with per-platform Post-MVP service criteria |
| **UX Design** | No changes applied (minor splash screen text change deferred to implementation) |

### Technical Impact
- Change A: No further code changes needed — fix is complete
- Change B: Future implementation requires platform abstraction for service management in the UI Rust backend

## Section 3: Recommended Approach

**Selected Path: Direct Adjustment (Hybrid)**

- **Change A:** Documentation-only updates to reflect the already-implemented proxy pattern and logging
- **Change B:** New post-MVP acceptance criteria added to existing packaging stories (6.2–6.4)

**Rationale:**
- Zero risk to current timeline — Change A is done, Change B is future planning
- No rework of completed stories — existing MVP acceptance criteria unchanged
- Service requirement properly captured per-platform for future implementation
- Sidecar model remains the MVP approach; service model is an additive enhancement

**Effort:** Low | **Risk:** Low | **Timeline Impact:** None

## Section 4: Detailed Change Proposals

### Architecture Document (3 edits)
1. **API & Communication Patterns:** Added Release Mode Proxy description
2. **Frontend Architecture:** Added Tauri Commands documentation
3. **New section — Logging & Diagnostics:** Documented file-based logging for release builds

### Integration Architecture Document (2 edits)
4. **UI → Daemon RPC section:** Updated source paths for dev vs release, added release mode note
5. **Image Proxy section:** Updated direction to show Tauri invoke chain, added base64 note

### Epics Document (3 edits)
6. **Story 6.2 (Windows MSI):** Added Post-MVP Windows Service registration criteria
6. **Story 6.3 (macOS DMG):** Added Post-MVP launchd agent criteria
6. **Story 6.4 (Linux Packages):** Added Post-MVP systemd user service criteria (AppImage falls back to sidecar)

### PRD (1 edit)
7. **FR20 & FR21:** Clarified MVP (sidecar) vs Post-MVP (OS-native service) approach

## Section 5: Implementation Handoff

**Change Scope: Minor**

All documentation updates have been applied directly. No further action needed for Change A. Change B is captured as post-MVP criteria in the packaging stories — implementation will be triggered when those stories are prioritized.

**Deliverables produced:**
- [x] Architecture doc updated (3 sections)
- [x] Integration architecture doc updated (2 sections)
- [x] Epics updated (3 stories)
- [x] PRD updated (2 requirements)
- [x] Sprint Change Proposal document (this file)

**Next steps:**
- Development team continues current sprint with no disruption
- Post-MVP service stories will be planned when Epic 6 packaging work begins

---

## Change C: Windows Daemon — Service → Startup Application

### Section 1: Issue Summary

**Problem:** Change B (above) added Post-MVP criteria for registering `jellyfinsync-daemon` as a **Windows Service** in Story 6.2. However, a Windows Service runs in Session 0 — an isolated, non-interactive session — which **cannot**:

1. Display a system tray icon (FR22) — the `tray-icon` + `tao` event loop requires a user desktop session
2. Show OS-native desktop notifications (FR23) — `notify-rust` requires user session access
3. Launch or interact with the Tauri UI process

Since the daemon's architecture is fundamentally a **tray application** (its main loop is a `tao` event loop managing the tray icon), running it as a Windows Service would strip away core functionality.

**Discovery:** Identified during Epic 6 implementation review. The Post-MVP section was aspirational and has not been implemented in code.

**Evidence:**
- Daemon architecture uses `tray-icon` + `tao` crate for its main event loop (see `src/main.rs`)
- Windows Session 0 isolation is a documented OS constraint preventing services from displaying UI elements
- macOS (`launchd` agent) and Linux (`systemd --user`) Post-MVP sections are already correct — they run in user sessions

### Section 2: Impact Analysis

#### Epic Impact
- **Epic 6 (Packaging & Distribution):** Only affected epic. Currently `in-progress`.
- **Story 6.2 (Windows Installer):** Marked `done` — Post-MVP section needs correction. No code changes.
- **All other epics (1-5):** No impact. All completed.
- **Stories 6.3-6.6:** No impact. macOS and Linux already use user-session daemon models.

#### Artifact Conflicts

| Artifact | Section | Conflict | Resolution |
|----------|---------|----------|------------|
| PRD | FR20 | Says "Windows Service" | Change to "Windows startup application" |
| PRD | FR21 | Implies service enable/disable for all platforms | Specify per-platform mechanisms |
| Epics | Story 6.2 Post-MVP | Full section describes Windows Service workflow | Rewrite for startup application (Registry Run key) |
| Architecture | — | No conflict (already tray-based) | None needed |
| UX Design | — | No conflict | None needed |

#### Technical Impact
- **Code:** No changes required. The `install-service` CLI command remains as an optional power-user feature.
- **Deployment:** When Post-MVP is implemented, the MSI installer will register a Registry `Run` key instead of a Windows Service — simpler to implement.

### Section 3: Recommended Approach

**Selected Path: Direct Adjustment**

Update three documentation sections in planning artifacts to replace the Windows Service model with a startup application model.

**Rationale:**
- No code has been written for the Windows Service Post-MVP feature — purely a documentation correction
- Startup application model is the **correct** architectural fit for a tray-icon-based daemon
- Aligns docs with the existing design intent (tray icon, notifications, UI interaction)
- macOS and Linux sections already correct and serve as the pattern to follow
- Zero timeline impact, zero risk to existing functionality

**Effort:** Low | **Risk:** Low | **Timeline Impact:** None

### Section 4: Detailed Change Proposals

#### Change C.1: PRD — FR20

**File:** `_bmad-output/planning-artifacts/prd.md`

**OLD:**
> FR20: The system can run as a background service (headless) with minimal resource usage. MVP: Tauri sidecar process. Post-MVP: OS-native service (Windows Service, systemd user unit, launchd agent).

**NEW:**
> FR20: The system can run as a background service (headless) with minimal resource usage. MVP: Tauri sidecar process. Post-MVP: OS-native user-session daemon (Windows startup application, systemd user unit, launchd agent).

#### Change C.2: PRD — FR21

**File:** `_bmad-output/planning-artifacts/prd.md`

**OLD:**
> FR21: Users can toggle "Launch on Startup" behavior. Post-MVP: Fulfilled natively by OS service enable/disable rather than a startup shortcut.

**NEW:**
> FR21: Users can toggle "Launch on Startup" behavior. Post-MVP: Fulfilled natively by platform-specific mechanisms (Windows Registry Run key, systemd user unit enable/disable, launchd agent load/unload).

#### Change C.3: Epics — Story 6.2 Post-MVP

**File:** `_bmad-output/planning-artifacts/epics.md`

**OLD:**
> **Post-MVP: Daemon as Windows Service**
> Given the MSI installation completes / When the service registration step runs / Then `jellyfinsync-daemon` is registered as a Windows Service (via `sc.exe` or NSSM). And the service is configured to start automatically on user login. And the UI detects the running service via a health-check RPC call instead of spawning a sidecar. And if the service is not running, the UI attempts to start it via `sc start`. And uninstallation removes the service registration.

**NEW:**
> **Post-MVP: Daemon as Windows Startup Application**
> Given the MSI installation completes / When the installer registers the startup entry / Then `jellyfinsync-daemon` is registered as a startup application via a Registry `Run` key (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`). And the daemon launches automatically when the user logs in, running in the user session with full tray icon and notification support. And the UI detects the running daemon via a health-check RPC call instead of spawning a sidecar. And if the daemon is not running, the UI attempts to launch it directly. And uninstallation removes the Registry `Run` entry.

### Section 5: Implementation Handoff

**Change Scope: Minor**

Documentation-only change to planning artifacts. Can be implemented immediately by Alexis (solo developer).

**Action items:**
- [x] Apply Change C.1 — Update FR20 in PRD
- [x] Apply Change C.2 — Update FR21 in PRD
- [x] Apply Change C.3 — Rewrite Story 6.2 Post-MVP section in Epics
- [x] Verify no remaining references to "Windows Service" as default deployment model

**Note:** The `install-service` CLI command in daemon code is retained as-is for power users who want headless operation without tray.
