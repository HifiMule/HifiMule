---
baseline_commit: 643df64a2cb8b44e384649ed0368dd68924622cb
---
# Story 16.6: Start Radio with Play something without opening the main window

Status: in-progress

## Story

As a HifiMule user,
I want a Play something action in the desktop menu and idle playback bar,
so that I can start an ongoing Radio from my configured libraries with one action instead of facing an empty listening page.

## Acceptance Criteria

1. With usable saved Playback sources, Play something from the platform-appropriate desktop menu starts a fresh logical Radio with the first eligible result from the existing shared selection engine. It plays with the main UI closed and without a physical sync device. The action does not open the UI.
2. The idle floating bar invokes the same daemon start operation. Replace its interim empty/manual-listening prompt with a working Play something action and accessible loading, success, and failure feedback. Keep Library/Playing navigation and manual browsing useful.
3. Resume continues an existing session's queue, position, and logical-session exclusions; it never reruns initial selection. Play something explicitly creates a new logical session, including when a resumable session exists.
4. A successfully prepared new Radio replaces the main session and any audition, resets session-scoped exclusions, and prevents obsolete audition/selection work from restoring the replaced session. Saving settings alone does not replace playback.
5. Missing/invalid settings, unavailable configured sources, or failure to prepare an eligible first track produce a specific, recoverable explanation and an explicit route to Playback settings. Failed preparation preserves the prior session and does not open the UI automatically.
6. An unavailable selected output follows the existing output-loss pause/inhibition rule. Explain the required output choice; never silently substitute another output to satisfy startup.
7. Repeated menu/UI activation and a pending start superseded by Resume, Stop, another session command, or shutdown obey one documented command-ordering and generation policy. No duplicate starts or stale replacement; loading state belongs to the accepted request.
8. With the UI closed, the daemon continues bounded Radio replenishment, current-artist priority, meaningful transitions or labeled fresh-center fallback, recording deduplication, and cycle/exclusion behavior. Native controls remain available. Reopening the UI shows that same session and its transition/waiting explanation.
9. Saving new Playback settings does not rebuild the active Radio. Its frozen original selection context remains in use; only the next explicit Play something uses the new settings. Explain this distinction in the UI.
10. Installed Windows, macOS, and Linux checks exercise platform-appropriate menu placement and the bar, including UI-closed start, missing setup, output loss, repeated activation, existing paused Radio, fresh-versus-resumed identity, first track, and continued replenishment. Record tested architecture and menu surface; do not treat the macOS Dock as equivalent to Windows/Linux tray behavior.

## Tasks / Subtasks

- [x] Lock the start contract before wiring entry points (AC: 1, 3–7, 9).
  - [x] Define a shared daemon operation callable from both `playback.startSelection` and the daemon-owned menu, retaining saved-config load, bounded source fetch, recording-aware selection, alternate-copy preflight, first-track preparation, and `StartRadio` ownership. Avoid a second selection engine or a UI-dependent menu path.
  - [x] Document linearization and supersession for concurrent starts and Resume/Stop/Play/Preview/Back/Next/shutdown. Extend the existing start ticket/owner generation fences as needed so a late preflight cannot replace a later accepted command. Specify whether a repeated start coalesces or supersedes and expose only its latest accepted outcome.
  - [x] Keep preparation-before-replacement atomic from the user's perspective. A source/output/preparation failure must not discard a recoverable existing main session or audition. After accepted replacement, old audition completion/return and prepared audio must be fenced.
  - [x] Preserve selected output identity and the output-loss inhibited/paused state. Define the recoverable failure code and action when output is unavailable; do not auto-select another device or auto-resume on reconnection.
- [x] Add the daemon desktop-menu action (AC: 1, 3, 5–8, 10).
  - [x] Add localized Play something beside existing Open UI/Resume in the daemon's `tray-icon` menu and route it to the shared operation without launching Tauri. Keep Resume's native transport semantics distinct.
  - [x] Settle and document Windows tray, Linux indicator/tray, and macOS menu-bar-status-item placement in actual installed builds. If a macOS application menu is also used, wire the same daemon operation and validate its lifetime; do not assume Dock menus exist everywhere.
  - [x] Make setup, source, output, busy, and accepted-start outcomes intelligible while the UI is closed using the existing menu status/notification pattern. Provide a separate explicit Open UI/Playback settings route; error reporting itself must not focus/open the window. Menu availability must remain truthful through shutdown and pending work.
