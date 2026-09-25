---
baseline_commit: e5ad913d2a3381942281520798cbf7c51617e587
---

# Story 17.9: Refine Audiobookshelf grouping and compatibility feedback

Status: review

## Story

As a listener, I want source groupings and compatibility feedback that make the integration clear,
so that I can curate and synchronize confidently as the library changes.

## Acceptance Criteria

1. Audiobookshelf Books series and collections appear as read-only playlists for the selected Books server. Their stable source identity, names, membership, and order are preserved where the source supplies them. Opening a grouping resolves its books through the existing playlist browse path; missing or removed books are handled safely.
2. The UI shows direct or transcoded compatibility ahead of sync only when the answer can be obtained cheaply and reliably from already available metadata or a bounded probe. Unknown is distinct from compatible and incompatible; browsing remains responsive and does not start per-item media sessions or fan out unbounded network probes.
3. Sync planning gives an actionable eligibility or block reason for each selected Audiobookshelf book part or podcast episode, using the actual target device profile and the existing provider-owned representation resolver. Preview and execution agree; an unknown browse-time status cannot be presented as guaranteed compatibility.
4. Series and collections cannot be edited, reordered, renamed, deleted, or written back from HifiMule. There is no Audiobookshelf source folder or collection filter. Existing device destination folders, selection, playback, podcast browse, and normal music playlist editing remain functional.
5. Offline fixtures and tests cover grouping pagination and membership, overlapping IDs or names, stale entries, read-only actions, compatibility known/unknown/blocked states, responsive browse behavior, and preview/execution parity. Verify existing Jellyfin/Subsonic browse and playlist writing paths remain functional.

## Tasks / Subtasks

- [x] Validate the Audiobookshelf grouping contract against the supported server fixture/version set (AC: 1, 5)
  - [x] Capture sanitized series and collection list/detail fixtures, including pagination, membership and ordering; document any version differences in `docs/audiobookshelf-integration-contract.md`.
  - [x] Keep grouping IDs namespaced by type and selected library/server; reject cross-library members.
- [x] Map groupings through `MediaProvider` read-only playlist methods (AC: 1, 4)
  - [x] Extend `AudiobookshelfProvider::list_playlists` and `get_playlist`; retain `supports_playlist_write: false` and reject mutation methods.
  - [x] Use existing playlist RPC and UI browse rendering. Expose a clear read-only affordance and hide/disable write controls only for this provider; keep music provider write behavior.
- [x] Add progressive compatibility feedback (AC: 2, 3)
  - [x] Define explicit direct, transcoded, blocked and unknown states and their localized user text. Use available facts for cheap browse-time status; defer uncertain decisions to sync planning.
  - [x] Reuse Story 17.8's target-profile preview admission and blocked reasons; avoid starting playback sessions or fetching full media during browsing.
- [x] Verify regression and responsiveness (AC: 1–5)
  - [x] Test bounded paging/lazy requests, stale and duplicate members, auth failure, and cross-server isolation.
  - [x] Test compatibility UI states against preview results, including unsupported formats and transcode failure; run focused daemon, UI, and contract checks.

## Dev Notes

### Current code and required changes

- `hifimule-daemon/src/providers/audiobookshelf.rs`: `list_playlists` and `get_playlist` currently return unsupported; provider capabilities already set `supports_playlist_write: false`. Implement only read mapping here, using the provider's existing authenticated request and selected-library scope. Preserve Books/Podcasts role separation and stable catalog identities.
- `hifimule-ui/src/library.ts`: existing `mapPlaylists`, `loadPlaylists`, and `loadPlaylistTracks` provide the browse path. Adapt display/read-only actions within those flows. `hifimule-ui/src/rpc.ts` already carries playlist contracts. Audit Autofill playlist-source capability gating so read-only ABS groupings do not accidentally imply playlist write support.
- `hifimule-daemon/src/sync.rs` and `hifimule-daemon/src/rpc.rs`: Story 17.8 already provides provider-owned media resolution, profile-aware pre-transfer compatibility, typed audiobook/podcast identity, and preview blocked reasons. Extend or surface those results; do not create a second compatibility engine. Preserve atomic staging, managed-only deletion, independent server budgets, media-role folder routing, and preview/execution parity.
- `hifimule-i18n/catalog.json` and UI tests: add wording for read-only groupings and compatibility states consistently across locales. Preserve accessible control patterns.

### Contract and implementation guardrails

