# Sprint Change Proposal — Desktop Playback release and Radio/Recommendations

Date: 2026-09-19
Project: HifiMule
Requested by: Alexis
Review mode: Batch
Scope: Moderate — backlog reorganization, a new transport story and a compact browse-navigation story
Status: Approved by Alexis ("go") and applied on 2026-09-19. Planning artifacts and sprint tracking updated; production implementation and publication remain separate.

## 1. Issue summary

Story 15.14 completes a coherent manual desktop-listening experience. Alexis wants to release that experience after adding Back transport, compacting the library browse-mode bar and completing packaging, without waiting for automatic selection and the remaining playback roadmap.

The requested change is a delivery-scope decision plus new transport and presentation requirements, not a failed implementation. Keep completed Stories 15.1–15.14 in Epic 15. Move every existing backlog story except packaging to **Epic 16: Radio/Recommendations**. Add Back to the playing bar and support the corresponding keyboard/media command where available. Add Story 15.16 to redesign the Tracks / Albums / Recently Added browse-mode bar with icons and smaller text; move packaging to 15.17.

Evidence:

- `sprint-status.yaml` and the Story 15.14 header mark Stories 15.1–15.14 done. Stories 15.15–15.29 are backlog; no dedicated implementation story files exist for those backlog entries.
- Old Story 15.29 depends on 15.1–15.28 and explicitly tests Radio, feedback, exports and adaptive quality. Moving it without changing these dependencies and criteria would leave the release blocked by Epic 16.
- `hifimule-daemon/src/playback/native.rs` currently advertises `previous: false` and does not map the native Previous event. Back therefore requires session/native/RPC behavior, not merely a bar icon.
- Story 15.14 and `docs/playback-installed-test-checklist.md` retain outstanding installed/full-application checks and older in-progress wording despite the current done status. Packaging must reconcile that evidence without assuming a completed tracker row certifies an installed release.

Interpretation of “all backlog”: the fourteen remaining Epic 15 feature/reliability stories, old 15.15–15.28. Older epics, completed stories, and the unrelated deferred-work ledger are not renumbered. Reporting, export and adaptive-quality stories apply beyond Radio, but move with this follow-on roadmap as explicitly requested.

## 2. Impact analysis

### Epics and story identities

Epic 15 remains **Desktop Playback**, with 17 stories: fourteen completed stories, a new Back story, a compact browse-mode bar story, and the rescoped packaging story. Epic 16 contains the fourteen moved backlog stories, preserving their relative order and substantive acceptance criteria except for dependency/reference changes and the later release-validation ownership described below.

| Old ID | New ID | Story |
|---|---|---|
| New | 15.15 | Restart the current track or return to the previous track |
| New | 15.16 | Compact the library browse-mode bar with icons and smaller labels |
| 15.29 | 15.17 | Ship verified playback builds for Windows, macOS and Linux |
| 15.15 | 16.1 | Configure Playback selection and start its first selected track |
| 15.16 | 16.2 | Replenish Radio within a bounded upcoming queue |
| 15.17 | 16.3 | Continue Radio through meaningful artist connections |
| 15.18 | 16.4 | Avoid duplicate Radio recordings across configured servers |
| 15.19 | 16.5 | Match Radio track loudness using available metadata |
| 15.20 | 16.6 | Start Radio with Play something without opening the main window |
| 15.21 | 16.7 | Report listening accurately to the source server |
| 15.22 | 16.8 | Save supported Like and Dislike preferences on the source server |
| 15.23 | 16.9 | Save an immutable local listening snapshot |
| 15.24 | 16.10 | Save a listening snapshot as playlists on its source servers |
| 15.25 | 16.11 | Add a listening snapshot to a connected device basket or replace it |
| 15.26 | 16.12 | Adapt playback quality at track boundaries using buffer health |
| 15.27 | 16.13 | Protect playback during sync without unnecessary throttling |
| 15.28 | 16.14 | Keep long listening sessions bounded and recoverable |

After application, Epic 15 stays in-progress, 15.15–15.17 are backlog, Epic 16 and 16.1–16.14 are backlog, and both retrospective entries are optional. Preserve all existing done statuses.

### Product, architecture and UX

