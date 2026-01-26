---
stepsCompleted: [1, 2, 3, 4, 5]
inputDocuments: []
date: 2026-01-26
author: Alexis
---

# Product Brief: JellyfinSync

## Executive Summary

The Jellyfin Sync Tool is a desktop application designed to bridge the gap between modern self-hosted media servers and legacy portable music hardware. It provides an "iTunes-like" synchronization experience for Jellyfin users, allowing them to manage their music collections on dedicated MP3 players and Digital Audio Players (DAPs) that mount as mass storage devices. By leveraging Jellyfin's rich metadata and transcoding engine, the tool automates the tedious process of selecting, re-encoding, and transferring music to mobile hardware, specifically targeting "Managed Sync" for legacy devices like iPods running Rockbox.

---

## Core Vision

### Problem Statement

Users of legacy MP3 players (such as the iPod Classic with Rockbox) or modern dedicated DAPs who use Jellyfin as their primary library manager lack a streamlined way to synchronize their music. Currently, this process requires manual file exploration, cumbersome copy-pasting between server shares and removable drives, and manual re-encoding of high-bitrate files to fit storage-constrained devices.

### Problem Impact

The manual nature of the current workflow makes it difficult to keep a portable music collection up-to-date. Users often struggle to remember what has already been copied, cannot easily sync specific Jellyfin playlists or genres, and waste significant time on manual transcoding. This friction often results in stale music libraries on dedicated hardware and a degraded listening experience.

### Why Existing Solutions Fall Short

Modern music management solutions almost exclusively focus on mobile app streaming or "offline modes" within a specific ecosystem (like Spotify or the Jellyfin mobile app). These solutions do not support dedicated hardware that lacks an internet connection or modern operating system. Existing file-sync tools are "dumb" to music metadata, meaning they can't sync based on Jellyfin playlists, artists, or genres, and don't handle intelligent audio transcoding.

### Proposed Solution

A desktop-based synchronization client that connects to a Jellyfin server and local mass storage devices. It features a "Split View" UI (Jellyfin Selection vs. Device Capacity) and implements a **Conflict-Free Managed Sync** model. The tool only manages files it "owns" (tracked via a hidden `.jellysync.json` manifest), ensuring that manually added legacy files in the same folder are never accidentally deleted. The tool initiates a "Transcoding Handshake" with Jellyfin to deliver optimized streams (e.g., 256kbps MP3) directly to the device using **Buffered Streaming** to ensure stability on slow legacy hardware.

### Key Differentiators

- **Jellyfin Native Metadata Sync:** Unlike generic folder-sync tools, it understands Jellyfin Playlists, Genres, and Artists.
- **Sync Manifest Awareness:** Uses a local manifest to track its own files, allowing for a safe hybrid storage model where user-managed and tool-managed music coexist.
- **Transcoding Handshake:** Prefers server-side transcoding via Jellyfin API to save local PC resources, with local FFmpeg as a fallback.
- **Legacy Hardware First:** Specifically optimized for "Mass Storage" and Rockbox-style devices, including **Buffered Streaming** and **Manifest Repair** utilities.
- **Scrobble Bridge (Technical Link):** The Sync Manifest acts as a translator between Rockbox's `.scrobbler.log` and Jellyfin IDs, enabling future cross-device watch status sync.

---

## Target Users

### Primary Users: The "Classic Collector"
- **Persona: Arthur**
- **Context:** Owns legacy hardware (iPod Classic/Rockbox) and a massive Jellyfin library (10,000+ songs).
- **Motivation:** Wants a focused, high-quality experience without formatting his device or losing legacy files.
- **Key Need:** A "Preserve existing files" toggle and robust support for the Rockbox database.

### Secondary Users: The "Performance Athlete"
- **Persona: Sarah**
- **Context:** Uses a Garmin watch for marathon training.
- **Motivation:** Needs a fast, one-click way to sync a "Running" playlist before leaving the house.
- **Key Need:** Credentials caching and "Auto-Sync on Connect" for rapid departures.

---

## User Journey

### 1. The Seamless Handshake
The app launches and silently reconnects to the cached Jellyfin server. The user is prompted to select their **User Profile** to ensure playlists and scrobbles are routed correctly.

### 2. The Takeover Protocol & Manifest Audit
When hardware is plugged in, the tool identifies the managed zone. It performs a **Manifest Audit**, distinguishing between tool-managed tracks and manually added media. A **Repair Utility** triggers if the manifest is corrupted, attempting to re-link files based on server IDs.

### 3. Selection & Preview
The user browses Jellyfin Playlists/Genres. A "Storage Gauger" provides real-time feedback, showing planned usage relative to total device capacity, including persistent awareness of space occupied by unmanaged user files.

### 4. Optimized Sync
The user clicks "Sync." Jellyfin transcodes the stream on-the-fly, and the tool writes it to the device via a **Memory-to-Disk Buffer**. A success message confirms the device can be safely ejected.

---

## Success Metrics

Success for JellyfinSync is defined by the speed and reliability of the data transfer. A "win" occurs when the tool significantly reduces the friction of moving music between the server and hardware compared to manual file management.

### Business Objectives
- **High Retention:** Users sync their devices weekly (or more frequently) due to the low-friction process.
- **Platform Reliability:** Zero instances of "unbootable" devices caused by failed Rockbox database updates or manifest corruption.
- **Jellyfin Ecosystem Value:** Strengthening Jellyfin as the primary source of truth for all user music, including offline hardware play.

### Key Performance Indicators
- **Time-to-Action:** Under 5 seconds from device connection to being ready to sync.
- **Incremental Sync Performance:** Under 10 seconds for updates where 90%+ of media is already present on the device.
- **Sync Completion Rate:** 99.9% of transfers complete without error, with clear logging/retries for any failures.
- **Transcoding Efficiency:** Transcoding and Bitstream transfer occurs faster than real-time playback speed for a standard album.

---

## MVP Scope

### Core Features
- **Profile Selection:** Multi-user support to sync against correct Jellyfin accounts.
- **Metadata Browsing:** Split-view browser for Jellyfin Playlists, Genres, and Artists.
- **Mass Storage Detection:** Automated detection of mounted USB/removable drives.
- **Conflict-Free Managed Sync:** Implementation of the `.jellysync.json` manifest with **Repair Utility** for corrupted states.
- **Direct Buffered Transfer:** Streaming files from server to device storage via memory buffer to mitigate slow USB/HDD speeds.
- **Playback Log Discovery:** Foundational logic to detect Rockbox playback logs during sync to support future scrobbling/smart playlists.

### Out of Scope for MVP
- **Transcoding (Re-encoding):** Deferred to a later version; initial version assumes compatible formats are available or handles direct copy only.
- **MTP Support:** Initial focus is strictly on Mass Storage devices.
- **Complex Re-organization:** Automatic folder nesting customization beyond standard Artist/Album structures.

### MVP Success Criteria
- **User Validation:** At least 5 "Arthur" or "Sarah" style users successfully complete a sync without data loss.
- **Technical Stability:** 0% failure rate for manifest-managed file operations.

### Future Vision
- **Smart Playlists (Log Sync):** Utilizing discovered playback logs to update Jellyfin "Played" status and create dynamic on-device collections. Research confirms that Jellyfin's `/Progress` API and Rockbox's `.scrobbler.log` can be linked using our manifest's `itemId` mapping.
- **Intelligent Re-encoding:** Automating the "Transcoding Handshake" to optimize storage on-the-fly.
- **Wi-Fi Sync:** Wireless synchronization for supported hardware.
