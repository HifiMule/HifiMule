# Sprint Change Proposal — Add Epic 6: Packaging & Distribution

**Date:** 2026-03-08
**Author:** Bob (Scrum Master)
**Requested By:** Alexis
**Status:** Pending Approval

---

## Section 1: Issue Summary

**Problem Statement:** The current 5-epic plan for HifiMule covers all application features (workspace foundation, connection, library browsing, sync engine, scrobbling) but contains no coverage for how the finished application is packaged into distributable installers and delivered to users across Windows, Linux, and macOS.

**Context:** Identified during epic plan review. Without a packaging epic, the application can be built from source but cannot be installed by end users — a fundamental gap for a desktop application targeting cross-platform parity (PRD: NFR8).

**Evidence:**
- No existing story in Epics 1–5 references `cargo tauri build`, installer creation, or CI/CD release pipelines
- PRD mandates "100% feature parity between Windows, Linux, and macOS" (NFR8) and "macOS sandbox compliance" (NFR9) — both require explicit packaging work
- Architecture specifies Tauri v2 + Rust Workspace with sidecar pattern, which has specific bundling configuration requirements per platform

---

## Section 2: Impact Analysis

### Epic Impact
- **Epics 1–5:** No modifications required. All existing stories remain valid and unchanged.
- **New Epic 6:** Added as the final epic. Depends on Epic 1 (workspace must exist) but can be worked on in parallel with later epics.
- **No resequencing** of existing epics needed.

### Story Impact
- No current stories require changes.
- 6 new stories added under Epic 6 (see Section 4).

### Artifact Conflicts
| Artifact | Impact | Action |
|----------|--------|--------|
| PRD | Minor | Add FR27 (platform-native installers) and FR28 (CI/CD pipeline) |
| Architecture | Minor | Add Packaging & Distribution subsection to Structure Patterns |
| Epics | Additive | Add Epic 6 with 6 stories, update FR Coverage Map |
| UX Design | None | No changes needed |

### Technical Impact
- No code changes required — this is a planning/documentation change
- Future implementation will involve Tauri bundler configuration, GitHub Actions workflows, and platform-specific installer testing

---

## Section 3: Recommended Approach

**Selected Path:** Hybrid (Direct Adjustment + New Epic)

**Rationale:**
1. Packaging is a distinct domain that warrants its own epic rather than being shoehorned into Epic 1 (Foundation)
2. The scope (3 platform installers + CI/CD + smoke tests) justifies 6 stories
3. Purely additive — zero disruption to existing plan, no rollback, no MVP scope reduction
4. Epic 6 sequenced last matches natural development flow (features → packaging)

**Effort Estimate:** Low — documentation changes only; implementation effort is contained within the new epic's stories
**Risk Level:** Low — no existing work is modified
**Timeline Impact:** None on current sprint; Epic 6 work begins after feature epics or in parallel

---

## Section 4: Detailed Change Proposals

### 4.1 PRD Changes

**File:** `prd.md`
**Section:** Functional Requirements → after Section 6

**Add:**
```
### 7. Packaging & Distribution
- **FR27:** The system can be packaged into platform-native installers (MSI for Windows, DMG for macOS, AppImage/.deb for Linux) using the Tauri v2 bundler.
- **FR28:** The build pipeline can produce signed, distributable artifacts for all three target platforms from a single CI workflow.
```

### 4.2 Architecture Changes

**File:** `architecture.md`
**Section:** Implementation Patterns → Structure Patterns (append after Project Organization)

**Add:**
```
**Packaging & Distribution:**
- **Bundler:** Tauri v2 built-in bundler for platform-native installers (MSI, DMG, AppImage/.deb).
- **Daemon Bundling:** The `hifimule-daemon` binary is included as a Tauri sidecar, bundled alongside the UI.
- **CI/CD:** GitHub Actions matrix build targeting Windows, Linux, and macOS with artifact upload to GitHub Releases.
- **Code Signing:** Platform-specific signing (Windows Authenticode, macOS notarization) deferred to post-MVP unless required for distribution.
```

### 4.3 Epics Changes

**File:** `epics.md`

**FR Coverage Map — Append:**
```
FR27: Epic 6 - Platform-Native Installer Bundling
FR28: Epic 6 - CI/CD Cross-Platform Build Pipeline
```

**New Epic — Append after Epic 5:**

#### Epic 6: Packaging & Distribution

Package HifiMule into platform-native installers and establish automated cross-platform build pipelines.

**Story 6.1: Tauri Bundler Configuration & Sidecar Packaging**
- Configure Tauri bundler to include `hifimule-daemon` as sidecar
- Single installer delivers both UI and headless engine
- Application icon, name, and metadata correctly embedded

**Story 6.2: Windows Installer (MSI)**
- Standard MSI installer with Start Menu shortcuts
- Daemon sidecar placed alongside main executable
- Clean uninstallation via Add/Remove Programs

**Story 6.3: macOS Installer (DMG)**
- DMG with drag-to-Applications install
- Runs without root/sudo (NFR9 compliance)
- Daemon sidecar embedded within .app bundle

**Story 6.4: Linux Packages (AppImage & .deb)**
- AppImage for universal Linux distribution
- .deb package with desktop entry for Debian-based systems
- Both formats include daemon sidecar

**Story 6.5: CI/CD Cross-Platform Build Pipeline**
- GitHub Actions workflow triggered on tagged releases
- Parallel builds on Windows, macOS, and Linux runners
- Artifacts uploaded to GitHub Release draft

**Story 6.6: Installation Smoke Tests**
- Install → launch → verify daemon health-check → uninstall
- Per-platform validation
- Clear diagnostic output on failure

---

## Section 5: Implementation Handoff

### Change Scope Classification: Minor

This is a purely additive documentation change that adds a new epic. No existing work is affected.

### Handoff Plan
| Role | Responsibility |
|------|---------------|
| Product Manager (John) | Apply PRD edits (FR27, FR28) |
| Scrum Master (Bob) | Update epics.md with Epic 6 and FR Coverage Map; update sprint-status.yaml |
| Architect (Winston) | Apply architecture.md packaging section |
| Dev Team (Amelia) | Implement Epic 6 stories when sprint-planned |

### Success Criteria
- All three artifact files (PRD, Architecture, Epics) updated with approved changes
- Epic 6 appears in sprint-status.yaml with status "backlog"
- No regression in existing epic definitions
