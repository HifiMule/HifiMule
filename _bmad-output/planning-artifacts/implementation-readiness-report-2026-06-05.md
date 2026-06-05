---
stepsCompleted: ["step-01-document-discovery", "step-02-prd-analysis", "step-03-epic-coverage-validation", "step-04-ux-alignment", "step-05-epic-quality-review", "step-06-final-assessment"]
documentInventory:
  prd: "_bmad-output/planning-artifacts/prd.md"
  architecture: "_bmad-output/planning-artifacts/architecture.md"
  epics: "_bmad-output/planning-artifacts/epics.md"
  ux: "_bmad-output/planning-artifacts/ux-design-specification.md"
---

# Implementation Readiness Assessment Report

**Date:** 2026-06-05
**Project:** HifiMule

---

## PRD Analysis

### Functional Requirements

FR1: The system can automatically detect Mass Storage (USB MSC) and MTP (Media Transfer Protocol) devices on Windows, Linux, and macOS.
FR2: Users can manually select a target device folder if automatic detection fails. Manual fallback applies to Mass Storage devices only; MTP devices must be detected automatically via the OS device manager.
FR3: The system can identify the presence of a `.hifimule.json` manifest on discovery.
FR4: The system can read persistent hardware identifiers to link devices across different sessions. When multiple managed devices are connected simultaneously, the system tracks all of them and allows the user to select the active device context.
FR5: Users can configure media server credentials (URL, server type, username, and either an API token for Jellyfin or username+password for Subsonic/OpenSubsonic servers). The system auto-detects the server type by pinging the URL when the user enters it.
FR6: Users can select a specific user profile from the connected media server for syncing.
FR7: The system can maintain a persistent, encrypted connection state to the configured media server. For Jellyfin, the access token is stored. For Subsonic/OpenSubsonic, the user password is stored (encrypted) and used to sign each request stateless-style.
FR8: Users can browse music from the connected media server through server-supported navigation modes: Playlists, Artists, Albums, Genres, Recently Added, Frequently Played, Recently Played, and Favorites. The provider abstraction normalizes these browse modes across Jellyfin, Navidrome, Subsonic, and OpenSubsonic-compatible servers. Unsupported modes are hidden or clearly unavailable based on provider capabilities.
FR9: Users can select specific playlists or entities for synchronization.
FR10: The system can report real-time storage availability on the target device.
FR11: Users can see a preview of "Proposed Changes" (files to add, remove, or update) before starting a sync.
FR12: The system can perform a differential sync based on the local manifest.
FR13: The system can protect unmanaged user files from deletion or modification.
FR14: The system can stream media files directly from the Jellyfin server to the device via memory-to-disk buffering, using the appropriate device IO backend (filesystem writes for MSC devices, WPD/libmtp object transfers for MTP devices).
FR15: The system can validate hardware-specific constraints (path length, character sets) before writing files.
FR16: The system can resume an interrupted sync session without restarting from scratch.
FR17: The system can detect Rockbox `.scrobbler.log` files on connected devices.
FR18: The system can report completed track plays to the Jellyfin server via the Progressive Sync API.
FR19: The system can track which scrobbles have already been submitted to prevent duplication.
FR20: The system can run as a background service (headless) with minimal resource usage. MVP: Tauri sidecar process. Post-MVP: OS-native user-session daemon.
FR21: Users can toggle "Launch on Startup" behavior.
FR22: The system can provide tray-icon status updates for sync progress and hardware state.
FR23: The system can send OS-native notifications for sync completion or errors.
FR25: The system retrieves and displays only music-centric content (Playlists, Albums, Artists, Tracks) with type-appropriate filtering per server type.
FR26: The system can initialize a new `.hifimule.json` manifest on a connected device, capturing hardware identifier, music sync folder path, playlist folder path, media-server user profile, display name, and optional icon identifier.
FR27: The system can be packaged into platform-native installers (MSI for Windows, DMG for macOS, AppImage/.deb for Linux) using the Tauri v2 bundler.
FR28: The build pipeline can produce signed, distributable artifacts for all three target platforms from a single CI workflow.
FR29: The system can reserve capacity in the sync basket via a virtual Auto-Fill slot; at sync time the daemon expands the slot using the priority algorithm (favorites first, then by play count, then by creation date) up to available capacity or a user-defined size limit.
FR30: The system can automatically trigger synchronization when a known, previously configured device is detected, without requiring user interaction.
FR31: The system can negotiate a transcoded stream URL from the Jellyfin server using a device-specific DeviceProfile payload, falling back to direct download when direct play is supported or transcoding fails.
FR32: The system can list available device transcoding profiles and assign one to a connected device, persisting the selection in both the device manifest and the local database.
FR33: The system presents a persistent device hub showing all connected managed devices, each identified by name and icon. The user can switch the active device context at any time. When no device is selected, the basket is empty and adding items is disabled.
FR34: The system can add an artist to the sync basket as a single entity reference; at sync time the daemon resolves the artist to its current track list, ensuring tracks added to the artist after basket construction are automatically included.
FR35: The system supports Jellyfin, Navidrome, Subsonic, and any OpenSubsonic-compatible media server. Server type is auto-detected at connection time by pinging the server URL. Detected capability extensions (OpenSubsonic) are cached and used to enable per-server features.
FR36: The system can edit an existing managed device manifest, allowing users to change device name, icon, transcoding profile, music folder, and playlist folder. Folder changes are reflected in the next sync preview and trigger managed relocation cleanup before new items are written.

