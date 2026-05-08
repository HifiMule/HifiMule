# Sprint Change Proposal — Multi-Provider Media Server Support

**Date:** 2026-05-08  
**Author:** Alexis  
**Change Scope:** MAJOR  
**Proposed Path:** Direct Adjustment (Option 1)  
**Status:** Pending approval

---

## Section 1: Issue Summary

### Problem Statement

HifiMule's API layer hard-codes Jellyfin as the only media server. The current architecture makes direct Jellyfin API calls throughout the sync engine, auto-fill algorithm, scrobble bridge, and authentication flow. Jellyfin and Subsonic/OpenSubsonic represent two fundamentally incompatible API paradigms that cannot be unified at the HTTP layer — they use different authentication schemes, URL shapes, response formats, duration units, and streaming models.

Adding Navidrome, Subsonic, and OpenSubsonic support without a clean provider abstraction would result in Jellyfin-specific branches proliferating across every layer of the daemon. This is architecturally unsustainable.

### Context & Discovery

This change was identified during planning as a strategic expansion to serve a significantly larger user base. Navidrome is the dominant self-hosted music-only server; Jellyfin is dominant for AV. HifiMule targeting only Jellyfin excludes the majority of the music-first self-hosting community.

Technical research was completed on 2026-05-08 (`research/technical-compare-jellyfin-navidrome-subsonic-opensubsonic-api-research-2026-05-08.md`), providing:
- Full API surface comparison across all four targets
- Validated `MediaProvider` trait architecture (from Supersonic/Go as production reference)
- Rust crate selection (`opensubsonic`, `jellyfin-sdk`, `async-trait`)
- 4-6 week implementation roadmap
- Risk assessment with mitigations

### Evidence of Need

- Jellyfin's `POST /Items/{id}/PlaybackInfo` for transcoding has no Subsonic equivalent — Subsonic uses stream URL parameters directly
- Subsonic embeds auth credentials in every URL (including stream/download URLs) — a security requirement to sanitize before logging
- Navidrome returns MD5 string IDs, not integers — any `i64` ID field breaks Navidrome compatibility
- Duration units differ: Jellyfin uses `runTimeTicks` (100ns intervals), Subsonic uses `duration` (seconds)
- Scrobble behavior differs: Navidrome only registers plays via explicit `scrobble?submission=true` calls; streaming does NOT increment play count (unlike Jellyfin)

---

## Section 2: Impact Analysis

### Epic Impact

| Epic | Impact | Required Change |
|------|--------|-----------------|
| **Epic 2** (Connection & Verification) | High | Stories 2.1, 2.5 are Jellyfin-only auth flows — must be updated for multi-provider auth |
| **Epic 3** (Curation Hub) | Medium | Story 3.1 references Jellyfin-specific browse API and cover art ID pattern |
| **Epic 4** (Sync Engine) | Medium | Story 4.8 (Transcoding) uses Jellyfin PlaybackInfo — must delegate to provider |
| **Epic 5** (Ecosystem) | Medium | Story 5.1 (Scrobble Bridge) targets Jellyfin Progressive Sync API only |
| **New Epic 8** | Required | Provider abstraction foundation (6 new stories) |
| **Epics 1, 6, 7** | None/Low | No changes required; CI may add Navidrome test container later |

### Artifact Conflicts

**PRD:**
- Core principle "Jellyfin-First" conflicts with multi-server goal
- FR5/FR6/FR7: Jellyfin-specific credential language needs updating
- FR8/FR25: Jellyfin-specific library browsing references need updating
- User journeys reference "Jellyfin server" explicitly
- New FR35 needed to formally capture multi-server support requirement

**Architecture:**
- `api.rs` Jellyfin client must become `providers/jellyfin.rs` (`JellyfinProvider`)
- No `MediaProvider` trait, domain models, or provider abstraction exists
- No Subsonic URL sanitization rule in enforcement guidelines
- Auth section covers only Jellyfin token — Subsonic stateless auth pattern not documented
- `DeviceManifest` lacks `server_id` field for multi-server manifests
- IPC: `server.connect` lacks server type parameter; `get_daemon_state` lacks `serverType` field
- Story 4.8 transcoding section is Jellyfin PlaybackInfo-specific

