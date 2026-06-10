---
stepsCompleted: ['step-01-document-discovery', 'step-02-prd-analysis', 'step-03-epic-coverage-validation', 'step-04-ux-alignment', 'step-05-epic-quality-review', 'step-06-final-assessment']
outputFile: '_bmad-output/planning-artifacts/implementation-readiness-report-2026-06-09.md'
inputDocuments:
  - prd.md
  - architecture.md
  - epics.md
  - ux-design-specification.md
---

# Implementation Readiness Assessment Report

**Date:** 2026-06-09
**Project:** HifiMule

---

## PRD Analysis

### Functional Requirements

FR1: The system can automatically detect Mass Storage (USB MSC) and MTP devices on Windows, Linux, and macOS.
FR2: Users can manually select a target device folder if automatic detection fails (MSC only; MTP must be auto-detected).
FR3: The system can identify the presence of a `.hifimule.json` manifest on device discovery.
FR4: The system can read persistent hardware identifiers to link devices across sessions; tracks all simultaneously connected managed devices; allows user to select active device context.
FR5: Users can configure media server credentials (URL, server type, username, API token for Jellyfin or username+password for Subsonic/OpenSubsonic). System auto-detects server type by pinging the URL.
FR6: Users can select a specific user profile from the connected media server for syncing.
FR7: The system can maintain a persistent, encrypted connection state to the configured media server. Jellyfin: access token stored. Subsonic/OpenSubsonic: password stored encrypted, used for per-request stateless signing.
FR8: Users can browse music via server-supported navigation modes: Playlists, Artists, Albums, Tracks, Genres, Recently Added, Frequently Played, Recently Played, and Favorites. Provider abstraction normalizes modes across Jellyfin, Navidrome, Subsonic, OpenSubsonic. Unsupported modes hidden.
FR9: Users can select server playlists or entities (artists, albums, genres, tracks) for synchronization (read path). Persisting selection back to server as playlist is covered by FR37.
FR10: The system can report real-time storage availability on the target device.
FR11: Users can see a preview of "Proposed Changes" (files to add, remove, update) before starting a sync.
FR12: The system can perform a differential sync based on the local manifest.
FR13: The system can protect unmanaged user files from deletion or modification.
FR14: The system can stream media files from the server to the device via memory-to-disk buffering, using the appropriate device IO backend (filesystem writes for MSC, WPD/libmtp object transfers for MTP).
FR15: The system can validate hardware-specific constraints (path length, character sets) before writing files.
FR16: The system can resume an interrupted sync session without restarting from scratch.
FR17: The system can detect Rockbox `.scrobbler.log` files on connected devices.
FR18: The system can report completed track plays to the Jellyfin server via the Progressive Sync API.
FR19: The system can track which scrobbles have already been submitted to prevent duplication.
FR20: The system can run as a background service (headless) with minimal resource usage.
FR21: Users can toggle "Launch on Startup" behavior.
FR22: The system can provide tray-icon status updates for sync progress and hardware state.
FR23: The system can send OS-native notifications for sync completion or errors.
FR24: The system provides visual feedback (splash screen) during startup and connection validation.
FR25: The system retrieves and displays only music-centric content (Playlists, Albums, Artists, Tracks).
FR26: The system can initialize a new `.hifimule.json` manifest on a connected device, capturing hardware ID, music sync folder, playlist folder, server profile, display name, and optional icon.
FR27: The system can be packaged into platform-native installers (MSI, DMG, AppImage/.deb) using Tauri v2 bundler.
FR28: The build pipeline can produce signed, distributable artifacts for all three platforms from a single CI workflow.
FR29: The system can reserve capacity in the sync basket via a virtual Auto-Fill slot; daemon expands at sync time using priority algorithm (favorites → play count → creation date).
FR30: The system can automatically trigger synchronization when a known configured device is detected, without user interaction.
FR31: The system can negotiate a transcoded stream URL from the Jellyfin server using a device-specific DeviceProfile payload.
FR32: The system can list available device transcoding profiles and assign one to a device, persisting selection in manifest and database.
FR33: The system presents a persistent device hub showing all connected managed devices. User can switch active device context at any time. When no device selected, basket empty and add-items disabled.
FR34: The system can add an artist to the sync basket as a single entity reference; daemon resolves to current track list at sync time.
FR35: The system supports Jellyfin, Navidrome, Subsonic, and any OpenSubsonic-compatible media server. Server type auto-detected at connection. Capability extensions cached.
FR36: The system can edit an existing managed device manifest (name, icon, transcoding profile, music folder, playlist folder). Folder changes surface cleanup/resync work.
FR37: The system can persist the current device selection as a media-server playlist (create new or update existing). Read-fresh before writing. Entities resolved to concrete track list. Auto-Fill slot excluded. Gated by `supports_playlist_write`.
FR38: Dual-panel playlist curation view: artists left, albums right (filtered to playlist). Track list below. Remove artist/album/track. "Add tracks" search dialog. Right-click "Add to playlist…" context actions. Right-click for send-to-playlist from browse. Statistics (track count, duration, size). Inline rename. Delete with confirmation. Track absolute position shown. Full playlist viewable with "All artists / All albums" filter.
FR39: The system can present any browse page or drill-down level as a virtualized list/table view. Single global grid/list toggle applies across all browse modes and depths.
FR40: The system can reorder tracks within an existing server playlist via per-track up/down controls in the curation view. Reordering via `playlist.reorder` RPC. Gated by `supports_playlist_write`.
FR41: The system can present the entire library as a flat Tracks browse mode with dual-panel artist/album filter layout. Each panel independently paginated with autoload-on-scroll. "All artists" / "All albums" entries available. Basket add/remove and "Add to playlist…" affordances per track. Gated by provider capability.

