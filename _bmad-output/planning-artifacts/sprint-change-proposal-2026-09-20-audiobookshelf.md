---
title: Sprint Change Proposal — Audiobookshelf Integration
date: 2026-09-20
status: approved
mode: batch
scope: major
source: ../brainstorming/brainstorming-session-2026-09-20-193424.md
---

# Sprint Change Proposal — Audiobookshelf Integration

## 1. Issue Summary

The product currently supports Jellyfin and Subsonic/OpenSubsonic music sources. The 2026-09-20 brainstorming session established a coherent additional provider: Audiobookshelf. The desired outcome is full Audiobookshelf support, delivered as phased stories within one epic: audiobook direct playback, player-centric progress continuity, podcast direct playback, local sync and Autofill, then grouping and compatibility refinement.

The change is a new stakeholder requirement, rather than a defect in delivered work. It must preserve the provider boundary, multi-server routing, playback safety, device-sync safeguards, and existing Jellyfin selection behavior.

Evidence and decisions from the session:

- A selected Books library creates an audiobook server; every book in that library is eligible.
- A book maps to an album and its ordered files/chapters map to tracks. A single-file book is a one-track album.
- Author is the primary artist-equivalent; narrator is a secondary credit.
- Direct server playback is the first outcome, but all later phases are owned by Epic 17.
- Editable collections, remote collection write-back, and Audiobookshelf-specific folder/collection filters remain excluded.

## 2. Impact Analysis

### Epic and story impact

No delivered epic or approved story is invalidated. Epic 8 supplies the provider boundary, Epic 2 supplies multi-server configuration and identities, Epic 9 supplies capability-gated browsing, and Epic 15 supplies direct album/track playback. Epic 17 is therefore additive and may start only after its stated dependency gates are satisfied; it does not wait for the unrelated Radio work in Epic 16.

| Artifact | Impact |
| --- | --- |
| Epic 2 | Server setup gains a post-authentication library-selection step for this provider only; existing server identity semantics remain unchanged. |
| Epic 8 | Add an `AudiobookshelfProvider`, provider type detection/explicit selection, library-role capability data, and safe credential/URL handling. |
| Epic 9 | Existing browse/search contracts need audiobook-aware presentation data, while preserving normal capability gating. |
| Epic 15 | Reuse direct album/track playback and extend it later with a player-centric, identity-safe continuity bridge. |
| Epic 12/13 and Epic 4 | Epic 17 reuses their per-server budget, Autofill, transfer, compatibility, and reconciliation mechanisms; it does not redefine them. |
| Epic 11 | Epic 17 adds read-only series/collection representations in its refinement phase but introduces no remote playlist/collection mutation. |

### PRD impact

The core MVP remains achievable and unchanged. Add Growth / expansion requirements after FR81:

```text
FR82: Users can configure an Audiobookshelf endpoint, authenticate, discover its libraries,
and create an independent HifiMule server from one selected Books library. The selected
library determines the server role; no Audiobookshelf-only folder or collection filter is
introduced.

FR83: HifiMule represents an Audiobookshelf audiobook as an album and its ordered
chapter/file parts as tracks, including cover art, author as primary artist-equivalent,
and narrator credits. A single-file book remains a one-track album.

FR84: Users can browse, search, and directly play eligible audiobooks through the existing
desktop playback path. Playback uses only a verified Audiobookshelf stream/format path and
explains an incompatibility rather than transferring or playing an unusable representation.

FR85: During HifiMule playback, the system can read and safely update Audiobookshelf progress
and completion using a stable remote item identity and a whole-book offset. It does not infer
identity or perform progress synchronization during background media synchronization.

FR86: Users can configure a selected Audiobookshelf Podcasts library as an independent server,
browse and directly play shows and episodes, and later use a per-server capacity-managed policy
to select recent unplayed episodes for local synchronization and Autofill.

FR87: Audiobookshelf local synchronization uses normal HifiMule reconciliation and per-server
capacity behavior. It transfers only a compatible direct/transcoded representation and gives a
clear ineligibility explanation when no compatible representation is available.
```

Add an explicit non-goal: editable Audiobookshelf collections, remote collection write-back, and Audiobookshelf-specific folder/collection selection. Progress is player-centric only; it is never part of ordinary background media synchronization.

### Architecture impact

The `MediaProvider` boundary remains the integration seam, but it needs these additions:

- `ServerType::Audiobookshelf`, endpoint authentication, library discovery, and a selected-library configuration record with its immutable upstream library ID and role.
- A provider capability/role contract that distinguishes Audiobookshelf Books and Podcasts libraries without treating the two as interchangeable catalog types.
- An audiobook mapper that produces existing album/ordered-track/artist-compatible values and retains the stable remote item ID as provider metadata. Narrators must not replace the author as the primary artist.
- A direct-stream/transcode acquisition path whose URLs/tokens remain daemon-side and are never logged or exposed to the UI, with transfer eligibility established before local synchronization.
- A playback identity bridge that retains stable remote IDs and whole-item offsets, reads remote progress at playback start, and writes progress/completion only while HifiMule plays a proven matching item.
- A distinct podcast show/episode mapper plus a per-podcast-server recent/unplayed retention-policy source for the existing Autofill pipeline.
- Normal removal reconciliation for disappeared remote items. No unobservable external-player protection rule is added.

Before implementation, the first story must validate the deployed Audiobookshelf API version(s), authentication mechanism, library discovery, book/chapter metadata, artwork, stream endpoint behavior, format compatibility, pagination/search semantics, and stable IDs. Its resulting contract is the authority for DTO fields and transport behavior.

### UX impact

The server-add flow needs an Audiobookshelf provider choice or reliable detection, followed by a library picker after successful authentication. A Books selection creates an audiobook server and a Podcasts selection creates a podcast server; both are independent Server Hub entries even when they share endpoint credentials. Library browsing needs domain-appropriate labels and metadata presentation while retaining established keyboard, focus, loading, error, and capability states. There is no collection editor or folder selector.

### Secondary artifacts

Update architecture, UX specification, requirement coverage map, test fixtures/mocks, provider capability documentation, and the sprint tracker. Ensure test fixtures cover ordered multi-part and single-file books, multiple authors/narrators, missing or changed remote items, authentication failure, pagination/search, artwork, and unavailable/incompatible streaming.

## 3. Recommended Approach

**Recommendation: Direct adjustment by adding a new Epic 17 that contains the complete phased roadmap.**

This preserves completed work and uses the existing provider, multi-server, browsing, playback, sync, and Autofill seams. A rollback is not justified: no delivered feature conflicts with Audiobookshelf. A broader MVP redefinition is also unnecessary: Audiobookshelf is a post-MVP expansion and must not delay the Epic 15 release verification.

Effort is high and risk is high, concentrated in API-contract validation, precise chapter/whole-book mapping, progress integrity, distinct podcast semantics, retention-policy behavior, and real streaming/transcoding compatibility. The risk is controlled by an explicit discovery/contract story, fixture-based mapper tests, and ordered internal phases.

Sequencing: complete Epic 15’s release-verification gate independently; then complete Epic 17 Stories 17.1–17.9 in order. Epic 16 may proceed independently. Each later phase has an explicit dependency on its preceding capability inside Epic 17.

## 4. Detailed Change Proposals

### Epics — add Epic 17 after Epic 16

**OLD**

```text
## Playback Story Coverage and Readiness
```

**NEW**

