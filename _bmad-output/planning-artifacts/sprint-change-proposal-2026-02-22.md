# Sprint Change Proposal — HifiMule
**Date:** 2026-02-22
**Author:** Alexis
**Scope Classification:** Minor — Direct implementation by development team

---

## Section 1: Issue Summary

### Problem Statement
Epic 4 ("The Sync Engine & Self-Healing Core") delivers a fully functional daemon-side sync engine across stories 4.1–4.4, covering differential manifest comparison, buffered IO streaming, legacy path validation, and dirty-manifest resume. However, no story captures the **UI-to-daemon IPC trigger** — the "Start Sync" button action that initiates the sync operation from the Sync Basket sidebar.

### Discovery Context
Gap identified by Alexis during post-Epic-4 review. All four existing stories address daemon internals; none define the frontend interaction that calls the sync engine.

### Evidence
- **PRD User Journey (Arthur):** *"He clicks 'Sync'."*
- **PRD User Journey (Sarah):** *"she clicks a single button: 'Update Running Mix'."*
- **Architecture — Communication Patterns:** *"The UI requests a 'Sync start'; the Daemon returns an immediate 'OK' and broadcasts progress via an `on_sync_progress` event stream."*
- **UX Journey Flow:** *"Sarah Clicks Commit → Daemon Performs Background IO → OS Bubble: Safe to Eject."*

---

## Section 2: Impact Analysis

### Epic Impact
| Epic | Impact |
|------|--------|
| Epic 4 — The Sync Engine & Self-Healing Core | **Affected** — Story 4.5 added. Epic remains `in-progress` until 4.5 is complete. |
| Epic 5 — Ecosystem Lifecycle & Advanced Tools | Not affected. Story 5.3 (Safe to Eject) is downstream and functions independently. |
| Epics 1–3 | Not affected. |

### Story Impact
| Story | Impact |
|-------|--------|
| Stories 4.1–4.4 | Not affected. All `done`. |
| **Story 4.5 (new)** | Added to Epic 4 backlog. |

### Artifact Conflicts
| Artifact | Conflict | Action Required |
|----------|----------|-----------------|
| PRD (`prd.md`) | None — FR12 implies a trigger mechanism | None |
| Architecture (`architecture.md`) | None — `sync.start` JSON-RPC pattern already documented | None |
| UX Design (`ux-design-specification.md`) | None — "Commit" button concept present in journey | None |
| `epics.md` | **Story 4.5 missing** | Add Story 4.5 |
| `sprint-status.yaml` | **Story 4.5 entry missing** | Add `4-5-start-sync-ui-to-engine-trigger: backlog` |

### Technical Impact
None. The IPC pattern (`sync.start` request → `{ jobId }` response → `on_sync_progress` event stream) is already defined in `architecture.md`. Implementation follows established patterns with no new architectural decisions required.

---

## Section 3: Recommended Approach

**Selected Path:** Option 1 — Direct Adjustment

Add Story 4.5 to Epic 4 within the existing sprint plan. No rollback or MVP scope reduction is warranted.

**Rationale:**
- The story is a clean additive change with no impact on completed work.
- The IPC contract is already documented — implementation risk is low.
- The architecture's Request-Response-Event pattern means this is a well-understood, bounded task.
- Blocking Epic 5 from moving forward without this story would leave the product with a sync engine that can only be triggered programmatically (no user-facing action).

**Effort:** Low
**Risk:** Low
**Timeline Impact:** Minimal — one additional story in Epic 4.

---

## Section 4: Detailed Change Proposals

### Change 4A — Add Story 4.5 to `epics.md`

**File:** `_bmad-output/planning-artifacts/epics.md`
**Location:** After Story 4.4, within Epic 4 section

**OLD:** *(Story 4.4 is the final story in Epic 4)*

**NEW — Add after Story 4.4:**

