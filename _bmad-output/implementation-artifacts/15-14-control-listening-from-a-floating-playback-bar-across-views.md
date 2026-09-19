---
baseline_commit: e35b1365a4c6db9fde3f1cfce5905feb0d83e827
---
# Story 15.14: Control listening from a floating playback bar across views

Status: done

## Story

As a HifiMule user,
I want playback controls to remain visible while I browse music or work with a device basket,
so that I can manage listening without navigating away from what I am doing.

**Requirements:** Floating-controls portion of FR59; presentation of FR58 and FR62–63; P-NFR6; snapshot/interpolation portion of P-AR10; P-UX-DR2–3, P-UX-DR7, P-UX-DR10–14.

**Dependencies:** Stories 15.1–15.13. The transport, shared snapshot store, output chooser, seeking, Preview, destination navigation and manual queue already exist. This story consolidates their presentation; it does not create another session owner.

## Acceptance Criteria

1. **Persistent shared bar.** Given any library or destination view, when the user changes browsed server, Playback or physical-device basket, then a translucent floating playback bar remains visible below the media browser using the established design tokens. Navigation neither recreates the session nor changes the source addressed by its controls. The bar component and its retained control nodes survive ordinary navigation.
2. **Authoritative transport and position.** Given playing, paused or stopped listening, when the bar renders, then implemented transport, position and output controls expose accurate capabilities. Track identity and elapsed position come from daemon state; interpolation occurs only while playback advances. Current-track metadata also works after paused restoration or the first manual queue append, when active pipeline metadata may be absent.
3. **Useful idle state.** Given no active listening before Radio is implemented, when the bar renders, then it stays visible with an actionable manual-listening empty state. “Browse library” exposes existing browsing without changing the daemon destination, session or basket. Resume appears only for an authoritative current occurrence that can be resumed, with unavailable output/source conditions explained. No dead “Play something” button is displayed; Story 15.20 owns its eventual working Radio replacement.
4. **Preview semantics.** Given an active audition, then the bar identifies Preview and offers Return to session only when a preserved main session exists. Return dispatches the existing command to restore the approved prior playing/paused intent; Stop restores the main session paused. A preview without a main session cannot offer Return. Output-loss inhibition remains authoritative even when prior intent was playing.
5. **Honest status and recovery.** Given loading, buffering, lost daemon connection, unavailable output/source or recoverable failure, then the bar explains the actual state and relevant recovery action without claiming audio is playing. Unsupported controls have accessible availability explanations where needed. Loss of UI connection is identified as unknown current playback status, not an invented daemon pause. Reconnection/output reattachment never automatically resumes or retries a mutation. This status area remains the extension point for later quality-adaptation messages.
6. **Unobscured rows and focus.** Given a long library, nested Tracks/curation list, Playback queue/history or physical basket, when the user reaches the bottom or focuses a final-row action, then every row/action can be brought fully above any obstructing controls. The bar does not cover the basket or intercept pointer input outside its visible surface. Text and control contrast meet the existing accessibility target over the actual translucent background.
7. **Responsive layout and zoom.** Given narrow, medium and wide layouts, window/split-panel resizing or enlarged text, then essential transport and current error/recovery actions remain reachable without basket overlap or horizontal page scrolling. Secondary output controls use accessible, viewport-bounded overflow. Translated labels are readable and the bar reserves its actual wrapped height rather than a fixed assumed height.
8. **Keyboard and announcements.** Given focus elsewhere, when metadata, time or status changes, then background updates never move focus or announce every elapsed tick. Meaningful state/error transitions are announced appropriately; all controls have keyboard operation, localized accessible names and visible focus. Focused controls and open chooser state survive unrelated snapshot/metadata updates.
9. **Reconnect and stale interaction safety.** Given missed updates or UI reconnection, when an authoritative snapshot arrives, then track, cursor, status and capabilities correct without replaying commands. Older asynchronous responses cannot restore a superseded instance/session/occurrence or apply queued seek/output/transport work to it. A new daemon instance may validly restart its sequence counter.
10. **Verified UI behavior.** Given Windows, macOS and Linux UI builds, when navigation, Preview return, output failures, idle state, long-list bottoms, keyboard, text zoom and responsive layouts are checked, then the bar remains usable and consistent with the daemon. Record accessibility and visual observations for supported OS themes and translucent backgrounds. Mock/DOM tests alone do not certify these checks; unavailable installed-platform rows remain explicitly unverified.

## Tasks / Subtasks

- [ ] **T1 — Mount one persistent floating bar below the browser** (AC: 1, 6, 7)
  - [x] Move the existing `#playback-controls-container` after the mutually exclusive library/Playback content hosts in `renderMainLayout()`, retaining the build-once shell and existing `PlaybackControls` instance lifetime.
  - [x] Implement the inset glass treatment and reserved-space layout specified below. Preserve split-panel proportions, destination header, browser DOM, sidebar and each existing content scroll owner.
  - [ ] Verify nested Tracks and playlist-curation scrollers, virtualized final rows, queue/history paging actions and basket bottom actions against the actual bar bounds.
