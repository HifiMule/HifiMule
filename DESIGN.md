---
name: "HifiMule"
description: "A quiet, precise desktop interface for listening and preparing portable media collections."
colors:
  void: "#0b1020"
  panel: "#151d31"
  panel-strong: "#10182b"
  surface: "#24252b"
  signal-cyan: "#22b8f0"
  ink: "#f1f5f9"
  ink-dim: "#b0bac6"
  amber-warn: "#ebb334"
  surface-border: "rgba(255, 255, 255, 0.1)"
  surface-border-soft: "rgba(255, 255, 255, 0.08)"
  surface-fill: "rgba(255, 255, 255, 0.03)"
  surface-fill-hover: "rgba(255, 255, 255, 0.06)"
  control-background: "hsl(240, 5.9%, 11%)"
  control-text: "hsl(240 5.6% 73%)"
  control-primary: "hsl(198.7 97.9% 57.2%)"
  control-primary-hover: "hsl(199.7 85.9% 47.7%)"
  control-danger: "hsl(0 98.6% 67.9%)"
  control-success: "hsl(141.6 72.4% 55.2%)"
typography:
  body:
    fontFamily: "Inter, system-ui, -apple-system, sans-serif"
    fontSize: "1rem"
  headline:
    fontFamily: "Inter, system-ui, -apple-system, sans-serif"
    fontSize: "2em"
    fontWeight: 600
    letterSpacing: "0"
  section-title:
    fontFamily: "Inter, system-ui, -apple-system, sans-serif"
    fontSize: "1.5em"
    fontWeight: 600
    letterSpacing: "0"
  title:
    fontFamily: "Inter, system-ui, -apple-system, sans-serif"
    fontSize: "0.9rem"
    fontWeight: 600
  label:
    fontFamily: "Inter, system-ui, -apple-system, sans-serif"
    fontSize: "0.8rem"
  basket-title:
    fontFamily: "Inter, system-ui, -apple-system, sans-serif"
    fontSize: "0.85rem"
    fontWeight: 500
  metadata:
    fontFamily: "Inter, system-ui, -apple-system, sans-serif"
    fontSize: "0.75rem"
rounded:
  xs: "3px"
  sm: "4px"
  md: "8px"
spacing:
  2xs: "0.25rem"
  xs: "0.5rem"
  sm: "0.75rem"
  md: "1rem"
  lg: "1.5rem"
components:
  button-primary:
    backgroundColor: "{colors.control-primary}"
    textColor: "{colors.control-background}"
    rounded: "{rounded.sm}"
    height: "2.5rem"
    padding: "0 1rem"
  button-default:
    backgroundColor: "{colors.control-background}"
    textColor: "{colors.control-text}"
    rounded: "{rounded.sm}"
    height: "2.5rem"
    padding: "0 1rem"
  button-danger:
    backgroundColor: "{colors.control-danger}"
    textColor: "{colors.control-background}"
    rounded: "{rounded.sm}"
    height: "2.5rem"
    padding: "0 1rem"
  input:
    backgroundColor: "{colors.control-background}"
    rounded: "{rounded.sm}"
    height: "2.5rem"
  card-media:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.ink}"
    rounded: "{rounded.md}"
  card-basket-item:
    backgroundColor: "{colors.surface-fill}"
    textColor: "{colors.ink}"
    rounded: "{rounded.md}"
    padding: "{spacing.xs}"
  browse-tab:
    backgroundColor: "transparent"
    textColor: "{colors.ink}"
    rounded: "{rounded.xs}"
    padding: "0 6px"
  browse-tab-selected:
    backgroundColor: "rgba(34, 184, 240, 0.09)"
    textColor: "{colors.signal-cyan}"
    rounded: "{rounded.xs}"
---

# Design System: HifiMule

## Overview

**Creative North Star: "The Record Crate"**

