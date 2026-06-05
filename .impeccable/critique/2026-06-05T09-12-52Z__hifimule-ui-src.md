---
target: hifimule-ui/src
total_score: 24
p0_count: 1
p1_count: 1
timestamp: 2026-06-05T09-12-52Z
slug: hifimule-ui-src
---
## Design Health Score

| # | Heuristic | Score | Key Issue |
|---|-----------|-------|-----------|
| 1 | Visibility of System Status | 3 | Daemon polling lag (2s) delays device detection; sync "starting" phase shows only a spinner with no indication of what's computing |
| 2 | Match System / Real World | 3 | cloud-download icon for a local USB sync is categorically wrong; "dirty manifest" leaks internal implementation language |
| 3 | User Control and Freedom | 2 | No cancel during active sync — a multi-minute operation that writes to a physical device; no undo on "Clear All" basket |
| 4 | Consistency and Standards | 3 | br tags in login form vs. CSS gap everywhere else; --sl-color-neutral-700 (light-mode token) in a dark panel |
| 5 | Error Prevention | 2 | No confirmation on "Clear All" (potentially 20 mins of curation lost on one misclick); device path fields accept arbitrary input with no validation |
| 6 | Recognition Rather Than Recall | 3 | Device path inputs require typed filesystem paths with no browse/picker; transcoding profile description disappears when dropdown closes |
| 7 | Flexibility and Efficiency | 2 | Zero keyboard shortcuts for any action; no multi-select in library grid; no search within a browsed level |
| 8 | Aesthetic and Minimalist Design | 3 | Background gradient + structural glassmorphism on the library panel add visual noise without perceptual payoff; unused .glass-card class |
| 9 | Error Recovery | 2 | Sync error list truncated at 120px with ellipsis; no copy-to-clipboard; no retry affordance -- only "Dismiss" |
| 10 | Help and Documentation | 1 | No tooltips on non-obvious controls; "dirty manifest" banner gives no context |
| **Total** | | **24/40** | **Acceptable -- significant improvements needed** |

## Anti-Patterns Verdict

LLM assessment: Partial pass. Background radial-gradient corner glow and structural glassmorphism on library panel are the AI cosmetic fingerprint. The two-panel workbench, media grid, and sync state machine are genuine purpose-built design.

Deterministic scan: 2 findings. Inter font (warning, styles.css:14) -- defensible for this product but noted. Layout property animation (warning, styles.css:281) -- transition:width on capacity segment causes layout thrash, should use transform:scaleX.

Browser visualization: Not available.

## Overall Impression

Engineering underneath is careful and professional. Design layer has not caught up. Three fixes before this feels like the tool it is: sync needs a cancel path, completion state needs to earn its moment, basket sidebar needs visual segmentation.

## What's Working

1. Server type probe on login -- auto-detecting Jellyfin/Navidrome and showing a badge is the best first impression beat in the product.
2. Sync state machine -- complete, professionally designed flow from basket to delta to confirmation to progress to outcome.
3. Scroll and page cache -- invisible infrastructure that will be noticed only by its absence.

## Priority Issues

**[P0] No sync cancel during active transfer**
Why: Sync can run minutes on a physical device. No exit while controls are locked. Safety issue, not just UX.
Fix: Cancel button in progress panel calling cancellation RPC, transitioning to "Sync cancelled" state.
Command: /impeccable harden

**[P1] cloud-download icon on a local USB sync**
Why: Directly contradicts HifiMule's identity. Marcus sees a cloud icon holding a physical device. Trust erosion on every sync.
Fix: Replace with usb-drive, device-hdd, arrow-down-circle, or any local-storage icon.
Command: /impeccable clarify

**[P2] "Clear All" basket with no confirmation**
Why: One misclick destroys up to 20 minutes of curation. Highest-effort/lowest-protection destructive action.
Fix: Confirmation popover or 5-second undo toast.
Command: /impeccable harden

**[P2] Contrast failures on small dimmed text**
Why: opacity stacking on 0.7rem text likely fails WCAG AA. Affects all users. --sl-color-neutral-700 in dark context is near-invisible.
Fix: Replace opacity-based dimming with solid colors verified at 4.5:1 against actual backgrounds.
Command: /impeccable audit

**[P3] Basket sidebar cognitive density**
Why: 7+ information groups in 330px with minimal segmentation. Cognitive load assessment failure.
Fix: 1px rgba dividers between logical sections. Collapse auto-fill controls by default.
Command: /impeccable layout

## Persona Red Flags

Alex (Power User): No keyboard shortcuts anywhere. No multi-select. No item counts on browse modes.
Sam (Accessibility): Device hub cards are div with click handlers -- not keyboard-accessible. Hover-only add-to-basket overlay has no keyboard equivalent. 0.7rem text below AA minimum.
Marcus (audiophile): Sync complete shows no transfer summary. "Dirty manifest" reads as corrupted. Transcoding profile description disappears. Cloud icon on local sync.

## Minor Observations

- --sl-color-neutral-700 in device-settings-label: light-mode token, near-invisible in dark mode
- Quick-nav-bar has two padding declarations (line 558 and 560), second overrides first
- transition:width on capacity-segment -- layout thrash, use transform:scaleX
- Login form uses br tags for spacing
- Logout icon in server chip has no label or tooltip
- renderAutoFillControls uses inline style attributes bypassing token system

## Questions to Consider

1. Sync completion is the highest emotional moment. A "847 files, 12.4 GB, now on your Sony ZX507" summary line is all the payoff the screen needs -- what would The Record Crate feel like when you close the crate?
2. Basket (deliberate curation) and auto-fill (strategic policy) have opposite assumptions. Which is primary?
3. No keyboard shortcuts on a desktop Tauri app for power users is the most surprising single gap. Is pointer-driven intentional?