- [x] **T2 — Consolidate implemented controls and idle navigation** (AC: 2–5, 7)
  - [x] Extend `PlaybackControls` with the local Browse library callback; reuse `showLibrarySurface()` instead of introducing a Library destination or invoking a playback mutation.
  - [x] Preserve Pause/Resume, Stop, Next, Retry, Preview Return, native range seeking and output selection behavior. Add artist/source presentation and bounded identity-safe metadata fallback for restored main playback.
  - [x] Position the existing output chooser above the bottom bar, keep it hoisted and bounded, and preserve select/option identity, reset/refresh controls and accessible unavailable-output explanations.
  - [x] Add visible/accessibility strings through EN/FR/ES/DE catalog entries; retain implemented-feature gating and omit future Radio/Like/export/quality actions.
- [x] **T3 — Make shared snapshot freshness and asynchronous recovery explicit** (AC: 2, 5, 9)
  - [x] Extend the existing singleton store with presentation-only connection/freshness notifications and a single coordinated refresh path; preserve equal-snapshot heartbeats and 500 ms polling.
  - [x] Show connecting/disconnected/stale state, stop interpolation on loss of freshness, and offer read-only refresh/reconnect recovery. Fence overlapping poll/manual refresh and command-result publication.
  - [x] Clear scrub previews and queued interactions when instance, session, generation or occurrence changes. Refresh on conflicts without replay; suppress stale success/error paints after disposal or identity changes.
  - [x] Ensure seek/output responses update shared state through the same ordering rules rather than advancing only a private controls snapshot. Transport success requests a shared refresh because its current wrapper returns `void`.
- [ ] **T4 — Preserve focus, announce changes and support constrained layouts** (AC: 6–9)
  - [x] Retain connected DOM nodes, update changed text only, keep elapsed ticks out of live regions, and distinguish command failures from ongoing transport/output/connection status.
  - [x] Keep native range keyboard behavior, focus rings, overflow keyboard dismissal/focus return and disabled-control explanations. Never move focus merely because a timer or server-label response arrives.
  - [ ] Verify wrapping, short window behavior, 200% text/zoom, long translated names, forced-colors/reduced-motion preferences and high-contrast fallback without backdrop filtering.
- [x] **T5 — Add regression coverage and record visual evidence** (AC: 1–10)
  - [x] Extend production-component/store tests in `scripts/tests/playback-ui.test.mjs` and integration/placement coverage in `scripts/tests/destination-ui.test.mjs` using deferred promises and controlled clocks.
  - [x] Run the UI test suites, production build and whitespace check; record actual results in the Dev Agent Record.
  - [x] Add Story 15.14 scenarios/results to `docs/playback-installed-test-checklist.md`. Capture real layout/keyboard/contrast observations and identify unavailable platform checks honestly.


### Review Findings

Review of `e35b136..3b74774` on 2026-09-19. Blind Hunter, Edge Case Hunter and Acceptance Auditor completed. Triage: 0 decisions, 7 patches, 2 pre-existing deferrals, 3 dismissed candidates. All seven authorized patches were subsequently applied; the two pre-existing deferrals remain unchanged.