**Total FRs: 35** (FR1–FR23, FR25–FR36; FR24 is absent from PRD — numbering gap flagged)

---

### Non-Functional Requirements

NFR1 (Performance — Memory): The headless Rust engine must consume < 10MB of RAM during idle states.
NFR2 (Performance — Sync Readiness): The system must complete a manifest audit and be "ready to sync" in < 5 seconds.
NFR3 (Performance — Throughput): Sync operations should be limited only by the target hardware's write speed or network bandwidth to the server.
NFR4 (Reliability — Write Integrity): The system must utilize OS-level file sync primitives (`sync_all`) to flush data to device before marking sync complete.
NFR5 (Reliability — Atomic Manifest): The `.hifimule.json` manifest must be updated atomically to prevent corruption during unexpected power loss or disconnection.
NFR6 (Reliability — Network): The system must handle network interruptions during buffered streaming, attempting to resume for at least 3 retry cycles.
NFR7 (Reliability — Hardware Disconnect): Mid-sync ejections must not result in unbootable/unmountable media; the system must gracefully mark the session as "Interrupted" and trigger Repair Utility on reconnection.
NFR8 (Cross-Platform — Feature Parity): 100% feature parity between Windows, Linux, and macOS distributions.
NFR9 (Cross-Platform — macOS Compliance): Application must adhere to modern macOS filesystem permission models; no root/sudo privileges required.
NFR10 (Cross-Platform — Resource Consistency): Memory and CPU usage must remain within a 15% delta across all supported OS environments.
NFR11 (Security — Credentials): Server tokens/passwords must be stored using OS-native secure storage (Windows Credential Manager, macOS Keychain).
NFR12 (Security — Privacy): All media synchronization occurs locally between the server and target device; zero user data is transmitted to third-party servers.
NFR13 (Maintainability — CLI-First): The core engine must remain fully functional and testable via CLI independent of the detached UI.
NFR14 (Maintainability — Tooling): The project should follow established Rust workspace patterns.

**Total NFRs: 14**

---

### Additional Requirements & Constraints

- **Tech Stack:** Rust (headless sync engine) + Tauri v2 (UI and packaging)
- **Developer:** Solo developer
- **Platforms:** Windows, Linux, macOS
- **Update Strategy:** Manual updates only for MVP; no built-in auto-update mechanism
- **Device IO:** MSC devices use filesystem writes; MTP devices use WPD/libmtp object transfers
- **`device-profiles.json`:** Stored in app data directory; editable by user; passthrough (direct download) is default
- **Measurable Outcomes:** < 5s device-to-ready state; < 10s incremental update (90%+ already on device); 100% scrobble accuracy for matched items
- **Destructive Safety Protocol:** Mandatory manual confirmation for manifest repair/cleanup exceeding 100MB of data deletion

---

### PRD Completeness Assessment

The PRD is well-structured and covers a broad range of functional and non-functional requirements. **One gap noted: FR24 is absent** — the numbering jumps from FR23 to FR25. This may be a deleted requirement or a drafting omission; it should be confirmed with the author.

The PRD references several "Post-MVP" features (Scrobble Queue & Retry, Manifest Repair GUI, Wi-Fi Sync, Smart Playlists, OS-native daemon) that are noted but not formally numbered as requirements. These are in scope for growth phases and should be verified not to have leaked into the epics prematurely.

---

## Epic Coverage Validation

### Coverage Matrix