**UX Design:**
- Login screen has no server type detection/badge interaction pattern
- User mental model section refers to "Jellyfin library"
- Color palette labels "Jellyfin Purple" (cosmetic rename to "Brand Purple")

### Technical Impact

- New Rust crate dependencies: `opensubsonic`, `async-trait`, pinned `jellyfin-sdk`
- New modules: `providers/mod.rs`, `providers/jellyfin.rs`, `providers/subsonic.rs`, `domain/models.rs`
- All existing callers in `sync.rs`, `rpc.rs`, `scrobble.rs` updated to use `Arc<dyn MediaProvider>`
- New unit test suite: DTO→domain conversions, `wiremock` HTTP mocks, `insta` snapshot tests
- Future CI: `testcontainers` integration tests against real Navidrome + Jellyfin containers

---

## Section 3: Recommended Approach

### Selected Path: Direct Adjustment (Option 1)

**Recommendation:** Introduce Epic 8 as the provider abstraction foundation, update the Architecture and PRD documents, and modify affected stories in Epics 2–5. Since HifiMule is still in the planning/greenfield phase, there is no code to roll back and no timeline to delay. Doing this now costs far less than retrofitting after implementation begins.

**Rationale:**
- The technical research has de-risked the path entirely — architecture is validated by production Go and Python clients
- The project is greenfield: no implementation cost, pure planning document updates
- Navidrome + Subsonic represent a larger user base than Jellyfin-only — this is a strategic win
- Epic 8 introduces zero behavior change for existing Jellyfin users (pure refactor + addition)
- The `MediaProvider` trait is an architectural forcing function that improves maintainability for all future server-specific features

**Effort estimate:** High (6 new stories in Epic 8, refactoring existing Jellyfin-only implementations across already-shipped Epics 2–5 — `api.rs`, `sync.rs`, `rpc.rs`, `scrobble.rs` all require changes)

**Risk level:** Medium (validated path, but SubsonicProvider implementation involves new crate + stateless auth pattern). Mitigated by pinning `opensubsonic` version and writing comprehensive unit + snapshot tests.

**Timeline impact:** Epic 8 adds ~4-6 weeks of implementation work before the affected Epic 2/3/4/5 stories can be implemented. No existing stories are blocked until their modified versions are reached in the sprint sequence.

---

## Section 4: Detailed Change Proposals

### A. Architecture Document Changes

#### A1: Add MediaProvider Trait Pattern (new section)

**Section: Daemon Responsibilities — UPDATE**

OLD:
```
- Auto-Fill Algorithm: Priority-based music selection engine (favorites → play count → creation date) 
  querying Jellyfin API (IsFavorite, PlayCount, DateCreated fields).
- Transcoding Negotiator: When a device has a transcoding_profile_id set, calls POST /Items/{id}/PlaybackInfo...
```

NEW:
```
- Media Provider Layer: All server communication is mediated through a `MediaProvider` trait 
  (`providers/jellyfin.rs` + `providers/subsonic.rs`). The daemon never calls server APIs directly — 
  it holds a `Arc<dyn MediaProvider>` resolved at connect time based on server type detection.
- Auto-Fill Algorithm: Priority-based music selection engine querying the active `MediaProvider` 
  via `get_favorites()`, `get_most_played()`, `get_recently_added()`.
- Transcoding Negotiator: Provider-specific. Jellyfin: POST /Items/{id}/PlaybackInfo with DeviceProfile. 
  Subsonic: stream?format=mp3&maxBitRate=192 — delegated to provider's `download_url()`.
```

**ADD new section "Media Provider Layer":**