```markdown
### Story 4.5: "Start Sync" UI-to-Engine Trigger

As a Convenience Seeker (Sarah) and Ritualist (Arthur),
I want to click a "Start Sync" button in the Sync Basket sidebar
to initiate the synchronization process with the daemon,
So that I can execute my prepared sync selection and monitor
real-time progress without leaving the UI.

**Acceptance Criteria:**

**Given** the Sync Basket is populated with items and storage projection is within safe limits
**When** I click the "Start Sync" button
**Then** the UI sends a `sync.start` JSON-RPC request to the daemon, including the basket's item list (Jellyfin IDs) and target device path.
**And** the daemon responds immediately with `{ "status": "success", "data": { "jobId": "<uuid>" } }`.
**And** the "Start Sync" button transitions to a disabled "Syncing..." state with a Shoelace progress indicator.
**And** the UI subscribes to the `on_sync_progress` event stream and displays real-time progress (files completed, percentage, current filename).

**When** the sync completes successfully
**Then** the UI displays "Sync Complete" status.
**And** the Sync Basket clears and the button resets to its default enabled state.

**When** the daemon returns an error or the device disconnects mid-sync
**Then** the UI displays a clear error message.
**And** the daemon marks the manifest as "Dirty" (per Story 4.4 behaviour).
**And** the UI offers a "Retry" or "Dismiss" option.

**Technical Notes:**
- IPC pattern: JSON-RPC 2.0 · Request: `sync.start` · Response: `{ jobId }` · Events: `on_sync_progress`
- Follows the architecture's Request-Response-Event communication pattern
- Button must be disabled when: basket is empty, storage projection is Over Limit, or a sync is already in progress
- ARIA-live region required for progress updates (WCAG 2.1 AA)
```

**Rationale:** Completes Epic 4 by capturing the UI entry point into the daemon's sync engine. Documented in PRD user journeys, the architecture event pattern, and the UX journey flow — missed during initial story decomposition.

---

### Change 4B — Update `sprint-status.yaml`

**File:** `_bmad-output/implementation-artifacts/sprint-status.yaml`
**Location:** Under `epic-4` section, after `4-4-self-healing-dirty-manifest-resume: done`

**OLD:**
```yaml
  epic-4: in-progress
  4-1-differential-sync-algorithm-manifest-comparison: done
  4-2-atomic-buffered-io-streaming: done
  4-3-legacy-hardware-constraints-path-char-validation: done
  4-4-self-healing-dirty-manifest-resume: done
  epic-4-retrospective: optional
```

**NEW:**
```yaml
  epic-4: in-progress
  4-1-differential-sync-algorithm-manifest-comparison: done
  4-2-atomic-buffered-io-streaming: done
  4-3-legacy-hardware-constraints-path-char-validation: done
  4-4-self-healing-dirty-manifest-resume: done
  4-5-start-sync-ui-to-engine-trigger: backlog
  epic-4-retrospective: optional
```

**Rationale:** Keeps sprint tracking accurate; Epic 4 remains `in-progress` until Story 4.5 reaches `done`.

---

## Section 5: Implementation Handoff

**Scope Classification:** Minor — Direct implementation by development team.

| Role | Responsibility |
|------|---------------|
| Developer (Amelia) | Implement Story 4.5 via `/bmad-bmm-dev-story` |
| Scrum Master (Bob) | Run `/bmad-bmm-create-story` for Story 4.5 to produce the dev-ready story file |

**Success Criteria:**
- Story 4.5 file created in `_bmad-output/implementation-artifacts/`
- "Start Sync" button in Sync Basket sends `sync.start` RPC and displays progress
- `on_sync_progress` event stream subscribed and rendered in UI
- Button disabled states enforced (empty basket, over-limit projection, sync in progress)
- ARIA-live region implemented for progress updates
- Story 4.5 status reaches `done` in `sprint-status.yaml`

**Next Workflow Steps:**
1. `/bmad-bmm-create-story` — Create the Story 4.5 dev-ready file (Bob · 🏃 Scrum Master)
2. `/bmad-bmm-dev-story` — Implement (Amelia · 💻 Developer Agent)
3. `/bmad-bmm-code-review` — Review before marking done (Amelia · 💻 Developer Agent)
