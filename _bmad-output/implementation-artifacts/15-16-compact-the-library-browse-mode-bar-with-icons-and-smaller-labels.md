---
baseline_commit: 2a88f9b1b410e14ef8fe22e80fcff8aef1d4a3de
---
# Story 15.16: Compact the library browse-mode bar with icons and smaller labels

Status: review

## Story

As a HifiMule user,
I want a compact library navigation bar with recognizable icons and smaller text,
so that I can switch between Tracks, Albums, Recently Added and other browse modes while leaving more room for music.

## Acceptance Criteria

1. **Compact icon-and-label controls.** Every available mode has a consistent recognizable icon and smaller visible localized label. Reduce layout bulk and spacing, retain readable labels and usable pointer targets, and record before/after measurements at matching viewport, library-column width, language and available-mode count. Shrinking text alone does not satisfy this criterion.
2. **Preserved navigation.** Activating each supported mode uses the existing view/loading/breadcrumb/selection-reset/cache path. Preserve capability filtering, returned ordering, disabled/loading state and current-mode indication, including after changing servers. Presentation changes do not duplicate fetches or alter playback, source selection or basket contents.
3. **Responsive reachability.** At narrow, medium and wide library widths, during divider resizing, with long translations and 200% text scaling, every mode remains reachable without clipping, overlap or horizontal page overflow. Use wrapping as specified below, preserve the grid/list toggle and its mode-specific availability, and keep the active mode identifiable. Constrained-height overflow must be keyboard-scrollable.
4. **Keyboard and accessible naming.** Each control exposes its full localized name and selected state, retains visible focus and Enter/Space activation, and avoids duplicate icon announcements. Background loading/metadata updates do not unexpectedly move focus. Full visible labels are the chosen design; any subsequent abbreviations require a full hover/focus hint in addition to the accessible name.
5. **Consistent visual treatment.** Use existing Shoelace primitives, application tokens and icons. Enabled/selected/disabled states remain distinguishable with sufficient text/icon contrast. Styling is scoped to this bar; unrelated buttons and track/album typography retain their sizes.
6. **Rendered and behavioral evidence.** Verify actual layout, switching, current state, grid/list availability, focus, constrained width and text scaling across representative provider mode sets and EN/FR/ES/DE. Record installed-platform checks actually performed; retain explicit unverified entries for packaging 15.17. DOM doubles alone cannot certify compactness or native keyboard/accessibility behavior.

## Tasks / Subtasks

- [x] Confirm application baseline and apply the prepared visual specification (AC: 1, 3, 5, 6).
  - [x] Capture the current real library bar before editing at matching widths/locales/mode sets; record bar height, rows, button size, gaps and library-content height.
  - [x] Use the mapping, dimensions and wrapping policy below; compare against the isolated design companion. Verify actual Shoelace internal button styling and vertical icon/label alignment.
  - [x] Exercise narrow/short-window and 200% text cases before finalizing CSS; keep navigation, content and playback controls reachable.
- [x] Extend the existing renderer without changing navigation ownership (AC: 1, 2, 4).
  - [x] Add an exhaustive typed icon map and full visible labels from `modeLabel()`; retain `data-mode` and `switchMode(mode)`.
  - [x] Reconcile rendered keys/order against `state.availableModes`, including source changes; reuse unchanged buttons and avoid duplicate listeners.
  - [x] Update selected, disabled and accessible states on creation and refresh. Preserve focus when the mode remains available; use a sensible fallback only when a focused control disappears.
- [x] Implement scoped compact/responsive styles and preserve the view toggle (AC: 3–5).
  - [x] Wrap modes naturally and move Grid/List together to the next row when needed. Retain Tracks/loading availability rules and shared grid/list state.
  - [x] Preserve `[hidden]` handling and retained Library/Playing DOM. Avoid recreating focused toggle controls on unchanged updates.
  - [x] Reuse local icons and theme tokens; change translations only if a new accessible group label is necessary, with four-locale parity.
