# Story 15.16 — implementation verification

Date: 2026-09-19. User selected design **B**, quiet navigation, after reviewing `15-16-browse-mode-design-options.html`. This choice supersedes the prepared solid-primary button treatment and icon mapping. Provider order and full labels are unchanged.

## Implementation

- Local icons: mic / disc / collection-play / music-note / tags / plus-square / repeat / clock-history / heart. No new dependency or translation.
- Scoped 11px labels, 14px icons, 30px minimum targets, 4px gaps, 4px vertical / 16px horizontal padding. Active mode uses the application cyan token, subtle fill and underline. Focus remains the Shoelace outline.
- Keyed reconciliation retains controls, listeners and focus across refreshes and capability changes. Removed focused controls fall back to a supported mode or the navigation container. Empty capabilities issue no unsupported root request.
- `aria-pressed` and accessible disabled state reach Shoelace's actual native shadow button. A focused mode uses aria-disabled during loading to avoid native disabled blur; existing loading admission guards reject activation. Other modes use native disabled too.
- Grid/List nodes are retained; hidden while loading, in Tracks, or with no capabilities. Existing list/grid preference and navigation loaders remain authoritative.
- Final scroll bound is `min(30vh, 20rem)`, with flex shrinking enabled. The initial 40vh/nonshrinking bound consumed too much space in the large-text fixture. Tab/Space brought Favorites into view with a visible focus ring.

## Automated verification

- `rtk node --test scripts/tests/*.test.mjs`: **159 passed**, zero failures.
- Seven new production-module Node tests: ordered capability reconciliation, stable nodes/listeners/focus, EN/FR/ES/DE labels and decorative icons, shadow-button pressed state, focus fallback, retained Grid/List, same-mode/loading guards and resets, focused disabled semantics, stale detached-control admission.
- Red phase: four of the initial five tests failed against the original renderer; mode reset-path test already passed. All tests passed after implementation.
- `rtk npm --prefix hifimule-ui run build`: passed TypeScript and Vite. Existing mixed static/dynamic-import and large-chunk warnings remain.
- `git diff --check` and preview script syntax check: passed. No dedicated frontend lint script is configured.

## Rendered checks (real production modules and Shoelace, mocked IPC)

Reproduce with `rtk node scripts/preview-browse-mode.mjs`, open `http://localhost:1422/.browse-preview.html`, then choose version, width, language, text scale and capability set. The script copies the baseline from commit `2a88f9b1b410e14ef8fe22e80fcff8aef1d4a3de` and current production library source, exposing a fixture-only state probe. It uses real production CSS, library loaders, i18n, components and local Shoelace assets. Tauri IPC is mocked with empty library responses; unexpected mutation requests throw. Generated files are temporary and are not production entry points.

The fixture is **not a complete native application**: header text, basket and playback regions are fixture content. Its optional divider uses the actual Shoelace split panel and application classes. Do not treat these measurements as packaged native results.

Matched comparison: 1280×720 browser viewport, 720px library column, EN, nine modes, Artists selected, 100% text, identical fixture shell:

| Measurement | Before | B |
|---|---:|---:|
| Bar height (including border) | 85px | 73px |
| Content client height | 376px | 388px |
| Mode rows | 2 | 2 |
| Minimum button height | 30px | 30px |
| Label font | 12px | 11px |
| Mode gap | 8px | 4px |
| Outer vertical / horizontal padding | 8px / 32px | 4px / 16px |

Final normal-text bar heights (360 / 720 / 1000px columns):

- EN: 141 / 73 / 39px.
- FR: 141 / 73 / 39px.
- ES: 175 / 73 / 73px (long labels naturally wrap).
- DE: 141 / 73 / 39px.

All 24 combinations of four locales × three widths × 100%/200% root text had **no horizontal bar overflow**. At 200%, the 360px and 720px bars were capped at 216px, leaving a 128px content client area in this fixture. At 1000px, EN/DE used 145px and FR/ES used 213px. Root-font scaling is verified; browser text-only zoom is not independently certified.

- All nine modes activated through existing production loaders. Tracks requested its own artist/album/track data and hid the view toggle. Repeated cached modes did not add duplicate root reads. No playback/source-selection/basket mutation appeared in the fixture RPC trace; no browser console errors were observed.
- Broad → limited (Albums/Tracks/Favorites) → empty capability changes removed stale controls, retained supported controls and hid unavailable toggles. Node coverage also restores the broad set and reverses order.
- Enter selected Albums and retained focus after the loading cycle. Space selected Favorites at 200% DE; the scroll region revealed it and showed an unclipped focus ring.
- Browser accessibility inspection exposed full localized names and selected/unselected values on the native controls, with no extra icon names. This is accessibility-tree inspection, not a screen-reader listening test.
- Library/Playing fixture visibility retained the bar and current mode. Existing production destination regressions also passed.
- Dragging the actual Shoelace divider changed the library from about 853px (two mode rows) to 931px (one row), with no horizontal bar overflow.
- Calculated contrast using application panel and selected-fill colors: normal ink approximately 13.72:1; selected cyan text/icons approximately 5.60:1; underline against panel approximately 6.57:1. Disabled controls remain visibly dimmed.

## Installed-platform evidence and release follow-up (15.17)

| Platform / artifact | Environment and actual check | Outcome |
|---|---|---|
| macOS, existing `/Applications/HifiMule.app` | Opened native app; inspected original FR nine-mode bar and library/basket/playback arrangement before implementation | Baseline visual inspection only; installed app does not contain this change |
| macOS, changed development build | Duplicate local Tauri dev launch reached a running process, but computer-use tooling could not bind the unbundled debug executable; stopped only this duplicate run | Updated native interaction **unverified** |
| Windows package | No changed package built or installed | **Unverified — 15.17** |
| Linux package | No changed package built or installed | **Unverified — 15.17** |
| macOS package | No changed package installed | **Unverified — 15.17** |

15.17 should verify the changed full native shell at minimum window height and divider extremes, real provider/server switching, live main/Preview playback with a physical-device basket, OS text scaling/text-only zoom, and screen-reader announcements. Fixture data, Node doubles and the installed baseline do not certify those scenarios. No Rust, playback protocol, provider implementation, device selection or basket mutation code changed.
