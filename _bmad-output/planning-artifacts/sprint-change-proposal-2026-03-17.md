# Sprint Change Proposal — "Synchronise All" & Auto-Sync on Connect

**Date:** 2026-03-17
**Author:** Alexis (with SM agent)
**Status:** Approved (2026-03-17)

---

## 1. Issue Summary

**Problem Statement:** JellyfinSync currently only supports manual curation via the Selection Basket (browse → pick → sync). There is no automated "fill my device" mode that intelligently selects music based on user preferences, nor any way to trigger sync automatically when a known device is connected.

**Context:** Identified as a missing core workflow inspired by iTunes "Sync All" functionality. Two concrete use cases: filling a 1TB iPod with all favorites, and exporting loved songs to a Garmin watch. The feature maps directly to the "Sprinter" persona (Sarah) and the "Auto-Pilot Policy" concept already described in the PRD's Innovation section.

**Evidence:** iTunes has this as a proven feature. The PRD already conceptualized "Auto-Pilot Policy" as an innovation area but did not include it in MVP scope.

---

## 2. Impact Analysis

### Epic Impact

| Epic | Impact | Description |
|------|--------|-------------|
| Epic 2 | **Modify** | Story 2.3 expanded — device detection now triggers auto-sync for configured devices |
| Epic 3 | **Add** | New Story 3.6 — "Auto-Fill Sync Mode (Synchronise All)" |
| Epic 4 | **Modify** | Story 4.5 expanded — daemon-initiated sync path alongside UI-triggered sync |
| Epic 1 | None | Foundation unaffected |
| Epic 5 | None | Scrobbling unaffected |
| Epic 6 | None | Packaging unaffected |

### Artifact Conflicts

| Artifact | Impact | Details |
|----------|--------|---------|
| **PRD** | Add | FR29 (auto-fill algorithm), FR30 (auto-sync on connect), MVP scope update, Sarah's user journey rewrite |
| **Architecture** | Add | New daemon responsibilities, IPC methods (`basket.autoFill`, `sync.setAutoFill`), SQLite schema fields, manifest config block |
| **UX Design** | Add | Auto-Fill toggle, Max Fill Size slider, Auto badges, device profile auto-sync toggle, headless sync feedback |
| **Epics** | Add/Modify | FR coverage map update, Story 2.3 expansion, new Story 3.6, Story 4.5 expansion |

### Technical Impact

- **Daemon:** New auto-fill priority algorithm querying Jellyfin API (IsFavorite, PlayCount, DateCreated)
- **IPC:** Two new JSON-RPC methods
- **SQLite:** Three new device profile fields (`auto_fill_enabled`, `max_fill_bytes`, `auto_sync_on_connect`)
- **Manifest:** Optional `autoFill` config block in `.jellyfinsync.json`
- **No changes to:** technology stack, IPC protocol, proxy pattern, naming conventions, logging strategy, packaging

---

## 3. Recommended Approach

**Selected: Direct Adjustment** — Modify and add stories within the existing epic structure.

**Rationale:**
- Project is in planning phase — zero rework cost
- Changes are additive, not disruptive to existing epic/story structure
- Feature aligns with existing PRD vision (Auto-Pilot Policy)
- Low technical risk: no architectural changes, no technology changes
- Epic dependency chain (1→2→3→4→5→6) remains intact

**Effort Estimate:** Medium — 1 new story + 2 story expansions + artifact updates
**Risk Level:** Low
**Timeline Impact:** Minimal — adds scope but no structural replanning needed

---

## 4. Detailed Change Proposals

### 4.1 PRD Changes

#### 4.1.1 New Functional Requirements

**Section:** Functional Requirements > 3. Content Selection & Browsing
**Action:** Add after FR11

```
- **FR29:** The system can automatically select music to synchronize based on a
  priority algorithm (favorites first, then by play count, then by creation date)
  up to the device's available capacity or a user-defined size limit.
- **FR30:** The system can automatically trigger synchronization when a known,
  previously configured device is detected, without requiring user interaction.
```

#### 4.1.2 MVP Feature Set Update

**Section:** Product Scope > MVP - Minimum Viable Product
**Action:** Add after Profile Selection

```
- **Auto-Fill Sync Mode:** Intelligent device-filling that selects music by
  priority (favorites → play count → creation date) up to capacity or a
  user-defined limit. Can be mixed with manual basket selections.
- **Auto-Sync on Connect:** Known devices with auto-sync enabled trigger
  synchronization automatically on detection, requiring zero user interaction.
```

