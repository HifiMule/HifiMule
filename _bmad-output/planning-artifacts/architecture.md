stepsCompleted: ['step-01-init', 'step-02-context', 'step-03-starter', 'step-04-decisions', 'step-05-patterns', 'step-06-structure', 'step-07-validation', 'step-08-complete']
workflowType: 'architecture'
status: 'complete'
completedAt: '2026-01-26'
---

# Architecture Decision Document

## Project Context Analysis

### Requirements Overview

**Functional Requirements:**
JellyfinSync requires a robust synchronization core capable of differential manifest management and direct Rockbox log processing. The architecture must support asynchronous device discovery and a detachable communication layer for the UI.

**Non-Functional Requirements:**
Architecture is driven by extreme efficiency (< 10MB RAM) and high stability (Atomic IO). Cross-platform parity is mandatory, requiring abstraction layers for Windows/Linux/macOS filesystem and notification primitives.

**Scale & Complexity:**
- Primary domain: Desktop Utility (Rust)
- Complexity level: Medium
- Estimated architectural components: 4 (Sync Core, Mount Dispatcher, Scrobble Processor, UI Bridge)

### Technical Constraints & Dependencies
- **No Heavy Runtimes:** The core engine cannot depend on Electron or Python runtimes if it is to meet the 10MB memory goal.
- **OS Native IO:** Dependence on `udev` (Linux), `WM_DEVICECHANGE` (Windows), and `DiskArbitration` (macOS) for event-driven discovery.
- **Manifest-Only Truth:** The `.jellysync.json` file on the target device is the definitive record of "Managed State".

## Starter Template Evaluation

### Primary Technology Domain
Desktop Utility (Rust Daemon + Tauri UI)

### Starter Options Considered
- **Standard Tauri v2 App:** Single-process; harder to manage a persistent background daemon that lives after the window closes.
- **Pure egui/Slint:** Leanest (~5-8MB), but UI development is more rigid and requires more boilerplate for media browsing.
- **[SELECTED] Tauri + Sidecar Workspace:** A Rust Workspace with two members: `jellysync-daemon` (engine) and `jellysync-ui` (Tauri).

### Selected Starter: Custom Tauri Sidecar Workspace

**Rationale for Selection:**
Isolates the sub-10MB headless engine from the active UI runtime. Allows for a rich media-browsing interface using web technologies without compromising the idle performance of the sync daemon.

**Initialization Command:**
```bash
# Workspace setup for multi-process isolation
cargo new jellysync-daemon --bin
npx create-tauri-app@latest jellysync-ui --template vanilla-ts
```

**Architectural Decisions Provided by Foundation:**
- **Language:** Rust 1.75+ (Crates: `tokio` for async daemon, `serde` for serialization).
- **Frontend:** Vanilla TypeScript for the detachable selection UI.
- **Build Tooling:** Cargo Workspace for multi-process coordination.
- **IPC Pattern:** JSON-RPC over Localhost (HTTP) or OS-native Named Pipes.

## Core Architectural Decisions

### Decision Priority Analysis

**Critical Decisions (Block Implementation):**
- **Architecture Style:** Detached Multi-Process (Rust Daemon + Tauri UI).
- **IPC Mechanism:** JSON-RPC over Localhost (HTTP).
- **Secure Storage:** `keyring` crate for OS-native credential management.

**Important Decisions (Shape Architecture):**
- **Data Persistence:** SQLite (`rusqlite`) for daemon state and scrobble history.
- **Async Runtime:** `tokio` for handling concurrent IO and mount events.

### Data Architecture
- **Daemon State:** Managed via a local SQLite database to ensure atomic scrobble commits and robust history tracking.
- **UI Preferences:** Stored in standard JSON configuration files for ease of access from the Tauri frontend.

### Authentication & Security
- **Credential Management:** All Jellyfin tokens are stored in the OS-native secure vault (Windows Credential Manager, macOS Keychain, Linux Secret Service) using the `keyring` crate.
- **Process Isolation:** The UI and Daemon communicate over a restricted local loopback, minimizing system exposure.

### API & Communication Patterns
- **Internal IPC:** JSON-RPC 2.0 protocol implemented over a local HTTP server within the daemon.
- **External API:** Direct utilization of the Jellyfin Progressive Sync API for scrobbling and playback reporting.

### Frontend Architecture
- **UI Type:** Webview-based via Tauri v2.
- **State Management:** Local selection state managed within the webview, synchronized with the daemon manifest via RPC.

## Implementation Patterns & Consistency Rules

### Pattern Categories Defined

**Critical Conflict Points Identified:**
3 areas where AI agents could make different choices (Naming, IPC, Safety).

### Naming Patterns

**Database Naming Conventions:**
- Tables: `snake_case` plural (e.g., `sync_history`, `devices`).
- Columns: `snake_case` (e.g., `play_count`, `last_synced_at`).

**API/IPC Naming Conventions:**
- **External Payload:** `camelCase` for all JSON-RPC fields (e.g., `syncProgress`, `deviceId`).
- **Automated Enforcement:** Use `ts-rs` or equivalent to generate TypeScript interfaces directly from Rust structs with a mandatory `#[serde(rename_all = "camelCase")]` policy.

**Code Naming Conventions:**
- **Rust (Daemon):** Standard `snake_case` for variables/functions.
- **TypeScript (UI):** Standard `camelCase` for variables/functions.

### Structure Patterns

**Project Organization:**
- Rust Workspace with crates: `jellysync-daemon` (engine) and `jellysync-ui` (Tauri).
- **Core Logic:** Extracted into a local `jellysync-core` library crate shared between binary crates if needed.
- **Tests:** Co-located in mod `tests` blocks (Rust) or `*.test.ts` (TypeScript).

### Format Patterns

**API Response Formats:**
- Wrap results in a success/fail envelope: `{ "status": "success", "data": { ... } }` or `{ "status": "error", "message": "...", "code": 102 }`.

### Communication Patterns

**Event System Patterns:**
- **Pattern:** Request-Response-Event.
- The UI requests a "Sync start"; the Daemon returns an immediate "OK" and broadcasts progress via an `on_sync_progress` event stream.

### Process Patterns

**Error Handling Patterns:**
- **Rust Internal:** `thiserror` crate for typed library errors.
- **Rust Top-level:** `anyhow` for binary-level error management.

**Loading State Patterns:**
- Background tasks (Syncing/Discovery) are represented as "Job IDs" in the state, allowing the UI to re-attach to long-running tasks.

### Safety & Atomicity Patterns
- **Atomic Manifest Commitment:** Utilize the "Write-Temp-Rename" pattern for all `.jellysync.json` updates to prevent state corruption during disconnection.
- **Database Consistency:** Mandatory Transaction wrapping for all multi-row scrobble history updates.

### Enforcement Guidelines

**All AI Agents MUST:**
- Use the provided `ts-rs` macros to ensure the IPC contract is strictly adhered to.
- Validate filesystem path lengths before attempting write operations on legacy hardware.
- Commit manifest changes ONLY after `sync_all` has returned successfully.