| FR # | PRD Description (short) | Epic Coverage | Status |
|------|------------------------|---------------|--------|
| FR1 | Auto-detect MSC/MTP devices (Win/Linux/Mac) | Epic 2 — Story 2.2, 2.10 | ✓ Covered |
| FR2 | Manual device folder fallback (MSC only) | Epic 2 — Story 2.2 | ✓ Covered |
| FR3 | Identify `.hifimule.json` on discovery | Epic 2 — Story 2.2 | ✓ Covered |
| FR4 | Persistent hardware IDs; multi-device tracking | Epic 2 — Stories 2.3, 2.7, 2.8 | ✓ Covered |
| FR5 | Configure server credentials (Jellyfin + Subsonic); auto-detect type | Epic 2 — Story 2.1; Epic 8 — Story 8.4 | ✓ Covered (note: epics FR5 text is Jellyfin-only — incomplete copy vs PRD, but implementation covers full scope via Epic 8) |
| FR6 | Select server user profile | Epic 2 — Story 2.5 | ✓ Covered |
| FR7 | Persistent, encrypted connection state (Jellyfin token + Subsonic password) | Epic 2 — Story 2.1; Story 7.5 | ✓ Covered (note: epics FR7 text is Jellyfin-only — incomplete copy, but Epic 7 Story 7.5 covers full implementation) |
| FR8 | Browse all server-supported navigation modes; provider abstraction | Epic 3 — Story 3.1; Epic 9 — Stories 9.1–9.6 | ✓ Covered |
| FR9 | Select playlists/entities for sync | Epic 3 — Story 3.2 | ✓ Covered |
| FR10 | Real-time storage availability on device | Epic 3 — Story 3.3 | ✓ Covered |
| FR11 | Preview "Proposed Changes" before sync | Epic 3 — Story 3.2 | ✓ Covered |
| FR12 | Differential sync from manifest | Epic 4 — Story 4.1 | ✓ Covered |
| FR13 | Protect unmanaged user files | Epic 3 — Story 3.4 | ✓ Covered |
| FR14 | Buffered streaming to device (MSC/MTP IO backends) | Epic 4 — Stories 4.2, 4.0 | ✓ Covered |
| FR15 | Validate path length / character constraints | Epic 4 — Story 4.3 | ✓ Covered |
| FR16 | Resume interrupted sync | Epic 4 — Story 4.4 | ✓ Covered |
| FR17 | Detect Rockbox `.scrobbler.log` | Epic 5 — Story 5.1 | ✓ Covered |
| FR18 | Report plays to server via Progressive Sync API | Epic 5 — Story 5.1 | ✓ Covered |
| FR19 | Track submitted scrobbles (deduplication) | Epic 5 — Story 5.2 | ✓ Covered |
| FR20 | Headless background service | Epic 1 — Story 1.1; Epic 6 — Stories 6.7, 6.2–6.4 | ✓ Covered |
| FR21 | Toggle "Launch on Startup" | Epic 1 — Story 1.2; Epic 6 — Story 6.7 | ✓ Covered |
| FR22 | Tray-icon status updates | Epic 1 — Story 1.2 | ✓ Covered |
| FR23 | OS-native notifications | Epic 5 — Story 5.3 | ✓ Covered |
| FR25 | Music-only content filtering (by server type) | Epic 3 — Story 3.5 | ⚠️ Functionally covered but **MISSING from FR Coverage Map** in epics.md |
| FR26 | Initialize `.hifimule.json` manifest (name, icon, profile, folders) | Epic 2 — Stories 2.6, 2.9; Epic 10 — Story 10.3 | ✓ Covered |
| FR27 | Platform-native installers (MSI/DMG/AppImage/.deb) | Epic 6 — Story 6.1–6.4 | ✓ Covered |
| FR28 | CI/CD cross-platform signed artifact pipeline | Epic 6 — Story 6.5 | ✓ Covered |
| FR29 | Auto-Fill virtual slot with priority algorithm at sync time | Epic 3 — Story 3.8 | ✓ Covered |
| FR30 | Auto-sync on known device connect | Epic 2 — Story 2.3; Epic 4 — Story 4.5 | ✓ Covered |
| FR31 | Transcoding stream negotiation (Jellyfin PlaybackInfo) | Epic 4 — Story 4.8 | ✓ Covered |
| FR32 | List/assign transcoding profiles; persist to manifest + DB | Epic 4 — Story 4.8 | ✓ Covered |
| FR33 | Persistent device hub; switch active device context | Epic 2 — Story 2.8 | ✓ Covered |
| FR34 | Artist entity basket item; resolve at sync time | Epic 3 — Story 3.9 | ✓ Covered |
| FR35 | Multi-provider support (Jellyfin/Navidrome/Subsonic/OpenSubsonic) | Epic 8 — Stories 8.1–8.6 | ✓ Covered |
| FR36 | Edit managed device manifest (name, icon, profile, folders) | Epic 10 — Stories 10.1–10.3 | ✓ Covered |

---

### FRs in Epics NOT in PRD (Documentation Gaps)

| FR # | Epic Description | Issue |
|------|-----------------|-------|
| FR24 | Startup splash screen with connection status (Story 2.4) | Present in epics Requirements Inventory and Coverage Map, but **absent from PRD Functional Requirements**. Added during epic creation — PRD was never updated. Sprint-status shows Story 2.4 is `done`. |
| FR37 | Persist basket selection as server playlist (write-back) | Added in `sprint-change-proposal-2026-06-05`. Present in epics Requirements Inventory. **PRD has NOT been updated** yet — sprint change proposal explicitly called for PRD update as a prerequisite step. |
| FR38 | Dual-panel playlist curation view + right-click "send to playlist" + statistics | Same as FR37 — in epics, absent from PRD. |
| FR39 | Virtualized list/table browse view for thousands of items | Same as FR37 — in epics, absent from PRD. |