- PRD: add Back to FR58 and distinguish the Epic 15 manual-playback release from the retained Epic 16 product outcomes. The original sync MVP and device safety requirements are unchanged.
- Architecture: keep daemon ownership, source identity, persistence, generation fencing and the existing audio stack. Change delivery order so manual playback packaging precedes Radio. Describe one shared Back command for UI and native delivery.
- UX: extend the current two-row playing bar from Story 15.14. Back sits alongside the existing transport controls, retains visible keyboard focus and localized naming, and does not obscure content. Idle manual browsing/Resume stays useful until working Play something arrives in 16.6.
- Browse navigation: Story 15.16 redesigns the existing capability-driven Tracks / Albums / Recently Added mode selector with icons and smaller visible labels, preserving every supported mode and its behavior.
- Coverage: FR55, FR59–60, FR68, FR71, FR73–75 have foundation work in Epic 15 and completion work in Epic 16. Do not describe all FR55–81 as completed by the first release.
- Context: replace the combined Epic 15 context with release scope and create an Epic 16 context retaining the moved Radio, reporting, export and reliability contracts.
- Validation: retain installed runtime, output safety, ordinary playback-plus-sync, accessibility and lifecycle verification in 15.17. Future adaptation/backoff/Radio measurements move to their owning Epic 16 stories; integrated installed verification of those features is retained under 16.14.

### Technical impact

This course correction changes planning documents, not production code, dependency versions, deployment scripts or release publication. The future Back implementation touches daemon session/commands/persistence as necessary, native capability/event routing, the RPC bridge and the existing `PlaybackControls.ts`. Exact replay identity and persistence changes must be settled during story preparation. No new playback state owner or recommendation engine is required for Back. The browse-navigation story extends `renderModeBar()` in `hifimule-ui/src/library.ts` and its existing styles/components; it preserves mode selection, provider capabilities and data-loading behavior.

## 3. Recommended approach

**Direct adjustment:** split the existing roadmap at the delivered manual-listening boundary, add Back and compact browse navigation, and rescope packaging. No completed work is rolled back and no approved Radio requirement is deleted.

- Planning effort: low; deterministic renumbering plus scope/coverage updates.
- Back implementation effort: medium; includes backward navigation through retained occurrences, pause/preview behavior and native command parity.
- Browse-mode redesign effort: low to medium; presentation changes plus responsive, localization and keyboard regression checks.
- Packaging effort: dependent on the current shipping matrix and unresolved evidence; this proposal does not invent a completion date or passing results.
- Main risks: stale references after renumbering, accidental dependencies from packaging back into Radio, incorrect replay/history semantics, and treating old test results as certification of a changed package.
- Timeline: Epic 15 can close after Back, compact browse navigation and release verification. Fourteen backlog stories no longer block that release. Epic 16 follows with the retained roadmap.

Rollback was considered and rejected: the completed manual player is the release foundation. A fundamental MVP redesign is unnecessary; the delivery boundary changes while the long-term requirements remain.

## 4. Detailed change proposals

### A. Epic scope and dependencies — `epics.md`

**OLD:** One consolidated Epic 15 owns all FR55–81 and includes Stories 15.1–15.29, ending with packaging dependent on all preceding stories.

**NEW — Epic 15 goal:** Deliver daemon-owned manual desktop listening on Windows, macOS and Linux: durable paused restoration, selected-track and album playback, output safety, album continuity/gain, full-track auditions, an editable queue, persistent playing-bar controls including Back, compact library navigation with icons and smaller labels, and verified installed packages. Close the release through Story 15.17 without requiring Radio or its follow-on integrations.

**NEW — Epic 16 goal:** Add Radio/Recommendations through playback-specific automatic selection, bounded replenishment, explainable artist progression, recording deduplication and track loudness. Extend the listening experience with source-server reporting/preferences, immutable snapshots and exports, adaptive quality, conditional sync protection, and sustained installed validation.

**Dependency rewrite:**