```text
## Epic 17: Audiobookshelf Integration

Add Audiobookshelf as a multi-server provider for independently selected Books and Podcasts
libraries. Deliver the full phased roadmap: audiobook catalog/direct playback; player-centric
progress continuity; podcast catalog/direct playback; local synchronization and Autofill; then
read-only series/collection grouping and compatibility refinement. Audiobooks and podcasts are
separate domain models with separate server budgets and selection behavior. Editable collections,
remote collection write-back, and source-specific folder/collection filtering are excluded.

Dependencies: completed Epic 8 provider and multi-server foundation; direct album/track
playback from Epic 15. The Epic 15 packaging/release-verification work remains independent.

### Story 17.1: Validate the Audiobookshelf integration contract
As a developer, I want a versioned, fixture-backed contract for the Audiobookshelf APIs used
by HifiMule, so that implementation does not depend on inferred endpoint or identity behavior.

Acceptance criteria: validate authentication, library discovery/type, Books catalog/search and
pagination, book/part ordering, author/narrator/artwork fields, stable item IDs, direct stream
URLs, failure semantics, and compatibility evidence against supported server versions. Record
redacted fixtures and unsupported/ambiguous behavior. No user-visible provider is enabled.

### Story 17.2: Connect an Audiobookshelf Books library as an independent server
As a user, I want to authenticate to Audiobookshelf and select one Books library, so that it
appears as its own HifiMule audiobook server alongside my other servers.

Acceptance criteria: credentials stay in the existing encrypted vault; endpoint/server type is
validated; only Books libraries are selectable in this story; selected upstream library ID and
role persist with a stable HifiMule server identity; duplicate/upsert, restart, auth failure,
and sanitized logging behave like existing providers. No folder/collection picker appears.

### Story 17.3: Map Audiobookshelf books faithfully into the HifiMule catalog
As a listener, I want books represented as ordered albums with accurate credits, so that I can
find and play long-form audio naturally.

Acceptance criteria: each book maps to an album; parts/chapters map to deterministic ordered
tracks; a single-file book maps to one track; cover art, author-primary and narrator-secondary
credits are preserved; remote IDs are retained as provider metadata; pagination/search and
remote removal follow provider conventions; fixtures cover metadata and ordering edge cases.

### Story 17.4: Browse and search an Audiobookshelf audiobook server
As a listener, I want to browse and search my selected audiobook library, so that I can choose
a book without confusing it with music or podcasts.

Acceptance criteria: existing browse/search/RPC capability conventions are reused; book and
chapter presentation is accessible and clearly labeled; author and narrator hierarchy is shown;
empty, loading, unavailable, and stale-source states are explained; existing music-server
behavior does not regress. No series/collections or podcast UI is introduced.

### Story 17.5: Directly play Audiobookshelf audiobooks through HifiMule
As a listener, I want to start a selected book or chapter through the established playback path,
so that I gain immediate Audiobookshelf value without device synchronization.

Acceptance criteria: direct server playback honors ordered tracks and existing session/output
safety; authenticated stream URLs remain daemon-side; only validated compatible delivery paths
are used; unavailable/incompatible media produces a truthful recoverable error. This story does
not yet write progress, transfer media locally, or run Autofill.

### Story 17.6: Preserve Audiobookshelf listening continuity safely
As a listener, I want HifiMule playback to resume and report an audiobook at its correct
whole-book position, so that I can move safely between HifiMule and Audiobookshelf.

Acceptance criteria: persist stable Audiobookshelf item identity plus a whole-item offset; read
the remote position when HifiMule starts playback; translate between the whole-item offset and
local chapter/track position; report position/completion only while HifiMule plays a proven
matching item; refuse write-back on missing or stale identity; do not add background sync-based
progress import or propagation.

### Story 17.7: Add Audiobookshelf podcast servers and direct playback
As a listener, I want to select a Podcasts library and browse/play its shows and episodes, so
that podcasts work without being forced into audiobook or album semantics.

Acceptance criteria: a selected Podcasts library creates an independent podcast server with its
own role and budget; show/episode mapping and presentation are distinct from book mapping;
browse/search/direct playback use existing safety contracts; an audiobook server never exposes
podcast catalog content, and vice versa.

### Story 17.8: Synchronize Audiobookshelf media with independent policies
As a portable-listening user, I want audiobooks and podcasts to participate in normal sync under
appropriate independent rules, so that durable books and changing episode feeds fit my device.

Acceptance criteria: audiobook servers reuse existing per-server selection, budget and removal
reconciliation behavior; podcast servers use a capacity-managed recent/unplayed retention policy
as a source for the existing Autofill pipeline; incompatible content is blocked before transfer
with a clear reason; each server has an independent budget; no unobservable external-playback
protection is promised.

### Story 17.9: Refine Audiobookshelf grouping and compatibility feedback
As a listener, I want source groupings and compatibility feedback that help me curate and sync
confidently, so that the integration stays understandable as my library changes.

Acceptance criteria: import Audiobookshelf series and collections as read-only playlists;
progressively disclose direct/transcoded compatibility where probing is cheap and reliable,
otherwise explain eligibility during sync planning; preserve responsive browsing; collection
editing/write-back and source-specific folder selection remain unavailable.
```

**Rationale:** Nine ordered stories isolate external-contract risk, configuration, mapping,
playback, progress integrity, podcast behavior, local sync/Autofill, and refinement while keeping
the full Audiobookshelf roadmap visible and owned by one epic.

### Architecture — amend provider and server configuration sections

**OLD**

```rust
pub enum ServerType { Jellyfin, Subsonic }
```