- [x] Verify behavior and rendered result, then record evidence (AC: 1–6).
  - [x] Exercise supported modes, same-mode/loading activation, capability changes, breadcrumbs, cached views, selection resets and unchanged basket/playback state.
  - [x] Use focused existing-style Node coverage for changed reconciliation behavior and stable node identity; do not add a test framework.
  - [x] Run the frontend build and relevant UI regressions. Record rendered dimensions, screenshots or reproducible observations, accessibility results and platform limitations.

## Dev Notes

### Scope, dependencies and precedence

- Scope is `#browse-mode-bar`: Artists, Albums, Playlists, Tracks, Genres, Recently Added, Most Played, Recently Played and Favorites, where supported. Transport, track actions and baskets are separate controls.
- Requirements: FR8, P-NFR6 and applicable P-UX-DR10–14. Existing navigation and delivered 15.14 Library/Playing integration are prerequisites. There is no Back or Radio dependency. Story 15.17 owns packaged release verification; 16.6 owns Play something.
- Preserve daemon-owned playback, main/Preview sessions, queue/source identity and physical-device selection. Browsing remains available without a device; only device-specific basket/sync actions are gated.
- Follow the approved 2026-09-19 Epic 15/16 split and current source. `project-context.md` principles apply, but its greenfield phase is stale. Historical architecture filenames and purple/Outfit UX styling do not override current code or `DESIGN.md` navy/cyan/Inter tokens. DESIGN's claim that selection lacks styling is stale: the renderer already uses primary/default variants.

### Prepared visual contract

Use full localized labels with decorative leading icons, as ordinary Shoelace buttons in provider-returned order. Choose wrapping instead of a hidden overflow menu; do not introduce a new tablist/radio model or arrow-key contract.

| BrowseMode | Existing label key | Local Shoelace icon |
| --- | --- | --- |
| `artists` | `library.mode.artists` | `music-note-beamed` |
| `albums` | `library.mode.albums` | `disc` |
| `playlists` | `library.mode.playlists` | `collection-play` |
| `tracks` | `library.mode.tracks` | `music-note-list` |
| `genres` | `library.mode.genres` | `tags` |
| `recentlyAdded` | `library.mode.recentlyAdded` | `calendar-plus` |
| `frequentlyPlayed` | `library.mode.frequentlyPlayed` | `bar-chart` |
| `recentlyPlayed` | `library.mode.recentlyPlayed` | `clock-history` |
| `favorites` | `library.mode.favorites` | `heart` |

`frequentlyPlayed` intentionally displays “Most Played” in English. Keep protocol keys unchanged. All nine assets exist locally.

- **Labels:** `0.6875rem` (11px at a 16px root), normal case, Inter/system fallback. Current small buttons already use `0.75rem` (12px); repeating `size="small"` or using DESIGN's 0.8rem label would not shrink them. This scoped one-pixel reduction was examined in the comparison. Do not shrink further; confirm real-app readability and let text scaling increase size.
- **Icons:** `0.875rem` (14px), decorative, same color as text, vertically centered with a 4px gap. Use the existing `sl-icon` prefix convention without a separate spoken label.
- **Targets:** retain the current `1.875rem` minimum height (30px), allowing height to grow for wrapped labels. Reduce surrounding padding rather than hit area. Preserve default focus rings and clearance.
- **Spacing:** mode gaps 8px → 4px; outer vertical padding 8px → 4px; horizontal padding 32px (24px at the existing breakpoint) → 16px. Reuse equivalent spacing tokens. Start with 6px button inline padding; remove redundant label/prefix padding using scoped public CSS parts.
- **Wrapping:** outer bar and inner group use `min-width: 0` and flex wrapping. A mode-group basis near `18rem` lets Grid/List remain alongside at generous widths and move intact below at narrow widths. Buttons use `max-width: 100%`; full labels may wrap, without forced ellipsis. Keep the toggle's own existing size and grouped presentation.
- **Short-window safeguard:** 200% text can make wrapping tall. Bound the navigation region in the real flex layout (initial maximum block size `min(40vh, 20rem)`, border-box sizing, vertical scrolling) so content is not permanently squeezed out. Tab must scroll focused controls into view. Reveal the selected control after explicit selection/capability fallback, not on every metadata tick. Revisit the bound if the actual header leaves insufficient space; reachability takes precedence over fixed height.
- **Theme:** reuse application ink/surface tokens and Shoelace primary/default states. Do not globally override `sl-button` size or use low-contrast neutral text stops. Preserve the authoritative hidden rule for Playing.