**Total FRs: 41** (FR1–FR41)

**⚠️ NOTE — PRD not yet updated with multi-server FRs:** The approved change proposal (2026-06-09) adds FR42, FR43, FR44 and modifies FR5/FR6/FR7, but the PRD still contains the old single-server versions. This is a known pending artifact update.

---

### Non-Functional Requirements

NFR1 (Performance): Headless Rust engine < 10MB RAM during idle states.
NFR2 (Performance): Manifest audit complete and "ready to sync" in < 5 seconds.
NFR3 (Performance): Sync throughput limited only by target hardware write speed or network bandwidth.
NFR4 (Performance): List/table browse views must use virtualized (windowed) rendering. Autoload-on-scroll for next page.
NFR5 (Reliability): Write-Verify-Commit — use OS-level file sync primitives (`sync_all`) before marking sync complete.
NFR6 (Reliability): Atomic manifest updates — `.hifimule.json` updated atomically to prevent corruption.
NFR7 (Reliability): Handle network interruptions during buffered streaming with at least 3 retry cycles.
NFR8 (Reliability): Mid-sync ejections must not result in unbootable/unmountable media; graceful "Interrupted" state + Repair Utility on reconnect.
NFR9 (Cross-Platform): 100% feature parity between Windows, Linux, and macOS.
NFR10 (Cross-Platform): macOS Sandbox Compliance — no root/sudo required.
NFR11 (Cross-Platform): Memory and CPU usage within 15% delta across all supported OS environments.
NFR12 (Security): Server credentials stored in hardware-bound encrypted vault (machine-uid + blake3 + ChaCha20-Poly1305 AEAD). Protected against offline disk exfiltration. Credentials lost if hardware fingerprint changes.
NFR13 (Security): All media sync occurs locally between configured server and device. Zero user data transmitted to third-party servers.
NFR14 (Security): Playlist write operations use existing stored credentials only. No new credential scope.
NFR15 (Maintainability): CLI-First Architecture — core engine fully functional and testable without UI.
NFR16 (Maintainability): Standard Rust workspace patterns for ease of future contribution.

**Total NFRs: 16** (NFR1–NFR16)

---

### Additional Requirements / Constraints

- **Single developer** resource model
- **Manual updates** only for MVP (no built-in auto-update)
- **System tray** integration required
- **Measurable outcomes:** < 5s device-to-sync-ready; < 10s incremental sync when 90%+ already present; 100% scrobble accuracy for matched items
- **Destructive Safety Protocol:** Mandatory manual confirmation for manifest-repair/cleanup > 100MB data deletion

---

### PRD Completeness Assessment

The PRD is well-structured and thorough for the current implemented scope (FR1–FR41, NFR1–NFR16). One known gap: the multi-server management requirements (FR42–FR44, updated FR5/FR6/FR7) from the approved 2026-06-09 change proposal are not yet reflected in the PRD document. This will surface as a gap in epic coverage for Story 2.11.

---

## Epic Coverage Validation

### Coverage Matrix

