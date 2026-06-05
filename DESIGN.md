---
name: HifiMule
description: Sync your open-source media server library to portable devices.
colors:
  void: "#0b1020"
  panel: "#151d31"
  panel-strong: "#10182b"
  surface: "#24252b"
  signal-cyan: "#22b8f0"
  ink: "#f1f5f9"
  ink-dim: "#b0bac6"
  amber-warn: "#EBB334"
  border-subtle: "#ffffff1a"
typography:
  body:
    fontFamily: "Inter, system-ui, -apple-system, sans-serif"
    fontSize: "1rem"
    fontWeight: 400
    lineHeight: 1.5
    letterSpacing: "normal"
  label:
    fontFamily: "Inter, system-ui, -apple-system, sans-serif"
    fontSize: "0.8rem"
    fontWeight: 600
    lineHeight: 1.2
    letterSpacing: "0.01em"
  title:
    fontFamily: "Inter, system-ui, -apple-system, sans-serif"
    fontSize: "0.95rem"
    fontWeight: 600
    lineHeight: 1.3
    letterSpacing: "normal"
  headline:
    fontFamily: "Inter, system-ui, -apple-system, sans-serif"
    fontSize: "1.25rem"
    fontWeight: 600
    lineHeight: 1.25
    letterSpacing: "-0.01em"
rounded:
  sm: "4px"
  md: "8px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "16px"
  lg: "24px"
  xl: "32px"
components:
  button-primary:
    backgroundColor: "{colors.signal-cyan}"
    textColor: "{colors.void}"
    rounded: "{rounded.sm}"
    padding: "0 1rem"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.ink}"
    rounded: "{rounded.sm}"
    padding: "0 1rem"
  card-media:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.ink}"
    rounded: "{rounded.md}"
    padding: "0"
  card-basket-item:
    backgroundColor: "rgba(255,255,255,0.045)"
    textColor: "{colors.ink}"
    rounded: "{rounded.md}"
    padding: "0.5rem"
---

# Design System: HifiMule

## 1. Overview

**Creative North Star: "The Record Crate"**

HifiMule is the quiet room where a serious listener manages their collection. The interface is dark by necessity and conviction: audiophiles work at their desk with a device in hand, focused. There are no recommendations, no discovery nudges, no promotional surfaces. The UI holds the library the way a crate holds records: organized, accessible, non-intrusive. The music is the object of attention; the interface is the furniture.

The visual language is deep navy and silence. A very dark blue-tinted void (`#0b1020`) recedes behind layered panel surfaces. The only color that speaks is Signal Cyan (`#22b8f0`): one live wire in an otherwise dark board, used exclusively on primary actions, active states, and real-time indicators. Everything else is slate, transparency, and restraint. New color elements must earn their place by communicating state, never by decorating.

The system explicitly rejects the iTunes paradigm: no glossy gradients, no cover-flow showmanship, no paternalistic "smart" features that make choices for the user. It also rejects the streaming-app aesthetic — no discovery carousels, no algorithmic surfaces, no warm gradients designed to keep users engaged. HifiMule is a tool for people who already know what they want.

**Key Characteristics:**
- Deep dark void with a cold blue tint — not generic dark mode, but a specific nautical-instrument depth
- Single accent color (Signal Cyan) used only on actionable and live-state elements
- Shoelace web components for consistent interactive primitives
- Tonal depth through transparency stacking over the gradient base rather than hard shadows
- Inter at fixed sizes: legibility over expression
- Density-comfortable: the primary view shows a full media grid with a persistent basket sidebar

## 2. Colors: The Signal Palette

One accent, eight shades of dark, and a warning amber for real problems.

### Primary
- **Signal Cyan** (`#22b8f0`): The sole active-state color. Used on: primary buttons, selected card ring, active device chip, managed folder icon, folder hover state, progress indicators, basket toggle hover border, and link elements. If it glows cyan, something is active or requires action. Never used decoratively.