### Design comparison performed during preparation

Companion: `15-16-browse-mode-comparison.html`, an isolated source-equivalent baseline and proposal using locally installed Shoelace **2.20.1** bundled CDN-format files. These are local package assets, not a network CDN. Serve the repository root and open the companion URL; controls select column width, locale and root text scaling.

| Scenario (nine modes, Albums selected) | Baseline | Proposed | Observation |
| --- | --- | --- | --- |
| 720px column, EN, 100% text | 84px bar; 12px label; 30px target | 72px bar; 11px label; 30px target | 12px less height; both fit 720px |
| 360px column, DE, 200% root text | 552px natural bar; scroll width 477px | 516px natural bar; scroll width 360px | Proposed controls fit; natural height requires the application scroll safeguard |

These are inspected browser prototype measurements, not full-app or installed-platform passes. The companion simplifies Grid/List to text controls, has no application state/RPC and exposes uncapped natural height to reveal the short-window issue. The 720px observation preceded the narrow-layout flex-basis refinement; measure final production dimensions afresh. Actual divider/header/content behavior, genuine text-only scaling, focus, screen-reader announcements and platform checks remain implementation work.

### Existing files: current state, changes and preservation

| File / symbols | Current behavior | Required change and preservation |
| --- | --- | --- |
| `hifimule-ui/src/library.ts`: `modeLabel`, `renderModeBar` | Small text-only `sl-button[data-mode]` from `availableModes`; primary/default and loading-disabled properties. Existing-button fast path only updates variant/disabled. | Add icon/full-label nodes and selected semantics. Reconcile keys/order on capability changes; retain unchanged nodes and their single handler. Do not preserve the stale-set fast-path defect. |
| Same: `renderViewToggle` | Removes/recreates Grid/List group; absent during loading and in Tracks. | Retain availability rules and shared list/grid preference without refetching. Avoid unnecessary replacement of focused controls. |
| Same: `switchMode`, `loadModeRoot` | Guards same-mode/loading; clears selection, saves scroll, releases Tracks subscriptions on exit, resets breadcrumbs/pagination/root data and dispatches the loader. | Keep this path; no new fetches, independent resets or duplicate Tracks subscriptions in presentation handlers. |
| Same: `initLibraryView`, `clearNavigationCache` | Refreshes modes on initialization; chooses Artists if supported, otherwise first returned mode. Layout can persist across source changes. | Preserve source/cache semantics. Verify broad → limited → broad capability sets without stale buttons or unsupported requests. |
| `hifimule-ui/src/styles.css` | Fixed outer flex row with 8px/32px padding; inner wrap with 8px gaps; right-aligned toggle; responsive padding and explicit hidden rule. | Scope compact/responsive/height styles here. Preserve separate playback bar stacking, content scrolling and unrelated controls. |
| `hifimule-i18n/catalog.json` (conditional) | Complete mode labels in EN/FR/ES/DE. | Reuse labels; add only genuinely needed group text in all locales. Never abbreviate by slicing translated strings. |

Read-only references: `main.ts` retains Library DOM while `showSurface()` hides browsing in Playing; `rpc.ts` defines BrowseMode/RPCs; `i18n.ts` defines runtime locale behavior; `vite.config.ts` copies local assets. No planned Rust, database, provider, transport or dependency change is required.

### Accessibility and focus guardrails

- Use `aria-pressed="true"/"false"` selection semantics and update them whenever `browseMode` changes. Verify exposure on the actual accessible button inside Shoelace's shadow DOM; a host attribute alone is not proof of an announcement.
- Full translated visible text supplies the name. Any explicit accessible name must match it. Icons are decorative (`aria-hidden="true"`, no icon `label`). Avoid duplicate host/inner-button tab stops.
- Keep native Enter/Space activation; do not add key handlers that duplicate click. Preserve loading-disabled admission without forcing focus onto disabled controls or reclaiming focus after the user moved elsewhere.
- Retain focused nodes while supported. When a source change removes a focused mode, use the current supported mode or an appropriate existing navigation target, never a detached node. Background refreshes must not reset scroll position.