| FR | PRD Requirement (Summary) | Epic Coverage | Status |
|---|---|---|---|
| FR1 | Auto-detect MSC/MTP devices | Epic 2 (Stories 2.2, 2.10) | ✓ Covered |
| FR2 | Manual device folder fallback (MSC only) | Epic 2 | ✓ Covered |
| FR3 | Detect .hifimule.json on discovery | Epic 2 | ✓ Covered |
| FR4 | Persistent hardware IDs, multi-device tracking | Epic 2 (Stories 2.3, 2.7, 2.8) | ✓ Covered |
| FR5 | Configure server credentials (multi-server scope) | Epic 2 (Story 2.1) | ⚠️ Stale — Story 2.1 has single-server text; multi-server amendment pending |
| FR6 | Select user profile | Epic 2 | ✓ Covered |
| FR7 | Persistent encrypted vault (multi-server) | Epic 2 (Story 2.1, Epic 7 Story 7.5) | ⚠️ Stale — vault restructuring amendment pending |
| FR8 | Browse all navigation modes | Epic 3 + Epic 9 | ✓ Covered |
| FR9 | Select entities for sync | Epic 3 | ✓ Covered |
| FR10 | Real-time storage availability | Epic 3 (Story 3.3) | ✓ Covered |
| FR11 | Preview proposed changes | Epic 3 | ✓ Covered |
| FR12 | Differential sync | Epic 4 (Story 4.1) | ✓ Covered |
| FR13 | Protect unmanaged files | Epic 3 (Story 3.4) | ✓ Covered |
| FR14 | Stream media via buffered IO | Epic 4 (Story 4.2) | ✓ Covered |
| FR15 | Validate hardware constraints | Epic 4 (Story 4.3) | ✓ Covered |
| FR16 | Resume interrupted sync | Epic 4 (Story 4.4) | ✓ Covered |
| FR17 | Detect Rockbox .scrobbler.log | Epic 5 (Story 5.1) | ✓ Covered |
| FR18 | Report plays to server (scrobble) | Epic 5 (Story 5.1) | ✓ Covered |
| FR19 | Track submitted scrobbles | Epic 5 (Story 5.2) | ✓ Covered |
| FR20 | Run as background service | Epic 1 (Story 1.1) | ✓ Covered |
| FR21 | Launch on startup toggle | Epic 1, Epic 6 (Story 6.7) | ✓ Covered |
| FR22 | Tray icon status | Epic 1 (Story 1.2) | ✓ Covered |
| FR23 | OS notifications | Epic 5 (Story 5.3) | ✓ Covered |
| FR24 | Splash screen | Epic 2 (Story 2.4) | ✓ Covered |
| FR25 | Music-only library filtering | Epic 3 (Story 3.5) | ✓ Covered |
| FR26 | Initialize device manifest | Epic 2 (Stories 2.6, 2.9) | ✓ Covered |
| FR27 | Platform-native installers | Epic 6 | ✓ Covered |
| FR28 | CI/CD build pipeline | Epic 6 | ✓ Covered |
| FR29 | Auto-Fill virtual slot | Epic 3 (Story 3.8) | ⚠️ Covered but serverId amendment not applied |
| FR30 | Auto-sync on device connect | Epic 2 (Story 2.3) | ✓ Covered |
| FR31 | Transcoding handshake | Epic 4 (Story 4.8) | ✓ Covered |
| FR32 | Device transcoding profiles | Epic 4 (Story 4.8) | ✓ Covered |
| FR33 | Persistent device hub | Epic 2 (Story 2.8) | ✓ Covered |
| FR34 | Artist entity basket item | Epic 3 (Story 3.9) | ✓ Covered |
| FR35 | Multi-provider server support | Epic 8 (Stories 8.1–8.6) | ✓ Covered |
| FR36 | Edit device manifest | Epic 10 (Stories 10.1–10.2) | ✓ Covered |
| FR37 | Save selection as server playlist | Epic 11 (Stories 11.1–11.5) | ✓ Covered |
| FR38 | Dual-panel playlist curation (full spec) | Epic 11 (Stories 11.6–11.10) | ✓ Covered |
| FR39 | Virtualized list/table view (all modes) | Epic 9 (Stories 9.7, 9.8) | ✓ Covered |
| FR40 | Reorder playlist tracks | Epic 11 (Stories 11.9–11.10) | ✓ Covered |
| FR41 | Tracks browse mode | Epic 9 (Stories 9.9–9.10) | ✓ Covered |
| FR42 | Server Hub (add/remove/select servers) | **NOT IN EPICS** | ❌ Missing — Story 2.11 not in epics.md |
| FR43 | Mixed-server basket (read-only non-selected) | **NOT IN EPICS** | ❌ Missing — Story 3.2 amendment not applied |
| FR44 | Playlist restricted to selected server | **NOT IN EPICS** | ❌ Missing — Stories 11.4, 11.5 amendments not applied |

*Note: FR42–FR44 are not yet in the PRD (pending PRD update per the 2026-06-09 change proposal). They are tracked here as upcoming gaps.*

---

### Missing Requirements

#### Critical — Story 2.11 missing from epics.md

**Story 2.11: Multi-Server Hub** is referenced in `sprint-status.yaml` as `backlog` but does not exist anywhere in `epics.md`.

- **Impact:** The next story to be devved has no story spec in the epics document. The dev agent will have no formal AC baseline.
- **Recommendation:** Add Story 2.11 to Epic 2 in `epics.md` using the detailed AC from the sprint change proposal (Section 4, Story 2.11).

#### High Priority — Story Amendments Not Applied

The following stories were modified by the 2026-06-09 change proposal but `epics.md` still has the old (pre-amendment) text:

| Story | Amendment Required |
|---|---|
| Story 2.1 | Multi-server vault; `server.connect` returns `serverId`; upsert-by-URL behavior |
| Story 2.5 | First-run-only full screen; add-server inline; per-server re-auth prompt |
| Story 3.2 | `BasketItem.serverId`; read-only locked rendering for non-selected server items; mixed-server sync routing |
| Story 3.8 | `AutoFillSlot.serverId`; overwrite-on-enable with new server; read-only slot for non-selected server |
| Story 11.4 | Server scope validation on `playlist.create`; 409 error for cross-server items |
| Story 11.5 | Cross-server exclusion notice; UI pre-filter to selected-server items only |

#### Moderate — Epics Requirements Inventory Section is Stale

The `Requirements Inventory` section at the top of `epics.md` only lists FR1–FR24 and an abbreviated FR37–FR39. FR25–FR36 and FR40–FR41 are present in the coverage map but absent from the inventory. This is cosmetic but can mislead future readers.

---

### Coverage Statistics