- 15.15 depends on 15.1–15.14. Story 15.16 uses the existing browse-mode navigation and 15.14 library/Playing integration; it does not technically depend on Back. Packaging 15.17 depends on 15.1–15.16. None depends on Epic 16.
- 16.1 uses the delivered Epic 15 foundation and the existing auto-fill engine. Subsequent 16.N stories depend on that foundation and preceding 16.1–16.(N−1), preserving the current sequence. Epic 16 is scheduled after the Epic 15 release gate.
- Translate old ranges such as `15.1–15.20` into the relevant Epic 15 foundation plus `16.1–16.6`, not a malformed cross-epic numeric range.
- Preserve named cross-story dependencies, including album gain in 15.10 and preview in 15.11.
- Update frontmatter approval metadata only after this proposal is approved, the Epic List, delivery groups, FR coverage, P-AR13 sequence and end-of-file readiness statement. Record the dated mapping so historical references remain interpretable.

Rationale: establish independent release closure without losing any planned outcome.

### B. New Story 15.15 — Restart the current track or return to the previous track

**OLD:** No Back story; FR58 names play/pause, stop, next, position and output, and native Previous is disabled.

**NEW:**

As a HifiMule user,
I want a Back button in the playing bar and equivalent supported keyboard/media control,
So that I can restart a track or return to the previous track without rebuilding my queue.

**Requirements:** amended FR58; FR59 transport presentation; FR60 persistence and FR61 ordering constraints; P-NFR3–6; P-AR3–4, P-AR6 and P-AR10; P-UX-DR2 and P-UX-DR12–14.

**Dependencies:** Stories 15.1–15.14. No Radio prerequisite.

**Acceptance criteria:**

1. **Restart after three seconds.** Given a main track with authoritative position greater than 3,000 ms, when Back is accepted, then the current track returns to its beginning. The decision uses daemon position rather than interpolated UI time. Playing remains playing and paused remains paused, subject to existing output/error inhibition.
2. **Previous near the beginning.** Given a main track at 0–3,000 ms inclusive and an available preceding occurrence in the retained main-session playback order, when Back is accepted, then that preceding track becomes current at its beginning. Preserve its source identity, accepted forward order and deliberate repeated entries. Back followed by forward progression returns through the same sequence without silently dropping or duplicating queued selections.
3. **Beginning and unavailable-source boundaries.** Given no preceding occurrence, Back restarts the current track where supported, without wrapping to the queue end. With no current track, it is unavailable. If the requested restart/previous source cannot be prepared, preserve recoverable state and expose an error; do not silently choose a different track or output.
4. **Preview isolation.** Given an active audition, Back restarts that audition and never navigates the preserved main history. The saved main occurrence, queue, position and return intent remain intact. Existing Return and Stop semantics are unchanged.
5. **Shared UI/native command.** Given an available Back action, clicking its bar button, activating the focused button with Enter/Space, or receiving the OS Previous/media-key command invokes the same daemon operation. Support native Previous/keyboard delivery wherever the platform integration provides it, including with the UI closed. Advertise truthful native availability; document unsupported delivery. Do not introduce an arbitrary global key combination or hijack text-editing, seek-slider or browser-navigation keys.
6. **Accessible bar integration.** Given any library/Playing/device view and supported layout, Back remains reachable beside transport controls with a localized accessible name, tooltip/help explaining restart versus previous behavior, visible focus and truthful disabled state. Preserve the current two-row layout, timeline, bottom-row reachability and focus during updates.
7. **Ordering and history integrity.** Given repeated commands or a race with automatic advance, queue edits, seek, preview, source replacement or Quit, the daemon serializes decisions, deduplicates retried command identities and fences obsolete preparation. Stale UI commands cannot rewind a newly selected session. Replaying earlier music must not overwrite already-recorded outcomes; specify replay occurrence identity and forward-cursor behavior before coding.
8. **Persistence and regression evidence.** Given a successful Back followed by orderly Quit/relaunch, restore the accepted current track, queue and position paused. Verify the 0, 3,000 and 3,001 ms boundaries; first-track fallback; repeated source IDs; manual and album queues; paused state; Preview with/without a main session; unavailable sources; output loss; concurrent commands; and restoration. Record UI/native API and physical keyboard/media-key results separately on shipping platforms.