### Neutral
- **Void** (`#0b1020`): The window background. The deepest layer. Covered by a radial cyan glow at top-left (12% opacity) and a 135° gradient to near-black at the bottom. Sets the whole atmospheric context.
- **Panel** (`#151d31`): Sidebar backgrounds, library header, browse-mode bar. One step lighter than Void; clearly a surface, not the floor.
- **Panel-Strong** (`#10182b`): A deeper panel variant used for high-contrast panel moments. Rarely deployed; reserve for elements that need to read as lower than Panel.
- **Surface** (`#24252b`): Card backgrounds, modal panel backgrounds. A deliberate step away from the navy toward warm charcoal — the neutral shelf things sit on.
- **Ink** (`#f1f5f9`): Primary body text. Light slate, not pure white. Stays on-tone with the cold-blue atmosphere.
- **Ink-Dim** (`#b0bac6`): Secondary metadata, captions, placeholder text. Used for less critical information like subtitle text under a card title, track counts, and folder paths.
- **Amber-Warn** (`#EBB334`): Reserved strictly for real problems: dirty manifest warning, capacity amber zone, sync warning banners. Not used for emphasis or decoration.
- **Border-Subtle** (`rgba(255,255,255,0.10)`): The default border for all surfaces. White transparency over dark backgrounds reads as a lift, not a line.

### Implemented Tokens (`:root` in `styles.css`)

The values above are declared as CSS custom properties. Reference these names; do **not** reach for Shoelace `--sl-color-neutral-*` for text or surfaces (its dark-theme ramp fails contrast below stop 600).

