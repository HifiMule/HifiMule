# Sprint Change Proposal: Server Identity for Multi-Server Management

**Project:** HifiMule  
**Date:** 2026-06-09  
**Author:** Codex  
**Workflow:** Correct Course  
**Mode:** Batch

## 1. Issue Summary

The first multi-server management amendment made multiple servers selectable and routable, but server identity is still mostly technical: URL, provider type, and username. This is weaker than the device identity model already used by HifiMule, where users can assign a display name and icon to distinguish similar devices quickly.

The requested change is to add user-facing server identity:

- A server display name.
- A server icon selected from built-in media-server and music/audiobook-related icons.
- Use the name and/or icon instead of provider type where the UI needs to identify a specific server, including the basket, server switch menu, server hub, and cross-server notices.

This is a refinement of the already-approved multi-server direction, not a reversal. It improves clarity when users have multiple servers of the same type, such as two Jellyfin instances or a Navidrome plus generic OpenSubsonic server.

## 2. Impact Analysis

### Checklist Status

- [x] 1.1 Triggering story identified: Story 2.11 Multi-Server Hub and Story 3.2 mixed-server basket behavior.
- [x] 1.2 Core problem defined: new usability requirement emerged from stakeholder review.
- [x] 1.3 Evidence gathered: current specs identify servers by URL/type/username; devices already use name/icon identity.
- [x] 2.1 Current epic impact assessed: Epic 2 can remain complete but should gain a small follow-up story.
- [x] 2.2 Required epic-level changes identified: add Story 2.12 Server Identity Name and Icon.
- [x] 2.3 Remaining epics reviewed: Epic 3 basket display and Epic 11 playlist notices need wording/contract awareness only.
- [x] 2.4 Future epic validity checked: no epics invalidated.
- [x] 2.5 Priority checked: should be implemented before further multi-server UI polish.
- [x] 3.1 PRD conflicts checked: MVP remains achievable; add/extend server management requirement.
- [x] 3.2 Architecture conflicts checked: extend `ServerRecord`, SQLite schema, RPC responses, and UI state types.
- [x] 3.3 UX conflicts checked: add Server Identity Settings and update Server Hub / basket badges.
- [x] 3.4 Other artifacts checked: sprint status should add the new story after approval.
- [x] 4.1 Direct Adjustment evaluated: viable, low-to-medium effort, low risk.
- [N/A] 4.2 Rollback evaluated: not needed.
- [N/A] 4.3 MVP Review evaluated: not needed.
- [x] 4.4 Recommended path selected: Direct Adjustment.
- [x] 5.1-5.5 Proposal components completed.
- [!] 6.3 User approval pending.
- [!] 6.4 Sprint status update pending approval.

### Epic Impact

Epic 2 should be reopened only for a focused follow-up story. Story 2.11 remains valid for multi-server routing, selection, removal, provider cache lifecycle, and migration. The new story should layer identity fields onto that model.

Epic 3 is impacted because mixed-server basket items currently show a server name badge; the definition of that badge should become the configured server display name plus icon, with provider type as secondary metadata.

Epic 11 is lightly impacted because playlist save notices refer to the selected server name. They should use the same display-name fallback logic.

### Artifact Impact

PRD:
- Add or amend a functional requirement under Server & Profile Management for custom server display name and icon.
- Clarify that provider type is detection metadata, not the primary user-facing label.

Architecture:
- Extend `ServerRecord` with `name` and `icon`.
- Extend `server_config` with `name TEXT NULL` and `icon TEXT NULL`.
- Extend `server.connect`, `server.list`, `server.update`, and `get_daemon_state` contracts.
- Define fallback label generation.

UX:
- Add a Server Identity Settings component analogous to Device Identity.
- Update Server Hub cards and compact switcher.
- Update basket locked badges and mixed-server notices.

Sprint status:
- Add Story 2.12 as `backlog` or `ready-for-dev`, depending on whether a story file is created immediately after approval.