**Implementation gate:** Inspect the existing history, queue cursor and persistence model; freeze replay identity, forward progression, stopped/completed-state behavior, command preconditions/error codes and per-platform native capability mapping. Restart may reuse seek or reopen from zero when safely supported; unsupported behavior must be explicit. Use existing command admission, generation fences and provider routing. Back is not a skip/dislike or an automatic-selection request. Future 16.7 reporting must account for replay without inferring completed listens merely from cursor movement.

The three-second boundary and audition restart rule are proposed product choices in this batch, not claims about existing implementation.

### C. New Story 15.16 — Compact the library browse-mode bar with icons and smaller labels

**OLD:** `renderModeBar()` creates text-only small Shoelace buttons from the available browse modes; `.browse-mode-bar` uses wrapping flex layout with a 0.5rem gap.

**NEW:**

As a HifiMule user,
I want a compact library navigation bar with recognizable icons and smaller text,
So that I can switch between Tracks, Albums, Recently Added and other browse modes while leaving more room for music.

**Requirements:** FR8; P-NFR6; applicable P-UX-DR10–14; existing provider-capability and browse-state contracts.

**Dependencies:** Existing browse-mode navigation and Story 15.14 library/Playing integration. Scheduled as 15.16 before packaging 15.17; no Radio prerequisite.

**Scope:** The library browse-mode selector (Tracks, Albums, Recently Added, Artists, Playlists, Genres, Frequently Played, Recently Played and Favorites where supported). This is the navigation bar identified by the user, not the playing transport bar or per-track basket action buttons.

**Acceptance criteria:**

1. **Compact icon-and-label controls.** Given the available library browse modes, when the bar renders, each mode has a consistent recognizable icon and a smaller visible localized text label. Reduce visual bulk and spacing relative to the current bar while keeping labels readable and pointer targets usable. Record before/after measurements at matching viewport, language and available-mode count; merely shrinking text without improving the layout is insufficient.
2. **Preserved navigation.** Given any supported mode, activating its redesigned control opens the same view and uses existing loading, breadcrumb, selection-reset and cached-view behavior. Retain capability filtering, loading/disabled state and clear current-mode indication. Do not introduce duplicate fetches or change playback, selected source or basket contents as a presentation side effect.
3. **Responsive layout.** Given narrow, medium and wide library columns, divider changes, long translated labels or 200% text scaling, every available mode remains reachable without clipped controls, overlap or horizontal page overflow. Choose and document wrapping or accessible overflow during story preparation; keep the selected mode identifiable and any overflow keyboard-operable. Preserve the grid/list toggle and its existing mode-specific availability.
4. **Keyboard and accessible naming.** Given keyboard or assistive-technology navigation, each control has its full localized accessible name, visible focus and programmatically exposed selection state. Icons do not cause duplicate announcements. Enter/Space retain button activation, and metadata/loading updates do not unexpectedly move focus. Any abbreviated visible label has a full hover/focus hint; tooltips are not the sole accessible name.
5. **Consistent visual treatment.** Given supported themes and enabled/selected/disabled states, the bar uses existing Shoelace tokens and icon conventions with sufficient text/icon contrast. Compact styling does not shrink unrelated application buttons or alter track/album content typography.
6. **Visual and behavior verification.** Given representative supported-provider mode sets and EN/FR/ES/DE labels, verify the actual rendered layout, mode switching, active state, grid/list toggle, keyboard focus and constrained-width/text-scale behavior. Record checks on supported installed platforms or retain explicit unverified entries for packaging 15.17; DOM-only assertions do not certify rendered compactness.

**Implementation gate:** Confirm the current bar boundaries, icon mapping, label sizing, spacing, target sizes and responsive policy against the existing library layout before coding. Use a focused visual comparison to settle these details; preserve existing navigation semantics rather than creating a new mode model.

Rationale: the requested release polish belongs before packaging and has no dependency on automatic selection.

### D. Packaging — old 15.29 becomes 15.17

**OLD requirements:** “release evidence for FR55–81 and applicable P-UX-DR1–15.”

**NEW requirements:** Installed-release evidence for implemented portions of FR55–64, FR68, FR71, FR73–75 and the new Back behavior, plus FR8 browse navigation and applicable P-NFR6/P-UX-DR10–14 checks for Story 15.16; applicable P-NFR1–6, P-AR5/P-AR14 and UI requirements. Do not claim completion of deferred Radio, reporting, feedback, snapshots, adaptive quality or conditional backoff.