HifiMule is the quiet room where a serious listener manages their collection. The interface holds the library the way a crate holds records: organized, accessible, non-intrusive. The music is the object of attention; the interface is the furniture. Quiet, precise, trustworthy remains the established direction.

Deep navy surfaces, restrained blue signals, compact controls, and visible state make this a working desktop tool. Artwork provides variety. The system rejects glossy iTunes-style showmanship, promotional streaming surfaces, and SaaS metric-card decoration. User-configured Auto-Fill and discovery controls are existing functionality, not a reason to introduce promotional browsing.

**Key Characteristics:**

- Dark navy foundations with translucent panel layers.
- Signal Cyan for custom actions and state; Shoelace sky-blue and semantic colors for inherited controls.
- Inter for application text, with native system-font stacks inside Shoelace primitives.
- Dense library browsing beside a persistent, resizable device basket.
- Brief decelerating motion and explicit keyboard focus treatments.

This refresh describes the current working tree, including uncommitted UI changes. Sources: `hifimule-ui/src/styles.css`, `src/components/MediaCard.ts`, `src/components/BasketSidebar.ts`, `src/components/ServerHub.ts`, `src/login.ts`, `index.html`, and the installed Shoelace dark theme and button styles. Desktop and compact layout fixtures plus the selection fixture were visually inspected. Fixtures contain placeholder content; they are not complete production-screen or accessibility verification.

## Colors

The palette keeps the established dark-blue atmosphere; semantic color communicates action, selection, storage, and outcomes.

### Primary

**Signal Cyan** is the custom accent (`--accent`, with raw channels in `--accent-rgb`). Custom focus outlines, selected browse text, tint backgrounds, and capacity segments use it directly.

**Control Primary** and **Control Primary Hover** record Shoelace's installed dark-theme sky colors. Shoelace controls retain that primary ramp through `.sl-theme-dark`; custom actions and selection treatments use Signal Cyan through `--accent`. The former root-only primary-600 override has been removed so the two roles are explicit and independent. Preserve this distinction when changing either palette.

### Neutral

**Void** is the page foundation. **Panel** and **Panel Strong** support navy layers; **Surface** backs media cards and dialog panels. **Ink** carries primary text, **Ink Dim** secondary metadata. **Surface Border**, **Surface Border Soft**, **Surface Fill**, and **Surface Fill Hover** are the reused white-alpha surface vocabulary. Control Background and Control Text belong to Shoelace's dark neutral palette.

The body includes a restrained cyan radial wash over a diagonal dark gradient. These existing atmospheric backgrounds are compatible with the established direction; promotional gradient heroes are not.

### Semantic states

**Amber Warn** marks interrupted sync, repair, and capacity warnings. Shoelace success and danger tokens mark synced/completed and destructive/error states. These are semantic colors, not extra brand accents.

**The One Wire Rule.** Use the blue accent family to communicate interaction or state, not to decorate headings or unrelated surfaces.

**The Amber Threshold Rule.** Reserve amber for warnings requiring attention; the ready-to-sync prompt is neutral with a blue icon.

## Typography

Application text uses Inter with system fallbacks; Inter is bundled locally as variable WOFF2 subsets from `@fontsource-variable/inter` 5.2.8, with its SIL OFL license included in the app. `src/assets/fonts/inter.css` uses `font-display: swap` and Unicode ranges so only needed subsets load; startup does not depend on Google Fonts. System fallbacks cover unsupported scripts. Shoelace controls use their own `--sl-font-sans` through `--sl-input-font-family` unless a component explicitly inherits the application font.

The frontmatter records observed roles, not a newly imposed scale. Body remains the browser-default medium size (normally 16px); h1 and h2 retain browser-default relative sizes (2em and 1.5em) with weight 600 and zero tracking. The inspected fixture rendered those headings at 32px and 24px. There is no authored universal 1.25rem headline or 1.5 body line-height.

