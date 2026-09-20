---
stepsCompleted: [1, 2, 3, 4]
inputDocuments: []
session_topic: 'Audiobookshelf support for audiobooks and podcasts in HifiMule'
session_goals: 'Map Audiobookshelf concepts to HifiMule items; define a phased delivery plan and architecture; support selected sync folders for media and collections; treat two-way progress and completion sync as optional later scope.'
selected_approach: 'AI-Recommended Techniques'
techniques_used: ['Morphological Analysis']
ideas_generated: ['Separate audiobook and podcast domain models', 'Model each audiobook as an album', 'Map author to primary artist and narrator to contributor', 'Map series and collections to playlists with different edit authority', 'Defer collection editing and write-back', 'Use policy-based podcast retention', 'Use selected podcast shows as Autofill sources', 'Use a dedicated capacity-managed podcast Autofill slot', 'Give audiobooks and podcasts independent server budgets', 'Configure Audiobookshelf connector instances by media role', 'Keep Audiobookshelf selection parity with Jellyfin', 'Infer HifiMule server type from the selected Audiobookshelf library', 'Use the selected Books library as the complete eligible audiobook catalog', 'Reconcile remote removals with existing HifiMule semantics', 'Make Audiobookshelf progress a player integration, not a sync feature', 'Use whole-item offsets to enable cross-player audiobook handoff', 'Require stable identity before writing playback progress', 'Use capability-driven transcoding with explicit incompatibility handling', 'Progressively disclose compatibility based on probe cost', 'Prioritize direct Audiobookshelf server playback']
context_file: ''
technique_execution_complete: true
facilitation_notes: 'The session converged quickly on a pragmatic server-per-library architecture and requested an early move to architecture and phased planning.'
session_active: false
workflow_completed: true
---

# Brainstorming Session Results

**Facilitator:** Alexis
**Date:** 2026-09-20

## Session Overview

**Topic:** Audiobookshelf support for audiobooks and podcasts in HifiMule

**Goals:** Map Audiobookshelf concepts to HifiMule items; define a phased delivery plan and architecture; support selected sync folders for media and collections; treat two-way progress and completion sync as optional later scope.

### Context Guidance

Focus on a pragmatic server integration that fits existing HifiMule concepts, beginning with one-way media and collection synchronization. Treat remote listening progress and finished status as a separately validated, later-phase capability.

### Session Setup

The session will explore product boundaries, server-to-domain mapping, storage and synchronization architecture, and a phased delivery plan. Ideas will remain broad initially, with two-way progress synchronization deliberately excluded from the baseline release.

## Technique Selection

**Approach:** AI-Recommended Techniques

**Analysis Context:** Audiobookshelf support for audiobooks and podcasts in HifiMule, emphasizing a practical resource mapping and phased rollout.

**Selected Technique:** Morphological Analysis — systematically map the design dimensions and identify the coherent combinations for a first delivery.

**AI Rationale:** The request is a bounded integration problem with several interdependent data, sync, and UX decisions. A parameter-based map will expose the minimum viable shape without prematurely committing to two-way progress synchronization.

## Emerging Ideas