**OLD dependencies:** “Stories 15.1–15.28.”

**NEW dependencies:** “Stories 15.1–15.16. Completes packaged integration and verification for the manual Desktop Playback release. Epic 16 is not a prerequisite. Actual publication remains a separate release action.”

**Acceptance-criterion replacements:**

| Existing packaging criterion | Proposed replacement |
|---|---|
| Integrated manual album, preview, Radio, feedback, snapshot, playlist and basket workflows | Installed selected-track/album playback, manual queue edits, Preview/Return, playing-bar transport including Back/Next/seek, output selection, source routing and existing device-sync regression. The manual idle action stays available; unimplemented Radio actions are not advertised. |
| Link adaptive-quality results and sustained resource measurements | Link version-matched continuity, bounded-resource, recovery and ordinary real-sync-coexistence measurements for shipped manual playback. Set verification workload/budgets before acceptance. Adaptive quality, conditional backoff and full Radio soak remain 16.12–16.14. |
| Accessibility of complete Playback including export results | Accessibility of shipped bar/queue/preview/errors, including Back and the compact browse-mode bar, narrow layouts, zoom, localization, keyboard and screen-reader behavior. Export UI acceptance moves with Epic 16. |
| Boundary-only adaptation is the initial supported scope | This release makes no adaptive-quality claim; retain implemented source selection and buffering/retry. Epic 16 owns boundary adaptation; mid-track replacement remains disabled without separate validation. |

Keep the other packaging criteria: controlled runtime and loaded versions, licensing/signing assessment, clean installation, upgrades/migrations, every shipping architecture, daemon lifetime, native API versus physical keys, output loss, sleep/wake, safe Quit during sync, explicit failures and Linux teardown assessment. Do not remove ordinary playback safety or actual resource verification merely because the extended soak story moves.

Add an explicit evidence-reconciliation criterion: inventory outstanding applicable checks and deferred defects from Stories 15.1–15.14 and the installed checklist, including the recorded 15.12 R16 basket guard and 15.14 R8/R9 issues. Resolve release blockers or document an explicit scoped disposition with evidence; preserve accurate unverified rows. Reconcile stale in-progress prose without rewriting historical results as passes.

### E. Preserve the later installed-release gate — Story 16.14

**OLD:** Old 15.28 says packaged distribution and the complete shipping-platform matrix are covered separately by the later packaging story.

**NEW:** Reuse the packaging process established in 15.17 and extend the installed shipping-platform matrix to Epic 16's automatic selection, Radio, feedback/reporting, snapshots, playlist/basket exports, adaptation and conditional sync protection. Retain all original soak/recovery criteria. Changed runtime dependencies or affected workflows require fresh corresponding evidence. Earlier package results do not certify newly added features. Actual publication stays outside story validation.

Rationale: packaging stays in Epic 15, while moving its future-feature checks does not leave the Radio release without an installed acceptance owner. Split platform execution tasks during story preparation if needed; no additional product story is introduced by this proposal.

### F. PRD amendments — `prd.md`

**Purpose/scope OLD:** “The implementation stages in the architecture are delivery order, not permission to omit requirements below.”

**NEW addition:** “Delivery is split into Epic 15 (manual Desktop Playback release, including Back, compact browse navigation and packaging) and Epic 16 (Radio/Recommendations and the remaining playback roadmap). All requirements below remain product commitments. Completion of Epic 15 establishes only its explicitly mapped release subset; it does not claim Radio, listening exports, adaptive quality or other Epic 16 features.”

**FR58 OLD:** “Users can control play/pause, stop, next, playback position where supported, and output selection.”

**FR58 NEW:** “Users can control play/pause, stop, Back (restart the current track or return to the previous track), next, playback position where supported, and output selection. Back restarts after three seconds of main-track playback and selects the previous occurrence at or before three seconds; without a previous occurrence it restarts the current track. During Preview it restarts the audition without altering the preserved main session. UI and supported native Previous/keyboard controls use the same session command, preserving paused intent and output safety. Capability limits and recoverable errors are visible rather than silently ignored.”