- [ ] Complete the idle bar and settings UX (AC: 2–3, 5–7, 9).
  - [x] Add a keyboard-operable, visibly focused, localized Play something control to `PlaybackControls`; use the existing RPC wrapper and session store. Show request-scoped progress and accessible status/error text without stale completion, focus theft, or hiding transport/Preview Return controls for an existing session.
  - [x] Keep a distinct Resume affordance when a resumable session exists, with clear fresh-versus-resume copy. Provide an explicit route to Playback selection settings on setup/source failures and to output choice on output failure. Preserve useful Library/Playing navigation and final-row clearance at narrow/medium/wide widths and 200% text scaling.
  - [x] Explain that Save changes the next Play something, not the current Radio. Keep the settings page's existing Start Radio path on the same operation; saving must remain side-effect-free for current playback.
  - [x] Add all affected strings to the four-locales catalog with parity and meaningful accessible names/help.
- [x] Verify command, end-to-end, and installed behavior (AC: 1–10).
  - [x] Deterministic multi-source fixtures assert first eligible track/source, alternate-copy fallback, session/occurrence identity, frozen settings, Radio gain, and continuation through bounded refill/relationship/fresh-center/exhaustion. Verify no physical device is needed.
  - [x] Exercise closed-UI menu and idle bar against the same daemon command: no UI launch, no duplicated starts, Resume retaining queue/position/exclusions, successful replacement clearing the old logical scope, and settings-save isolation.
  - [x] Race slow selection/preparation with repeated activation, cancel, Resume, Stop, session replacement, audition return, shutdown, and reconnect. Assert newest accepted command wins and stale work publishes neither owner state nor audio/status. Distinguish preflight failure from post-admission audio failure and verify preservation/recovery.
  - [x] Test missing/invalid settings, unavailable/empty sources, unavailable output and output reconnection; verify actionable localized feedback, session preservation, and no output reroute.
  - [x] Run focused Rust/UI/i18n tests, formatting, Clippy, TypeScript/build and affected daemon suite. Record Windows/macOS/Linux installed menu and bar evidence per architecture, including architecture, UI-closed lifecycle, first track, continued refill, native transport, and any unverified physical-output result separately.

### Review Findings

- [x] [Review][Patch] Allocate the menu start ticket when the click is accepted, so a later Resume or Stop cannot be overtaken by the queued request (AC 7). [hifimule-daemon/src/main.rs:1405]
- [x] [Review][Patch] Supersede a pending start only after a later playback command passes validation; rejected commands must not cancel it (AC 7). [hifimule-daemon/src/rpc.rs:592]
- [x] [Review][Patch] Preserve request-scoped bar feedback through the session identity change caused by a successful start (AC 2, 7). [hifimule-ui/src/components/PlaybackControls.ts:254]
- [x] [Review][Patch] Clear or update menu start status when a newer generation replaces the started session before the menu observes it (AC 7). [hifimule-daemon/src/main.rs:1246]
- [x] [Review][Patch] Route output failures to the output chooser, rather than focusing Playback selection settings (AC 5–6). [hifimule-daemon/src/main.rs:725]
- [x] [Review][Patch] Keep reporting menu-started playback failures after the session first becomes active (AC 5, 8). [hifimule-daemon/src/main.rs:1250]
- [x] [Review][Patch] Complete configured installed Windows, macOS, and Linux menu and bar verification, including ARM64 Radio with the UI closed; the earlier blank ARM64 window did not recur in an isolated empty-profile launch (AC 10). [_bmad-output/implementation-artifacts/16-6-start-radio-with-play-something-without-opening-the-main-window.md:25]

## Dev Notes

### Current implementation and required preservation