- [x] [Review][Patch] **R1 / P1 — Flush the outgoing device basket before opening Playing.** The new direct `destinationSelect({ kind: 'playback' })` bypasses `DestinationHub.select()` and its `basketStore.flushPendingSave()`. Switching during the one-second debounce clears the daemon's selected device before the pending manifest write; returning to the device can rehydrate the older basket and lose edits. Reuse the guarded destination-selection path and abort the switch if saving fails. [hifimule-ui/src/main.ts:553] **Resolution:** Playing now enters the shared DestinationHub selection path, reads the outgoing destination and awaits its basket flush before mutation; a failed flush aborts navigation.
- [x] [Review][Patch] **R2 / P2 — Keep final rows above persistent status guidance.** The absolute overlay has no measured scroll clearance or dismissal. In the real macOS in-app renderer at a 599px stage, French disconnected guidance occupied y=352.6–444 while the focused final row occupied y=354.8–390.8 at maximum scroll (2450px); the entire row was obscured. The issue also occurred at a 900px stage with 200% fixture text. Preserve the approved overlay design while ensuring sufficient clearance in each actual scroll owner, or an accessible way to dismiss the obstruction (AC6/7). [hifimule-ui/src/styles.css:2240] **Resolution:** An always-reachable, localized guidance toggle dismisses/reopens the overlay with Enter or Space; unchanged polling leaves it dismissed, while a new meaningful status is exposed. Bar height is unchanged by dismissal.
- [x] [Review][Patch] **R3 / P2 — Fence Playing navigation against newer destination choices.** `showPlaybackSurface()` awaits state, destination selection and hub refresh, then unconditionally displays Playing. A later physical-device choice can be overridden by the older request's completion. Coordinate navigation with destination selection, invalidate superseded requests and avoid late surface/focus changes. [hifimule-ui/src/main.ts:551] **Resolution:** Destination mutations are serialized and request-fenced. Superseded reads, save completions and mutation results cannot show or focus Playing; a later device choice wins. Main consumes accepted destination observations directly, removing the extra racing read.
- [x] [Review][Patch] **R4 / P2 — Report Playing navigation failures.** The surface callback discards `showPlaybackSurface()`'s promise, and the function has no rejection handling. A failed state read or destination selection leaves the sole Playing switch apparently inert and produces an unhandled rejection. Expose the localized selection failure and a usable retry without replaying a mutation automatically. [hifimule-ui/src/main.ts:551] **Resolution:** Selection failures are caught in the shared selector and shown in a visible localized alert, including without connected device chips. Recovery polling does not replay selection; another user activation retries.
- [x] [Review][Patch] **R5 / P2 — Focus a visible target when opening Playing.** `button:not([disabled])` matches the destination's hidden, enabled Retry button before queue controls; focusing it does nothing and prevents the fallback. The fallback container is not focusable either. Focus a stable, visible queue heading or another deliberately focusable destination target. [hifimule-ui/src/main.ts:535] **Resolution:** PlaybackDestination.focus() targets the retained, visible Upcoming heading, which already has tabindex=-1, independently of hidden recovery controls.
- [x] [Review][Patch] **R6 / P2 — Make the source icon hint keyboard accessible.** The source icon is `aria-hidden`, and its tooltip has no focusable trigger or alternate accessible source text. The source name is now hover-only, so keyboard users cannot identify a source that differs from the browsed server. Retain the requested icon presentation while providing keyboard access and an accessible label (AC8). [hifimule-ui/src/components/PlaybackControls.ts:95] **Resolution:** The source icon now has a focusable wrapper with a localized source name, visible focus styling and the existing Shoelace focus tooltip; the decorative icon stays hidden from accessibility APIs.
- [x] [Review][Patch] **R7 / P3 — Reconcile the implementation status narrative.** At review start, the story header and sprint tracking said `review`, but Completion Notes claimed status remained `in-progress` and explicitly denied readiness for review. Describe the actual review state while retaining the outstanding T1/T4 and installed-platform evidence limits. [_bmad-output/implementation-artifacts/15-14-control-listening-from-a-floating-playback-bar-across-views.md:7] **Resolution:** Story and sprint tracking now consistently say in-progress after review, specifically for outstanding full-application/platform verification and unresolved pre-existing issues; code-review patch completion is recorded separately.
- [x] [Review][Defer] **R8 / P2 — Cancel queued seeks after a conflict.** Two scrub commits followed by a conflict on the first dispatch the second mutation before authoritative refresh completes, using the stale observation and clearing the conflict message. Reproduced with controlled promises; the baseline already dispatches queued work after rejection. Clear queued work on conflict and recover without replay. [hifimule-ui/src/components/PlaybackControls.ts:461] — deferred, pre-existing
- [x] [Review][Defer] **R9 / P2 — Clear or scope command errors when playback identity changes.** A failure belonging to an old occurrence/session remains visible after an authoritative identity change because interaction invalidation never clears `commandError`. Baseline behavior also retained that error. Scope errors to their interaction identity or clear them when superseded. [hifimule-ui/src/components/PlaybackControls.ts:284] — deferred, pre-existing

Review validation: 80/80 focused playback/destination tests passed; production TypeScript/Vite build passed with existing chunk/import warnings; baseline diff whitespace check passed. Two temporary harness probes confirmed R8 and disproved the suspected false-command-error-on-refresh-failure issue (connection invalidation already suppresses it). Browser fixture evidence verifies the obstruction above, not native installed-platform acceptance. Dismissed candidates: false transport error after failed refresh (handled); metadata re-request on queue revision (valid revision fencing, bounded requests); height-cap overflow allegation (insufficiently established as a separate defect).

## Dev Notes

### Scope and architecture guardrails

- Keep Vanilla TypeScript, Shoelace, Tauri `rpc_proxy` and the existing daemon. No framework, browser audio player, independent bar poller, database migration, new playback endpoint or audio dependency is required.
- `PlaybackControls.ts` already implements the transport controller planned abstractly as `PlaybackBar.ts` in the architecture. Extend it in place; do not implement a parallel controller to match the older filename. `PlaybackDestination.ts` already owns bounded queue presentation; do not duplicate it into a new `PlaybackQueue.ts`.
- Session and audio ownership remain in the daemon. UI-local layout/connection freshness is presentation state, never a persisted replacement session. Playback source is `current.source.serverId` plus track/occurrence identity, independent of browsed server and selected device.
- Preserve opaque decimal-string revisions/sequences and compare using `BigInt`, not `Number`. Position/duration/seek are milliseconds. Queue revision is structural; ordinary progress must not be treated as a queue mutation.
- Keep credentials, authenticated stream URLs and provider-specific API calls daemon-side. Do not add artwork via a currently-browsed-server proxy unless source-aware routing is already verified; artwork is not required here.
- Do not implement selection configuration (15.15), Radio replenishment (15.16), final Play something (15.20), reporting/preferences/exports (15.21–25), or quality adaptation (15.26). Preserve their eventual integration points without placeholder controls.