```
### Media Provider Layer

All server communication is routed through the `MediaProvider` trait:

```rust
#[async_trait]
pub trait MediaProvider: Send + Sync {
    async fn list_libraries(&self) -> Result<Vec<Library>, ProviderError>;
    async fn list_artists(&self, library_id: Option<&str>) -> Result<Vec<Artist>, ProviderError>;
    async fn get_artist(&self, id: &str) -> Result<ArtistWithAlbums, ProviderError>;
    async fn get_album(&self, id: &str) -> Result<AlbumWithTracks, ProviderError>;
    async fn search(&self, query: &str) -> Result<SearchResult, ProviderError>;
    fn download_url(&self, track_id: &str, profile: &TranscodeProfile) -> Result<Url, ProviderError>;
    fn cover_art_url(&self, item_id: &str, size: u32) -> Result<Url, ProviderError>;
    async fn list_playlists(&self) -> Result<Vec<Playlist>, ProviderError>;
    async fn get_playlist(&self, id: &str) -> Result<PlaylistWithTracks, ProviderError>;
    async fn changes_since(&self, since: SystemTime) -> Result<Vec<ChangeEvent>, ProviderError>;
    fn server_type(&self) -> ServerType;
    fn capabilities(&self) -> &Capabilities;
}

enum ServerType { Jellyfin, Subsonic }
```

Domain types (`Song`, `Album`, `Artist`) live in `domain/models.rs` — independent of API DTOs.
DTOs map to domain types via `From` conversions at the adapter boundary.

**Key normalization rules:**
- All IDs: `String` (Navidrome uses MD5 hashes, never integers)
- Duration: `u32` seconds (`runTimeTicks ÷ 10_000_000` for Jellyfin, direct for Subsonic)
- Bitrate: `u32` kbps (convert Jellyfin bps fields at DTO boundary)
- Cover art ref: `Option<String>` (Subsonic `coverArt` field ≠ song ID)

**Project structure additions:**
```
hifimule-daemon/src/
├── providers/
│   ├── mod.rs      (MediaProvider trait, ProviderError, ServerType)
│   ├── jellyfin.rs (JellyfinProvider — wraps existing api.rs)
│   └── subsonic.rs (SubsonicProvider — opensubsonic crate)
├── domain/
│   └── models.rs   (Song, Album, Artist, Playlist — API-agnostic)
```

**Crate additions:**
- `jellyfin-sdk = "=0.x.y"` (pin exact pre-1.0 version)
- `opensubsonic = "latest"`
- `async-trait = "0.1"`
```

#### A2: Auth, IPC & Manifest Additions

**Section: Authentication & Security — UPDATE credential management:**

OLD:
```
Credential Management: All Jellyfin tokens are stored in the OS-native secure vault using the `keyring` crate.
```

NEW:
```
Credential Management: Server credentials are stored in the OS-native secure vault using the `keyring` crate.
- Jellyfin: stores rotatable access token. Re-authenticates on 401.
- Subsonic/OpenSubsonic: stores the user password (stateless auth — credentials sent on every 
  request as MD5 token+salt). Password stored encrypted in keychain; used only to compute 
  per-request `t=md5(password+salt)`.
```

**Section: API & Communication Patterns — ADD IPC entries:**

```
- Server Connect IPC:
  - `server.connect(params: { url: string, serverType: 'jellyfin' | 'subsonic' | 'auto', 
    username: string, password: string })` → `{ ok: true, serverType: string, serverVersion: string }`
  - When `serverType: 'auto'`: daemon pings URL, checks `openSubsonic` flag, falls back to Jellyfin detection
- `get_daemon_state` response gains: `serverType: 'jellyfin' | 'subsonic' | null` and `serverVersion: string | null`
```

**Section: Data Architecture — ADD to Manifest Extension:**

```
- DeviceManifest extension: add `server_id: Option<String>` — stores the server URL (normalized) 
  this device was configured against. Uses `#[serde(default)]` for backward compatibility.
```

**Section: Enforcement Guidelines — ADD security rule:**

```
### Subsonic URL Sanitization (Security Requirement)