#### 4.1.3 User Journey Update

**Section:** User Journeys > Sarah's Pre-Run Dash

**Replace with:**
```
### Sarah's Pre-Run Dash (Speed Success)
*   **Narrative:** At 6:00 AM, Sarah plugs in her Garmin watch on her way out the
    door. The daemon recognizes the device, auto-syncs her favorites and most-played
    tracks to fill the watch, and a tray notification confirms "Sync Complete" before
    she's finished tying her shoes.
*   **Success Moment:** She unplugs and leaves. Zero clicks. The auto-fill prioritized
    her favorite running tracks and the tool handled everything silently in the background.
```

### 4.2 Epic & Story Changes

#### 4.2.1 FR Coverage Map Addition

**Action:** Add at end of FR Coverage Map

```
FR29: Epic 3 - Auto-Fill Priority Selection Algorithm
FR30: Epic 2 - Auto-Sync on Known Device Detection
```

#### 4.2.2 Story 2.3 Expansion — Multi-Device Profile Mapping & Auto-Sync Trigger

**Replace Story 2.3 with:**

```
### Story 2.3: Multi-Device Profile Mapping & Auto-Sync Trigger

As a Convenience Seeker (Sarah),
I want the tool to remember that my Garmin watch belongs to my "Running" Jellyfin
profile and automatically start syncing,
So that I can plug in and walk away without any interaction.

**Acceptance Criteria:**

**Given** a known device (has `.jellyfinsync.json` with a unique ID) is connected
**When** the daemon reads the ID
**Then** it automatically loads the associated Jellyfin User Profile and Sync Rules.

**Given** a known device with `auto_sync_on_connect` enabled in its profile
**When** the device is detected and profile is loaded
**Then** the daemon automatically initiates a sync operation (using auto-fill
selection or the last basket configuration).
**And** the tray icon transitions to "Syncing" state.
**And** no UI interaction is required.

**When** auto-sync completes
**Then** an OS-native notification is sent: "Sync Complete. Safe to eject."
**And** the tray icon returns to "Idle" state.
```

#### 4.2.3 New Story 3.6 — Auto-Fill Sync Mode (Synchronise All)

**Add after Story 3.5:**

```
### Story 3.6: Auto-Fill Sync Mode (Synchronise All)

As a Convenience Seeker (Sarah),
I want the basket to automatically fill with music from my entire library
prioritized by my favorites, most-played, and newest additions,
So that I can fill my device without manually browsing and selecting every album.

**Acceptance Criteria:**

**Given** the Basket sidebar is visible
**When** I enable the "Auto-Fill" toggle
**Then** the daemon queries the Jellyfin library and ranks all music tracks using
the priority algorithm: favorites first, then by play count (descending), then by
creation date (descending).
**And** the basket populates with tracks up to the device's available capacity or
a user-defined size limit.
**And** the Storage Projection bar updates in real-time.

**Given** Auto-Fill is enabled and I have manually added artists/playlists to the basket
**When** the auto-fill algorithm runs
**Then** manual selections take priority and occupy space first.
**And** auto-fill uses the remaining capacity for algorithmically selected tracks.
**And** duplicates between manual and auto-fill selections are excluded.

**Given** Auto-Fill is active
**When** I adjust the optional "Max Fill Size" slider
**Then** the basket recalculates to respect the new limit.
**And** tracks beyond the limit are removed from the basket in reverse priority order.

**Given** Auto-Fill items are displayed in the basket
**When** I view the item list
**Then** auto-filled items show a distinct "Auto" badge to differentiate them from
manually added items.
**And** each item shows its priority reason (e.g., "★ Favorite", "▶ 47 plays", "New").

**Technical Notes:**
- Priority algorithm runs daemon-side via Jellyfin API queries (IsFavorite,
  PlayCount, DateCreated)
- IPC: `basket.autoFill` JSON-RPC method with params:
  { deviceId, maxBytes?, excludeItemIds[] }
- Response streams items progressively as the daemon calculates
- Device profile stores auto-fill preferences: `auto_fill_enabled`, `max_fill_bytes`
- Post-MVP: allow scoping to specific libraries/collections
```

#### 4.2.4 Story 4.5 Expansion — Daemon-Initiated Sync Path

**Replace title with:**
```
### Story 4.5: "Start Sync" UI-to-Engine & Daemon-Initiated Trigger
```