### Implementation gate — decisions fixed for this story

**Placement and space reservation:** Use a stable final, nonshrinking flex child within `.library-view`, below both alternate content hosts and outside their scrollers. Give the bar an inset rounded translucent panel appearance and bottom spacing using existing spacing tokens. It stays pinned while the browser scrolls; its real content height participates in layout, automatically reserving space when controls/errors wrap. Do not place the queue absolutely, add a viewport-wide fixed layer over the basket, or calculate clearance from a guessed 52 px height. Keep content flex children at `min-height: 0`, with their current bounded scroll ownership. This closes the floating-placement requirement without reintroducing the Story 15.12 queue overlay defect. Use normal padding/scroll padding for focus breathing room, not a second scrollbar around the entire queue. Transparent outer spacing must not intercept content interactions.

**Responsive groups:** Preserve the current viewport families (`<600 px`, `600–1000 px`, `>1000 px`), but wrap against actual library-column width, including divider changes. Wide layouts place identity/status, timeline and primary transport together where they fit. Medium/narrow layouts stack identity/status, full-width timeline and wrapping actions. Pause/Resume, Stop, Next, active Preview Return and any relevant Retry/reconnect action stay directly reachable. Keep the speaker/output trigger in the action group; the existing output chooser is secondary overflow. Do not invent a general hidden transport menu when wrapping suffices. No clipped recovery copy or horizontally scrolling page. The current basket remains at least 300 px wide below 980 px; there is no existing narrow-screen collapse behavior to assume. Fit the bar to the remaining column and do not hide the basket to pass responsive checks. Any necessary narrow surface-switching fix must retain an explicit keyboard-accessible route to both browsing and basket actions. For short windows, constrain the chooser to available viewport height with internal scrolling; verify the main content retains reachable scrolling at supported window sizes.

**Output overflow:** Change bottom-edge dropdown preference to `top-end`, preserving `hoist` and automatic fit/flip behavior. Limit its width to the viewport and, where space allows, the library column; it must not permanently obscure basket actions. Support keyboard entry, native select operation, Escape dismissal and focus return to the trigger. Use real Shoelace/browser tests for this behavior; fake DOM cannot establish it.

**Status policy:** Keep elapsed/duration text and range updates outside live regions. A stable polite atomic status region announces only meaningful transport, Preview, connection and output changes, with unchanged text left untouched. A stable command-error alert announces a newly occurring action failure once; unrelated heartbeat/metadata success must not clear it. Give recovery guidance visible text; do not rely exclusively on a disabled element's tooltip. Routine metadata refresh does not refocus or recreate controls. A user-activated Browse library may deliberately focus the existing browse control via `showLibrarySurface()`.

**Freshness policy:** Initial load shows Connecting until an authoritative snapshot arrives. A failed latest refresh immediately marks the connection unavailable; a read that hangs beyond 2 seconds since the last successful session response marks the view stale. Before the first successful response, measure that deadline from the initial subscription/read start so Connecting cannot persist indefinitely. Two seconds is a UI freshness threshold, not an audio timeout or network SLA. Keep the last identity/cursor as last-known information, stop interpolation, and show that current playback status cannot be confirmed. Disable stale transport/seek/output mutations until a successful fresh session read; retain local Browse library and a read-only refresh action. Successful equal-sequence responses also restore freshness and run inventory heartbeats. Do not overwrite daemon transport state with a fabricated disconnected/pause state. No queued mutation is replayed on recovery.

**Ordering policy:** Coalesce shared reads or fence them with a request lifecycle token so an older in-flight response cannot undo a newer refresh, new daemon instance or remounted subscriber lifecycle. Within one instance/session, accept only a newer `stateSequence`; accept a new instance/session only from a valid current read/admission context. Command replies must still belong to the observed instance/session and relevant current occurrence/generation before publication. Do not compare sequence numbers across different instances. Retain a single scheduled poll while subscribers exist and no timer after the final unsubscribe. Connectivity notifications must not require equal snapshots to masquerade as state changes.

### Existing files: current behavior, intended changes and preservation