---

### Missing Requirements

#### FR25 — Coverage Map Omission (Minor)

**FR25:** The system retrieves and displays only music-centric content (Playlists, Albums, Artists, Tracks), with type-appropriate filtering per server type.

- **Impact:** Low. Story 3.5 fully implements this requirement. The gap is purely documentation — the FR Coverage Map jumps from FR23 to FR27 without listing FR25.
- **Recommendation:** Add `FR25: Epic 3 - Music-Only Library Filtering (Story 3.5)` to the FR Coverage Map in `epics.md`.

#### FR24 — PRD Omission (Minor)

**FR24:** Startup splash screen with connection status.

- **Impact:** Low. The story is already implemented (`done`). The gap is that the PRD formal requirements section does not enumerate this requirement, though the epics document includes it.
- **Recommendation:** Either add FR24 to the PRD's Functional Requirements section, or accept the epics as the authoritative source for this requirement.

#### FR37, FR38, FR39 — PRD Not Updated After Sprint Change Approval (Moderate)

- **Impact:** Moderate. The sprint change proposal (approved 2026-06-05) explicitly listed "PRD Update (add FR37, FR38, FR39 + NFR addenda)" as **Step 1** in the implementation handoff. The stories are currently in `backlog` status, so this gap must be resolved before implementation of Epic 11 begins.
- **Recommendation:** Update `prd.md` to add FR37, FR38, FR39 and the two NFR addenda (security/privacy note on write-scope; performance note on virtualized list rendering) per the sprint change proposal Section 4.1 before any Epic 11 story moves to `in-progress`.

---

### NFR Coverage

| NFR # | PRD Description | Epics Mapping | Status |
|-------|----------------|---------------|--------|
| NFR1 | < 10MB RAM idle | Epics NFR1 ✓ | ✓ |
| NFR2 | < 5s manifest audit + ready-to-sync | Epics NFR2 ✓ | ✓ |
| NFR3 | Throughput limited by hardware/network only | Epics NFR3 ✓ | ✓ |
| NFR4 | Write-Verify-Commit (`sync_all`) | Epics NFR4 ✓ | ✓ |
| NFR5 | Atomic manifest updates | Epics NFR5 ✓ | ✓ |
| NFR6 | 3 retry cycles on network interruption | Epics NFR6 ✓ | ✓ |
| NFR7 | Graceful "Interrupted" + repair trigger on mid-sync eject | Epics NFR7 ✓ | ✓ |
| NFR8 | 100% cross-platform feature parity | Epics NFR8 ✓ | ✓ |
| NFR9 | macOS sandbox compliance (no root/sudo) | Epics NFR9 ✓ | ✓ |
| NFR10 | Resource usage within 15% delta across OSes | Epics NFR10 ✓ | ✓ |
| NFR11 | **PRD:** OS-native secure storage (Keychain/Credential Manager). **Epics:** Hardware-bound encrypted file (`secrets.enc`, machine-uid + blake3 + ChaCha20-Poly1305) | ⚠️ **Divergence** — implementation (Story 7.5) changed approach from OS-native keyring to machine-bound file, but PRD was not updated | ⚠️ Divergence |
| NFR12 | Zero third-party data transmission | Epics NFR12 ✓ | ✓ |
| NFR13 | CLI-first architecture (core engine testable without UI) | Epics NFR13 ✓ | ✓ |
| NFR14 | Standard Rust workspace patterns | **NOT in epics NFR list** | ⚠️ Not mapped (minor) |

---

### Coverage Statistics

- **Total PRD FRs:** 35 (FR1–FR23, FR25–FR36; FR24 absent from PRD)
- **FRs explicitly in epics Coverage Map:** 34 out of 35 (FR25 omitted from map but functionally covered)
- **FRs functionally implemented:** 35 of 35 (100%)
- **FRs from epics absent from PRD:** 4 (FR24 from epic creation; FR37, FR38, FR39 from approved sprint change proposal pending PRD update)
- **Epic/story implementation status:** Epics 1–10 all `done`; Epic 11 + Story 9.7 in `backlog` (next to implement)
- **NFR coverage:** 12/14 fully mapped; 1 divergence (NFR11 — implementation changed approach, PRD not updated); 1 unmapped but low-risk (NFR14)

---

## UX Alignment Assessment

### UX Document Status

**Found:** `ux-design-specification.md` (10.3 KB, completed 2026-01-27)

---

### UX ↔ PRD Alignment