**Replace user story line with:**
```
As a Convenience Seeker (Sarah) and Ritualist (Arthur),
I want to click a "Start Sync" button in the Sync Basket sidebar or have the daemon
automatically trigger sync on device connect,
So that I can either manually execute my selection or enjoy zero-touch automatic
synchronization.
```

**Add after existing acceptance criteria:**
```
**Given** a known device is connected with `auto_sync_on_connect` enabled
**When** the daemon detects the device and loads its profile
**Then** the daemon internally triggers `sync.start` using the device's auto-fill
configuration (without a UI-initiated RPC call).
**And** the sync follows the same differential algorithm, buffered IO, and manifest
update logic as a UI-triggered sync.
**And** if the UI is open, it reflects the in-progress sync state via
`on_sync_progress` events.
**And** if the UI is closed, the tray icon and OS notifications provide progress
and completion feedback.
```

### 4.3 Architecture Changes

#### 4.3.1 Daemon Responsibilities

**Add:**
```
- Auto-Fill Algorithm: Priority-based music selection engine
  (favorites → play count → creation date) querying Jellyfin API
- Auto-Sync Controller: Monitors device detection events and triggers
  sync automatically for configured devices without UI interaction
```

#### 4.3.2 IPC Methods

**Add:**
```
- basket.autoFill: Configure and trigger auto-fill calculation
  Params: { deviceId, maxBytes?, excludeItemIds[] }
  Response: Streams ranked item list progressively
- sync.setAutoFill: Persist auto-fill settings per device profile
  Params: { deviceId, autoFillEnabled, maxFillBytes?, autoSyncOnConnect }
```

#### 4.3.3 SQLite Schema

**Add device profile fields:**
```
- auto_fill_enabled BOOLEAN DEFAULT false
- max_fill_bytes INTEGER NULL (null = fill to capacity)
- auto_sync_on_connect BOOLEAN DEFAULT false
```

#### 4.3.4 Manifest Extension

**Add optional block to `.jellyfinsync.json`:**
```json
"autoFill": {
  "enabled": true,
  "maxBytes": null,
  "autoSyncOnConnect": true
}
```

### 4.4 UX Design Changes

#### 4.4.1 Basket Sidebar Components

**Add:**
```
- Auto-Fill Toggle: Shoelace <sl-switch> in the Basket header area
- Max Fill Size Control: <sl-range> slider (visible when Auto-Fill active)
- Auto Badge: Distinct visual indicator on auto-filled items
- Priority Reason Tags: Inline labels (★ Favorite, ▶ 47 plays, "New")
```

#### 4.4.2 Device Profile Settings

**Add:**
```
- "Auto-sync on connect" toggle (<sl-switch>) in device profile panel
- Helper text: "Automatically start syncing when this device is connected.
  Works with or without the UI open."
```

#### 4.4.3 Headless Sync Feedback

**Add:**
```
- Without UI: tray icon animation + OS-native notification on completion
- With UI open: Basket reflects live sync state via on_sync_progress events
```

#### 4.4.4 Persona Notes

**Add:**
```
- The Sprinter (Sarah): Auto-fill + auto-sync-on-connect is the primary
  zero-friction path. Plug in, walk away.
- The Ritualist (Arthur): Can ignore auto-fill entirely and continue manual
  curation. "Auto" badges provide transparency if mixed mode used.
```

---

## 5. Implementation Handoff

### Change Scope: Moderate

Since Epics 2, 3, and 4 are already completed, this is a **Moderate** scope change requiring backlog reorganization — all modifications fit within the existing epic structure and can be implemented directly by the development team.

### Handoff Plan

| Recipient | Responsibility |
|-----------|---------------|
| **PM / Analyst** | Apply PRD edits (FR29, FR30, MVP scope, user journey) |
| **Architect** | Apply architecture updates (daemon responsibilities, IPC methods, schema, manifest) |
| **UX Designer** | Apply UX spec updates (basket components, device profile settings, headless feedback) |
| **SM / Dev** | Update epics document (FR map, Story 2.3, new Story 3.6, Story 4.5), update sprint plan |

### Success Criteria

- [ ] All 4 planning artifacts updated with approved changes
- [ ] FR29 and FR30 fully traceable through epics → stories → architecture → UX
- [ ] Story 3.6 is implementation-ready with clear acceptance criteria
- [ ] Stories 2.3 and 4.5 expansions are consistent with new auto-sync behavior
- [ ] Sprint plan updated to reflect new story and expanded stories