- **Total PRD FRs (current):** 41 (FR1–FR41)
- **FRs covered in epics:** 38 (FR1–FR41 minus FR42–FR44 which are not yet in PRD)
- **Coverage of current PRD FRs:** 100% (all FR1–FR41 have epic coverage)
- **Pending amendments to apply:** 6 stories (2.1, 2.5, 3.2, 3.8, 11.4, 11.5)
- **Missing story:** Story 2.11 (in sprint plan but not in epics.md)

---

## UX Alignment Assessment

### UX Document Status

**Found:** `ux-design-specification.md` (16.6K, completed 2026-01-27)

### UX ↔ PRD Alignment

Overall: Strong alignment for all pre-multi-server requirements.

| Area | UX Section | PRD Requirement | Status |
|---|---|---|---|
| User personas | §1.2 | User journeys (Arthur, Sarah, Alexis) | ✓ Aligned |
| Invisible sync flow | §4.2 (Sarah's Dash flow) | FR30 auto-sync on connect | ✓ Aligned |
| Browse modes | §5.1 Navigation | FR8 all browse modes | ✓ Aligned |
| List/table view | §5.1 List/Table Browse View | FR39 virtualized view | ✓ Aligned (global single toggle confirmed) |
| Auto-Fill virtual slot | §5.3 Auto-Fill Slot Card | FR29 lazy slot at sync time | ✓ Aligned |
| Save as playlist | §5.2 "Save selection as playlist" | FR37 playlist persistence | ✓ Aligned |
| Dual-panel curation | §5.2 Playlist Curation View | FR38 full curation spec | ✓ Aligned (rename, delete, reorder all spec'd) |
| Tracks browse mode | §5.2 Tracks Browse View | FR41 flat tracks mode | ✓ Aligned |
| Playlist reorder | §5.2 (up/down, `playlist.reorder`) | FR40 track reorder | ✓ Aligned |
| Device hub | §5.6 Device Hub | FR33 persistent hub | ✓ Aligned |
| Device settings | §5.4 | FR36 edit manifest | ✓ Aligned |
| Accessibility | §6.3 WCAG 2.1 AA | PRD Accessibility requirement | ✓ Aligned |
| **Server Hub** | **Not mentioned** | **FR42 (pending in PRD)** | ⚠️ UX not designed |
| **Mixed-server basket** | **Not mentioned** | **FR43 (pending in PRD)** | ⚠️ UX not designed |
| **Playlist scope UI** | **Not mentioned** | **FR44 (pending in PRD)** | ⚠️ UX not designed |

### UX ↔ Architecture Alignment

| UX Requirement | Architecture Support | Status |
|---|---|---|
| 70/30 basket-centric layout | Frontend Architecture §4.1 | ✓ Supported |
| All RPC calls via daemon | Architecture §API & Communication Patterns | ✓ Supported |
| Capability-gated UX (playlist write, browse modes) | Architecture `Capabilities` struct | ✓ Supported |
| Lazy auto-fill slot (no API at toggle) | Architecture `sync.start` autoFill param | ✓ Supported |
| `playlist.rename` RPC | Architecture Epic 11 section | ✓ Supported |
| `playlist.reorder` RPC | Architecture Epic 11 section | ✓ Supported |
| Server type detection badge | Architecture `providers::connect()` factory | ✓ Supported |

### Warnings

⚠️ **Multi-Server UX not designed.** The UX document has no wireframes, component specs, or interaction patterns for:
  - The **Server Hub** panel (add, remove, switch servers)
  - **Read-only basket item** visual state for non-selected server items
  - **Inline "Add Server" form** (distinct from first-run full-screen flow)
  - **Per-server re-auth prompt** modal
  
  This is expected — the UX predates the 2026-06-09 change proposal. However, Story 2.11 will require UX design before or during implementation. The architecture document defines behavior; the visual design is undefined.

ℹ️ No critical pre-existing UX/architecture misalignments found.

---

## Epic Quality Review

### Initialization

All 11 epics and their stories validated against create-epics-and-stories best practices:
- Epics must deliver user value (not technical milestones)
- Epics must be independent (no forward-epic dependencies)
- Stories must have clear, testable ACs in Given/When/Then form
- No forward story dependencies within an epic
- Database/schema created story-by-story when first needed

---

### Epic-by-Epic Findings

#### Epic 1 — Foundation & Project Genesis

| Check | Result |
|---|---|
| User value | ✅ Stories 1.2/1.3 are user-facing (tray hub, detachable window) |
| Independence | ✅ Fully self-contained |
| Story sizing | ✅ Three small, well-scoped stories |
| AC quality | ✅ Clear Given/When/Then |
| Dependencies | ✅ None |
| FR traceability | ✅ FR20, FR21, FR22 |

🟡 **Minor**: Epic description uses technical language ("Establish the robust, multi-process Rust workspace and cross-platform Tray hub") — borderline technical milestone framing. Story content is user-centric; description should lead with user benefit.

---

#### Epic 2 — Connection & Verification (The Handshake)

| Check | Result |
|---|---|
| User value | ✅ Managing server connections and device identity |
| Independence | ✅ Builds on Epic 1 only |
| Story sizing | ✅ Stories appropriately scoped |
| AC quality | ⚠️ Stories 2.1 and 2.5 have stale pre-amendment ACs |
| Dependencies | ⚠️ Stories 2.1 and 2.5 reference "Story 8.4 factory" (Epic 8) — documented forward dep in Epic 8 preamble |
| FR traceability | ✅ FR1–FR7, FR24, FR26, FR30, FR33 |

🔴 **Critical**: **Story 2.11 (Multi-Server Hub) does not exist in `epics.md`**. It is referenced in `sprint-status.yaml` as `backlog` and has full AC definition in the 2026-06-09 change proposal (Section 4) — but the story spec has never been written into the epics document. The next development task has no formal AC baseline.

🟠 **Major**: Story 2.1 ("Secure Media Server Link") ACs are stale. The current text describes single-server credential replacement. The 2026-06-09 change proposal amends this story to: multi-server vault (`HashMap<serverId, ServerCredentials>`), `server.connect` returns `serverId`, upsert-by-URL semantics. None of this appears in the story's ACs.

🟠 **Major**: Story 2.5 ("Interactive Login & Identity Management") ACs are stale. The current text describes a full-screen login flow that replaces existing credentials. The amendment introduces: first-run-only full-screen mode, per-server re-auth prompt (inline modal), "Add Server" inline affordance. None reflected in current ACs.

🟡 **Minor**: Story 2.7 and part of Story 2.3 carry "Status: superseded by..." notes — stale story content that will confuse implementors. Superseded stories should be removed or explicitly archived, not left in-line.

---

#### Epic 3 — The Curation Hub (Basket & Library)

| Check | Result |
|---|---|
| User value | ✅ Core curation surface |
| Independence | ✅ Builds on Epics 1+2 |
| Story sizing | ✅ Generally appropriate; Story 3.6 superseded |
| AC quality | ⚠️ Story 3.2 has only 2 sparse ACs; Stories 3.2/3.8 have stale multi-server gaps |
| FR traceability | ✅ FR8–FR11, FR13, FR25, FR29, FR33, FR34 |

🟠 **Major**: Story 3.2 ("The Live Selection Basket") has only 2 ACs covering only the happy path (item added, intent overlay shown). Missing ACs for: no-device-selected guard (basket disabled), `BasketItem.serverId` field, read-only locked rendering for items from a non-selected server, error state when add fails. This story is the basket entry point and must be robust.

🟠 **Major**: Story 3.8 ("Lazy Auto-Fill Virtual Slot") has pre-amendment ACs. The `AutoFillSlot` virtual item has no `serverId` field. No AC covers the locked-slot behavior when a different server is selected after the slot was added.

🟡 **Minor**: Story 3.6 ("Auto-Fill Sync Mode") carries "Status: Superseded by Story 3.8" but remains in the epic in full. This is dead content that will confuse implementation.

---

#### Epic 4 — The Sync Engine & Self-Healing Core

| Check | Result |
|---|---|
| User value | ✅ Differential sync, atomic IO, resume |
| Independence | ✅ Builds on Epics 1–3 |
| Story sizing | ✅ All stories appropriately sized |
| AC quality | ⚠️ Story 4.5 `sync.start` itemIds format is pre-amendment |
| FR traceability | ✅ FR12, FR14–FR16, FR31–FR32 |

🟠 **Major**: Story 4.5 ("Start Sync UI-to-Engine") describes `itemIds` as `string[]` in the Technical Notes IPC pattern. The 2026-06-09 architecture amendment changed this to `Array<{id: string, serverId: string}>` so the daemon can group items by server and route each group to the correct provider. The story AC and Technical Notes must reflect this.

---

#### Epic 5 — Ecosystem Lifecycle & Advanced Tools

| Check | Result |
|---|---|
| User value | ✅ Scrobble, notifications, repair |
| Independence | ✅ |
| AC quality | ✅ Clear and appropriately detailed |
| FR traceability | ✅ FR17–FR19, FR23 |

✅ No quality violations found.

---

#### Epic 6 — Packaging & Distribution

| Check | Result |
|---|---|
| User value | ✅ Platform-native installers, CI |
| Independence | ✅ |
| AC quality | ✅ Well-structured across 7 stories |
| FR traceability | ✅ FR27, FR28 |

✅ No quality violations found. Story 6.7 (`Status: backlog`) is appropriate future-work marking.

---

#### Epic 7 — Technical Hardening & Deferred Fixes

| Check | Result |
|---|---|
| User value | ⚠️ Epic description is technical; individual stories have user personas |
| Independence | ✅ Designed for parallel work |
| Story sizing | ⚠️ Story 7.1 is oversized (13 ACs across 8 distinct technical items) |
| AC quality | ⚠️ Story 7.5 references old single-server `Secrets` struct |
| FR traceability | Implicit (hardening existing FRs) |

🟠 **Major**: Epic description ("Address the accumulated technical debt and deferred code-review findings") is a technical milestone framing — no user outcome stated. This is the archetypal "wrong epic" pattern. However, given this is explicitly a technical debt cleanup epic and individual stories have user personas and user-centric ACs, this is a structural compromise that is workable if accepted intentionally.

🟠 **Major**: Story 7.1 (MTP IO & WPD Hardening) has 13 acceptance criteria spanning: `IPortableDeviceContent` refactor, stream-write loop fix, CoTaskMemFree memory safety, multi-storage selection, STA threading, Shell session batching, UUID temp filenames, dirty-marker assertion, fallback logging, directory enumeration error handling, hardware-GUID matching, delete failure chain, concurrent dir creation, and unit test coverage. This is 3–4 stories' worth of work. Risk: oversized story blocks the entire Epic 7 if any sub-item is blocked.

🟡 **Minor**: Story 7.5 ("Machine-Bound Credential Vault") ACs describe returning a single `Secrets` blob on `load_secrets()`. After the 2026-06-09 architecture amendment, `vault.rs` stores `HashMap<String, ServerCredentials>` (keyed by server UUID), not a single `Secrets` struct. The story ACs do not reflect this restructuring.

---

#### Epic 8 — Multi-Provider Media Server Support

| Check | Result |
|---|---|
| User value | ✅ Enables Navidrome/Subsonic users |
| Independence | ✅ Designed as prerequisite for downstream story amendments |
| AC quality | ✅ Well-specified at trait and adapter level |
| FR traceability | ✅ FR35 |

🟠 **Major**: Epic 8's preamble explicitly states "Epic 8 introduces zero user-visible behavior change for existing Jellyfin users — it is a pure refactor + addition at the provider layer" and "Phase B: Modified existing stories 2.1, 2.5, 3.1, 4.8, 5.1 (in their original epic order)." Those Phase B story amendments have NOT been applied to the story text in their respective epics. The modified stories still carry pre-multi-provider ACs. The sequencing dependency is documented but the downstream stories were never updated.

---

#### Epic 9 — Rich Library Navigation

| Check | Result |
|---|---|
| User value | ✅ Browse modes, virtualized views, tracks mode |
| Independence | ✅ Builds on Epic 8 provider layer |
| AC quality | ✅ Well-detailed across 10 stories |
| FR traceability | ✅ FR8, FR39, FR41 |

🟡 **Minor**: Story 9.7 Technical Notes embed "superseded by Story 9.8 — the toggle is now a single global value" — stale in-story note that should be in changelog/history, not active story content.

✅ Story 9.10's `Depends on Story 9.9` is an acceptable within-epic dependency, documented and scoped to the provider contract that must precede the UI.

---

#### Epic 10 — Device Configuration Editing

| Check | Result |
|---|---|
| User value | ✅ Edit device settings post-initialization |
| Independence | ✅ Builds on device init (Story 2.6/2.9) |
| AC quality | ✅ Well-specified; relocation cleanup well-handled |
| FR traceability | ✅ FR36 |

✅ No quality violations found.

---

#### Epic 11 — Selection-as-Playlist & Curation

| Check | Result |
|---|---|
| User value | ✅ Save selection, curate playlists, reorder |
| Independence | ✅ Builds on Epic 8 provider layer |
| Story sizing | ⚠️ Story 11.6 is dense but manageable |
| AC quality | ⚠️ Stories 11.4 and 11.5 have pre-amendment ACs |
| FR traceability | ✅ FR37, FR38, FR40 |

🟠 **Major**: Story 11.4 ("Daemon RPCs — playlist.create / addTracks / removeTracks / delete") calls `require_provider()` before dispatch. After the 2026-06-09 architecture amendment, `require_provider()` routes through `ServerManager.selected_server_id`. The AC should add a server scope validation: if any `itemId` in the playlist.create call originated from a non-selected server, the daemon must return a scoped 409 error. This is not in the current ACs.

🟠 **Major**: Story 11.5 ("Basket Save as Playlist and Send to Playlist UI") does not mention: cross-server item exclusion notice (items from non-selected server are pre-filtered from the save dialog), or the fact that the "Save selection as playlist" action should only consider the selected server's items. The multi-server basket context is absent from these ACs.

---

### Dependency Analysis Summary

#### Cross-Epic Forward References

| Story | References | Assessment |
|---|---|---|
| Story 2.1 | "Story 8.4 factory" | ⚠️ Documented in Epic 8 preamble as intended prerequisite. Acceptable but increases sequencing risk. |
| Story 2.5 | "Story 8.4" | Same as above. |
| Story 2.3 | "auto-fill selection" (Story 3.8) | 🟡 Conceptual reference; 2.3 triggers auto-sync, 3.8 defines the slot. Manageable. |
| Story 4.5 | Story 3.8 autoFill params | 🟡 Same pattern — 4.5 is the trigger, 3.8 defines the input. Acceptable. |

#### Within-Epic Dependencies

All within-epic dependencies (9.9→9.10, 11.4→11.5, 11.4→11.7, 11.4→11.8, 11.9→11.10) are explicitly documented and ordered correctly. No circular dependencies found.

---

### Best Practices Compliance Summary

| Epic | User Value | Independent | Story Sizing | No Fwd Deps | Clear AC | FR Trace | Overall |
|---|---|---|---|---|---|---|---|
| Epic 1 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | 🟡 Minor only |
| Epic 2 | ✅ | ✅ | ✅ | ⚠️ | ⚠️ | ✅ | 🔴 Critical (2.11 missing) |
| Epic 3 | ✅ | ✅ | ✅ | ✅ | ⚠️ | ✅ | 🟠 Major (3.2 sparse, 3.8 stale) |
| Epic 4 | ✅ | ✅ | ✅ | ✅ | ⚠️ | ✅ | 🟠 Major (4.5 itemIds stale) |
| Epic 5 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ Pass |
| Epic 6 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ Pass |
| Epic 7 | ⚠️ | ✅ | ⚠️ | ✅ | ⚠️ | ✅ | 🟠 Major (technical epic, 7.1 oversized) |
| Epic 8 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | 🟠 Major (downstream amendments not applied) |
| Epic 9 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | 🟡 Minor (stale note in 9.7) |
| Epic 10 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ Pass |
| Epic 11 | ✅ | ✅ | ⚠️ | ✅ | ⚠️ | ✅ | 🟠 Major (11.4/11.5 stale) |

---

### Violations Register

#### 🔴 Critical

| ID | Epic | Story | Violation | Remediation |
|---|---|---|---|---|
| V1 | Epic 2 | Story 2.11 | Story spec does not exist in epics.md; referenced in sprint plan as next item to dev | Add Story 2.11 to Epic 2 using the AC from the 2026-06-09 change proposal Section 4 |

#### 🟠 Major

| ID | Epic | Story | Violation | Remediation |
|---|---|---|---|---|
| V2 | Epic 2 | Story 2.1 | Pre-amendment ACs — single-server vault, no `serverId` returned, no upsert-by-URL | Apply change proposal amendment to Story 2.1 ACs and Technical Notes |
| V3 | Epic 2 | Story 2.5 | Pre-amendment ACs — full-screen-only login, no inline "Add Server", no per-server re-auth | Apply change proposal amendment to Story 2.5 ACs and Technical Notes |
| V4 | Epic 3 | Story 3.2 | Only 2 ACs; missing `BasketItem.serverId`, locked-state rendering, no-device guard, error handling | Expand Story 3.2 ACs per architecture amendment; add serverId and locked-item ACs |
| V5 | Epic 3 | Story 3.8 | Pre-amendment ACs — `AutoFillSlot` has no `serverId`; no locked-slot behavior for non-selected server | Apply change proposal amendment to Story 3.8 |
| V6 | Epic 4 | Story 4.5 | `sync.start` Technical Notes show `itemIds: string[]`; should be `Array<{id, serverId}>` | Update Story 4.5 Technical Notes and IPC pattern section |
| V7 | Epic 7 | *(Epic)* | Epic description is a technical milestone ("address accumulated technical debt") — no user outcome | Rephrase epic goal to describe the user-facing reliability improvements delivered |
| V8 | Epic 7 | Story 7.1 | 13 ACs spanning 8 distinct technical items — oversized story | Consider splitting into Story 7.1a (WPD/COM Core) and Story 7.1b (concurrency + temp files + error surfaces) |
| V9 | Epic 7 | Story 7.5 | `load_secrets()` ACs describe returning a single `Secrets` blob — stale post vault restructuring | Update ACs to return `HashMap<String, ServerCredentials>` per architecture amendment |
| V10 | Epic 8 | *(Epic)* | "Phase B modified stories" (2.1, 2.5, 3.1, 4.8, 5.1) documented as planned but never applied | Apply Phase B amendments to the identified stories in their respective epics |
| V11 | Epic 11 | Story 11.4 | `playlist.create` has no server-scope validation AC; cross-server items should return 409 | Add AC: if itemIds contain items from a non-selected server, return scoped error |
| V12 | Epic 11 | Story 11.5 | "Save as playlist" UX has no cross-server item exclusion or pre-filter notice | Add ACs: pre-filter items to selected server; show informational notice for excluded items |

#### 🟡 Minor

| ID | Epic | Story | Violation | Remediation |
|---|---|---|---|---|
| V13 | Epic 1 | *(Epic)* | Technical description language ("multi-process Rust workspace") rather than user-benefit language | Update epic description to lead with user outcome |
| V14 | Epic 2 | Story 2.7 | "Status: superseded" in-story annotation left in active epic content | Remove Story 2.7 body or archive it; keep title/link for traceability only |
| V15 | Epic 3 | Story 3.6 | Full superseded story body left in active epic | Remove Story 3.6 body or archive it |
| V16 | Epic 9 | Story 9.7 | "superseded by Story 9.8" embedded in Technical Notes — stale in-story commentary | Remove supersession note; it belongs in history, not active AC content |
| V17 | Epics.md | Requirements Inventory | Only lists FR1–FR24, FR37–FR39 — FR25–FR36 and FR40–FR41 are in coverage map but not inventory | Update Requirements Inventory section to list all 41 FRs |

---

## Summary and Recommendations

### Overall Readiness Status

**⚠️ NEEDS WORK**

The planning artifacts are comprehensive and well-structured for the existing implemented scope (Epics 1–11 as originally written). However, the 2026-06-09 multi-server change proposal has not been fully propagated through the epics. The project cannot begin Story 2.11 development until the story spec exists. Six adjacent stories have stale pre-amendment ACs that will cause implementation drift if devved without correction.

---

### Issue Count Summary

| Severity | Count | Description |
|---|---|---|
| 🔴 Critical | 1 | Story 2.11 missing from epics.md |
| 🟠 Major | 12 | Stale ACs, oversized story, technical epic, amendment gaps |
| 🟡 Minor | 4 | Stale story bodies, cosmetic inventory gaps |
| **Total** | **17** | Across 4 categories |

---

### Critical Issues Requiring Immediate Action

**1. Story 2.11 does not exist in `epics.md`** (V1)

This is the highest priority item. The next sprint task is "dev Story 2.11" but no story spec exists in the epics document. The full AC is available in the 2026-06-09 change proposal (Section 4). Until this story is added to `epics.md`, the Create Story workflow and Dev Story workflow cannot be invoked reliably.

**Blocked by:** Nothing. The AC source exists. This is a copy/formalize task.

---

### Recommended Next Steps

The following are ordered by dependency — each step unblocks the next.

**Step 1 — Add Story 2.11 to `epics.md`** (resolves V1)
- Copy the Story 2.11 AC from `sprint-change-proposal-2026-06-09-multi-server-management.md` Section 4
- Insert it as Story 2.11 in Epic 2 of `epics.md`, after Story 2.10
- Update the `lastAmended` frontmatter and add `'multi-server-management'` to the `amendments` array
- Update the FR coverage map to add: `FR42: Epic 2 — Server Hub (Story 2.11)`

**Step 2 — Apply the 6 story amendments to `epics.md`** (resolves V2, V3, V4, V5, V11, V12)
- Story 2.1: Add multi-server vault ACs; `server.connect` returns `serverId`; upsert-by-URL semantics
- Story 2.5: Add first-run-only mode; inline "Add Server" affordance; per-server re-auth prompt
- Story 3.2: Add `BasketItem.serverId`; locked-rendering AC for non-selected server items; expand sparse ACs
- Story 3.8: Add `AutoFillSlot.serverId`; locked-slot behavior for non-selected server
- Story 11.4: Add server-scope validation AC for `playlist.create` (cross-server 409 error)
- Story 11.5: Add cross-server item exclusion notice and pre-filter behavior

**Step 3 — Apply remaining architectural amendment ACs** (resolves V6, V9)
- Story 4.5: Update Technical Notes to show `itemIds: Array<{id: string, serverId: string}>`
- Story 7.5: Update ACs to describe `HashMap<String, ServerCredentials>` vault structure

**Step 4 — UX Design for multi-server features** (addresses UX warning from Step 4)
- Design: Server Hub panel (add, remove, switch servers)
- Design: Read-only basket item visual state for non-selected server items
- Design: Inline "Add Server" form vs first-run full-screen flow
- Design: Per-server re-auth prompt modal
- This can run in parallel with Steps 1–3 if time allows

**Step 5 — Create Story 2.11 via `bmad-create-story` workflow**
- With Story 2.11 now in `epics.md`, invoke the create-story skill to produce the detailed development story with technical tasks
- Prerequisite: Steps 1–3 complete so the adjacent amended stories (2.1, 2.5) are consistent

**Step 6 — Dev Story 2.11**
- Once the story spec exists and adjacent stories are amended, dev can begin

**Optional cleanup** (resolves V7, V8, V13–V17):
- Rephrase Epic 7 description with user-benefit framing
- Consider splitting Story 7.1 into two stories
- Remove/archive superseded story bodies (Stories 2.7, 3.6)
- Fix minor stale notes (Story 9.7 Technical Notes)
- Update the Requirements Inventory section in `epics.md` to include FR25–FR41

---

### Risks If Proceeding Without Remediation

| Risk | Likelihood | Impact |
|---|---|---|
| Dev agent uses stale Story 2.1/2.5 ACs → implements single-server credential replacement | High | High — will conflict with Story 2.11 architecture |
| Story 3.2 sparse ACs → basket implemented without serverId | High | High — all multi-server basket routing breaks |
| Story 4.5 old itemIds format → sync.start doesn't group by serverId | Medium | High — multi-server sync not routable |
| Story 7.5 old Secrets struct → vault restructure incompatible with multi-server | Medium | Medium — vault migration needed post-hoc |
| Story 2.11 missing spec → dev guesses requirements from change proposal alone | High | Medium — change proposal has enough detail but is not story-formatted |

---

### Final Note

This assessment identified **17 issues across 4 categories**. The 1 critical issue (Story 2.11 missing) and the 6 high-priority story amendment gaps are all traceable to the same root cause: the 2026-06-09 multi-server change proposal was approved and the architecture was updated, but the downstream propagation to `epics.md` story ACs was not completed.

The planning artifacts are otherwise in strong shape — FR coverage is 100% for current scope, the UX and architecture are well-aligned for pre-multi-server features, and the epic structure is sound. The remediation work described above is primarily copy/formalization from the already-approved change proposal. Steps 1–3 are low-effort, high-value tasks that will put the project in a **READY** state for Story 2.11 development.

**Assessment Date:** 2026-06-09
**Assessor:** BMad Implementation Readiness Workflow