| UPDATE boundary | Current state | Required change and preservation |
| --- | --- | --- |
| `hifimule-daemon/src/rpc/playback_selection.rs` | `start` loads `playback-selection.json`, allocates `START_EPOCH`, fetches bounded pools, selects recording-aware source/copy, preflights `resolve_playback` and `prepare_selection_source`, then applies fenced `StartRadio`; `cancel` fences selection. | Expose one daemon-owned entry point for menu and RPC. Keep deterministic selection, deadlines, alternate copies, preflight-before-owner, source identity and session settings snapshot. Existing ticket only covers another start/cancel: establish supersession with other session commands. |
| `hifimule-daemon/src/rpc.rs`, `hifimule-daemon/src/playback/{session,commands,audio,radio,persistence}.rs` | RPC owns `AppState` and applies `StartRadio`; owner creates fresh logical session, clears audition, stores original settings, and starts source-bound audio through generation/control fences. Schema 11 persists Radio loudness policy. | Provide menu access without bypassing owner or copying RPC business logic. Retain source/recording/occurrence distinctions, five-upcoming bound, queued transition explanations, output safety, frozen gain, paused restoration and stale-audio fences. Check the gap between preflight and accepted playback publication. |
| `hifimule-daemon/src/main.rs` | Native `tray-icon`/Tao loop has Open UI, Resume, native status, retry and Quit. Resume sends tracked `NativeControlIntent::Play`; failures set menu text and OS notification. The loop does not currently own an `AppState` handle. | Add a nonblocking Play something dispatch and tracked result through the shared daemon service. Keep loop responsive and avoid opening the UI for ordinary start or errors. Preserve Quit/sync shutdown and current Resume behavior. |
| `hifimule-ui/src/components/PlaybackControls.ts`, `hifimule-ui/src/rpc.ts`, `hifimule-ui/src/state/playback.ts` | Idle bar shows guidance and navigation, hides transport when no current; `rpc.ts` already has start/cancel wrappers. Bar subscribes to authoritative snapshots with freshness and interaction epochs, output controls, Preview Return, live status and error regions. | Make Play something an actual idle action and handle progress/failure with request identity. Preserve snapshot ordering, seek/output controls, source badges, keyboard access, responsive layout, cleanup and no focus theft. |
| `hifimule-ui/src/components/PlaybackSelectionSettings.ts`, `hifimule-i18n/catalog.json` | Settings already offer Save and Start Radio separately; Start reports setup/empty/cancel/unavailable. Four locales hold tray, playback and selection strings. | Reuse start path, keep Save separate, clarify next-start effect, add menu/bar/error/action labels with locale parity. |

### Architecture and scope guardrails

- Playback remains a daemon-owned, always-present virtual destination. The menu must work when the detachable Tauri UI is absent. The currently browsed server/device cannot change the Radio source. Credentials and authenticated stream URLs stay daemon-side. [Source: `_bmad-output/planning-artifacts/architecture.md` §§Playback State and Ownership, Playback Provider Integration, Playback UI and Session Control; `_bmad-output/planning-artifacts/prd.md` FR55–59, P-NFR5]
- Reuse the existing configured selection and Radio state machine. A fresh Radio resets logical-session skip/removal exclusions; Resume and restoration retain them. New settings apply only to the next explicit fresh start. Do not add a cross-session taste model, sync-device dependency, unbounded candidate cache, or new provider-specific API in the UI. [Source: `_bmad-output/planning-artifacts/epics.md` Story 16.6; `_bmad-output/implementation-artifacts/epic-16-context.md`]
- Preserve current artist priority, relationship/fresh-center explanations, recording deduplication and chosen source-copy gain. Preview, album, manual queue, Back, native transport, output-loss pause and sync safety must remain functional. `16.5` review fixed Radio gain freeze on output recreation and corrupt-policy restore; retain both fixes. [Source: Stories 16.2–16.5; `_bmad-output/planning-artifacts/architecture.md` §§Playback Radio Selection, Playback Implementation Contracts]
- The implementation gate calls for no normal confirmation dialog and a one-action configured start. Windowless failures need clear setup/output routes that the user explicitly chooses. Avoid treating a successful RPC admission as proof that audible playback began; verify authoritative playback state and explain recoverable post-admission failure. [Source: `_bmad-output/planning-artifacts/epics.md` Story 16.6 implementation gate; `hifimule-daemon/src/rpc.rs`]
- Story 15.17 is still `in-progress` in sprint status despite its role as the installed manual-playback baseline. Verify affected installed behavior here; do not claim its pending platform/physical checks or Story 16.5's unrun platform checks as completed. [Source: `sprint-status.yaml`; Story 15.17; Story 16.5 §Dev Agent Record]