**Success/release evidence OLD:** A single success paragraph requires Play something and playlist/basket exports.

**NEW:** Separate Epic 15 success (manual tracks/albums, Preview, queue, Back, compact browse navigation, durable session and installed validation) from Epic 16 success (automatic selection, Radio, source reporting/feedback, exports and adaptive reliability). Keep current measurement cautions and long-term outcomes intact.

Mirror FR58 and the phase boundary into the requirements inventory in `epics.md` to prevent divergence.

### G. Architecture and UX amendments

**Architecture OLD:** Seven stages end with combined adaptation, stress tests and packaged checks after Radio and library integration.

**NEW sequence:**

1. Retain delivered lifecycle, player, album/preview and manual Playback UI foundations (15.1–15.14).
2. Add shared Back transport (15.15), compact the library browse-mode bar (15.16), then verify/package that manual release (15.17).
3. Implement automatic-selection settings and Radio (16.1–16.6).
4. Implement source-server reporting/preferences and exports (16.7–16.11).
5. Implement adaptation, conditional backoff and sustained/installed validation of the expanded release (16.12–16.14).

Add to UI/session-control contracts: one daemon-owned Back decision based on authoritative position and occurrence context, preserving preview isolation, playback intent, source identity and generation fencing. Existing audio topology and component ownership do not change; no topology diagram update is needed. Update the handoff's seven-stage wording accordingly.

**UX OLD:** Base `ux-design-specification.md` does not specify Back; playback-specific requirements are in `epics.md`, and Story 15.14 documents the implemented two-row bar.

**NEW:** Add a focused playback-bar subsection referencing that implemented layout and the new 15.15 behavior. Extend P-UX-DR2/P-UX-DR13 in `epics.md` to cover Back and supported Previous keyboard/media delivery. Retain the manual Library/Playing navigation and idle browsing/Resume until 16.6 supplies working Play something. Update the existing §5.1 Navigation description to specify compact icon-and-small-label browse controls, with Story 15.16 owning the detailed layout, responsive behavior and accessibility checks. No unrelated redesign of the base UX document.

### H. Tracking, coverage and context artifacts

