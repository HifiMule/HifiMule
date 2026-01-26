# Project Context: JellyfinSync

## Overview
**JellyfinSync** is a desktop application for synchronizing Jellyfin media libraries to legacy mass-storage MP3 players (DAPs, iPods with Rockbox, etc.).

## Current Status
- **Phase:** Analysis / Planning
- **State:** Greenfield
- **Key Artifacts:** 
  - [Product Brief](file:///wsl.localhost/Ubuntu/home/alexis/bmad/_bmad-output/planning-artifacts/product-brief-bmad-2026-01-26.md)

## Core Principles (from Product Brief)
1. **Managed Sync Mode:** Sync only what the tool "owns" using a `.jellysync.json` manifest.
2. **Jellyfin-First:** Connect to the server before hardware.
3. **Speed is King:** Focus on buffered streaming and server-side transcoding handshakes.
4. **Scrobble Bridge:** Future-proofing for playback history sync via Rockbox logs.

## Next Workflow
- `create-prd`: Transform the brief into detailed technical requirements.