Media titles use the title role; subtitles use label size and clamp to three lines with long-word wrapping. Basket titles and metadata have their own smaller roles. Browse tabs use 0.6875rem text, 1.3 line-height, and wrapping. Short server-group labels use 0.7rem, weight 600, uppercase, and 0.04em tracking. Do not generalize that treatment to titles or prose.

**The No-Display Rule.** Keep the functional sans-serif hierarchy; do not add a decorative heading face.

## Layout

The viewport-height shell is split between a flexible library and an end-anchored basket. The split divider is 4px with a 12px hit area. Basket bounds are 330–440px, reduced to 300–360px at viewport widths up to 980px. The basket's own minimum follows that bound. Library content and basket items scroll independently; short windows can also scroll the containing panels so controls remain reachable.

The media grid starts with auto-filled columns of at least 190px and a 1.25rem gap. At viewport widths up to 1120px, its minimum becomes 170px and library horizontal padding becomes 1.5rem. The library is an inline-size query container: at 47.5rem or less, its header stacks and the server switcher stretches; at 37.5rem or less, padding and grid gap become 1rem and columns can shrink below 170px to fit the panel. Window widths and library container widths are different conditions.

The shared spacing scale maps directly to `--space-2xs` through `--space-lg`; card bodies and other local patterns retain their observed intermediate values. Browse controls wrap in a bounded scroll region (maximum block size `min(30vh, 20rem)`). A 720px maximum-height query reduces vertical padding. Playback controls live in the library layout flow, with additional narrow-screen and container rules; the desktop shell is not a fully separate mobile composition.

## Elevation & Depth

Tonal layers and hairline borders carry most structural separation. Media cards acquire a hover shadow (`0 10px 24px rgba(0, 0, 0, 0.28)`) and move up 2px. The synced badge has a small resting shadow (`0 2px 4px rgba(0, 0, 0, 0.3)`), and playback popovers have stronger shadows. The old blanket ban on resting shadows did not describe these existing exceptions.

Blur is local and functional: track-count badges use 4px over artwork; playback controls can use 10px on an isolated background layer. Do not turn those exceptions into a frosted-card visual system.

Motion uses the authored quart and expo deceleration curves, with fast/base durations of 140ms and 200ms. Capacity segments animate their transforms over 300ms. Selection rings reveal over 240ms and reverse over 120ms; newly added basket rows enter from the library side over 200ms, while existing rows stay still on refresh. Capacity segments retain their displayed transform across sidebar rerenders and settle over 300ms; device switches do not interpolate between devices. Reduced motion uses a 120ms row fade and immediate capacity updates. Reduced-motion CSS suppresses general animation and hover movement while retaining a short 120ms opacity alternative for selection feedback. Shadow-DOM component internals require separate verification; a universal light-DOM selector is not proof of full reduced-motion coverage.

## Shapes

Cards and panels share the 8px `--radius`. Shoelace medium control corners are 4px and small corners are 3px in the installed theme. Browse tabs also use 3px. Capacity bars use 4px corners, with 2px swatches. Artwork is square with rounded upper corners; the selected inset ring follows that silhouette. Borders are usually 1px, with dashed borders distinguishing configured Auto-Fill slots.

## Components

### Buttons and inputs

Compact and familiar. Use Shoelace variant APIs for primary, default, danger, loading, and disabled states. Standard primary buttons take primary-600 with neutral-0 text; hover uses primary-500. Default buttons have a dark neutral fill and border, not a universal transparent ghost treatment. Danger is reserved for destructive operations. Medium controls are 2.5rem tall; local small sizes remain valid.

Inputs use Shoelace background, border, text, and focus tokens. Preserve component keyboard and loading behavior, and application error copy. Retaining library focus styling is useful but does not by itself establish WCAG compliance. Custom controls use visible 2px blue outlines with offsets appropriate to the surface.

### Navigation and server connection