- `sprint-status.yaml`: replace old 15.15–15.29 backlog keys according to the mapping, add the new 15.15 and 15.16 keys, retain 15.1–15.14 done, add epic-16 and its optional retrospective, and update the amendment comment/date. Result: 31 stories across the two epics, 17 in Epic 15 and 14 in Epic 16.
- `playback-epic-validation.md`: replace the single-epic/29-story claims and ambiguous bare story numbers with fully qualified IDs. Move old 29 coverage to 15.17 only for shipped features; assign future installed integration to 16.14. Add 15.15 to FR58/FR59 and applicable state, native, accessibility and persistence coverage. Add 15.16 to FR8 and applicable usability, design-token, browse-preservation and responsive/accessibility coverage. Preserve open implementation gates and distinguish planning coverage from runtime evidence.
- `playback-story-review.md`: replace the current all-29-approved summary with the approved split and link this amendment after approval.
- `epic-15-context.md`: narrow its goal, story list, requirements and cross-story dependencies to manual release scope, referencing Epic 16 for later work.
- New `epic-16-context.md`: carry the moved feature contracts, source/recording/occurrence distinctions, exclusions, provider semantics, bounded work and dependency sequence.
- Completed implementation stories with future-facing references: translate live references to moved work (for example 15.14's old 15.20 becomes 16.6). Preserve completed story IDs, execution history and historical evidence. Do not blindly replace every historical mention of `15.15` now that it denotes Back.
- `docs/playback-installed-test-checklist.md`: record 15.17 as the current release-validation owner and 16.14 as the later expanded owner. Actual test-result updates belong to execution, not this planning change.
- Historical proposals/reviews remain historical; this mapping resolves their old numbering. No dedicated implementation files are renamed for the moved backlog because none exist yet. New entries remain backlog until create-story preparation.

## 5. Implementation handoff and success criteria

**Product Owner / planning owner:** after approval, apply the document and tracker amendments together, regenerate/synchronize the two epic contexts, and audit all active references and requirement ownership. No need to dispatch another agent merely to make these documentation edits.

**Developer / story author:** prepare and implement 15.15 next, closing replay/native/state contracts; prepare and implement the compact browse-mode bar in 15.16; then prepare and execute 15.17 against actual artifacts and platform evidence. Address applicable release blockers through that gate. Continue Epic 16 afterward in its preserved order.

**Release owner:** decide publication only after the packaging story's evidence is reviewed. This proposal neither bumps a version nor publishes a release.

Success checks for applying the correction:

- Exactly 31 unique story IDs across these epics: 15.1–15.17 and 16.1–16.14.
- Fourteen completed stories remain done; seventeen stories remain backlog (three in Epic 15 and fourteen in Epic 16).
- Every old backlog story has one mapped owner; Back and compact browse navigation are the two new stories.
- No dependency from 15.15, 15.16 or 15.17 to Epic 16; moved dependencies remain backward and explicit.
- No orphaned FR/P-NFR/P-AR/P-UX responsibility; earlier release evidence and later feature completion remain distinct.
- Packaging verifies manual playback, Back and compact browse navigation without requiring unimplemented Radio features; Epic 16 retains installed-validation responsibility for its additions.
- All active future-facing references resolve; historical numbering is explicitly identified.
- No runtime test pass, release certification or completed implementation status is manufactured by the documentation change.

## 6. Change-navigation checklist and workflow log

| Checklist items | Status | Result |
|---|---|---|
| 1.1–1.3 Trigger, problem, evidence | [x] | 15.14 completion, release boundary and missing Back transport identified. |
| 2.1–2.5 Epic scope, future impact, sequencing | [x] | Explicit 15/16 mapping; no other epic invalidated. |
| 3.1–3.3 PRD, architecture, UX | [x] | Specific amendments above; existing technical architecture retained. |
| 3.4 Other artifacts | [x] | Tracker, contexts, coverage, active references and evidence ownership identified. |
| 4.1 Direct adjustment | [x] | Viable and selected. |
| 4.2 Rollback | [N/A] | No failed work to revert. |
| 4.3 Fundamental MVP review | [N/A] | Sync MVP unchanged; playback staged release clarified. |
| 4.4 Recommendation | [x] | Moderate backlog reorganization plus new transport and browse-navigation stories. |
| 5.1–5.5 Proposal and handoff | [x] | Issue, mapping, exact changes, release scope and responsibilities documented. |
| 6.1–6.2 Review completeness/consistency | [x] | Mapping and release-gate ownership checked. |
| 6.3 Complete-proposal approval | [x] | Alexis approved the complete revised batch with "go", including three-second/Preview behavior and the new browse story. |
| 6.4 Apply tracker/artifact changes | [x] | Applied approved epic, tracker, requirements, context and coverage changes. |
| 6.5 Confirm handoff | [x] | Approved sequence: 15.15 → 15.16 → 15.17 → Epic 16; developer/story preparation next. |

Activation: resolved customization; no prepend/append steps; loaded project context and configuration. Batch review selected by Alexis. Proposal prepared from current planning, tracker, implementation evidence and native Previous routing. Approval received and documentation handoff completed.

Revision requested by Alexis: added compact browse-mode navigation as 15.16 and moved packaging to 15.17. Alexis subsequently approved the revised proposal with "go"; live epic and sprint-tracker amendments are applied.


### Application record

Applied epics/story mapping and acceptance changes, sprint tracking, PRD, architecture sequence/contracts, UX refinements, coverage/review summaries, Epic 15/16 contexts, active future-facing references in completed stories and installed-evidence ownership. Historical results remain historical. Back and browse redesign remain backlog entries in epics.md, ready for dedicated story preparation; no implementation readiness is fabricated. Handoff: Developer/story author prepares 15.15 next, then 15.16 and release verification 15.17; Epic 16 follows.

Validation completed: 31 unique approved headings and matching tracker/context entries; 14 done and 17 backlog; existing statuses and completed planning-story content preserved; dependency references point backward with no Epic 15 dependency on Epic 16; moved feature acceptance criteria preserved; FR58 synchronized and FR55–81/P-NFR1–6 coverage retained. Git whitespace checks pass. No production tests were needed for this documentation-only amendment.
