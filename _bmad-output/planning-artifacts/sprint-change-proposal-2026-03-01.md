# Sprint Change Proposal — JellyfinSync
**Date:** 2026-03-01
**Author:** Alexis
**Scope Classification:** Minor — Direct implementation by development team

---

## Section 1: Issue Summary

### Problem Statement
All five epics (1–5) have been completed, delivering a fully functional JellyfinSync application. However, a gap was identified in the device connection flow: no story covers the **initialization of a new removable disk** that has never been used with JellyfinSync. The application can detect a device (Story 2.2), recognize a known managed device (Story 2.3), and operate against an existing manifest (Epics 3–5), but it has no path for a device that arrives without a `.jellyfinsync.json` manifest.

### Discovery Context
Gap identified by Alexis during a post-sprint review of the completed backlog. The "unrecognized device" state was implied by the manifest-check logic in Story 2.2 but never given its own user-facing story or implementation target.

### Evidence
- **FR3 (PRD):** *"The system can identify the presence of a `.jellyfinsync.json` manifest on discovery."* — Covers detection only; prescribes no action when absent.
- **Story 2.2 AC:** *"it checks for the presence of a `.jellyfinsync.json` manifest in the root directory"* — No branch defined for "manifest not found."
- **Story 2.3 AC:** *"Given a known device (has `.jellyfinsync.json` with a unique ID)"* — Explicitly assumes the manifest already exists.
- **PRD MVP scope:** *"Conflict-Free Manifest Sync: Implementation of the `.jellyfinsync.json` logic for managed-folder isolation"* — Creating the initial manifest is a prerequisite for the managed model but was never explicitly scoped as a story.

---

## Section 2: Impact Analysis

### Epic Impact
| Epic | Impact |
|------|--------|
| Epic 2 — Connection & Verification (The Handshake) | **Affected** — Story 2.6 added. Epic status reverts to `in-progress` until 2.6 is `done`. |
| Epics 1, 3, 4, 5 | Not affected. All stories remain `done`. |

### Story Impact
| Story | Impact |
|-------|--------|
| Stories 2.1–2.5 | Not affected. All `done`. |
| **Story 2.6 (new)** | Added to Epic 2 backlog. |

### Artifact Conflicts
| Artifact | Conflict | Action Required |
|----------|----------|-----------------|
| `prd.md` | FR3 covers detection but not creation; no FR for initialization | Add FR26 |
| `epics.md` | Story 2.6 missing from Epic 2 | Add Story 2.6 |
| `sprint-status.yaml` | Epic 2 shows `done`; Story 2.6 entry missing | Reopen Epic 2, add 2.6 entry |
| `architecture.md` | None — atomic Write-Temp-Rename pattern already covers initial creation | None |
| `ux-design-specification.md` | None — new "unrecognized device" state fits within the established Device State panel pattern | None |

### Technical Impact
Low. The implementation follows fully established patterns:
- New `on_device_unrecognized` daemon event (same broadcast pattern as existing device events)
- New `device.initialize` RPC method in `rpc.rs` (same JSON-RPC 2.0 envelope pattern)
- Initial `.jellyfinsync.json` written using the existing Write-Temp-Rename atomic pattern
- New "Initialize Device" banner in `BasketSidebar.ts` (same pattern as dirty manifest banner from Story 5.4)
- New initialization dialog using Shoelace `<sl-dialog>` (same approach as `RepairModal.ts`)

---

## Section 3: Recommended Approach

**Selected Path:** Option 1 — Direct Adjustment

Add Story 2.6 to Epic 2 within the existing sprint plan. Reopen Epic 2 to `in-progress`.

**Rationale:**
- The story is a clean additive change with zero impact on completed work.
- All surrounding infrastructure (device detection, RPC layer, manifest schema, UI device state panel) is already in place — this story only fills the "unrecognized → managed" transition gap.
- The Write-Temp-Rename atomic pattern and Shoelace dialog pattern are fully documented and proven by prior stories.
- Reopening Epic 2 for one story is preferable to creating a new epic for a single story.

**Effort:** Low
**Risk:** Low
**Timeline Impact:** Minimal — one additional story in Epic 2.

---

## Section 4: Detailed Change Proposals

### Change 4A — Add FR26 to `prd.md`

**File:** `_bmad-output/planning-artifacts/prd.md`
**Location:** Functional Requirements → 1. Device Connection & Discovery, after FR4

**OLD:**
```
- FR3: The system can identify the presence of a `.jellyfinsync.json` manifest on discovery.
- FR4: The system can read persistent hardware identifiers to link devices across different sessions.
```

**NEW:**
```
- FR3: The system can identify the presence of a `.jellyfinsync.json` manifest on discovery.
- FR4: The system can read persistent hardware identifiers to link devices across different sessions.
- FR26: The system can initialize a new `.jellyfinsync.json` manifest on a connected device
  that has not previously been managed, capturing a hardware identifier, a designated
  sync folder path, and an associated Jellyfin user profile.
```