| Area | UX Spec | PRD | Status |
|------|---------|-----|--------|
| Core UX model — "Delta-Sync Handshake" | §2.1: Differential scan on device connect, "Live Delta" in basket | FR11, FR12: Proposed Changes preview + differential sync | ✓ Aligned |
| Target personas | Ritualist (Arthur), Sprinter/Sarah, Admin (Alexis) | Arthur/Sarah/Alexis user journeys | ✓ Aligned |
| Auto-Fill (virtual slot model) | §5.3: Lazy slot card, no API call until sync time | FR29: Auto-Fill virtual slot expanded at sync time | ✓ Aligned |
| Auto-Fill superseded eager model | §5.3: Auto Badge/Priority Reason Tags removed | Story 3.6 marked "Superseded by 3.8" | ✓ Aligned |
| Server type auto-detection badge | §5.2: Live badge on login screen | FR5/FR35: Auto-detect server type on URL entry | ✓ Aligned |
| Device Identity + Folders dialog | §5.4: Name, icon, transcoding profile, music/playlist folders | FR26, FR32, FR36, Epic 10 | ✓ Aligned |
| Device Hub | §5.6: Persistent hub, no-device-selected state, always visible | FR33, Stories 2.8/2.9 | ✓ Aligned |
| Browse modes | §5.1: Compact browse-mode control, unsupported provider modes hidden | FR8, FR25, Epic 9 | ✓ Aligned |
| Headless feedback (no UI) | §5.5: Tray animation + OS notification | FR22, FR23 | ✓ Aligned |
| Responsive/accessibility | §6: Detachable sidebar, WCAG 2.1 AA, ARIA-live | PRD NFRs, Epics Additional Requirements | ✓ Aligned |
| **Epic 11 — Playlist Curation View** | **MISSING from UX spec** | FR38: Dual-panel curation + right-click + stats | ❌ Not in UX spec |
| **Epic 11 — "Save as playlist" basket action** | **MISSING from UX spec** | FR37: Persist selection as server playlist | ❌ Not in UX spec |
| **Story 9.7 — List/Table Browse View** | **MISSING from UX spec** | FR39: Virtualized list/table browse for large libraries | ❌ Not in UX spec |

---

### UX ↔ Architecture Alignment

| Area | UX Spec | Architecture | Status |
|------|---------|--------------|--------|
| UI Framework | Shoelace + Vanilla TypeScript + Tauri v2 | Tauri v2, Webview, Vanilla TS | ✓ Aligned |
| 70/30 Basket Centric layout | §4.1 | IPC contract supports basket + library split | ✓ Aligned |
| Live server type badge | §5.2 | `server.connect` with `serverType: 'auto'` | ✓ Aligned |
| Auto-fill IPC model | §5.3: No API at toggle; expansion in `sync.start` | Architecture: `sync.start` `autoFill` param; `basket.autoFill` debug-only | ✓ Aligned |
| Device Hub IPC | §5.6: `device.list`, `device.select`, `device.update_manifest` | Architecture fully defines all three RPCs | ✓ Aligned |
| Transcoding profile selector | §5.4 | `device_profiles.list` + `device.set_transcoding_profile` | ✓ Aligned |
| ARIA-live for sync progress | §6.3 | `on_sync_progress` event stream + ARIA-live (Story 4.5 AC) | ✓ Aligned |
| Credential security (UX seamless) | §5 implies transparent auth | Architecture: `secrets.enc` hardware-bound vault (Story 7.5) | ✓ Aligned — UX unaffected |
| **Epic 11 RPCs defined in architecture** | **Not reflected in UX spec** | Architecture amended: `playlist.create/addTracks/removeTracks/delete` | ⚠️ Architecture ahead of UX |

---

### Alignment Warnings

#### ⚠️ WARNING: UX Spec Not Updated for Epic 11 (Moderate Risk)

The sprint change proposal (approved 2026-06-05) explicitly listed the following as required UX spec updates **before Epic 11 implementation begins** (Section 4.3):

1. **§5.2 Custom Components — add:**
   - **Playlist Curation View:** Dual-panel; left panel = artists in playlist, right panel = their albums filtered to playlist. Remove-artist and remove-album affordances. Statistics header (track count, total duration, total size).
   - **Context Menu (right-click):** On artists/albums in browse and curation views — "Send to playlist…" and remove actions.

2. **§5 Component Strategy — add:**
   - **List/Table Browse View:** Virtualized list/table mode for Artist and Album pages as an alternative to paginated album-art grids. Handles thousands of items. Honors existing A–Z quick-nav and breadcrumbs.

3. **Basket (§5.2) — add:**
   - **"Save selection as playlist" action:** In basket header; prompts for playlist name or pick existing managed playlist. Shows inline Auto-Fill exclusion notice when slot is present.

**Current state:** UX spec has NOT been updated. Architecture was amended (last amended 2026-06-05). Architecture is ahead of UX spec for Epic 11.