Subsonic embeds auth credentials (`u`, `p`, `t`, `s`) as query parameters in every URL, 
including stream/download URLs. This is a security requirement:

- All Subsonic URLs MUST be sanitized via `sanitize_subsonic_url()` before logging.
- The function strips `u`, `p`, `t`, `s` params and replaces with `[REDACTED]`.
- Stream and download URLs must NEVER appear in log files with credentials.
- Enforcement: AI agents MUST call `sanitize_subsonic_url()` on any Subsonic URL before 
  passing to `tracing::` macros or file-based logging.
```

---

### B. PRD Changes

#### B1: Core Principles & Success Criteria

| Location | Old | New |
|----------|-----|-----|
| Core Principle #2 | "Jellyfin-First: Connect to the server before hardware" | "Media Server Multi-Provider: Connect to the media server (Jellyfin, Navidrome, Subsonic, or any OpenSubsonic-compatible server) before hardware. The sync engine uses a provider abstraction layer." |
| Business Success | "top recommendation for the Rockbox/DAP community" | "top recommendation for the Rockbox/DAP community regardless of media server choice (Jellyfin, Navidrome, Subsonic-compatible servers)" |
| User Success | (existing items) | ADD: "Server Flexibility: Users are not locked into a single media server. HifiMule works seamlessly with Jellyfin, Navidrome, Subsonic, and any OpenSubsonic-compatible server." |
| User Journey: Silent Engine | "connect to his Jellyfin server" | "connect to his media server (Jellyfin or Navidrome), which is auto-detected by type" |

#### B2: Functional Requirements

| FR | Old | New |
|----|-----|-----|
| FR5 | "Configure Jellyfin server credentials (URL, username, token)" | "Configure media server credentials (URL, server type, username, and either API token for Jellyfin or username+password for Subsonic/OpenSubsonic). System auto-detects server type by pinging the URL." |
| FR6 | "Select a specific Jellyfin user profile for syncing" | "Select a specific user profile from the connected media server for syncing" |
| FR7 | "Maintain persistent encrypted connection state to the Jellyfin server" | "Maintain persistent encrypted connection state. Jellyfin: access token stored. Subsonic: password stored encrypted for per-request MD5 signing." |
| FR8 | "Browse Jellyfin Playlists, Genres, and Artists" | "Browse server Playlists, Artists, and Albums from the connected media server, regardless of type. Provider abstraction normalizes Jellyfin's unified /Items query and Subsonic's method-per-level hierarchy." |
| FR25 | "automatically filtering out movies, series, and books from Jellyfin views" | "For Jellyfin: applies IncludeItemTypes filter. For Subsonic/Navidrome: uses music-specific endpoints which are inherently music-only." |
| FR35 (NEW) | — | "The system supports Jellyfin, Navidrome, Subsonic, and any OpenSubsonic-compatible media server. Server type is auto-detected at connection time. Detected OpenSubsonic capabilities are cached for feature gating." |

---

### C. Epics Changes

#### C1: New Epic 8 — Multi-Provider Media Server Support

**ADD after Epic 7:**

```
## Epic 8: Multi-Provider Media Server Support (Prerequisite for Stories 2.1, 2.5, 3.1, 4.8, 5.1)

Introduce a provider abstraction layer enabling HifiMule to connect to Jellyfin, Navidrome, 
Subsonic, and any OpenSubsonic-compatible media server.

Stories 8.1–8.4 must be completed before implementing the modified versions of Stories 2.1, 
2.5, 3.1, 4.8, and 5.1. Epic 8 introduces zero user-visible behavior change for existing 
Jellyfin users — it is a pure refactor + addition at the provider layer.

Recommended sequencing:
  Phase A: 8.1 → 8.2 → 8.3 → 8.4 → 8.5 (8.6 can be parallel with 8.3)
  Phase B: Modified existing stories 2.1, 2.5, 3.1, 4.8, 5.1 (in their original epic order)
