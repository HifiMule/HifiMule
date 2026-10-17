# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

The web UI runs inside a Tauri desktop shell on Windows, macOS, and Linux. This is a desktop application with a Rust background daemon, not a mobile-native interface.

## Users

Self-hosting enthusiasts who manage their own music, audiobook, and podcast libraries through Jellyfin, Subsonic-compatible servers such as Navidrome, or Audiobookshelf. They listen on their computer and move selected media onto portable devices, including DAPs, Rockbox-patched iPods, USB players, MTP phones, and Garmin watches. Music listeners care about audio quality, including lossless sources. Device preparation is deliberate, with high stakes for library integrity and storage; desktop listening also supports ongoing use. They expect precise control and trust the tool to handle their collection without surprises.

## Product Purpose

HifiMule connects self-hosted media libraries to desktop listening and portable-device playback. Users browse, play, queue, and select music, audiobooks, and podcasts, then sync their selection to a device. Reliable, user-controlled transfers remain the core promise: delta sync, resume, scrobbling, and manifest tracking reduce manual reconciliation. Success means dependable desktop playback and a completed sync whose device contents match the user's choices.

## Positioning

A desktop bridge between self-hosted media servers and portable hardware, combining listening, selection, and device preparation. Provider-specific browsing, device profiles, compatibility-aware transcoding, and tracked transfers support collections across both modern devices and legacy players. Users own the selection and automation rules.

## Operating Context

- Connect and switch between media servers; basket items retain their source server.
- Browse provider-specific libraries, listen on the computer, preview tracks, and manage the playback queue. Music playlist editing is supported on Jellyfin and Subsonic/Navidrome.
- Connect a device, initialize it when needed, configure its folders and transcoding profile, then prepare a manual selection or user-configured Auto-Fill policy.
- Inspect sync changes and storage, transfer with cancellation and resume support, and use repair, prune, or relink tools when necessary. Optional auto-sync starts when a device connects.
- A background daemon handles playback, providers, devices, and sync; the Tauri window can detach. Rockbox playback logs can report listening history back to the server.

## Capabilities and Constraints

- Supported sources include Jellyfin, Subsonic/OpenSubsonic, and Audiobookshelf Books and Podcasts libraries. Browsing modes and operations depend on the provider and media role.
- Device transports include mass storage and MTP. Profiles account for device-compatible formats, path lengths, and filename restrictions; transcoding is applied when needed.
- Music, Books, and Podcasts have distinct folder and Auto-Fill policies. Auto-Fill can use budgets, ordering, filters, quality preferences, and discovery mechanics chosen by the user.
- Delta transfers and a compressed `.hifimule.json.zst` manifest track device contents. Quality-aware replacement, resumable transfers, and explicit destructive operations must remain understandable to the user.
- Server credentials are stored in a local, machine-bound encrypted vault. The UI communicates with the daemon over authenticated loopback JSON-RPC.
- The interface supports English, French, Spanish, and German.

## Evidence on Hand

- `README.md`: current product capabilities, supported providers and devices, workflows, and architecture.
- `docs/index.md`: project documentation map; `docs/audiobookshelf-implementation.md`: Books and Podcasts behavior and boundaries.
- `docs/sync-performance.md`: synchronization controls and constraints.
- `docs/images/`: screenshots of connection, library, device settings, sync, and playback workflows; these illustrate captured states and are not proof that every current screen is unchanged.
- `hifimule-ui/src/assets/logo.png`: existing product logo.
- `hifimule-ui/` and `hifimule-daemon/`: implementation evidence. Do not invent customer endorsements, adoption figures, or performance benchmarks.

## Brand Personality

Quiet, precise, trustworthy. The UI should feel like a well-made piece of hardware: no noise, no persuasion, just function. Confidence comes from density and correctness, not from decoration.

## Anti-references

- **Apple iTunes** — the corporate music-management paradigm: glossy, paternalistic, feature-bloated, locked to an ecosystem. HifiMule should feel like the opposite: open, lean, technically honest.
- Streaming-app aesthetics (Spotify, Tidal) — designed to sell subscriptions, not manage a personal library. Avoid hero-image discovery UI, algorithmic nudges, or anything that suggests the user doesn't already know what they want.
- SaaS dashboard clichés — metric cards, gradient heroes, "enterprise-grade" chrome.

## Design Principles

1. **The library is the center of gravity.** Every screen decision should reduce friction between the user and their media collection. Controls earn their place; decoration doesn't.
2. **Trust through precision.** State is always visible: what's synced, what's pending, what failed. The UI never hides errors or softens bad news.
3. **Power without ceremony.** Advanced operations (repair, init, profiles) should be reachable without a tutorial, but not in the way during a routine sync.
4. **Visual restraint is respect.** The user chose a self-hosted, open-source stack because they don't want software making choices for them. The UI should follow the same philosophy.
5. **Dark is the natural habitat.** Audiophiles and DAP users interact with devices in dim, focused contexts. Dark mode isn't a preference — it's the primary environment.

## Accessibility & Inclusion

WCAG 2.1 AA. Desktop Tauri app — keyboard navigation for all primary workflows, sufficient contrast on the dark theme (≥4.5:1 for body text), reduced-motion support for sync animations and transitions.
