---
title: 'Start radio from library artists and playlists'
type: 'feature'
created: '2026-09-30'
status: 'done'
baseline_commit: '7a4be8f81726bd8d19e0bcb3bbab9b799a47c7bf'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The playback bar uses a text-heavy “Play something” button, and library users cannot start radio directly from an artist or playlist. Choosing a new starting point currently requires manually editing the radio sources.

**Approach:** Replace the playback bar's text button with a broadcast icon and add matching radio actions to eligible artist and playlist items in both card and list layouts. A library radio action saves the selected item as the only source, then starts radio through the existing selection flow.

## Boundaries & Constraints

**Always:** Replace the complete saved sources array with one source containing the item's portable server identity, source kind, and original item ID. Preserve schema version, ordering, seed, and maxTracks. Save successfully before requesting radio startup. Capture artist source identity when mapping the item so later server changes cannot retarget it. Keep accessible translated labels and tooltips on icon controls. Prevent action clicks from opening the item or selecting it. Retain existing direct-play and basket actions.

**Ask First:** Expanding radio eligibility beyond music artists and music playlists, or changing daemon selection semantics.

**Never:** Add radio actions to book authors, book collections, albums, tracks, or podcasts. Append the item to existing sources. Reset unrelated radio preferences. Change tray “Play something” text or behavior as part of this UI improvement.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Artist radio | Music artist with captured server identity | Save exactly one artist source, then start selection | Surface preparation or startup failure using existing translated guidance or toast conventions |
| Playlist radio | Music playlist with server identity | Save exactly one playlist source, then start selection | Same failure reporting as artist radio |
| Existing settings | Multiple sources and customized ordering, seed, cap | Replace all sources; retain every other config value | Configuration read failure prevents saving and starting |
| Save failure | Replacement config cannot be persisted | No start request; action becomes available for retry | Explain failure; do not report success |
| Start failure | Save succeeds but selection is empty, unavailable, busy, or output missing | Replacement source remains saved; no false success state | Surface the existing failure reason and permit retry |
| Repeated activation | Start is pending | Suppress overlapping library source replacement/start operations | Restore availability after settlement |
| Ineligible item | Missing identity, book author/collection, or known empty playlist | No actionable radio control | No configuration mutation |
| Server switch | Item belongs to a previously selected server | Action targets the captured server and original ID | Unavailable server is reported rather than silently replaced |

</frozen-after-approval>

## Code Map

- `hifimule-ui/src/components/PlaybackControls.ts` — playback bar button, tooltip helpers, existing radio startup and guidance.
- `hifimule-ui/src/components/MediaCard.ts` — card action rendering and BrowseDisplayItem shape.
- `hifimule-ui/src/library.ts` — list row actions, regular/search/favorite artist mapping, and playlist mapping.
- `hifimule-ui/src/components/PlaylistPlayButton.ts` — existing icon action pattern with propagation suppression, busy state, and toast errors.
- `hifimule-ui/src/rpc.ts` — existing get/save selection config and start selection calls; source/config types.
- `hifimule-ui/src/state/basket.ts` — selected portable server identity for artist mapping.
- `hifimule-i18n/catalog.json` — shared English, French, German, and Spanish UI labels.
- `scripts/tests/playback-ui.test.mjs`, `scripts/tests/playback-selection-ui.test.mjs` — existing UI harnesses and selection behavior coverage.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-ui/src/components/PlaybackControls.ts` — replace text rendering with a broadcast icon and accessible tooltip while retaining startup guards and visibility rules.
- [x] `hifimule-ui/src/components/RadioPlayButton.ts` — introduce a shared artist/playlist radio action with serialized config read, source replacement, save, then start; prevent overlapping actions and surface failures.
- [x] `hifimule-ui/src/library.ts` — capture server identity in regular and favorite music artist mappings; distinguish book collections from music playlists; render shared radio actions on eligible list rows.
- [x] `hifimule-ui/src/components/MediaCard.ts` — render the same radio action on eligible cards using original item IDs and portable server identity.
- [x] `hifimule-i18n/catalog.json` — add translated item-specific radio labels for every supported locale.
- [x] `scripts/tests/library-radio-ui.test.mjs` — cover the matrix, especially preserved settings, save-before-start, failures, concurrent activation, identity capture, and event propagation; extend playback bar assertions in `scripts/tests/playback-ui.test.mjs`.

**Acceptance Criteria:**
- Given the playback bar is visible, when radio startup is available, then its control displays a broadcast icon with an accessible name and tooltip and retains existing visibility and disabled behavior.
- Given an eligible artist or playlist in either library layout, when the user activates radio, then it becomes the only saved source and radio starts without navigating into the item.
- Given supported locales, when the radio controls render, then labels are translated and keyboard accessible.

## Spec Change Log

## Design Notes

Use the existing selection RPCs rather than inventing a new radio backend. Source replacement is persistent: if startup fails after saving, the chosen item remains the configured source. Artist IDs must come from the underlying item, not synthetic favorite basket IDs. A shared action coordinator prevents two library buttons from interleaving their save/start sequences. Existing daemon validation remains authoritative for source availability.

## Verification

**Commands:**
- `rtk node --test scripts/tests/library-radio-ui.test.mjs scripts/tests/playback-ui.test.mjs scripts/tests/playback-selection-ui.test.mjs scripts/tests/playback-radio-ui.test.mjs` — expected: all focused UI tests pass.
- `rtk npm --prefix hifimule-ui run build` — expected: TypeScript checks and production build pass.

**Manual checks:**
- Inspect artist and playlist actions in cards, list rows, search, and favorites; verify tooltips, keyboard activation, and layout spacing. Check book surfaces do not offer radio and verify settings show only the selected source after startup.

**Results:** All 80 focused tests passed. TypeScript checks and the production build passed using the Node entry points because npm was unavailable on PATH. Existing chunk warnings remain. Interactive desktop inspection was not performed. Review corrected save failures to use the translated save-failure message.

## Suggested Review Order

**Source replacement and startup**

- Follow source capture, eligibility, serialized save/start, and translated failure handling.
  [RadioPlayButton.ts:9](../../hifimule-ui/src/components/RadioPlayButton.ts#L9)

**Library and playback controls**

- Capture music artist server identity before rendering actions.
  [library.ts:227](../../hifimule-ui/src/library.ts#L227)
- Add the shared radio action to library cards.
  [MediaCard.ts:156](../../hifimule-ui/src/components/MediaCard.ts#L156)
- Match list actions to the card behavior.
  [library.ts:1376](../../hifimule-ui/src/library.ts#L1376)
- Replace the playback text control with an accessible broadcast icon.
  [PlaybackControls.ts:192](../../hifimule-ui/src/components/PlaybackControls.ts#L192)

**Verification and translations**

- Check saved preferences, ordering, failures, identity, and concurrent activations.
  [library-radio-ui.test.mjs:32](../../scripts/tests/library-radio-ui.test.mjs#L32)
- Verify playback radio icon accessibility and existing start behavior.
  [playback-ui.test.mjs:241](../../scripts/tests/playback-ui.test.mjs#L241)
- Review item-specific labels, repeated in all four locales.
  [catalog.json:267](../../hifimule-i18n/catalog.json#L267)
