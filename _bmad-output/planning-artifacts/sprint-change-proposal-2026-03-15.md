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