Browse tabs are small, wrapping controls. The pressed state uses cyan text, a faint cyan background, and an inset 2px blue underline; unavailable controls have reduced opacity. The server switcher is a bordered, dark connection chip with truncated identifying text and a dropdown. Its width responds to the library container. Navigation state must remain explicit; the earlier description of undifferentiated browse buttons is obsolete.

### Media cards

Artwork leads; controls remain compact. The surface has the shared radius and border; content padding is `0.65rem 0.75rem 0.7rem`. Hover lifts and tints the border. The dark selection overlay becomes visible on hover, focus within, or selection. Selected artwork has a 4px inset blue ring; the ring transition reverses cleanly on rapid toggles. A title button carries keyboard focus. Synced cards use 0.7 opacity and a success badge; navigating cards use 0.75 opacity, including when already synced.

Cards use `content-visibility: auto` with an intrinsic size estimate. Radio cards temporarily relax containment and clipping for interactive hints. Preserve these behavioral exceptions when reusing the pattern.

### Basket rows

A 48px thumbnail, compact title and metadata, and removal controls form each row. The row has half-rem padding, a three-quarter-rem gap, a faint white fill, and an 8px radius. Hover uses the shared hover fill and cyan-tinted border. Mixed-server rows retain source labels; locked content has an explicit placeholder. Auto-Fill slots use dashed borders; artist and genre groups have their own icon treatment.

### Capacity bar

A horizontal 8px bar showing device storage state. Segments: unmanaged used content (white 25%), reserved selection (solid Signal Cyan), recorded Auto-Fill usage (Signal Cyan at 35% opacity), and available space (transparent). Auto-Fill is shown only when recorded usage is positive; clip its segment to the width left after the reservation while retaining its full measured size in the text.

The Compact layout keeps manual track/size totals above the bar. Below it, the primary row shows available space after the reservation and recorded Auto-Fill usage, followed by a secondary row with a translucent cyan swatch and “used by Auto-Fill.” Labels wrap in narrow sidebars, and the bar has a text description for assistive technology. With no recorded Auto-Fill, retain the existing remaining-space wording and omit the secondary row. Amber and red warnings keep priority and override the reserved-selection segment and primary status; measured Auto-Fill text remains secondary. These display values do not change sync admission or Auto-Fill budgets.

### Playback, status, and dialogs

Playback controls persist below library content, with title/source, transport actions, seek progress, and output selection. Guidance and output panels use bounded overlays and visible focus. Empty, disconnected, and unavailable states retain explanatory text rather than relying on color.

The ready-to-sync banner is neutral with a blue signal icon. Active sync exposes progress and current work; terminal states distinguish completion and failure. Modal forms use Shoelace dialogs with local surface and width overrides. Keep dialog-specific sizing and scroll behavior with the component rather than assuming all dialogs share a single size.

## Do's and Don'ts

### Do:

- **Do** preserve the Record Crate's quiet, precise, trustworthy atmosphere.
- **Do** use shared spacing and white-alpha surface tokens before adding local values.
- **Do** distinguish custom cyan tokens from inherited Shoelace theme tokens.
- **Do** retain selected, focused, disabled, pending, synced, and failed states with meaningful text.
- **Do** let translated labels wrap and keep controls reachable in compact windows.
- **Do** preserve reduced-motion alternatives and verify keyboard behavior on custom controls.
- **Do** treat WCAG 2.1 AA as the accessibility target, not as a certification supplied by a component library.

### Don't:

- **Don't** introduce promotional streaming chrome, cover-flow showmanship, or decorative metric cards.
- **Don't** add a decorative display font or uppercase body copy.
- **Don't** convert local blur and shadow exceptions into an ornamental card system.
- **Don't** erase user-configured Auto-Fill or discovery functions through an aesthetic prohibition.
- **Don't** assume a root token override replaces a theme class applied on a descendant.
- **Don't** describe unverified contrast, focus, or responsive behavior as guaranteed compliance.