| File | Current behavior | Story change / preserve |
| --- | --- | --- |
| `hifimule-ui/src/main.ts` | Builds the split shell once; mounts `PlaybackControls` above library content; destination refresh toggles library/queue visibility. `showLibrarySurface()` locally returns to browsing and explicitly focuses it. | Move the persistent host below content, pass the existing local callback. Preserve build-once guard, destination selection, retained library DOM, teardown on true lifecycle/login exits and independent basket mounting. |
| `hifimule-ui/src/components/PlaybackControls.ts` | Retained controls; shared-store subscription; 100 ms repaint; output heartbeat/discovery; transport busy state; coalesced seek; Preview Return; portable source labels. Its idle state lacks a manual action and restore can lack a title. | Extend presentation and idle behavior, connection status, metadata fallback and shared-result publication. Preserve all existing control semantics, discovery throttling, focused options, scrub behavior and disposal guards. |
| `hifimule-ui/src/state/playback.ts` | One 500 ms subscriber-driven poller; rejects equal/older sequence within instance/session; equal snapshots still run output-inventory heartbeats. Failed reads are silently swallowed and concurrent refreshes lack cross-instance request fencing. | Add connection/freshness subscription and coordinated ordering as specified above. Preserve `current()`, snapshot subscribers, heartbeat behavior and compatibility with queue callers. |
| `hifimule-ui/src/styles.css` | Navy/cyan token system; split panel and hidden destination siblings; library/queue and nested view scroll owners; wrapping top transport with bottom-end output panel. | Add inset bottom bar, fit/wrap/overflow/focus styles. Preserve `[hidden]` overrides, split sizing, virtual row geometry and bounded queue scrolling; replace low-opacity status styling if measured contrast fails. |
| `hifimule-i18n/catalog.json` | Shared EN/FR/ES/DE strings for transport, errors, output, Preview and browsing. | Reuse existing keys; add complete four-locale keys for manual-listening guidance, connecting/stale/disconnected recovery and any new accessible labels. Preserve placeholders and parity. |
| `scripts/tests/playback-ui.test.mjs` | Executes real transpiled component/store with fake DOM, RPC and clock; tests seek/output/Preview/focus/sequence behavior. | Add meaningful behavioral regressions below, preserving old coverage; improve test doubles where connection/focus lifecycle requires it. |
| `scripts/tests/destination-ui.test.mjs` | Tests production destination/queue presentation and shell/CSS integration, keyed focus and async region recovery. | Update intentional host-position assertions; prove persistent controls and local Browse library navigation retain existing destination/queue behavior. |
| `docs/playback-installed-test-checklist.md` | Per-story automated evidence and explicitly incomplete installed-platform observations. | Append Story 15.14 layout/accessibility/connection checks and actual observations. Do not promote earlier unverified rows. |

**Read/reuse boundaries, not default rewrite targets:** `rpc.ts` provides all needed wrappers; `PlaybackDestination.ts` provides identity-safe current metadata resolution and bounded queue behavior; `serverIdentity.ts` formats portable source labels; `library.ts`, `TracksBrowseView.ts`, `PlaylistCurationView.ts` and `BasketSidebar.ts` own existing browsing/scroll/selection behavior. If a real integration defect requires editing these, read the whole file first and keep the fix local.

### Transport, metadata and asynchronous details

- Existing `playbackControl(action, observed)` sends `schemaVersion`, instance/session, a fresh command ID, expected generation and current occurrence. Actions are `pause`, `resume`, `stop`, `next`, `retry`, `returnToSession`. It returns `void`; refresh shared state on completion rather than optimistically inventing a cursor/status.
- `playbackSeek(positionMs, observed)` sends the same identity fences; the range previews locally on `input` and commits on `change`. Pending/failed seek targets never replace the actual elapsed cursor. Existing coalescing retains only the latest queued seek; extend invalidation to generation changes as well as instance/session/occurrence changes.
- `playbackSelectOutput` additionally fences `expectedOutputRevision`; output enumeration is per daemon instance. Selecting an output never starts audio. Unavailable/pending selection disables Resume while output controls remain usable on a fresh connection. Preserve explicit invalid-config replacement and inventory refresh even when session sequence is unchanged.
- Preserve interpolation's existing 100 ms paint, 750 ms maximum extrapolation and duration clamp. Advance only for fresh `playback.status === 'active'` with no pending seek. Reanchor immediately after authoritative pause, seek, track change and reconnect. Do not increase RPC polling to animate time or feed UI-extrapolated positions into reporting/persistence.
- Show active audition metadata during Preview, never canonical main metadata as the audition title. During main paused restoration/first append, reuse `playbackDescribeOccurrences` for the single authoritative current occurrence if pipeline metadata is absent. Verify session/revision/current identity after every await; discard obsolete success and failure responses. Cache only bounded current display data and retry on explicit refresh/meaningful identity changes, not every progress tick. If a loading preview has no metadata, show a localized loading/unknown-track label rather than querying a main-queue-only endpoint for the audition. Never display raw occurrence UUIDs as track names.
- Source labels always follow the occurrence's portable server ID. Late server-list recovery may update text without refocusing/rebuilding. A missing configured server gets a safe unavailable-source explanation, not the currently browsed server's name.
- Preserve separate connection, transport, seek, output and command-failure concerns. Output inventory success cannot erase a failed seek or transport command. Dispose subscriptions, paint timers and pending presentation callbacks on genuine destruction; moving between browse/queue views must not destroy the bar.