```

**Stories added:**

| Story | Title | Summary |
|-------|-------|---------|
| 8.1 | MediaProvider Trait & Domain Models | Define `MediaProvider` async trait, domain types (`Song`, `Album`, `Artist`, `Playlist`), normalization rules (String IDs, u32 seconds, u32 kbps). |
| 8.2 | JellyfinProvider Adapter | Wrap existing `api.rs` as `JellyfinProvider` implementing `MediaProvider`. Update all callers to use `Arc<dyn MediaProvider>`. |
| 8.3 | SubsonicProvider Adapter | `SubsonicProvider` via `opensubsonic` crate. Full Subsonic v1.16.1 + OpenSubsonic. Auth: per-request MD5 signing. `changes_since` via `getIndexes` + album fallback. |
| 8.4 | Runtime Server-Type Detection Factory | Auto-detect server type by pinging URL (Subsonic ping → Jellyfin ping fallback). Factory returns `Box<dyn MediaProvider>`. |
| 8.5 | Subsonic URL Credential Sanitization | `sanitize_subsonic_url()` utility. Strip `u`, `p`, `t`, `s` params from all logged Subsonic URLs. Security requirement. |
| 8.6 | Incremental Sync — Subsonic Album-Level Fallback | Detect song-level changes within unchanged albums on Navidrome (getIndexes is artist-level only — re-fetch albums as fallback). |

*(Full acceptance criteria and technical notes for each story documented in Proposal C1 above.)*

**FR Coverage Map additions:**
```
FR35: Epic 8 — Multi-Provider Server Support (Stories 8.1–8.6)
```

#### C2: Modified Story 2.1 — "Secure Media Server Link"

- **Title change:** "Secure Jellyfin Server Link" → "Secure Media Server Link"
- **AC updated:** Server type auto-detection at URL entry. Jellyfin: stores access token. Subsonic/Navidrome: stores encrypted password for per-request signing. Connection validated by ping/library query.

#### C2: Modified Story 2.5 — "Interactive Login & Identity Management"

- **AC updated:** Login URL field triggers server type auto-detection. UI shows live type badge ("Jellyfin" / "Navidrome / Subsonic" / "Unknown"). Jellyfin path: retrieves session token. Subsonic path: verifies credentials via ping (stateless). Error messages include "Unknown server type at this URL".
- **Technical Note added:** Server type badge updates live as URL is typed (debounced). Auto-detection is primary; manual override is advanced fallback.

#### C3: Modified Story 3.1 — "Immersive Media Browser (Multi-Server Integration)"

- **Title change:** "...Jellyfin Integration" → "...Multi-Server Integration"
- **AC added:** Subsonic/Navidrome browse path: `list_artists()` → `get_artist()` via provider. Cover art uses `song.cover_art_id` field (Subsonic `coverArt` ≠ song ID). All data flows through daemon RPC → MediaProvider, never direct server calls from UI.

#### C3: Modified Story 4.8 — Transcoding Handshake

- **AC added:** Subsonic path: `provider.download_url(id, profile)` returns `/rest/stream.view?format=mp3&maxBitRate=192` (kbps, not bps). No PlaybackInfo call for Subsonic. Auth params sanitized before logging.
- **Technical Note added:** Bitrate unit conversion happens inside `SubsonicProvider` — sync engine always works in kbps domain.

#### C3: Modified Story 5.1 — Rockbox Scrobbler Bridge

- **AC added:** Subsonic scrobble path: `POST /rest/scrobble.view?id={id}&submission=true&time={epoch_ms}`. Note: Navidrome streaming does NOT increment play count — explicit scrobble call required.
- **Technical Note added:** `ScrobbleSubmitter` becomes provider-aware. Server-side track ID (from manifest) used for Subsonic scrobble calls.

#### C4: Sequencing Note & FR Coverage Map

- Added sequencing header before Epic 8 (see C1 above)
- FR35 added to FR Coverage Map

---

### D. UX Design Changes

#### D1: Login, Mental Model & Visual Theme

| Section | Change |
|---------|--------|
| 2.2 User Mental Model | "Jellyfin library" → "media server library (Jellyfin, Navidrome, or any Subsonic-compatible server)" |
| 3.2 Visual Theme | "#52348B (Jellyfin Purple)" → "#52348B (Brand Purple)" (color unchanged, label rename) |
| 5.2 Custom Components | ADD "Server Type Badge (Login Screen)" interaction pattern: live auto-detecting badge next to URL field showing "Jellyfin", "Navidrome", "Subsonic", "Unknown" |

---

## Section 5: Implementation Handoff

### Change Scope Classification: MAJOR

> **⚠️ Important context discovered:** `sprint-status.yaml` shows that Epics 1–7 are all `done`. The Jellyfin-only implementations of Stories 2.1, 2.5, 3.1, 4.8, and 5.1 already exist in the codebase. Epic 8 stories will therefore involve **refactoring existing code**, not greenfield implementation. Effort estimates should account for this — Story 8.2 (JellyfinProvider adapter) in particular requires migrating all existing `api.rs` callers without regression.

This change requires Product Manager and Solution Architect review and sign-off because:
1. It changes a foundational architectural assumption (Jellyfin-only → multi-provider) in an **existing codebase**
2. It requires a new epic (Epic 8) and code changes to already-shipped implementations in Epics 2–5
3. It changes the PRD's positioning and functional requirements
4. It introduces a new provider abstraction layer requiring refactoring of existing `api.rs`, `sync.rs`, `rpc.rs`, and `scrobble.rs`

### Handoff Recipients & Responsibilities

| Role | Responsibility |
|------|----------------|
| **Architect** | Review and approve Architecture document changes (A1, A2). Validate `MediaProvider` trait interface design. Confirm `DeviceManifest` backward compatibility approach. |
| **Product Manager** | Review and approve PRD changes (B1, B2). Validate FR35 language. Confirm "Server Flexibility" positioning. |
| **Developer Agent** | Implement Epic 8 stories in sequence (8.1 → 8.2 → 8.3 → 8.4 → 8.5, with 8.6 parallel to 8.3). Then implement modified stories 2.1, 2.5, 3.1, 4.8, 5.1. |
| **UX Designer** | Review D1 changes. Design the "Server Type Badge" component for the Login screen. |

### Success Criteria

- `MediaProvider` trait defined; 100% of daemon server calls routed through it
- `JellyfinProvider` passes all existing tests with identical behavior
- `SubsonicProvider` verified against live Navidrome instance (Docker)
- Zero Subsonic auth params appearing in any log file (verified by test)
- All ID fields use `String` type throughout (no `i64`/`u64` for IDs)
- Login flow works with both Jellyfin and Navidrome URLs in manual testing

### Implementation Sequencing

```
Week 1-2:  Story 8.1 (MediaProvider trait + domain models)
           Story 8.2 (JellyfinProvider adapter — zero regression)
Week 3-4:  Story 8.3 (SubsonicProvider — opensubsonic crate)
           Story 8.5 (URL sanitization — concurrent with 8.3)
Week 4-5:  Story 8.4 (Runtime server-type detection factory)
           Story 8.6 (Incremental sync Subsonic fallback — concurrent)
Week 5+:   Modified Epic 2 stories (2.1, 2.5)
           Modified Epic 3/4/5 stories (3.1, 4.8, 5.1) in original epic order
```

---

## Appendix: Source Documents

| Document | Path |
|----------|------|
| PRD | `_bmad-output/planning-artifacts/prd.md` |
| Epics | `_bmad-output/planning-artifacts/epics.md` |
| Architecture | `_bmad-output/planning-artifacts/architecture.md` |
| UX Design | `_bmad-output/planning-artifacts/ux-design-specification.md` |
| Technical Research | `_bmad-output/planning-artifacts/research/technical-compare-jellyfin-navidrome-subsonic-opensubsonic-api-research-2026-05-08.md` |