### Architecture and dependencies

Keep vanilla TypeScript + Shoelace in Tauri v2. Manifest versions: Shoelace `^2.19.1` (installed/locked 2.20.1), TypeScript `~5.6.2`, Vite `^6.0.3`, Tauri API `~2.10`. Use the current lockfile and local asset paths; add no icon package, remote script, framework or dependency upgrade.

Documentation checked 2026-09-19: [Shoelace Button](https://shoelace.style/components/button) documents small sizing, prefix slots and CSS parts; [Shoelace Icon](https://shoelace.style/components/icon) covers names and decorative behavior; [WAI button pattern](https://www.w3.org/WAI/ARIA/apg/patterns/button/) covers Enter/Space and pressed-state semantics. Official Shoelace documentation currently identifies 2.20.1 and sunset status; migration to Web Awesome is separate work. Rust/audio API updates are irrelevant to this presentation scope.

### Testing requirements

Follow production-module Node patterns in `scripts/tests/destination-ui.test.mjs` and `scripts/tests/playback-ui.test.mjs`. A focused `scripts/tests/browse-mode-ui.test.mjs` may cover changed reconciliation; do not add tests that merely assert CSS strings.

- Full/limited provider sets, preserved order, source changes removing/adding modes, stable nodes and one listener per control.
- Existing navigation invoked once; same-mode/loading activation remain no-ops; current/disabled/pressed states agree and refresh preserves focus where possible.
- Grid/List preserves no-refetch behavior; Tracks retains dual-panel semantics; loading hides the toggle. Library → Playing → Library retains browse state and hides the bar correctly.
- All locale names, empty/loading capabilities, long labels, breadcrumbs, cache/scroll and mode-change selection resets.
- Browse during playback/Preview with a device basket; confirm no added playback, source-selection or basket mutation requests.

Commands from repository root:

```sh
rtk npm --prefix hifimule-ui run build
rtk node --test scripts/tests/destination-ui.test.mjs scripts/tests/playback-ui.test.mjs
# If a focused regression file is added:
rtk node --test scripts/tests/browse-mode-ui.test.mjs
# Only when catalog changes:
rtk cargo test -p hifimule-i18n
```

Rendered checks: column widths around 360/720/1000px plus actual divider extremes, EN/FR/ES/DE, broad/limited capabilities, selected/unselected/loading states, keyboard navigation, 200% text and short-window scrolling. Exercise actual supported themes without inventing another theme. Check normal-text contrast (4.5:1), applicable icon/state contrast and unclipped focus rings. Capture matched before/after bar/content dimensions. Smaller natural height at normal scale plus safe scroll at large text is acceptable; the prototype alone does not certify these constraints.

Record installed Windows/macOS/Linux separately with artifact/version/environment/outcome, or mark unverified for 15.17. No application build/tests or installed checks were run during story preparation.

### Previous story and git intelligence

Story 15.15 is done; daemon Back semantics and the two-row transport remain unchanged. Carry forward production-module tests, four-locale coverage, stable controls and accurate evidence limitations. Historical unit-test counts do not certify native UI behavior.

Recent commits inspected: `53da1a4` Review 15.15; `97d8545` Dev 15.15; `3efa452` Story 15.15; `f22793a` Correct course; `7188e18` Fix layout. The course correction defines scope; the layout fix protects playback-bar stacking and hoisted guidance placement. Avoid global CSS regressions. Outstanding 15.12 R16 and 15.14 R8/R9 remain release reconciliation for 15.17 unless directly affected here.

### Project Structure Notes

Primary updates belong in existing `library.ts` and `styles.css`; retain camelCase TypeScript and module boundaries. Optional catalog/Node regression updates use existing paths. The HTML comparison is a planning artifact, never a second application implementation.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 15, Stories 15.16–15.17]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md` — §5.1–5.2 browse/grid-list contracts; §6–7 accessibility; §7.2 compact navigation]
- [Source: `_bmad-output/planning-artifacts/prd.md` — FR8, P-NFR6, manual-playback release scope]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Frontend Architecture; Library Browsing RPC Contract; Playback Implementation Contracts; approved release split]
- [Source: `_bmad-output/planning-artifacts/project-context.md` — provider and managed-sync principles]
- [Source: `DESIGN.md` — tokens, typography, button states]
- [Source: `_bmad-output/implementation-artifacts/15-15-restart-the-current-track-or-return-to-the-previous-track.md` — Dev Agent Record and review]
- [Source: `hifimule-ui/src/library.ts`, `hifimule-ui/src/styles.css`, `hifimule-ui/src/main.ts`, `hifimule-ui/src/rpc.ts`, `hifimule-i18n/catalog.json` — current implementation]
- [Source: `hifimule-ui/package.json`, `hifimule-ui/package-lock.json`, `hifimule-ui/vite.config.ts` — dependency and asset conventions]

## Dev Agent Record

### Agent Model Used

Story preparation: Codex. Implementation agent: record during dev-story.

### Debug Log References

- Red phase: four initial reconciliation/accessibility/identity tests failed before implementation; all seven final tests pass.
- Verification run: `scripts/preview-browse-mode.mjs`; actual module, Shoelace, loader and CSS browser checks with isolated mock IPC.

- Planning/code/previous-story/git analysis and official documentation checked 2026-09-19.
- Isolated comparison: `15-16-browse-mode-comparison.html`; measurements and limitations above.

### Completion Notes List

- Implemented by Codex on 2026-09-19. User explicitly chose proposal B before application implementation: quiet controls with cyan underline/fill and reference-inspired local icons. This supersedes the prepared solid-primary presentation and initial icon mapping.
- Retained keyed mode and Grid/List controls, localized labels, native shadow-button selected/disabled semantics, loading focus, capability fallback, and existing navigation ownership. Empty capabilities and stale detached buttons cannot trigger unsupported navigation.
- Scoped compact styles retain 30px targets, wrap labels, preserve focus rings and bound scrolling at min(30vh, 20rem); 40vh was reduced after the large-text check.
- 159 Node tests passed, including seven focused production-module regressions. TypeScript/Vite build and diff whitespace checks passed. Real Shoelace rendering checked across 24 locale/width/text-size combinations; matched EN 720px bar shrank 85→73px and content grew 376→388px.
- Detailed evidence and reproduction: `15-16-browse-mode-verification.md`. Updated packaged native checks, live playback/device integration, OS text scaling and screen-reader listening remain explicitly unverified for 15.17. The existing installed macOS app was baseline-only; browser verification used production modules with mocked IPC.

- Ultimate context engine analysis completed - comprehensive developer guide created.
- Preparation readiness notes are superseded by the implementation and verification results above.
- Preparation checklist applied: design policy, code reuse, capability reconciliation, focus/accessibility, localization, short-window safeguard and truthful evidence boundaries included.

### File List

Implementation and verification files:
- `hifimule-ui/src/library.ts`
- `hifimule-ui/src/styles.css`
- `scripts/tests/browse-mode-ui.test.mjs`
- `scripts/preview-browse-mode.mjs`
- `_bmad-output/implementation-artifacts/15-16-browse-mode-design-options.html`
- `_bmad-output/implementation-artifacts/15-16-browse-mode-verification.md`
- `_bmad-output/implementation-artifacts/15-16-compact-the-library-browse-mode-bar-with-icons-and-smaller-labels.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`

Story preparation artifacts:
- `_bmad-output/implementation-artifacts/15-16-compact-the-library-browse-mode-bar-with-icons-and-smaller-labels.md`
- `_bmad-output/implementation-artifacts/15-16-browse-mode-comparison.html`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`

## Change Log

- 2026-09-19: Implemented user-selected design B, stable capability reconciliation and accessible selection/loading focus; added Node regressions and reproducible browser evidence. Status set to review; installed release checks explicitly recorded for 15.17.