| Token | Value | Use |
|-------|-------|-----|
| `--ink` | `#f1f5f9` | Primary text |
| `--ink-dim` | `#b0bac6` | Secondary text / metadata (8.5:1 on panels) |
| `--amber-warn` | `#ebb334` | Warnings only |
| `--surface-border` | `rgba(255,255,255,0.10)` | Strong hairline (panel edges, action dividers) |
| `--surface-border-soft` | `rgba(255,255,255,0.08)` | Standard divider |
| `--surface-fill` | `rgba(255,255,255,0.03)` | Subtle panel/card wash |
| `--surface-fill-hover` | `rgba(255,255,255,0.06)` | Interactive surface (hover) |
| `--radius` | `8px` | Card / panel corners (Shoelace's 6px stays for inputs) |
| `--ease-out-quart` / `--ease-out-expo` | cubic-beziers | State / spatial motion curves |
| `--dur-fast` / `--dur-base` | `140ms` / `200ms` | Transition durations |

### Named Rules
**The One Wire Rule.** Signal Cyan appears on ≤10% of any given screen. Only active and actionable elements carry it. A card grid where 40% of items glow cyan means 40% are selected; it conveys real state. Decorative use is prohibited by definition.

**The Amber Threshold.** Amber (`#EBB334`) appears only when the user needs to act or be warned. If you are tempted to use it for visual interest, choose a neutral instead.

## 3. Typography

**Body Font:** Inter (with system-ui, -apple-system fallback)

**Character:** A single family across every text role. Product UI — no display/body pairing needed. Inter's legibility at small sizes and its precise weight range (400 body, 500 medium, 600 semibold) carry the entire hierarchy without a second typeface. The dark background demands clean, hinted glyphs; Inter delivers.

### Hierarchy
- **Headline** (600, 1.25rem, -0.01em tracking): Section titles — library view h1, basket header, modal titles. One per major region.
- **Title** (600, 0.95rem, normal tracking): Component titles, card label rows, panel headers like "Devices." Tight but readable.
- **Body** (400, 1rem, 1.5 line-height): Form labels, descriptive paragraphs, long-form content. Line length capped at 65ch in prose contexts.
- **Label** (600, 0.8rem, 0.01em tracking): Metadata chips, capacity status, sync counters, browse mode button text, folder type badges. Upper-weight labels read clearly at small sizes without uppercase.
- **Caption** (400, 0.75rem): Timestamps, secondary metadata, item paths. Uses Ink-Dim color.

### Named Rules
**The Fixed-Scale Rule.** No fluid/clamp typography. HifiMule runs as a desktop Tauri app at consistent DPI. `clamp()` headings that shrink in a narrow panel introduce noise, not responsiveness. All sizes are fixed rem.

**The No-Display Rule.** Inter is body and label. Do not introduce a display or serif family. The collection cover art provides visual richness; the type system provides structure.

## 4. Elevation

This system is flat by default. Depth is conveyed through tonal layering: each panel level has a distinct background color, and transparency stacking over the gradient base creates perceived depth without casting shadows.

Media cards receive a shadow on hover only (`0 10px 24px rgba(0,0,0,0.28)`) — depth is a state signal, not a structural role. At rest, cards sit flat. The hover lift confirms interactivity without pre-announcing it.

Modals use the Surface color (`#24252b`) and an 8px radius. They feel like a shelf raised above the panel, not a floating card with glow.

The backdrop-filter blur on the library panel and browse-mode bar (`blur(10px)`, `blur(8px)`) is structural: it reinforces the panel hierarchy by visually separating the sticky header from scrolling content beneath. It is not decorative glassmorphism.

### Named Rules
**The Flat-At-Rest Rule.** Shadows appear only in response to hover or selection state. A shadow on an interactive element at rest reads as noise, not depth.

**The Blur-Is-Structural Rule.** `backdrop-filter: blur()` is used only on sticky/pinned layers that need to separate from content scrolling beneath them. Never on cards, list items, or anything rendered mid-scroll.

## 5. Components

### Buttons
Delegated to Shoelace (`sl-button`). The system uses Shoelace's variant API directly.

- **Primary** (`variant="primary"`): Signal Cyan background, void text. Full-width in forms; auto-width in toolbars. Radius inherits from `--sl-border-radius-medium` (Shoelace default: ~4px). Primary action in any screen — "Connect", "Sync now", "Save changes".
- **Default / Ghost** (`variant="default"`): Transparent with a border. Used for secondary actions — "Cancel", filter chips in browse mode, basket item removal.
- **Danger** (`variant="danger"`): Reserved for destructive actions only — prune, delete, format device.

States: Shoelace handles hover, focus, active, disabled, and loading natively. Do not override Shoelace's focus ring; it satisfies WCAG 2.1 AA keyboard requirements.

### Browse Mode Chips
The `#browse-mode-bar` hosts filter buttons (Artists, Albums, Genres, Playlists, Songs). These are Shoelace default-variant buttons at small size. The active/current view is not visually distinguished at the moment — a clear selected state using Signal Cyan border or background tint would complete this pattern.

### Media Cards (`.media-card`)
The primary browsing surface.

- **Shape:** 8px radius, matching dialog and surface radius
- **Background:** Surface (`#24252b`) with a 1px Border-Subtle border
- **Image area:** 1:1 aspect ratio, full-width, covers the upper card
- **Content block:** Padding `0.65rem 0.75rem`, title in Title weight, subtitle in Caption/Ink-Dim
- **Hover:** `translateY(-2px)` + shadow (`0 10px 24px rgba(0,0,0,0.28)`) + Signal Cyan border tint (`rgba(34,184,240,0.35)`)
- **Selected state:** 4px inset Signal Cyan ring on image (`box-shadow: inset 0 0 0 4px #22b8f0`), selection overlay at 0.4 opacity with basket toggle button
- **Synced state:** 0.7 opacity; a success badge top-right

### Basket Item Cards (`.basket-item-card`)
List items in the right sidebar.

- **Shape:** 8px radius
- **Background:** `rgba(255,255,255,0.045)` with `rgba(255,255,255,0.075)` border
- **Layout:** Flex row — 48px thumbnail, title/meta block, remove action
- **Hover:** Background lifts to `rgba(255,255,255,0.06)`, Signal Cyan border tint
- **Special variants:** Auto-fill slot (dashed Signal Cyan border), Artist/Genre items (neutral border, icon placeholder)

### Capacity Bar (`.capacity-bar`)
A horizontal 8px bar showing device storage state. Three segments: used (white 25%), pending selection (Signal Cyan), free (transparent). Amber and red color overrides on the status text when thresholds are crossed.

### Sync Progress Panel (`.sync-progress-panel`)
Replaces the basket item list during an active sync. Shows current file (truncated), byte progress, file counter. Transitions to success or error state on completion.

- **Success:** centered icon + label, Shoelace success color
- **Error:** centered icon + label + scrollable error list

### Modals and Dialogs
Three dialogs: Init Device, Repair, Device Settings. All use Shoelace `<sl-dialog>` with a custom panel override:

- **Background:** Surface (`#24252b`) with Ink (`#f1f5f9`) text
- **Max width:** `min(92vw, 720px)` (Repair), `min(92vw, 560px)` (Init Device)
- **Radius:** 8px on the panel
- **Body:** scrollable with `max-height` constraints; a right-side padding offset for the scrollbar

### Inputs and Forms
Shoelace `<sl-input>` components. Height medium set to `2.5rem` via `--sl-input-height-medium`. Labels in Label weight. Error states via Shoelace's built-in danger variant.

## 6. Do's and Don'ts

### Do:
- **Do** use Signal Cyan exclusively for actionable and live-state elements. If it's cyan, it means the user can interact or something is happening right now.
- **Do** convey depth through background color steps (Void → Panel → Surface), not shadow stacks.
- **Do** use Amber (`#EBB334`) only for warnings that require user attention: manifest integrity, capacity thresholds, sync failures.
- **Do** keep type at fixed rem sizes. This is a desktop Tauri app at consistent DPI; fluid typography adds noise.
- **Do** maintain WCAG 2.1 AA contrast: `#f1f5f9` (Ink) on `#151d31` (Panel) is approximately 12:1; `#b0bac6` (Ink-Dim) on `#0b1020` (Void) must be verified to stay above 4.5:1.
- **Do** use `backdrop-filter: blur()` only on sticky/pinned layers separating from scrolling content beneath. One structural purpose.
- **Do** use Shoelace's native button states (hover, focus, active, disabled, loading). Do not override the focus ring; it handles WCAG keyboard compliance.
- **Do** show hover lift (`translateY(-2px)` + shadow) on media cards as a state signal, not a structural treatment.

### Don't:
- **Don't** use Signal Cyan decoratively. Not on headings, not as a background tint on non-active states, not as a gradient.
- **Don't** add a second typeface. Inter carries the entire hierarchy. A display or serif family for headings would be decorative, not communicative.
- **Don't** build a glassmorphism card system. The blur on the library panel header is earned by being pinned above scrolling content. A blurry media card with frosted-glass texture is the exact visual language HifiMule rejects.
- **Don't** import the iTunes paradigm: no cover-flow, no "smart" auto-curated sections, no gradient hero for featured content, no promotional chrome of any kind.
- **Don't** use streaming-app aesthetics: no discovery carousels, no recommendation surfaces, no mood-based color washes.
- **Don't** use SaaS dashboard patterns: no hero-metric cards (big number + small label + gradient accent), no stats overview panels.
- **Don't** add shadows at rest. Shadows on interactive elements before hover introduces visual noise that implies permanence where there should be none.
- **Don't** use uppercase for body copy or titles. Reserve uppercase for very short labels (4 words or fewer) like filter chips or folder-type badges. Sentences in uppercase are unreadable at the sizes used in this UI.
- **Don't** introduce a new accent color without removing one. The palette is deliberately monochromatic except for Signal Cyan and Amber-Warn. A third accent is a third distraction.