## 3. Recommended Approach

Use **Direct Adjustment**.

This change is scoped and additive. It does not alter the selected-server routing model, provider abstraction, vault layout, or sync grouping behavior. It only adds identity metadata and updates the UI to prefer that metadata over provider type when identifying a server to the user.

Estimated effort: Low-to-medium.

Risk: Low. The main risks are migration/backward compatibility and inconsistent fallback labels. Both are manageable with a single shared display helper in the UI and nullable/defaulted fields in daemon models.

Timeline impact: One focused story.

## 4. Detailed Change Proposals

### PRD Change

Section: Functional Requirements, Server & Profile Management

OLD:

```markdown
FR5: Users can configure media server credentials (URL, server type, username, and either an API token for Jellyfin or username+password for Subsonic/OpenSubsonic servers). The system auto-detects the server type by pinging the URL when the user enters it.
```

NEW:

```markdown
FR5: Users can configure media server credentials (URL, server type, username, and either an API token for Jellyfin or username+password for Subsonic/OpenSubsonic servers). The system auto-detects the server type by pinging the URL when the user enters it.

FR45: Users can assign each configured media server a custom display name and icon from a built-in icon library that includes provider icons (Jellyfin, Navidrome/Subsonic/OpenSubsonic where available) plus music and audiobook-oriented icons. The UI uses the configured name and/or icon as the primary server identity in the Server Hub, compact server switcher, basket server badges, playlist notices, and other multi-server contexts. Provider type remains visible as secondary metadata when useful.
```

Rationale: This makes server identity explicit and aligns server management with the existing device name/icon model.

### Epics Change

Add after Story 2.11:

```markdown
### Story 2.12: Server Identity Name and Icon

As a System Admin (Alexis) and multi-server user,
I want each configured media server to have a custom display name and icon,
So that I can quickly distinguish servers in the hub, switcher, basket, and playlist flows without relying on provider type.

**Acceptance Criteria:**

**Given** I add a new server
**When** the connection succeeds
**Then** the server is created with a default display name derived from the server URL or detected provider name.
**And** the server receives a default icon based on detected provider type when available.

**Given** I open Settings -> Servers for an existing server
**When** I edit its display name or icon
**Then** `server.update({ id, name, icon })` persists the changes.
**And** the Server Hub, compact switcher, basket badges, and playlist notices update without reconnecting the server.

**Given** I choose a server icon
**Then** the picker offers provider icons plus generic music/audio icons such as music note, headphones, library, album, radio, audiobook/book, and generic server.
**And** unsupported or missing provider logos fall back to a generic server or music icon.

**Given** the basket contains items from multiple servers
**When** basket items render
**Then** each server badge uses the configured server icon and display name.
**And** provider type is shown only as secondary metadata or tooltip text when helpful.

**Given** a legacy server record has no name or icon
**When** it is loaded
**Then** the UI uses a stable fallback label: configured name -> URL host -> username + provider type -> provider type.
**And** no migration blocks startup.

**Technical Notes:**
- `server_config` adds nullable `name` and `icon` columns.
- `ServerRecord` gains `name: Option<String>` and `icon: Option<String>`.
- `server.connect` accepts optional `name` and `icon`, and still works when omitted.
- `server.list` and `get_daemon_state.servers` return `{ id, url, serverType, username, name, icon, selected }`.
- New `server.update({ id, name?: string, icon?: string | null }) -> { ok: true }`.
- UI should share one `formatServerIdentity(server)` helper so hub cards, switcher labels, basket badges, notices, and playlist dialogs do not drift.
```

Rationale: The existing Story 2.11 is complete and already reviewed. A follow-up story avoids rewriting history while making the new requirement implementable.

### Architecture Change

Section: Multi-Server Management - Architectural Decisions

OLD:

```rust
pub struct ServerRecord {
    pub id: String,           // stable UUID
    pub url: String,
    pub server_type: String,  // 'jellyfin' | 'subsonic'
    pub username: String,
}
```

NEW:

```rust
pub struct ServerRecord {
    pub id: String,              // stable UUID
    pub url: String,
    pub server_type: String,     // 'jellyfin' | 'subsonic'
    pub username: String,
    pub name: Option<String>,    // user-facing display name
    pub icon: Option<String>,    // built-in icon identifier
}
```

OLD:

```sql
CREATE TABLE IF NOT EXISTS server_config (
    id          TEXT    PRIMARY KEY,
    url         TEXT    NOT NULL,
    server_type TEXT    NOT NULL,
    username    TEXT    NOT NULL,
    selected    INTEGER NOT NULL DEFAULT 0,
    updated_at  INTEGER NOT NULL
);
```

NEW:

```sql
CREATE TABLE IF NOT EXISTS server_config (
    id          TEXT    PRIMARY KEY,
    url         TEXT    NOT NULL,
    server_type TEXT    NOT NULL,
    username    TEXT    NOT NULL,
    name        TEXT    NULL,
    icon        TEXT    NULL,
    selected    INTEGER NOT NULL DEFAULT 0,
    updated_at  INTEGER NOT NULL
);
```

IPC amendments:

```typescript
server.connect(params: {
  url: string;
  serverType: 'jellyfin' | 'subsonic' | 'auto';
  username: string;
  password: string;
  name?: string;
  icon?: string | null;
}) -> { ok: true; serverId: string; serverType: string; serverVersion: string }

server.list() -> Array<{
  id: string;
  url: string;
  serverType: string;
  username: string;
  name: string | null;
  icon: string | null;
  selected: boolean;
}>

server.update(params: {
  id: string;
  name?: string;
  icon?: string | null;
}) -> { ok: true }
```

Rationale: Server identity is metadata. It belongs in the server record and should not affect credentials, provider cache keys, or routing.

### UX Change

Section: Custom Components

Add:

```markdown
**Server Identity Settings:** The Server Hub displays each configured server as a compact identity card with icon, display name, provider badge, username, and host. The display name is the primary label; provider type is secondary. Server settings include a required display-name input and an icon picker mirroring the device icon picker pattern. The icon library includes provider icons plus generic music and audiobook icons. The compact server switcher uses icon + name; basket badges use icon + name; tooltips or secondary text may show provider type and host for disambiguation.
```

Rationale: The UI should identify servers the way humans remember them: "Home Music", "Audiobooks", "NAS Jellyfin", not just "Jellyfin".

## 5. Implementation Handoff

Scope classification: **Minor to Moderate**.

Recommended route: Developer agent can implement directly after approval, with one new story file if desired.

Implementation tasks:

- Add `name` and `icon` fields to server persistence, daemon models, and RPC serialization.
- Add migration for nullable `server_config.name` and `server_config.icon`.
- Add `server.update`.
- Update TypeScript server types and state handling.
- Add or reuse a server identity formatter/helper.
- Update Server Hub, compact switcher, basket server badges, mixed-server notices, and playlist save notices.
- Add tests for migration, `server.update`, RPC payloads, and UI fallback labels.

Success criteria:

- Existing servers load without identity fields.
- New servers get sensible default name/icon.
- Users can edit name/icon without reconnecting credentials.
- The UI uses name/icon before provider type in all multi-server identification contexts.
- Mixed-server baskets remain routable exactly as before.

## 6. Approval

Approved by Alexis on 2026-06-09.

Applied artifact updates:

- Added FR45 to `prd.md`.
- Added Story 2.12 and FR45 coverage to `epics.md`.
- Updated multi-server architecture with server identity fields and RPC amendments.
- Added Server Identity Settings to the UX specification.
- Added `2-12-server-identity-name-and-icon: backlog` to `sprint-status.yaml`.