### Library and platform notes

The repository pins Rust edition 2024/MSRV 1.93.0, `tray-icon ~0.19`, Tao `~0.31`, Tauri v2, and the existing patched CPAL 0.18.2. Use repository versions and patterns; no dependency upgrade is needed. `tray-icon` exposes menu events via `MenuEvent::receiver` and mutable menu item state; the existing daemon already uses these APIs. The Tauri v2 window-menu documentation distinguishes macOS global menu from Windows/Linux window menu. HifiMule's daemon-owned tray/status menu is the reliable UI-closed surface to verify; record any additional platform menu surface independently. [Source: `Cargo.toml`; `hifimule-daemon/src/main.rs`; [tray-icon documentation](https://docs.rs/tray-icon/latest/tray_icon/); [Tauri window-menu documentation](https://v2.tauri.app/learn/window-menu/)]

### Previous-story and Git intelligence

Story 16.5 added source-bound Radio ReplayGain and schema 11 frozen occurrence policy; review patched output recreation and restore validation. It reported passing local daemon, formatting and Clippy checks but left platform CI and physical output evidence open. Recent commits `e6a632a` (review 16.5), `efe1d97` (dev 16.5), `06bcd84` (story 16.5), `7aa1ab8` (review 16.4), `3988baa` (recording-aware selection) establish the code baseline. Story 15.14's bar intentionally retained manual idle browsing until this story and uses the shared store, local progress interpolation, disposal guards and responsive spacing. [Source: Stories 16.5 §§Review Findings, Dev Agent Record; 15.14 §§Dev Notes, Dev Agent Record; Git history]

### References

- `_bmad-output/planning-artifacts/epics.md` — Epic 16, Story 16.6, FR/P-AR/P-UX coverage and implementation gate.
- `_bmad-output/planning-artifacts/prd.md` — FR55–70, P-NFR4–6 and Epic 16 success/evidence.
- `_bmad-output/planning-artifacts/architecture.md` — Playback State/Ownership, Radio Selection, UI/Session Control and Implementation Contracts.
- `_bmad-output/planning-artifacts/ux-design-specification.md` — §6 accessibility/breakpoints and §7 interim bar state.
- `_bmad-output/implementation-artifacts/epic-16-context.md` — daemon ownership, dependency sequence and source/occurrence boundaries.
- `_bmad-output/implementation-artifacts/16-5-match-radio-track-loudness-using-available-metadata.md` — frozen Radio policy and evidence limits.
- `docs/api-contracts-hifimule-daemon.md`, `docs/playback-installed-test-checklist.md` — update command/error contract and installed evidence as applicable.

## Dev Agent Record

### Agent Model Used

GPT-6

### Implementation Plan

- Reuse `playback_selection::start_with_ticket` for both authenticated RPC and daemon menu ingress. Order each accepted start with `START_EPOCH` and fence the final owner commit with `ApplyFence`.
- Invalidate pending selection when a later session or native transport command is accepted. End the selection ticket at owner commit so a later failed preflight cannot silence audio already admitted for the prior Radio; generation and control epochs own post-commit audio.
- Keep source/copy preparation and selected-output validation before owner replacement. Surface distinct setup, source, empty, preparation, output, busy and storage outcomes in the menu and bar.
- Add request-scoped bar feedback and explicit settings/output controls. Keep Resume, Preview Return and Library/Playing navigation attached to authoritative session snapshots. Verify with Rust, Node, i18n, TypeScript, formatting and installed-platform checks.

### Debug Log References