**NEW**

```rust
pub enum ServerType { Jellyfin, Subsonic, Audiobookshelf }
```

Add `AudiobookshelfProvider` behind `MediaProvider`, a persisted selected-library identity and role, a book-to-album mapper, a distinct podcast show/episode mapper, a capability-gated direct stream/transcode contract, and a stable-ID/whole-item playback identity bridge. Retain existing `Song`/`Album` primitives only where they faithfully represent the audiobook slice; podcast behavior must not be forced through that model. The progress bridge is used only by the player, not background synchronization.

### UX — amend server setup and library browsing sections

**OLD**

```text
The connection form detects and configures the existing music-server providers.
```

**NEW**

```text
For Audiobookshelf, successful endpoint authentication opens a library picker. Selecting one
Books library creates an independent audiobook server; selecting one Podcasts library creates
an independent podcast server. The browser uses accessible domain-appropriate labels and
author/narrator hierarchy for books. It does not offer a collection editor or
Audiobookshelf-only folder/collection filter.
```

### Sprint tracker — add after Epic 16

```yaml
  # Epic 17 — Audiobookshelf Integration
  epic-17: backlog
  17-1-validate-audiobookshelf-integration-contract: backlog
  17-2-connect-audiobookshelf-books-library-as-independent-server: backlog
  17-3-map-audiobookshelf-books-into-the-hifimule-catalog: backlog
  17-4-browse-and-search-an-audiobookshelf-audiobook-server: backlog
  17-5-directly-play-audiobookshelf-audiobooks-through-hifimule: backlog
  17-6-preserve-audiobookshelf-listening-continuity-safely: backlog
  17-7-add-audiobookshelf-podcast-servers-and-direct-playback: backlog
  17-8-synchronize-audiobookshelf-media-with-independent-policies: backlog
  17-9-refine-audiobookshelf-grouping-and-compatibility-feedback: backlog
  epic-17-retrospective: optional
```

## 5. Implementation Handoff

**Scope classification: Major.** The proposal requires Product Manager/Architect review for the new provider-role and catalog contract, then Product Owner/Developer backlog preparation and implementation.

| Recipient | Responsibility |
| --- | --- |
| Product Manager / Architect | Approve FR82–FR87, the explicit non-goals, provider-role/data contract, and phased epic boundaries. |
| Product Owner | Add approved Epic 17 and stories to the backlog; preserve ordering and dependency gates. |
| Developer | Execute Story 17.1 first, then implement the nine stories in dependency order after the validated contract is recorded. |
| QA | Maintain redacted integration fixtures and prove mapping, compatibility/error behavior, playback safety, and non-regression across supported platforms. |

Success means a user can configure independent Books and Podcasts library servers, browse and
play each domain safely, carry audiobook progress using proven stable identity, locally sync
media under its correct per-server policy, use read-only series/collections, and receive truthful
compatibility feedback. Editable collections, remote collection write-back, and extra
Audiobookshelf-only filters remain excluded.

## Checklist Status

- [x] 1.1–1.3 Trigger and supporting evidence: the completed brainstorming session is the source of the new requirement and phased decisions.
- [x] 2.1–2.5 Epic impact: additive Epic 17; no completed epic is invalidated; Epic 8/15 are dependencies; Epic 16 remains independent.
- [x] 3.1–3.4 Artifact impact: PRD, epics, architecture, UX, tracker, fixtures, and provider tests need amendments.
- [x] 4.1 Direct adjustment: viable; high effort, high risk.
- [N/A] 4.2 Rollback: no delivered work needs reverting.
- [N/A] 4.3 MVP review: core MVP remains unchanged; this is a growth expansion.
- [x] 4.4 Recommended path: one phased Epic 17.
- [x] 5.1–5.5 Proposal and handoff prepared.
- [x] 6.3 User approval: approved on 2026-09-20.
- [x] 6.4 Sprint tracker: Epic 17 and nine backlog stories added.
- [x] 6.5 Handoff: Product Manager/Architect contract review, Product Owner backlog stewardship,
  Developer implementation in story order, and QA fixture/integration coverage.

## Approval and Handoff Record

Alexis approved the complete proposal on 2026-09-20. The PRD, epics, architecture, UX
specification, and sprint tracker have been amended. Epic 17 is backlog and must begin with the
validated Audiobookshelf integration contract before any user-visible provider work.