- The official Audiobookshelf API documents `GET /api/libraries/<ID>/series` with required positive `limit` and zero-indexed `page`, and `GET /api/libraries/<ID>/collections` with optional paging. Collection detail is `GET /api/collections/<ID>`. Validate actual supported-server responses before deciding whether list data already carries complete members; do not assume the search endpoint paginates the same way. [Audiobookshelf API reference](https://api.audiobookshelf.org/)
- The controlled contract has not yet established series/collection ordering and membership behavior. If source ordering is absent, use a documented stable fallback; never invent a source order. Keep credentials and private media URLs inside the provider and redact fixtures/logs.
- A prior forced-direct probe accepted an unsupported MIME type; that result cannot establish actual device compatibility. Only a verified representation plus target profile can justify a definitive claim. Cache only with a valid scope/key, and invalidate when server, item, profile or representation changes.
- Do not add new dependencies or upgrade pinned libraries for this story. Existing provider, playlist, sync-preview, and UI mechanisms cover the intended path.

### Previous story intelligence

- Story 17.8 completed typed ABS sync admission, profile compatibility preview, podcast retention, and separate device destination folders. Review fixed staged direct-media retry validation and unknown-duration capacity estimates. Maintain those guards while adding feedback. Its tests passed offline, but no live device or remote server was used; this story needs fixture-backed endpoint evidence.
- Git commit inspection in this workspace was unavailable due to repository ownership checks. Use the story record and current files rather than assuming recent commit contents.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` § Epic 17, Story 17.9]
- [Source: `_bmad-output/planning-artifacts/prd.md` § Audiobookshelf Extension, FR82–FR87 and non-goals]
- [Source: `_bmad-output/planning-artifacts/architecture.md` § Epic 17 — Audiobookshelf Architecture Amendment]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md` § 8]
- [Source: `_bmad-output/implementation-artifacts/17-8-synchronize-audiobookshelf-media-with-independent-policies.md`]
- [Source: `docs/audiobookshelf-integration-contract.md`]

## Dev Agent Record

### Agent Model Used

GPT-6

### Debug Log References

- 2026-09-25: Began Story 17.9 at baseline `e5ad913d2a3381942281520798cbf7c51617e587`. The required first task needs observed series and collection list/detail responses from a supported server. Existing v2.36.1 fixtures contain no grouping responses, and this workspace exposes no controlled probe configuration. Official API documentation gives endpoint hypotheses but cannot establish version-specific membership or ordering. Paused before creating fixtures or implementing grouping behavior; requested controlled server access or sanitized responses.
- 2026-09-25: User supplied controlled v2.36.1 test-server access. Probed Books series/collection paging and detail shapes with a disposable account; kept only alias-only observations. The initial pause was resolved in this run.
- 2026-09-25: Implemented paged read-only group mapping, source-order part resolution from embedded book metadata, selected-library checks, stale/duplicate skipping, and portable server tagging on playlist RPC results. Kept playlist mutations unsupported.
- 2026-09-25: Added unknown browse labels and profile-backed direct/blocked sync planning feedback for audiobook parts and podcast episodes. Transcoded wording is defined for a verified future representation, while the current ABS resolver only admits verified direct delivery.

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- Controlled v2.36.1 grouping list/detail responses were validated and summarized in an alias-only fixture and contract record.
- Books series and collections now browse as read-only playlists with distinct source identities. Opening a group uses embedded members and does not start media sessions or fetch each item.
- The selected server ID follows playlist browse results into device selection. Sync planning lists verified direct eligibility or an actionable block reason using the selected device profile; browse-time compatibility remains unknown when metadata cannot prove it.
- Verification: full daemon suite passed (1,139 unit tests, 5 contract tests, 6 ignored); 26 UI tests passed; TypeScript check, production UI build, Rust formatting, Git diff check, and Clippy completed successfully. Clippy reported existing non-fatal warnings.

### File List

- `_bmad-output/implementation-artifacts/17-9-refine-audiobookshelf-grouping-and-compatibility-feedback.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `docs/audiobookshelf-integration-contract.md`
- `hifimule-daemon/src/providers/audiobookshelf.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-daemon/tests/fixtures/audiobookshelf/2.36.1/README.md`
- `hifimule-daemon/tests/fixtures/audiobookshelf/2.36.1/grouping-observations.json`
- `hifimule-i18n/catalog.json`
- `hifimule-ui/src/components/BasketSidebar.ts`
- `hifimule-ui/src/library.ts`
- `hifimule-ui/src/rpc.ts`
- `hifimule-ui/tests/audiobookshelfBrowse.test.mjs`

## Change Log

- 2026-09-25: Implemented Audiobookshelf read-only series/collection browsing and profile-backed compatibility feedback; validated controlled v2.36.1 grouping behavior and regression suites.