**Recommendation:** Update `ux-design-specification.md` per sprint change proposal Section 4.3 before any Epic 11 story or Story 9.7 moves to `ready-for-dev`. This is a **blocking gap for Epic 11 readiness**.

#### ✓ PASS: No UX gaps for Epics 1–10

All implemented features (Epics 1–10) have complete UX specification coverage. No post-implementation UX regressions identified.

---

## Epic Quality Review

### Review Scope

Epics 1–10 are `done`. This review focuses on identifying any structural defects that could create rework risk, and on validating Epic 11 + Story 9.7 (`backlog`) as ready for implementation.

---

### Epic-Level Validation

| Epic | Title | User Value | Independent | Verdict |
|------|-------|-----------|-------------|---------|
| 1 | Foundation & Project Genesis | Tray hub is user-visible; workspace init is greenfield necessity | ✓ — foundation | 🟡 Minor: "Foundation" is partially technical. Acceptable for Epic 1 greenfield. |
| 2 | Connection & Verification | Users can connect to server + recognize device | ✓ Depends on Epic 1 only | ✓ Good |
| 3 | The Curation Hub (Basket & Library) | Users can browse library and build sync basket | ✓ Depends on Epics 1–2 | ✓ Good |
| 4 | The Sync Engine & Self-Healing Core | Users can sync to their device | ✓ Depends on Epics 1–3 | 🟡 Minor: Story 4.0 is technical (IO abstraction) — covered below |
| 5 | Ecosystem Lifecycle & Advanced Tools | Scrobbling, notifications, repair | ✓ Depends on Epics 1–4 | ✓ Good |
| 6 | Packaging & Distribution | Users can install the app | ✓ Depends on Epics 1–5 | ✓ Good |
| 7 | Technical Hardening & Deferred Fixes | Stability / security improvements | ✓ Depends on Epics 1–6 | 🟠 Major: Technical epic name — no user value in title. Stories do have user benefit. See below. |
| 8 | Multi-Provider Media Server Support | Users can connect Navidrome/Subsonic | ✓ Depends on Epics 1–2 | 🟡 Minor: "Provider abstraction" framing is technical; user value is real but indirect for Stories 8.1–8.2. |
| 9 | Rich Library Navigation | Full browse mode coverage | ✓ Depends on Epics 1–8 | ✓ Good |
| 10 | Device Configuration Editing | Edit device without reinitializing | ✓ Depends on Epics 1–3 | ✓ Good |
| 11 | Selection-as-Playlist & Curation | Save/curate device selections as server playlists | ✓ Depends on Epics 1–9 | 🟡 Minor: Story 11.1 is technically framed — see below |

---

### 🔴 Critical Violations

**None identified.** No epics have forward dependencies, no circular dependencies exist, and no epics are so large they cannot be delivered incrementally.

---

### 🟠 Major Issues

#### 1. Epic 7 is a Technical Debt Epic

**Epic 7: Technical Hardening & Deferred Fixes** violates the user-value principle for epic naming. The title describes process ("deferred fixes", "hardening") rather than user outcomes. Story titles within it are also technically framed:

- Story 7.1: "MTP IO & WPD Hardening" — technical
- Story 7.2: "DeviceManager Concurrency Refactor" — technical
- Story 7.4: "Packaging & CI/CD Hardening" — technical
- Story 7.5: "Machine-Bound Credential Vault (Replace Keyring)" — borderline (security user value)

**Impact:** Low — all stories are `done`. This is a retrospective observation, not a blocker.

**Recommendation:** For future reference, tech debt items are better folded into the epics they serve, or renamed with user-outcome framing (e.g., "MTP Device Reliability" instead of "MTP IO & WPD Hardening").

---

### 🟠 Pre-Implementation Gap: Sprint Change Proposal RPC Name Drift

The sprint change proposal (Section 4.2) defines `update_playlist(playlist_id, track_ids)` (full replace). The epics (Stories 11.1–11.4) and architecture use `add_to_playlist` + `remove_from_playlist` (diff-based). The stories and architecture are internally consistent with each other. The proposal was an earlier draft.

**Impact:** Low — the stories are the implementation spec. A developer reading only the proposal could be confused.

**Recommendation:** Add a note to the sprint change proposal or the Epic 11 header that `update_playlist` was refined to `add_to_playlist`/`remove_from_playlist` in the final design.

---

### 🟡 Minor Concerns

#### 2. Story 4.0 — Technical Story

**Story 4.0: Device IO Abstraction Layer** is framed as "As a System Admin, I want all device file operations to go through a single abstract interface" — a technical requirement masquerading as a user story. The value to the user (MTP device support) is indirect. The AC is testable and the story is done; this is retrospective only.

#### 3. Story 11.1 — Technical Story Framing

