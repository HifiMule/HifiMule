# Sprint Change Proposal - Server Context and Logout Guardrails

## 1. Issue Summary

Now that HifiMule can connect to different server types, the main UI needs to make the active server visible, the basket must not silently retain selections from a previous server, and users need a logout/disconnect control.

This was discovered after Story 8.4 made runtime server detection and provider switching available. Without these guardrails, a user can connect to one server, build a basket, then connect to another server while the UI and manifest still contain stale item identifiers from the previous server.

## 2. Impact Analysis

Epic impact:
- Epic 2 connection flow is affected because the connected server needs a visible identity and a logout action.
- Epic 3 basket behavior is affected because basket selections must be scoped to the active media server as well as the active device.
- Epic 8 provider support is affected because server identity now needs to flow from the daemon into UI state.

Story impact:
- Story 2.5 gains a post-login connected-server hint and logout affordance.
- Story 3 basket persistence gains current-server validation.
- Story 8.4 daemon state output is extended with current server metadata.

Artifact conflicts:
- PRD FR5/FR7 already support server credentials and persistent connection state; no product-scope conflict.
- UX login badge guidance exists, but the main library view also needs an active server badge.
- Architecture remains aligned: server identity stays daemon-owned, and UI consumes it via `get_daemon_state`.

Technical impact:
- Add `server.logout` RPC to clear active provider, persistent server config, keyring credentials, and connection cache.
- Add `currentServer` metadata to `get_daemon_state`.
- Add `serverId` to basket items and filter saved/hydrated basket items to the active server.
- Add main UI server badge and logout button.

## 3. Recommended Approach

Direct Adjustment.

This is minor scope because it does not change provider factory architecture or sync engine semantics. The safest path is to add a deterministic server identity from persisted server config, stamp basket items with that identity in the UI, and enforce the same identity in daemon basket RPCs.

Risk is low. The main compatibility tradeoff is that legacy basket items without a `serverId` are hidden once a current server is known, which favors correctness over preserving potentially stale cross-server selections.

## 4. Detailed Change Proposals

Story: Story 2.5 Interactive Login & Identity Management
Section: Acceptance Criteria

OLD:
- The UI transitions to the main Library Browser on success.

NEW:
- The UI transitions to the main Library Browser on success.
- The main Library Browser shows the connected server type, username, URL, and version when available.
- The main Library Browser provides a logout button that disconnects the active server and returns the user to login.

Rationale: Users need a clear hint of which server is active after multi-server support is enabled.

Story: Story 3 Basket / Content Selection
Section: Acceptance Criteria

OLD:
- Users can select specific playlists or entities for synchronization.

NEW:
- Users can select specific playlists or entities for synchronization.
- Basket selections are scoped to the currently connected server; stale selections from another server are not shown or persisted for the active server.

Rationale: Server item IDs are provider/server-local and cannot be safely reused across different server connections.

Story: Story 8.4 Runtime Server-Type Detection Factory
Section: Acceptance Criteria

OLD:
- `get_daemon_state` response gains `serverType: string | null`.

NEW:
- `get_daemon_state` response gains `serverType: string | null`, `serverVersion: string | null`, and `currentServer` metadata containing `serverId`, `url`, `username`, `serverType`, and `serverVersion`.

Rationale: UI server context and basket enforcement need a daemon-owned current-server identity.

## 5. Implementation Handoff

Scope classification: Minor.

Route to: Developer agent for direct implementation.

Success criteria:
- A connected server hint appears in the main UI.
- Logout clears active provider state and returns to login.
- Basket items added after connection are stamped with current `serverId`.
- Basket hydrate/save paths filter out items that do not belong to the current server.
- Daemon tests pass.
