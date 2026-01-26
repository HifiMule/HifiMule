stepsCompleted: ['step-01-init', 'step-02-discovery', 'step-03-core-experience', 'step-04-emotional-response', 'step-05-inspiration', 'step-06-design-system', 'step-07-defining-experience', 'step-08-visual-foundation', 'step-09-design-directions', 'step-10-user-journeys', 'step-11-component-strategy', 'step-12-ux-patterns', 'step-13-responsive-accessibility', 'step-14-complete']
inputDocuments: ['prd.md', 'architecture.md', 'product-brief-bmad-2026-01-26.md', 'project-context.md']
status: 'complete'
completedAt: '2026-01-27'

# UX Design Specification: JellyfinSync

**Author:** Alexis
**Date:** 2026-01-26

---

## 1. Executive Summary

### 1.1 Project Vision
To create an "Invisible Sync" experience that automates the synchronization between modern media servers and legacy hardware, removing the friction of manual file management while providing a premium, modern selection interface.

### 1.2 Target Personas
*   **The Ritualist (Arthur):** Needs high transparency and "Managed Zone" isolation to protect his manual file structure.
*   **The Sprinter (Sarah):** Needs zero friction and "One-Click Dash" syncs to get out the door fast.
*   **The System Admin (Alexis):** Needs a low-footprint background daemon that remains under 10MB RAM.

---

## 2. Core User Experience

### 2.1 The Defining Experience: "The Delta-Sync Handshake"
The core interaction is the moment a legacy device is connected. The system immediately performs a differential scan and presents a "Live Delta" in the Selection Basket, allowing for a one-click commitment to synchronize.

### 2.2 User Mental Model
Users perceive their legacy hardware as a **physical extension of their Jellyfin library**. They expect server-level metadata (playlists, album art) to be "pushed" to the device without manually managing folder hierarchies.

### 2.3 Success Criteria
*   **Predictive Syncing:** Automatic rule-matching for known devices.
*   **Managed Transparency:** Visual proof that "Personal" files are isolated and safe.
*   **Silent Scrobbling:** Zero-touch processing of Rockbox `.scrobbler.log` files.

### 2.4 Emotional Design Goals
*   **Subtle Reliability:** The tool should feel like a premium, invisible utility.
*   **Calm Assurance:** Eliminating "Sync Anxiety" with clear pre-sync diffs and "Safe to Eject" confirmations.

---

## 3. Design System & Visual Foundation

### 3.1 Design System: Shoelace + Custom Tokens
We utilize **Shoelace (Web Components)** for its extreme performance in Tauri v2 and its framework-agnostic stability.

### 3.2 Visual Theme: "Vibrant Hub"
*   **Primary Palette:** `#52348B` (Jellyfin Purple), `#EBB334` (Amber Gold), `#1A1A2E` (Midnight Surface).
*   **Aesthetic:** Glassmorphism overlays with rich album art grids.
*   **Typography:** **Outfit** (Brand/Headers) and **Inter** (Data/Paths).

---

## 4. Interaction Design & Layout

### 4.1 Chosen Layout: "Basket Centric"
A 70/30 split layout where the **Library Browser** (Left) allows for immersive curation, while the **Selection Basket Sidebar** (Right) provides a detailed, high-confidence overview of the sync delta and storage projections.

### 4.2 User Journey Flow (Sarah's Dash)
```mermaid
graph TD
    A[Sarah Plugs in Garmin] --> B[Daemon Detects Managed Device]
    B --> C[Tray Pulse: 'Ready to Update Run Mix']
    C --> D[Sarah Clicks Commit]
    D --> E[Daemon Performs Background IO]
    E --> F[OS Bubble: Safe to Eject]
```

---

## 5. Component Strategy

### 5.1 Foundation Components
*   **Library Grid:** Uses Shoelace `<sl-card>` with custom aspect-ratio tokens for album art.
*   **Navigation:** Vertical sidebar using `<sl-tree>` for folder exploration and `<sl-tab-group>` for views.

### 5.2 Custom Components
*   **The Sync Basket:** A real-time "Staging Area" component that calculates literal disk bytes based on transcoding rules.
*   **The Media Delta Overlay:** A visual overlay for album covers showing `(+) Add`, `(-) Remove`, or `(=) Synced` status.

---

## 6. Responsive Design & Accessibility

### 6.1 Responsive Strategy
JellyfinSync utilizes a **"Detachable Sidebar"** strategy. The UI remains fully functional even when shrunk to a compact utility state, ensuring users can monitor sync progress without sacrificing screen real estate.

### 6.2 Breakpoint Strategy
*   **Narrow (< 600px):** Compact list-view for rapid library scanning.
*   **Standard (600px - 1000px):** Full Basket-Centric split layout.
*   **Wide (> 1000px):** Enhanced data-density view for power users.

### 6.3 Accessibility Strategy
*   **Compliance:** Target WCAG 2.1 Level AA.
*   **Visibility:** High-contrast focus states for keyboard-only navigation.
*   **Semantics:** ARIA-live regions for background sync status updates.

### 6.4 Testing Strategy
*   **Visual Regression:** Testing "Vibrant Hub" aesthetics against diverse OS themes.
*   **A11y Audits:** Automated Lighthouse/Axe verification within the Tauri environment.