**Story 11.1: MediaProvider Playlist-Write Trait Amendment** — "As a System Admin... I want the MediaProvider trait to expose playlist write operations." The user value is architectural (enables 11.2–11.6) rather than directly experienced by the user. The technical notes are excellent but the user story framing is weak.

**Recommendation:** Story 11.1 could be rephrased: "As a user connected to Jellyfin or Navidrome, I want the system to be able to create and modify playlists on my server, so that I can save my device selection as a durable playlist." The implementation detail (trait amendment) belongs in Technical Notes, not the user story itself.

This does NOT block implementation — the ACs and technical notes are clear and correct.

#### 4. Within-Epic Ordering for Epic 11 — Well-Documented but Sequential

Epic 11's stories have strict within-epic ordering:
```
11.1 → (11.2 ‖ 11.3) → 11.4 → 11.5 → 11.6
9.7 (independent)
```

Story 11.2 and 11.3 cannot start until 11.1's `MediaProvider` trait amendment is in place. Story 11.4 cannot start until the adapters (11.2, 11.3) are done. This is correctly documented in the epic header.

**Impact:** None for a solo developer. With multiple developers, Story 11.2 and 11.3 can be parallelized in a worktree after 11.1 is done.

**Recommendation:** No change needed — the sequencing is correctly documented in the epic.

---

### Story Quality Assessment

#### AC Format Compliance (Given/When/Then)

All stories use proper BDD Given/When/Then format. ✓

#### Error Condition Coverage

Reviewed for backlog stories (Epic 11 + Story 9.7):

| Story | Error Paths Present | Notes |
|-------|-------------------|-------|
| 11.1 | ✓ | `ProviderError::NotSupported` path covered |
| 11.2 | ✓ | HTTP mock, auth error coverage noted |
| 11.3 | ✓ | Classic Subsonic + OpenSubsonic shapes covered |
| 11.4 | ✓ | `require_provider()` failure, unsupported capability error |
| 11.5 | ✓ | Capability-gating (hide affordance when unsupported) |
| 11.6 | ✓ | Missing `sizeBytes` excluded from total without error |
| 9.7 | ✓ | No new error paths — pure UI rendering |

#### Story Sizing

All Epic 11 stories are reasonably sized — each delivers one named, independently releasable behavior within the epic's ordering constraints. No story appears bloated beyond a sprint's work.

---

### Dependency Analysis Summary

**Cross-epic dependencies (backlog stories only):**

| Story | Depends On | Status |
|-------|-----------|--------|
| 11.1 | `MediaProvider` trait (providers/mod.rs) — Epic 8 | `done` ✓ |
| 11.1 | `Capabilities` struct — Epic 8 | `done` ✓ |
| 11.2 | JellyfinProvider (providers/jellyfin.rs) — Epic 8 | `done` ✓ |
| 11.3 | SubsonicProvider (providers/subsonic.rs) — Epic 8 | `done` ✓ |
| 11.4 | `rpc.rs:807–866` container expansion — Epic 3 | `done` ✓ |
| 11.4 | `require_provider()` helper — Epic 8 | `done` ✓ |
| 11.5 | `capabilities().supports_playlist_write` — Story 11.1 | within-epic, ordered ✓ |
| 11.6 | `browse.getPlaylist` RPC — Epic 3/9 | `done` ✓ |
| 9.7 | `browse.*` RPC layer + `library.ts` — Epics 3/9 | `done` ✓ |

**No forward dependencies identified.** All cross-epic dependencies are on completed work.

---

### Best Practices Compliance Summary

| Check | Epics 1–10 (done) | Epic 11 + 9.7 (backlog) |
|-------|------------------|------------------------|
| Delivers user value | ✓ (minor issues noted: Epic 7, Story 4.0) | ✓ (minor: Story 11.1 framing) |
| Epic independence | ✓ | ✓ |
| No forward dependencies | ✓ | ✓ (within-epic ordering documented) |
| Testable ACs in Given/When/Then | ✓ | ✓ |
| Error conditions covered | ✓ | ✓ |
| Technical Notes support implementation | ✓ | ✓ — high quality |
| FR traceability maintained | ✓ (minor: FR25 missing from map) | ✓ (FR37–39 in epics; PRD pending update) |

---

## Summary and Recommendations

### Overall Readiness Status

| Scope | Status |
|-------|--------|
| Epics 1–10 (all `done`) | ✅ **COMPLETE** — No action required |
| Story 9.7 (`backlog`) | ✅ **READY** — Independent UI story, no blockers |
| Epic 11 Stories 11.1–11.6 (`backlog`) | ⚠️ **NEEDS WORK** — 2 required documentation updates before implementation begins |

**Overall verdict: CONDITIONALLY READY**

The project's implemented foundation is solid. Story 9.7 can begin immediately. Epic 11 requires two documentation updates before any of its stories move to `ready-for-dev`.

---

### Required Actions Before Epic 11 Implementation