### Design and dependency requirements

`DESIGN.md` and implemented `styles.css` tokens supersede the older UX document's purple/Outfit palette: use Inter, `--ink`, `--ink-dim`, `--panel-bg`, `--surface-border`, `--radius`, spacing tokens and Signal Cyan `--accent`; amber denotes actual warnings. Use a sufficiently opaque navy backing plus structural blur, with opaque fallback when blur is unsupported or contrast preferences require it. Do not reduce essential status contrast with the current `.72` opacity without measuring its composited result. Preserve Shoelace focus treatment; use visible native-control focus styles and forced-color compatible borders.

Declared frontend versions are Shoelace `^2.19.1`, Tauri API `~2.10`, TypeScript `~5.6.2`, Vite `^6.0.3`; retain the repository lockfile resolutions. This is a UI consolidation story, with no package upgrade or audio-runtime/version change required. Revalidate current locked component behavior rather than migrating to a newer library for overflow.

Primary documentation checked on 2026-09-19:

- [Shoelace dropdown](https://shoelace.style/components/dropdown): existing placement/hoist facilities support a bottom bar's output chooser. Hoisting avoids ancestor clipping; it does not replace keyboard/viewport validation.
- [WAI slider pattern](https://www.w3.org/WAI/ARIA/apg/patterns/slider/): retain native range semantics, a localized name and readable elapsed/duration value text; verify arrow and Home/End operation in supported webviews.
- [W3C focus not obscured](https://www.w3.org/WAI/WCAG22/Understanding/focus-not-obscured-minimum.html): sticky surfaces can hide focus and translucent overlap can compromise contrast. This story's AC6 requires full row/action reachability, which is stronger than the cited minimum's partial-visibility threshold.

No new provider API, authentication scheme or latest-version migration is involved. The applicable research concerns current documented component/accessibility behavior; do not claim that this story audits every dependency for security updates.

### Previous-story intelligence and git baseline

Baseline at story creation: `a27f1bf` (2026-09-19), clean worktree. Last five commits: `a27f1bf` Review 15.13; `cc7b148` Dev 15.13; `a9e4a4e` Story 15.13; `36add0b` Review 15.12; `912af9f` Dev 15.12.

- Story 15.13 review fixed canonical-main metadata, per-region async fencing, connected keyed focus, independent error recovery and conflict refresh without mutation replay. Apply those lessons to the shared bar; a timer/metadata reply must not rebuild interactive DOM or erase an unrelated error.
- Story 15.12 introduced the singleton store and equal-snapshot heartbeat specifically to preserve output discovery. The two completed 15.12 follow-ups fixed hidden/flex destination layout and added local Back to library. Retain both contracts while moving the bar.
- The previous review's 68 passing UI tests and daemon/audio results are historical evidence only. They do not verify this story's layout, connection changes or accessibility.
- Tracking marks 15.12/15.13 done, but the inherited 15.12 **R16 physical-basket mutation guard acceptance gap** remains recorded in `deferred-work.md` and the installed checklist. This story introduces no basket mutation path and must not claim to resolve that gap. Existing incomplete installed/physical-output evidence also remains incomplete.
- Persistent `project-context.md` contains stale greenfield/status and historical links. Retain its provider and managed-sync principles; use current source, playback architecture amendments, approved epics and recent story evidence for implementation facts.

### Testing requirements

Use production component/store tests with controllable deferred responses and clocks; do not test only markup strings or copied implementation logic.

| Area | Required checks |
| --- | --- |
| Persistence/navigation | Same bar/control nodes and one store poller while switching server, Playback, Back to library and device; no session/source mutation from layout/navigation. Idle Browse works with no physical device and does not call `destination.select`. |
| Capabilities | Idle/current/restored/loading/paused/stopped/completed/error; no false Resume without current; output availability; Next gating; meaningful unsupported-seek explanation; Preview with/without main and Return versus Stop command routing. |
| Freshness/reconnect | Initial load failure and initial hung read with no prior snapshot; poll rejection; hung read past freshness threshold; equal-sequence recovery heartbeat; new daemon with lower sequence; overlapping explicit refresh/poll response from old instance; no mutation replay and no stale private snapshot overwrite. |
| Seek/races | No interpolation while stale/paused/loading/seeking; bounded active extrapolation; backward reanchor; old seek/output/metadata success and failure after track, generation, instance change or disposal; queued seek cancelled on identity change. |
| Metadata | Restored/first-append main without pipeline metadata; source differs from browsed server; Preview never displays main title as audition; late metadata/server-label results preserve focus and do not refetch on progress. |
| Accessibility | Connected focused node preserved over snapshot/label/time updates; no elapsed live announcements; meaningful error announced once; separate errors not erased by unrelated success; all new labels exist in four locales. |
| Lifecycle | Last unsubscribe cancels timer; re-subscribe during an in-flight read does not start duplicate poll loops; actual destruction cancels paints and prevents late DOM work. |

Required local commands from repository root:

```bash
rtk proxy node --test scripts/tests/playback-ui.test.mjs scripts/tests/destination-ui.test.mjs
rtk npm --prefix hifimule-ui run build
rtk git diff --check
```

No daemon tests are mandated for a UI-only implementation. If production daemon/RPC behavior is changed, justify it and run focused regression checks through the repository's controlled FFmpeg build environment.

**Real UI evidence matrix:** exercise library cards, virtual list, nested Tracks artist/album/track panes, playlist curation, Playback current/upcoming/history and a physical basket. Include final row keyboard focus and action activation; long source/title/translated recovery text; divider extremes; 200% zoom/text; Preview return; missing/reappearing output; lost/recovered daemon; open chooser resizing; and OS theme/high-contrast preferences. Use narrow 599 px, medium 800/1000 px and wide 1280 px CSS viewport probes, plus actual supported window minima/work-area fit. Existing sizing targets include 1280×860, comfort minimum 1040×720 and absolute minimum 900×640; smaller responsive probes do not redefine native minimum-window policy. Measure bar/content/basket rectangles and contrast in a real renderer; DOM mocks and CSS assertions cannot prove non-overlap. Record OS, architecture, source revision/package identity, viewport/zoom, screen reader when used, observations and unavailable cases. Do not assert all-platform acceptance from macOS-only tests.

### Project Structure Notes

Implementation belongs in the existing UI component/store/style/test boundaries above. Preserve the split-panel library/basket hierarchy, scope hidden/visibility styles to appropriate hosts, and keep no-device listening independent of physical-device sync locking. No new architecture scaffolding, provider cache, playlist exporter or unbounded metadata cache is part of this story.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 15; Story 15.14; Stories 15.15–15.29 boundaries]
- [Source: `_bmad-output/planning-artifacts/prd.md` — playback FR58–59, FR62–63 and P-NFR6]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Playback UI and Session Control; Implementation Contracts; Project Structure; Validation Refinements]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md` — component, responsive and accessibility guidance; reconcile visual tokens with `DESIGN.md`]
- [Source: `_bmad-output/planning-artifacts/project-context.md` — provider abstraction and managed-sync principles]
- [Source: `_bmad-output/implementation-artifacts/epic-15-context.md` — delivery order and cross-story constraints]
- [Source: `_bmad-output/implementation-artifacts/15-13-edit-the-upcoming-listening-queue.md` — review findings, current metadata, focus and async lessons]
- [Source: `_bmad-output/implementation-artifacts/15-12-access-playback-as-an-always-available-destination.md` — shared store, navigation and deferred R16]
- [Source: `_bmad-output/implementation-artifacts/spec-15-12-fix-playback-destination-layout.md`; `spec-15-12-return-to-library-from-playback.md` — completed layout/navigation contracts]
- [Source: `docs/api-contracts-hifimule-daemon.md` — Playback seek; typed destinations; manual listening queue contracts]
- [Source: `hifimule-ui/src/components/PlaybackControls.ts`; `state/playback.ts`; `rpc.ts`; `main.ts`; `styles.css`; `DESIGN.md` — current implementation and design authority]
- [Source: `docs/playback-installed-test-checklist.md`; `_bmad-output/implementation-artifacts/deferred-work.md` — evidence limits and inherited acceptance gap]

## Dev Agent Record

### Agent Model Used

GPT-6 (Codex), story-context preparation.

### Debug Log References

- 2026-09-19 implementation: user selected design B (two rows, icon controls with hover/focus hints).
- Red/green checks covered final host placement, icon/idle navigation, restored metadata and shared freshness/ordering. Final UI regression: 80 passed, 0 failed. Production TypeScript/Vite build and whitespace checks passed; existing chunk/import warnings remain.
- Real Codex in-app-browser checks ran on macOS ARM64 using `hifimule-ui/tests/playback-bar.html`. Detailed dimensions, keyboard observations, conservative contrast calculations and explicit evidence gaps are in `docs/playback-installed-test-checklist.md`.
- Browser findings fixed: Shoelace inner-button accessible labels, output Escape focus return, measurable tooltip trigger, and backdrop-filter containment clipping the hoisted popup.
- User-feedback refinement consolidated navigation further: the persistent bar now owns a two-state Library/Playing icon switch; the top Playback chip, duplicate Playing/Back heading, and current-occurrence block were removed. Status guidance overlays the content without changing bar height, the configured server icon precedes the title, and top-end hints remain visible beside the basket.
- Refinement verification: 80/80 focused playback/destination tests and 141/141 repository JavaScript tests passed; the production TypeScript/Vite build and whitespace check passed. A real French renderer check confirmed the 305×131 px bar height stayed constant when disconnected guidance appeared as an overlay.

- Creation analyzed approved planning artifacts, current frontend and RPC contracts, previous story/review history and official component/accessibility documentation. No production implementation or runtime test execution occurred during story creation.

### Completion Notes List

- 2026-09-19 review patches: R1–R7 applied. Shared destination navigation preserves pending basket writes, serializes competing choices and reports failures without replay. Playing entry focuses the Upcoming heading. Guidance can be hidden/reopened by keyboard without changing bar height, and source identity is keyboard accessible. Eight added behavioral regressions bring the focused suite to 88 passing tests; the full repository JavaScript suite, production TypeScript/Vite build and whitespace check pass. Real macOS browser checks covered French narrow layout, guidance dismissal/reopening, retained final-row focus, 200% fixture text and source tooltip keyboard access. R8/R9 and unavailable installed-platform checks remain unresolved.

- Implemented B in the existing retained controller: bottom inset panel, full-width second-row timeline, icon-only visual actions, localized hover/focus hints and accessible text. The bar's Library/Playing icon is the sole Playback surface navigation; returning to Library remains local, while opening Playing selects the existing Playback destination when required.
- Added bounded restored-current metadata with source/identity fencing; Preview never queries main-only metadata. Added artist plus the configured server icon before the title, with the server label retained as a hint. Idle, Preview, output, seek, error and connection guidance appears in a non-layout-shifting overlay.
- Shared store owns 500 ms polling, 2-second freshness, explicit refresh request fencing, equal-sequence heartbeats and guarded seek/output publication. Identity or freshness changes cancel scrub/queued work and release obsolete busy states. Mutations never replay after recovery.
- Stable live regions update only changed text; elapsed ticks remain outside them. Routine status is visually compact while exceptional guidance overlays the content above the bar. Output chooser opens above the bar and is bounded by the column and viewport.
- T1/T4 real-application validation remains incomplete: production nested/virtualized lists and physical basket actions, installed OS preferences/screen readers, and native 200% zoom were not exercised. Browser fixture results are not promoted to those checks. The code review and all seven authorized patches are complete. Status is `in-progress` for remaining real-application/platform validation and unresolved pre-existing issues; full acceptance is not claimed.

- Ultimate context engine analysis completed - comprehensive developer guide created.
- Layout, space reservation, status/announcement, overflow and reconnect contracts are specified above; no product clarification is required to begin implementation.
- At story creation, status was `ready-for-dev`; see the implementation notes above for current progress and remaining validation.

### File List

- `_bmad-output/implementation-artifacts/15-14-control-listening-from-a-floating-playback-bar-across-views.md` — baseline, progress and evidence.
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — in-progress tracking.
- `hifimule-ui/src/main.ts` — persistent bottom host, shared surface title and Library/Playing switching.
- `hifimule-ui/src/components/DestinationHub.ts` — physical-device destination strip with Playback surface navigation delegated to the bar.
- `hifimule-ui/src/components/PlaybackDestination.ts` — queue/history-only surface without duplicate heading or current occurrence.
- `hifimule-ui/src/components/PlaybackControls.ts` — icon controls, metadata, freshness, race fencing and retained accessibility.
- `hifimule-ui/src/state/playback.ts` — coordinated reads, connection subscription and command publication.
- `hifimule-ui/src/styles.css` — two-row inset panel, overflow and contrast fallback.
- `hifimule-i18n/catalog.json` — complete EN/FR/ES/DE labels and guidance.
- `scripts/tests/playback-ui.test.mjs` — component, metadata, interaction and announcement regressions.
- `scripts/tests/destination-ui.test.mjs` — shared-store ordering/lifecycle and placement regressions.
- `hifimule-ui/tests/playback-bar.html` — reproducible real-renderer fixture using production controls and Shoelace.
- `docs/playback-installed-test-checklist.md` — actual browser evidence and explicit unverified installed rows.

## Change Log

- 2026-09-19: Implemented user-selected design B and Story 15.14 UI/state changes; 80 UI tests, production build and whitespace checks pass. Retained in-progress status for outstanding real-application validation.
- 2026-09-19: Applied visual feedback: integrated the Library/Playing switch into the bar, overlaid status messages, replaced source text with the server icon, and removed redundant Playback headings/current occurrence. Focused tests remain 80/80; all repository JavaScript tests pass 141/141.
- 2026-09-19: Hid the retained server selector on Playing and restored it on Library, freeing header space without recreating selector state.
- 2026-09-19: Replaced upcoming-list Move up, Move down and Remove text actions with compact icons, retaining localized accessible names and hoisted top tooltips.

- 2026-09-19: Applied all seven code-review patches and synchronized story/sprint status to `in-progress` for remaining validation and pre-existing issues. Focused regressions (88), repository JavaScript tests, production build and whitespace check pass.