**Rationale:** FR3 covers detection of an existing manifest but prescribes no action when absent. FR26 closes the "unrecognized device" gap — without it, new devices can be detected but never onboarded into the managed sync model.

---

### Change 4B — Add Story 2.6 to `epics.md`

**File:** `_bmad-output/planning-artifacts/epics.md`
**Location:** Epic 2, after Story 2.5

**OLD:** *(Story 2.5 is the final story in Epic 2)*

**NEW — Add after Story 2.5:**

```markdown
### Story 2.6: Initialize New Device Manifest

As a Ritualist (Arthur) and Convenience Seeker (Sarah),
I want the application to detect when a connected removable disk has no
`.jellyfinsync.json` manifest and guide me through initializing it,
So that I can bring a brand-new device into the managed sync model without
manually creating any files.

**Acceptance Criteria:**

**Given** a USB mass storage device is connected with no `.jellyfinsync.json`
present in its root
**When** the daemon completes its device discovery scan
**Then** it broadcasts an `on_device_unrecognized` event to the UI.
**And** the UI displays an "Initialize Device" banner in the Device State panel.

**Given** the "Initialize Device" banner is visible
**When** I click "Initialize"
**Then** a dialog prompts me to confirm or change the target sync folder path
on the device (defaulting to the device root).
**And** I can select the associated Jellyfin user profile for this device.
**When** I click "Confirm"
**Then** the UI sends a `device.initialize` JSON-RPC request to the daemon
with the chosen folder path and profile ID.
**And** the daemon writes an initial `.jellyfinsync.json` to the device using
the atomic Write-Temp-Rename pattern, containing a new unique hardware ID
and the selected profile.
**And** the daemon broadcasts an updated device state marking the device as "Managed".
**And** the UI transitions to the normal sync-ready state.

**When** the initialization fails (e.g., device is read-only or disk full)
**Then** the UI displays a clear error message with a "Retry" or "Dismiss" option.
```

**Rationale:** Closes the "unrecognized device" gap in Epic 2. Without this story, new devices can be detected (Story 2.2) but never onboarded; the sync flow (Epic 4) has no valid manifest to operate against.

---

### Change 4C — Update `sprint-status.yaml`

**File:** `_bmad-output/implementation-artifacts/sprint-status.yaml`
**Location:** `development_status` → `epic-2` block

**OLD:**
```yaml
  epic-2: done
  2-1-secure-jellyfin-server-link: done
  2-2-mass-storage-heartbeat-autodetection: done
  2-3-multi-device-profile-mapping: done
  2-4-startup-splash-screen-with-connection-status: done
  2-5-interactive-login-and-identity-management: done
  epic-2-retrospective: done
```

**NEW:**
```yaml
  epic-2: in-progress
  2-1-secure-jellyfin-server-link: done
  2-2-mass-storage-heartbeat-autodetection: done
  2-3-multi-device-profile-mapping: done
  2-4-startup-splash-screen-with-connection-status: done
  2-5-interactive-login-and-identity-management: done
  2-6-initialize-new-device-manifest: backlog
  epic-2-retrospective: optional
```

**Rationale:** Epic 2 must reopen to `in-progress` since Story 2.6 is not yet done. The retrospective reverts to `optional` since it was run before this story existed and should be revisited after 2.6 completes.

---

## Section 5: Implementation Handoff

**Scope Classification:** Minor — Direct implementation by development team.

| Role | Responsibility |
|------|---------------|
| Scrum Master (Bob) | Run `/bmad-bmm-create-story` for Story 2.6 to produce the dev-ready story file |
| Developer (Amelia) | Implement Story 2.6 via `/bmad-bmm-dev-story` |
| Developer (Amelia) | Run `/bmad-bmm-code-review` before marking done |

**Success Criteria:**
- Story 2.6 file created in `_bmad-output/implementation-artifacts/`
- `on_device_unrecognized` daemon event broadcast when no manifest found on device detection
- `device.initialize` RPC method writes an atomic initial `.jellyfinsync.json` (Write-Temp-Rename pattern)
- "Initialize Device" banner visible in `BasketSidebar.ts` when unrecognized device connected
- Initialization dialog lets user confirm sync folder and select Jellyfin profile
- Device transitions to "Managed" state after successful initialization
- Error states handled (read-only device, disk full) with clear UI messaging
- Story 2.6 status reaches `done` in `sprint-status.yaml`
- Epic 2 status updated to `done` after Story 2.6 completion

**Next Workflow Steps:**
1. Apply artifact changes (Changes 4A, 4B, 4C above)
2. `/bmad-bmm-create-story` — Create Story 2.6 dev-ready file (Bob · 🏃 Scrum Master)
3. `/bmad-bmm-dev-story` — Implement (Amelia · 💻 Developer Agent)
4. `/bmad-bmm-code-review` — Review before marking done (Amelia · 💻 Developer Agent)