#### ACTION 1 (Required — PRD Update) 🟠

**Update `prd.md` to add FR37, FR38, FR39 and NFR addenda.**

The sprint change proposal (Section 4.1, approved 2026-06-05) defines the exact text for each addition. These requirements exist in the epics and architecture but not in the PRD — creating a traceability gap that would fail any future requirements audit.

- Add FR37 (playlist write-back) to Section 6, Group 3 (Content Selection)
- Add FR38 (dual-panel curation + right-click + stats) to Section 6
- Add FR39 (virtualized list/table browse view) to Section 6
- Add NFR security/privacy note on write-scope (Section 7.4)
- Add NFR performance note on virtualized rendering (Section 7.1)

**Who:** PM/Product Owner (Alexis)
**When:** Before any Epic 11 story moves to `ready-for-dev`

---

#### ACTION 2 (Required — UX Spec Update) 🟠

**Update `ux-design-specification.md` with Epic 11 UI components.**

The sprint change proposal (Section 4.3) defines the exact additions needed. Without this, developers implementing Epic 11's UI stories (11.5, 11.6) have no UX specification to reference.

- §5.2 — Add: Playlist Curation View (dual-panel component spec)
- §5.2 — Add: Context Menu (right-click "send to playlist" spec)
- §5 Component Strategy — Add: List/Table Browse View (virtualized, for Story 9.7 and Epic 11)
- Basket (§5.2) — Add: "Save selection as playlist" action spec (with Auto-Fill exclusion notice behavior)

**Who:** UX/PM (Alexis)
**When:** Before any Epic 11 story moves to `ready-for-dev`; before Story 9.7 moves to `ready-for-dev` (for the list/table view spec)

---

### Recommended Documentation Cleanups (Non-Blocking)

These are minor issues that will not block implementation but should be addressed to keep the artifact set consistent:

1. **Add `FR25: Epic 3 - Music-Only Library Filtering (Story 3.5)` to the FR Coverage Map in `epics.md`** — the only FR missing from the coverage map.

2. **Add FR24 to the PRD Functional Requirements section** (or accept epics as authoritative) — FR24 exists in epics but not in PRD. Since Story 2.4 is done, this is a retrospective documentation gap.

3. **Update PRD NFR11** to reflect the implemented approach: machine-bound encrypted vault (`secrets.enc`) instead of OS-native keyring. Story 7.5 changed this by design; the PRD should reflect the actual implementation.

4. **Add a note to the sprint change proposal** clarifying that `update_playlist` in Section 4.2 was refined to `add_to_playlist`/`remove_from_playlist` in the final design, to avoid confusion for future readers.

---

### Findings Summary

| Category | Severity | Count | Description |
|----------|----------|-------|-------------|
| PRD not updated after sprint change approval | 🟠 Required | 1 | FR37, FR38, FR39 + NFR addenda not in prd.md |
| UX spec not updated after sprint change approval | 🟠 Required | 1 | Epic 11 UI components + Story 9.7 list view absent from ux-design-specification.md |
| NFR11 implementation divergence | 🟡 Minor | 1 | PRD says OS-native keyring; implementation uses machine-bound file vault |
| FR25 missing from Coverage Map | 🟡 Minor | 1 | FR25 covered by Story 3.5 but not listed in epics Coverage Map |
| FR24 absent from PRD | 🟡 Minor | 1 | FR24 (splash screen) exists in epics, absent from PRD |
| Epic 7 technical naming | 🟡 Retrospective | 1 | Technical epic title; all stories done |
| Story 11.1 technical framing | 🟡 Retrospective | 1 | User story framing is weak; ACs are clear |
| Sprint change proposal RPC name drift | 🟡 Informational | 1 | `update_playlist` in proposal vs `add/remove` in stories/architecture |

**Total: 8 issues** — 2 required before Epic 11, 3 minor documentation cleanups, 3 retrospective/informational.

---

### Project Strengths Observed

- **Architecture quality is excellent** — Multi-process IPC, DeviceIO abstraction, and MediaProvider trait are all well-designed and fully documented.
- **Epic 11 stories are implementation-ready** — Technical Notes in Stories 11.1–11.6 provide line-level guidance (specific file paths, function names, RPC signatures). These are among the highest-quality story specs reviewed.
- **FR coverage is effectively 100%** — All 35 PRD FRs have implemented stories; the coverage map is 97% accurate.
- **No forward dependencies** — Clean left-to-right epic ordering with no circular dependencies.
- **Sprint change proposals are thorough** — Proper impact analysis, design decisions documented, scope classification, and implementation handoff steps all present.

---

**Report generated:** 2026-06-05
**Assessor:** Implementation Readiness Skill (BMAD)
**Input documents:** prd.md, architecture.md, epics.md, ux-design-specification.md, sprint-change-proposal-2026-06-05.md, sprint-status.yaml