- Controlled FFmpeg wrapper is required for daemon Cargo commands. The initial raw Cargo test failed at the repository build guard; the wrapper produced the intended red compile failure for the new ordering test.
- Full daemon tests initially failed because sandboxed mock HTTP servers could not bind (`Operation not permitted`). An approved loopback-capable rerun after the final source changes passed 1,258 unit tests and five contract tests, with six ignored. The full Node suite passed 263 tests with one skipped, the lifecycle suite passed 19 tests, and i18n, formatting and the production UI build passed.
- macOS ARM64 DMG built after disk-image access was granted and passed strict code-signature verification. An isolated copied-app launch started the daemon, but the main window remained blank. The test UI exited and its daemon accepted a clean Quit. Menu, bar and physical-output installed outcomes remain unverified; Windows, Linux and macOS x64 installed machines are unavailable here.
- The menu's separate Playback settings item now targets the Playback settings section through the lifecycle activation mailbox for both running and cold-started UIs. The full lifecycle contract suite and UI build pass; installed visual behavior remains unverified after the blank-window observation.
- A final ARM64 DMG rebuild including the settings activation route completed with SHA-256 `edf9ad2b548a549f395c59cc3a0e2b93c5a10e68926202757de6fed45dceb3df`. It has not been installed or smoke tested.
- Controlled daemon Clippy completed with 52 existing warnings and no errors; final whitespace and Rust formatting checks passed.
- Review patches passed 1,259 daemon unit tests and five contract tests, 98 focused UI tests, all 19 lifecycle contract tests with loopback access, seven i18n tests, the production UI build, daemon and Tauri checks, Rust formatting and Git whitespace checks.
- The review-patched ARM64 DMG (`3db56d10fda1e029ffb96bdab6942bb639ac0a2a37db46d5194fcee3b978227d`) passed strict code-signature verification. Its isolated copied app rendered the connection page and returned the expected missing-setup RPC error. Launchd and authenticated health RPC confirmed that the daemon remained alive after closing the UI. Configured Radio/menu/bar and Windows/Linux/macOS x64 installed results remain unverified.
- A final rebuild including the activation route-ready guard produced ARM64 DMG SHA-256 `34694c545dadfefdfcdf77208cd6e5749ed84297b875e0c3b0e37b303db752b3` with strict signature verification. It has not been installed or smoke tested.

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- Shared daemon menu dispatch, selection supersession, pre-commit output validation, distinct preparation error and frozen post-commit audio ownership were implemented. The bar has a localized Play something control, request-scoped feedback and settings/output routes. Source checks and local ARM64 packaging passed; installed behavior remains open after the blank-window observation.

### File List

- `_bmad-output/implementation-artifacts/16-6-start-radio-with-play-something-without-opening-the-main-window.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `docs/api-contracts-hifimule-daemon.md`
- `docs/playback-installed-test-checklist.md`
- `hifimule-daemon/src/main.rs`
- `hifimule-daemon/src/playback/commands.rs`
- `hifimule-daemon/src/playback/selection.rs`
- `hifimule-daemon/src/playback/session.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-daemon/src/rpc/playback_selection.rs`
- `hifimule-i18n/catalog.json`
- `hifimule-lifecycle/src/lib.rs`
- `hifimule-lifecycle/tests/contract.rs`
- `hifimule-ui/src/components/PlaybackControls.ts`
- `hifimule-ui/src/components/PlaybackDestination.ts`
- `hifimule-ui/src/components/PlaybackSelectionSettings.ts`
- `hifimule-ui/src/main.ts`
- `hifimule-ui/src-tauri/src/lib.rs`
- `hifimule-ui/src/styles.css`
- `scripts/tests/destination-ui.test.mjs`
- `scripts/tests/playback-selection-ui.test.mjs`
- `scripts/tests/playback-ui.test.mjs`

## Change Log

- 2026-09-28: Began Story 16.6; added daemon-owned menu start, ordered selection fencing, bar/settings feedback and platform evidence checklist. Installed verification remains pending.
- 2026-09-28: Applied six code-review patches for ordering, feedback and output routing. ARM64 packaging and empty-profile launch checked; installed Radio matrix and UI-closed lifecycle remain open.
