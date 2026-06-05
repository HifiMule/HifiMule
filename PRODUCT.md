# Product

## Register

product

## Users

Music enthusiasts who are also self-hosting power users: people who run Jellyfin or Navidrome, own DAPs or legacy hardware like Rockbox-patched iPods, and care about lossless audio quality. They open HifiMule to move music from their library onto a portable device — the workflow is deliberate and infrequent, with high stakes (library integrity, device storage). They expect precise control and trust the tool to handle their collection without surprises.

## Product Purpose

HifiMule bridges open-source media servers and legacy mass-storage devices. It handles delta sync, resume, scrobbling, and manifest tracking so the user never has to babysit transfers or reconcile state manually. Success looks like a sync that completes without errors and a device that plays exactly what was chosen.

## Brand Personality

Quiet, precise, trustworthy. The UI should feel like a well-made piece of hardware: no noise, no persuasion, just function. Confidence comes from density and correctness, not from decoration.

## Anti-references

- **Apple iTunes** — the corporate music-management paradigm: glossy, paternalistic, feature-bloated, locked to an ecosystem. HifiMule should feel like the opposite: open, lean, technically honest.
- Streaming-app aesthetics (Spotify, Tidal) — designed to sell subscriptions, not manage a personal library. Avoid hero-image discovery UI, algorithmic nudges, or anything that suggests the user doesn't already know what they want.
- SaaS dashboard clichés — metric cards, gradient heroes, "enterprise-grade" chrome.

## Design Principles

1. **The library is the center of gravity.** Every screen decision should reduce friction between the user and their music collection. Controls earn their place; decoration doesn't.
2. **Trust through precision.** State is always visible: what's synced, what's pending, what failed. The UI never hides errors or softens bad news.
3. **Power without ceremony.** Advanced operations (repair, init, profiles) should be reachable without a tutorial, but not in the way during a routine sync.
4. **Visual restraint is respect.** The user chose a self-hosted, open-source stack because they don't want software making choices for them. The UI should follow the same philosophy.
5. **Dark is the natural habitat.** Audiophiles and DAP users interact with devices in dim, focused contexts. Dark mode isn't a preference — it's the primary environment.

## Accessibility & Inclusion

WCAG 2.1 AA. Desktop Tauri app — keyboard navigation for all primary workflows, sufficient contrast on the dark theme (≥4.5:1 for body text), reduced-motion support for sync animations and transitions.