**[Domain Model #1]: Two Media Worlds**

_Concept_: Treat Audiobookshelf audiobooks as library media analogous to HifiMule's existing audio catalog, organized around book, author, series, and user-curated collections. Treat podcasts as a distinct, dynamic subscription domain centered on a feed/show and its changing episode set.

_Novelty_: The integration does not force both source types into one generic "audio item" abstraction; it preserves the different expectations users bring to durable books and continually changing podcasts.

**[Audiobook Mapping #2]: Book-as-Album**

_Concept_: Represent each audiobook as an album. A multi-file or chapterized book produces ordered tracks; a single-file book produces a one-track album. Reuse existing album artwork, track ordering, playback, and file synchronization behavior.

_Novelty_: The model accommodates both common Audiobookshelf file layouts without a separate playback type or a special-case data model for single-file books.

**[Metadata Mapping #3]: Author Leads, Narrator Credits**

_Concept_: Map the Audiobookshelf author to HifiMule's primary artist-equivalent field, driving browse, search, and grouping. Map narrator(s) to a secondary contributor/credit field, analogous to session musicians or featured performers.

_Novelty_: This preserves the information hierarchy users care about while fitting the library's established music-oriented discovery model.

**[Grouping Mapping #4]: Authority-Aware Playlists**

_Concept_: Map both Audiobookshelf series and collections to HifiMule playlists. Series are source-defined/read-only playlists; collections are the editable playlist type, with edits ultimately belonging to Audiobookshelf.

_Novelty_: One existing interface represents both concepts while retaining the only behavioral distinction that matters: who may change membership and order.

**[Release Boundary #5]: Catalog Before Curation**

_Concept_: Import Audiobookshelf series and collections for browsing and discovery, but make them read-only in HifiMule's first release. Defer collection editing, membership changes, and remote write-back until after the core catalog and playback path is proven.

_Novelty_: The scope is prioritized around long-form listening value rather than reproducing playlist controls that are less central for audiobooks.

**[Podcast Sync #6]: Policy-Based Episode Window**

_Concept_: Represent a podcast sync selection as a policy rather than a static list: for example, retain the newest ten episodes not yet listened to. Re-evaluate the policy at each sync so new, relevant episodes replace older items according to the configured rule.

_Novelty_: The local podcast catalog is a useful, bounded listening queue instead of an ever-growing mirror of every available episode.

**[Podcast Delivery #7]: Shows as Autofill Sources**

_Concept_: A selected Audiobookshelf podcast show becomes an eligible source for HifiMule Autofill. Its unplayed recent episodes are candidates for the device/library target, subject to existing Autofill capacity and priority rules.

_Novelty_: Podcast delivery extends an established HifiMule workflow rather than creating a parallel sync system, while preserving the dynamic episode-window behavior.

**[Podcast Autofill #8]: Capacity-Managed Podcast Slot**

_Concept_: Give podcasts a dedicated Autofill slot with a user-selected total-size budget. Chosen shows supply unplayed episode candidates, and age/recency filters determine which candidates occupy that slot as new episodes arrive.

_Novelty_: Podcast freshness is managed as its own finite resource rather than through per-show weights or direct competition with static media.

**[Server Architecture #9]: Media-Specific Server Budgets**

_Concept_: Configure Audiobookshelf audiobook and podcast synchronization as distinct HifiMule server scopes, each with its own storage budget and selection behavior. The audiobook scope contains only static audiobook media; the podcast scope contains only dynamic episode candidates.

_Novelty_: Separation happens at the existing server-budget boundary, keeping capacity management and Autofill semantics simple for both media types.

**[Connector Configuration #10]: One Connector, Explicit Roles**

_Concept_: The Audiobookshelf connector can be instantiated as an audiobook source, a podcast source, or both. Each configured source has its own mechanism and budget, even when they share the same Audiobookshelf endpoint and credentials.

_Novelty_: Users can add multiple purpose-specific sources without an artificial one-connection/one-media restriction or tangled hybrid behavior.

**[UX Constraint #11]: Jellyfin-Parity Selection**

_Concept_: Do not add folder- or collection-level selection to the Audiobookshelf source configuration if equivalent selection does not exist for Jellyfin. Keep selection behavior consistent across server types.

_Novelty_: The integration favors a coherent HifiMule mental model over exposing every capability of the upstream server.

**[Server Creation #12]: Library-Determined Type**

_Concept_: During Audiobookshelf server creation, the user selects a library. A Books library automatically creates an audiobook server; a Podcasts library automatically creates a podcast server. Multiple selected libraries from the same endpoint become independent HifiMule servers.

_Novelty_: The upstream library type, rather than a duplicate user choice, determines behavior and prevents invalid mixed-media source configurations.

**[Audiobook Eligibility #13]: Library as Catalog Boundary**

_Concept_: Every book in the selected Audiobookshelf Books library is eligible for the HifiMule audiobook server. Existing HifiMule capacity and Autofill rules—not new Audiobookshelf-specific folder or collection filters—decide which eligible books reach a target.

_Novelty_: The source definition is simple and predictable while reusing the proven selection machinery already used by other media servers.

**[Lifecycle #14]: Honest Reconciliation**

_Concept_: When an item disappears from the selected Audiobookshelf library, apply HifiMule's normal reconciliation/removal semantics. Do not promise to preserve an item because it might be playing on an external device when that state is not observable.

_Novelty_: The design favors consistent, truthful synchronization guarantees over a protection mechanism that would be unreliable by definition.

**[Playback State #15]: Player-Centric Progress Bridge**

_Concept_: Do not import or propagate listening progress as part of ordinary media synchronization. In a later playback integration, HifiMule reads the Audiobookshelf position when playback begins and updates position/completion while HifiMule itself plays the item.

_Novelty_: Progress is attached to the action that can meaningfully use and author it—the player—rather than to background file synchronization.

**[Cross-Player Continuity #16]: Stable Item ID + Global Offset**

_Concept_: Persist the Audiobookshelf item identity and a whole-book/episode playback offset. At playback time, translate the offset to a local chapter/track and, when reporting progress, translate local playback back to the same whole-item offset.

_Novelty_: HifiMule and Audiobookshelf can hand off an audiobook naturally even when the local representation contains several chapter tracks.

**[Progress Safety #17]: No Guessing on Write-Back**

_Concept_: Only write Audiobookshelf progress when HifiMule can prove the local media maps to the expected remote item identity. If the mapping is absent or stale after a restructure, do not guess; require re-sync or re-linking.

_Novelty_: The player may gracefully lose convenience, but it cannot silently corrupt another book's listening state.

**[Media Delivery #18]: Capability-Gated Transfer**

_Concept_: Use an Audiobookshelf delivery/transcoding path only when it can yield a format compatible with the target. If it cannot, block the item's download and report incompatibility clearly instead of transferring unusable media.

_Novelty_: Compatibility is a transparent eligibility condition, not a hidden failure late in the sync process.

**[Compatibility UX #19]: Progressive Capability Disclosure**

_Concept_: Probe and show format/transcoding compatibility during browsing or setup when the check is inexpensive and reliable. Otherwise defer the check to sync planning and provide a clear ineligibility explanation then.

_Novelty_: The user receives early feedback where affordable, without making ordinary browsing sluggish merely to predict every transfer outcome.

**[Primary Value #20]: Server Playback First**

_Concept_: Prioritize playing Audiobookshelf audiobooks and podcast episodes directly from the server through HifiMule's existing audio-server playback path. Treat local synchronization, Autofill, and offline capacity management as subsequent capabilities.

_Novelty_: The integration delivers immediate listening value using an established HifiMule interaction before taking on the separate complexity of local media transfer.

## Technique Execution Results

**Morphological Analysis:**

- **Interactive Focus:** Separate audiobook and podcast semantics; map each Audiobookshelf library type to a purpose-specific HifiMule server; identify metadata, grouping, lifecycle, capacity, and playback-state boundaries.
- **Key Breakthroughs:** Books map to album/track structures; authors are primary artists and narrators secondary credits; series and collections are read-only playlists initially; podcast libraries become a dynamic, budgeted episode source; listening progress belongs to a later player bridge using a stable remote item ID and whole-item offset.
- **User Creative Strengths:** Strong preference for parity with existing HifiMule/Jellyfin behavior, clear distinctions between durable long-form media and dynamic feeds, and well-calibrated scope boundaries.
- **Energy Level:** Focused and decisive; the user explicitly directed the session to architecture and phased planning after the core dimensions were established.

### Creative Facilitation Narrative

The collaboration started from a seemingly simple Audiobookshelf server request and uncovered a coherent split between two media models. The crucial move was to let the selected Audiobookshelf library type determine the HifiMule server type, so books and podcasts share a connector but not storage budgets, selection policies, or playback expectations. The discussion also isolated progress synchronization as a later player responsibility rather than a background-sync feature.

## Idea Organization and Prioritization

### Thematic Organization

**Server Model and Configuration**

- An Audiobookshelf endpoint may yield multiple independent HifiMule servers.
- Server creation selects an Audiobookshelf library; Books creates an audiobook server and Podcasts creates a podcast server.
- Each server retains its own budget and mechanisms even when endpoint credentials are shared.
- Maintain selection parity with Jellyfin: do not add folder- or collection-level selection solely for Audiobookshelf.

**Audiobook Domain Mapping**

- Audiobookshelf book maps to a HifiMule album.
- Audiobookshelf chapter/file maps to an ordered HifiMule track; a single-file book maps to a one-track album.
- Audiobookshelf author maps to the primary artist-equivalent; narrator maps to a secondary contributor/credit.
- Audiobookshelf series and collections can map to playlists, initially read-only and lower priority.

**Podcast Domain Mapping**

- Podcasts remain a distinct show/episode model, not an audiobook or album variant.
- A podcast server supports direct playback first.
- In a later local-sync phase, selected shows feed a capacity-managed, recent/unplayed episode policy within that server's independent budget.

**Playback and Synchronization Integrity**

- Direct server playback is the primary initial value, using HifiMule's existing audio-server playback path.
- A later continuity bridge persists the Audiobookshelf item identity plus whole-book/episode offset, translating it to local chapter position when required.
- Write back progress and finished state only when that item identity is proven stable; do not fuzzy-match.
- For local transfer, use Audiobookshelf-compatible delivery/transcoding when possible and transparently block incompatible items otherwise.

### Confirmed Release Plan

**Phase 1 — Audiobookshelf Audiobooks: Direct Playback**

1. Add an Audiobookshelf connector with endpoint authentication and library discovery.
2. Create a HifiMule audiobook server by selecting an Audiobookshelf Books library.
3. Browse/search and play books directly from that server through HifiMule's existing audio path.
4. Implement Book-as-Album mapping, chapter ordering, artwork, author-first metadata, and narrator credits.

**Phase 2 — Audiobookshelf Audiobooks: Playback Continuity**

1. Persist stable Audiobookshelf item IDs and whole-item offsets.
2. Read remote position when HifiMule begins playback.
3. Report position and completion to Audiobookshelf while HifiMule plays.
4. Fail safe when stable identity mapping is unavailable.

**Phase 3 — Audiobookshelf Podcasts: Direct Playback**

1. Create a HifiMule podcast server by selecting an Audiobookshelf Podcasts library.
2. Browse shows and episodes and play them directly through the same server-audio path.
3. Add podcast-appropriate show/episode presentation without forcing album semantics.

**Phase 4 — Local Synchronization and Autofill**

1. Audiobooks: reuse normal per-server capacity and selection behavior.
2. Podcasts: use a separate podcast-server budget and a dynamic recent/unplayed episode retention policy.
3. Apply existing reconciliation behavior to remote removals; only protect playback state HifiMule can observe.

**Phase 5 — Refinement**

1. Add read-only series and collections playlists.
2. Preflight compatibility where it is cheap enough to keep browsing responsive.
3. Consider editable collections and folder-level selection only with a cross-server parity case.

### Architecture Elements

- **Audiobookshelf client:** authentication, library discovery, catalog browsing, playback URL/stream acquisition, and later progress API calls.
- **Library-role resolver:** determines audiobook versus podcast server behavior from the selected library type.
- **Audiobook mapper:** materializes book/chapters/metadata into existing album/track/artist primitives.
- **Podcast mapper:** materializes show/episode data into a distinct dynamic catalog representation.
- **Playback identity bridge:** stores remote item identity and whole-item offsets for safe handoff and reporting.
- **Transfer eligibility adapter:** later evaluates direct compatibility/transcoding support before local transfer.

### Action Planning

**Immediate next steps**

1. Audit the existing audio-server connector and playback abstractions to identify the extension points for a new server type.
2. Validate Audiobookshelf's library discovery, book/podcast catalog, streaming, authentication, and progress APIs against the Phase 1 and Phase 2 requirements.
3. Write the concrete server/domain mapping contract and acceptance scenarios for Phase 1 audiobook playback.
4. Implement and test the audiobook vertical slice before designing podcast Autofill/local-sync behavior.

**Success indicators**

- A user can add a Books library, see correctly ordered books and chapters, and play them from HifiMule.
- Author and narrator metadata appear in their intended hierarchy.
- Progress can be safely handed between Audiobookshelf and HifiMule once Phase 2 ships.
- A Podcasts library can later be added without altering audiobook server behavior or budget semantics.

## Session Summary and Insights

The key decision is to treat Audiobookshelf as a shared connector with library-determined, independent HifiMule server roles. Audiobook playback is the first vertical slice; podcast playback follows separately; local sync and Autofill are deliberately deferred. Progress synchronization belongs to player integration, never generic file synchronization.
