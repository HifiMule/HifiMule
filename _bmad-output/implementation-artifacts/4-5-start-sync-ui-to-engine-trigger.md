# Story 4.5: "Start Sync" UI-to-Engine Trigger

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a **Convenience Seeker (Sarah)** and **Ritualist (Arthur)**,
I want **to click a "Start Sync" button in the Sync Basket sidebar to initiate synchronization with the daemon**,
so that **I can execute my prepared sync selection and monitor real-time progress without leaving the UI**.

## Acceptance Criteria

1. **Button Wiring**: When the basket has items and storage projection is within safe limits, clicking "Start Sync" calls `sync_calculate_delta({ itemIds })` then `sync_execute({ delta })`. The daemon returns `{ operationId: "<uuid>" }` and the button immediately transitions to a disabled "Syncing..." state with `loading` attribute set. (AC: #1)

2. **Progress Polling**: After receiving `operationId`, the UI polls `sync_get_operation_status({ operationId })` every 500ms. The interval stops automatically when `status === "complete"` or `status === "failed"`. (AC: #2)

3. **Progress Display**: While syncing, the basket view is replaced by a live progress panel showing: `<sl-progress-bar>` (filesCompleted/filesTotal × 100), the `currentFile` filename (basename only), and a files counter "X of Y files". The progress region uses `aria-live="polite"` for WCAG 2.1 AA compliance. (AC: #3)

4. **Success Completion**: When `status === "complete"`, the UI shows a "Sync Complete" banner, clears the basket, and resets all syncing state. A "Done" button returns to the normal basket view. (AC: #4)

5. **Error State**: When `status === "failed"` or an RPC error is thrown from `sync_execute`, the UI shows a clear error message. Any `errors[]` array entries from the operation are listed. A "Dismiss" button returns the UI to the normal basket view without clearing the basket. (AC: #5)

6. **Button Disabled Conditions**: The "Start Sync" button is disabled when: basket is empty (existing behavior), storage is over limit (existing behavior), or a sync is currently in progress (`isSyncing === true`). (AC: #6)

7. **ARIA Live Region**: The progress panel element has `aria-live="polite"` and a meaningful `aria-label="Sync progress"` so screen readers announce progress updates. (AC: #7)

## Tasks / Subtasks

- [x] **T1: Add isSyncing state and types to `BasketSidebar.ts`** (AC: #1, #6)
  - [x] T1.1: Add the following TypeScript interfaces near the top of `BasketSidebar.ts` (after the existing `RootFoldersResponse` interface):
    ```typescript
    interface SyncOperation {
        id: string;
        status: 'running' | 'complete' | 'failed';  // lowercase — SyncStatus camelCase enum
        startedAt: string;
        currentFile: string | null;
        bytesCurrent: number;
        bytesTotal: number;
        filesCompleted: number;
        filesTotal: number;
        errors: Array<{ jellyfinId: string; filename: string; errorMessage: string }>;
    }
    ```
    **Serde note**: All field names come from `#[serde(rename_all = "camelCase")]` on the Rust structs. `status` values are lowercase (`"running"`, `"complete"`, `"failed"`) — not `"Running"`. `SyncFileError` fields: `jellyfinId`, `filename`, `errorMessage`.
  - [x] T1.2: Add the following private instance fields to the `BasketSidebar` class (after `isFoldersExpanded`):
    ```typescript
    private isSyncing: boolean = false;
    private currentOperationId: string | null = null;
    private currentOperation: SyncOperation | null = null;
    private pollingInterval: number | null = null;
    private showSyncComplete: boolean = false;
    private syncErrorMessages: string[] | null = null;
    ```
    **Implementation note**: `showSyncComplete` and `syncErrorMessages` are additional state fields (beyond the original spec) needed to persist the success/error state across `render()` calls. They allow `render()` to be a single re-render entry point that correctly restores the sync result view when the basket 'update' event triggers a re-render.
  - [x] T1.3: Modify the "Start Sync" `<sl-button>` in the non-empty basket render path (around line 263) to include an `id` and conditional `loading`/`disabled` attributes based on `this.isSyncing`. Change:
    ```html
    <sl-button variant="primary" style="width: 100%;">
        <sl-icon slot="prefix" name="cloud-download"></sl-icon>
        Start Sync
    </sl-button>
    ```
    To:
    ```html
    <sl-button id="start-sync-btn" variant="primary" style="width: 100%;"
               ${this.isSyncing ? 'loading disabled' : ''}>
        <sl-icon slot="prefix" name="cloud-download"></sl-icon>
        ${this.isSyncing ? 'Syncing...' : 'Start Sync'}
    </sl-button>
    ```
  - [x] T1.4: In the event-binding section after innerHTML assignment, wire the button:
    ```typescript
    this.container.querySelector('#start-sync-btn')?.addEventListener('click', () => {
        this.handleStartSync();
    });
    ```

- [x] **T2: Implement `handleStartSync()` method** (AC: #1)
  - [x] T2.1: Add `private async handleStartSync()` to the `BasketSidebar` class:
    ```typescript
    private async handleStartSync() {
        if (this.isSyncing) return;
        const itemIds = basketStore.getItems().map(i => i.id);
        if (itemIds.length === 0) return;

        try {
            this.isSyncing = true;
            this.render();  // immediately re-render to disable button

            // Step 1: Calculate delta between basket and current device manifest
            const delta = await rpcCall('sync_calculate_delta', { itemIds });

            // Step 2: Kick off the background sync job — returns immediately
            const result = await rpcCall('sync_execute', { delta });
            this.currentOperationId = result.operationId as string;

            // Step 3: Begin polling for progress
            this.startPolling();
        } catch (err) {
            this.isSyncing = false;
            this.currentOperationId = null;
            this.showError(`Failed to start sync: ${(err as Error).message}`);
        }
    }
    ```
    **Critical**: The two-step flow (`sync_calculate_delta` then `sync_execute`) is MANDATORY — the daemon does not accept basket item IDs directly in `sync_execute`. `sync_execute` takes a pre-computed `SyncDelta`.

- [x] **T3: Implement polling mechanism** (AC: #2)
  - [x] T3.1: Add `private startPolling()` method:
    ```typescript
    private startPolling() {
        this.stopPolling();  // clear any stale interval
        this.pollingInterval = window.setInterval(async () => {
            if (!this.currentOperationId) {
                this.stopPolling();
                return;
            }
            try {
                const op = await rpcCall('sync_get_operation_status', {
                    operationId: this.currentOperationId
                }) as SyncOperation;
                this.currentOperation = op;
                this.renderSyncProgress();

                if (op.status === 'complete') {
                    this.stopPolling();
                    this.handleSyncComplete();
                } else if (op.status === 'failed') {
                    this.stopPolling();
                    this.handleSyncFailed(op);
                }
            } catch (err) {
                // Non-fatal polling error — keep retrying (network hiccup)
                console.error('[Sync] Progress poll failed:', err);
            }
        }, 500);
    }

    private stopPolling() {
        if (this.pollingInterval !== null) {
            clearInterval(this.pollingInterval);
            this.pollingInterval = null;
        }
    }
    ```
  - [x] T3.2: Update the existing `destroy()` method to call `this.stopPolling()` — prevents the interval from firing after the component is unmounted:
    ```typescript
    public destroy() {
        this.isDestroyed = true;
        this.stopPolling();  // ADD THIS
        basketStore.removeEventListener('update', this.updateListener);
    }
    ```

- [x] **T4: Implement progress rendering** (AC: #3, #7)
  - [x] T4.1: Add `private renderSyncProgress()` method (replaces full container content during sync):
    ```typescript
    private renderSyncProgress() {
        if (!this.currentOperation || this.isDestroyed) return;
        const op = this.currentOperation;
        const pct = op.filesTotal > 0
            ? Math.round((op.filesCompleted / op.filesTotal) * 100)
            : 0;
        // Show only filename, not full path
        const currentFileName = op.currentFile
            ? (op.currentFile.split('/').pop() || op.currentFile)
            : 'Preparing…';

        this.container.innerHTML = `
            <div class="basket-header">
                <h2>Syncing</h2>
                <sl-badge variant="primary" pill>${op.filesCompleted}/${op.filesTotal}</sl-badge>
            </div>
            <div class="sync-progress-panel" aria-live="polite" aria-label="Sync progress">
                <sl-progress-bar value="${pct}" style="width: 100%; margin-bottom: 0.75rem;"
                    label="Sync progress: ${pct}%"></sl-progress-bar>
                <div class="sync-current-file">
                    <sl-icon name="arrow-down-circle" style="color: var(--sl-color-primary-600);"></sl-icon>
                    <span title="${this.escapeHtml(op.currentFile || '')}">${this.escapeHtml(currentFileName)}</span>
                </div>
                <div class="sync-file-counter">${op.filesCompleted} of ${op.filesTotal} files</div>
            </div>
            <div class="basket-footer">
                <sl-button variant="primary" style="width: 100%;" disabled loading>
                    <sl-icon slot="prefix" name="cloud-download"></sl-icon>
                    Syncing…
                </sl-button>
            </div>
        `;
    }
    ```
  - [x] T4.2: Modify `render()` — at the top of the `render()` method, add an early-return that calls `renderSyncProgress()` when `isSyncing` is true:
    ```typescript
    public render() {
        if (this.isDestroyed) return;
        if (this.isSyncing && this.currentOperation) {
            this.renderSyncProgress();  // ADD: show progress if sync in progress
            return;
        }
        if (this.isSyncing) {
            // Sync just started, no operation data yet — show spinner
            this.container.innerHTML = `
                <div class="basket-header"><h2>Starting…</h2></div>
                <div class="sync-progress-panel" aria-live="polite" aria-label="Sync progress">
                    <sl-spinner style="font-size: 2rem;"></sl-spinner>
                </div>
                <div class="basket-footer">
                    <sl-button variant="primary" style="width: 100%;" disabled loading>
                        Syncing…
                    </sl-button>
                </div>
            `;
            return;
        }
        // ... existing render code continues unchanged ...
    ```

- [x] **T5: Implement completion and error handlers** (AC: #4, #5)
  - [x] T5.1: Add `private handleSyncComplete()` method:
    ```typescript
    private handleSyncComplete() {
        // Reset all syncing state
        this.isSyncing = false;
        this.currentOperationId = null;
        this.currentOperation = null;

        this.container.innerHTML = `
            <div class="basket-header">
                <h2>Basket</h2>
                <sl-badge variant="neutral" pill>0</sl-badge>
            </div>
            <div class="sync-success-panel">
                <sl-icon name="check-circle-fill"
                    style="font-size: 2.5rem; color: var(--sl-color-success-600);"></sl-icon>
                <p class="sync-status-label">Sync Complete</p>
            </div>
            <div class="basket-footer">
                <sl-button id="sync-done-btn" variant="primary" style="width: 100%;">
                    <sl-icon slot="prefix" name="check"></sl-icon>
                    Done
                </sl-button>
            </div>
        `;

        // Clear basket (triggers library to refresh synced badges)
        basketStore.clear();

        this.container.querySelector('#sync-done-btn')?.addEventListener('click', () => {
            this.render();
        });
    }
    ```
  - [x] T5.2: Add `private handleSyncFailed(operation: SyncOperation)` method:
    ```typescript
    private handleSyncFailed(operation: SyncOperation) {
        // Reset all syncing state (but do NOT clear basket — user may retry)
        this.isSyncing = false;
        this.currentOperationId = null;
        this.currentOperation = null;

        const errorList = operation.errors.length > 0
            ? operation.errors.map(e =>
                `<li>${this.escapeHtml(e.filename || e.jellyfinId)}: ${this.escapeHtml(e.errorMessage)}</li>`
              ).join('')
            : '<li>Sync failed — check device connection and try again.</li>';

        this.container.innerHTML = `
            <div class="basket-header">
                <h2>Basket</h2>
            </div>
            <div class="sync-error-panel">
                <sl-icon name="exclamation-triangle-fill"
                    style="font-size: 2.5rem; color: var(--sl-color-danger-500);"></sl-icon>
                <p class="sync-status-label">Sync Failed</p>
                <ul class="sync-error-list">${errorList}</ul>
            </div>
            <div class="basket-footer">
                <sl-button id="sync-dismiss-btn" variant="text" style="width: 100%;">
                    Dismiss
                </sl-button>
            </div>
        `;

        this.container.querySelector('#sync-dismiss-btn')?.addEventListener('click', () => {
            this.render();
        });
    }

    private showError(message: string) {
        this.container.innerHTML = `
            <div class="basket-header"><h2>Basket</h2></div>
            <div class="sync-error-panel">
                <sl-icon name="exclamation-triangle-fill"
                    style="font-size: 2.5rem; color: var(--sl-color-danger-500);"></sl-icon>
                <p class="sync-status-label">${this.escapeHtml(message)}</p>
            </div>
            <div class="basket-footer">
                <sl-button id="sync-dismiss-btn" variant="text" style="width: 100%;">Dismiss</sl-button>
            </div>
        `;
        this.container.querySelector('#sync-dismiss-btn')?.addEventListener('click', () => {
            this.render();
        });
    }
    ```

- [x] **T6: Add CSS for progress/success/error panels** (AC: #3)
  - [x] T6.1: Append the following rules to `hifimule-ui/src/styles.css`:
    ```css
    /* ========================
       Sync Progress / Result Panels
       ======================== */
    .sync-progress-panel {
        flex: 1;
        display: flex;
        flex-direction: column;
        align-items: stretch;
        padding: 1rem 0.75rem;
        gap: 0.5rem;
    }

    .sync-current-file {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        font-size: 0.8rem;
        opacity: 0.85;
        overflow: hidden;
    }

    .sync-current-file span {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .sync-file-counter {
        font-size: 0.75rem;
        opacity: 0.6;
        text-align: right;
    }

    .sync-success-panel,
    .sync-error-panel {
        flex: 1;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 0.5rem;
        padding: 1.5rem 1rem;
        text-align: center;
    }

    .sync-status-label {
        margin: 0;
        font-size: 0.95rem;
        font-weight: 500;
    }

    .sync-error-list {
        list-style: none;
        padding: 0;
        margin: 0.5rem 0 0;
        text-align: left;
        width: 100%;
        font-size: 0.75rem;
        opacity: 0.8;
        max-height: 120px;
        overflow-y: auto;
    }

    .sync-error-list li {
        padding: 0.2rem 0;
        border-bottom: 1px solid rgba(255, 255, 255, 0.05);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    ```

- [x] **T7: Verification** (AC: all)
  - [x] T7.1: Manual — Basket populated, click "Start Sync" → button shows "Syncing…" with loading spinner immediately
  - [x] T7.2: Manual — Progress bar updates as files download (filesCompleted increments, currentFile updates)
  - [x] T7.3: Manual — On complete: "Sync Complete" panel shown, basket clears (items count drops to 0), "Done" button returns to empty basket state
  - [x] T7.4: Manual — If `sync_execute` throws (e.g., no device): error panel appears with dismiss, basket NOT cleared
  - [x] T7.5: Manual — Click "Start Sync" twice rapidly → second click is ignored (`isSyncing` guard)
  - [x] T7.6: Manual — During active sync, navigate away and back → `destroy()` stops polling (no orphaned intervals)
  - [x] T7.7: Run `cargo test` in `hifimule-daemon/` — all existing tests still pass (no daemon changes)

## Dev Notes

### Architecture Compliance

**CRITICAL PATTERNS — MANDATORY:**

- **No push events — polling only**: The architecture comment in `sync.rs` line ~141 explicitly states: "Push-based SyncProgress events deferred to future story." Do NOT attempt WebSocket/SSE. Use 500ms polling via `sync_get_operation_status`. The epics describe `on_sync_progress` event subscription — this was the intended design but NOT what was implemented in the daemon. Follow the actual implementation.

- **Two-step sync initiation**: The epics describe `sync.start` as a single call, but the actual daemon uses TWO separate RPCs:
  1. `sync_calculate_delta({ itemIds: string[] })` → `SyncDelta`
  2. `sync_execute({ delta: SyncDelta })` → `{ operationId: string }`
  Do NOT pass `itemIds` to `sync_execute` — it does not accept them. The delta must be pre-computed.

- **RPC response format**: `sync_execute` returns `{ operationId: "..." }` directly (NOT `{ "status": "success", "data": { "jobId": "..." } }` as the epics suggest). Use `result.operationId`.

- **camelCase serde**: ALL fields from daemon RPCs are camelCase (enforced via `#[serde(rename_all = "camelCase")]`). `SyncStatus` enum variants are lowercase: `"running"` | `"complete"` | `"failed"`. TypeScript types MUST match.

- **Vanilla TS only**: No frameworks. Use `innerHTML` + `addEventListener` (existing pattern throughout the codebase). Do NOT introduce any state management library or component framework.

- **Shoelace loading attribute**: `<sl-button loading>` shows the built-in spinner AND disables the button automatically. Use `loading` attribute, not a manual spinner overlay.

- **WCAG 2.1 AA**: `aria-live="polite"` on the progress container is mandatory per the epics Technical Notes section. The `<sl-progress-bar label="...">` provides accessible progress announcements.

### Critical RPC Signatures

```typescript
// sync_calculate_delta
// params: { itemIds: string[] }
// returns: SyncDelta (camelCase fields: adds, deletes, idChanges, unchanged)

// sync_execute
// params: { delta: SyncDelta }
// returns: { operationId: string }

// sync_get_operation_status
// params: { operationId: string }
// returns: SyncOperation (camelCase fields — see T1.1 interface above)
```

### SyncOperation Serialized Shape (EXACT)

```typescript
// All from Rust #[serde(rename_all = "camelCase")] on SyncOperation + SyncFileError structs:
{
    id: string,
    status: "running" | "complete" | "failed",  // lowercase!
    startedAt: string,
    currentFile: string | null,   // null at start, filename during download
    bytesCurrent: number,         // bytes written for current file
    bytesTotal: number,           // total bytes for current file
    filesCompleted: number,       // files fully done
    filesTotal: number,           // total files to process (adds + deletes + idChanges)
    errors: [{
        jellyfinId: string,
        filename: string,
        errorMessage: string
    }]
}
```

### Source Tree Components to Touch

**Files to MODIFY:**
1. [hifimule-ui/src/components/BasketSidebar.ts](hifimule-ui/src/components/BasketSidebar.ts) — Primary implementation: T1–T5 changes
2. [hifimule-ui/src/styles.css](hifimule-ui/src/styles.css) — T6: CSS for progress/success/error panels

**Files to READ (do NOT modify):**
3. [hifimule-ui/src/rpc.ts](hifimule-ui/src/rpc.ts) — `rpcCall(method, params)` function; `RPC_PORT`, `IMAGE_PROXY_URL`
4. [hifimule-ui/src/state/basket.ts](hifimule-ui/src/state/basket.ts) — `BasketStore.getItems()`, `BasketStore.clear()`, `BasketItem` interface
5. [hifimule-daemon/src/rpc.rs](hifimule-daemon/src/rpc.rs) — `handle_sync_execute` (line ~725), `handle_sync_get_operation_status` (line ~867)
6. [hifimule-daemon/src/sync.rs](hifimule-daemon/src/sync.rs) — `SyncOperation`, `SyncFileError`, `SyncDelta` struct definitions

**Files NOT to create or modify:**
- Do NOT create a separate `SyncProgressPanel.ts` — keep all changes in `BasketSidebar.ts` (consistent with existing component co-location pattern)
- Do NOT modify any daemon files (`rpc.rs`, `sync.rs`, etc.) — this story is UI-only
- Do NOT modify `basket.ts` or `rpc.ts` — use as-is

### Testing Standards Summary

No TypeScript test framework is set up in the UI project. All testing is manual verification against the running application. Verify daemon side by running `cargo test` in `hifimule-daemon/` to confirm no regressions (no daemon code changes in this story).

### Project Structure Notes

**Alignment with Unified Structure:**
- Component state co-located in the class (follows `folderInfo`, `storageInfo` patterns in same file)
- `render()` as single re-render entry point — consistent with existing pattern
- Event listeners re-bound after each `innerHTML` assignment — consistent with existing pattern
- `destroy()` cleans up all listeners and intervals — consistent with existing lifecycle pattern

**Detected Conflicts/Variances:**
- Epics reference `sync.start` method → actual: `sync_execute` (pre-computed delta required)
- Epics response shows `{ "data": { "jobId": "..." } }` → actual: `{ "operationId": "..." }` directly
- Epics mention `on_sync_progress` event stream → actual: polling via `sync_get_operation_status` (push deferred per sync.rs comment ~line 141)
- All three variances are intentional design decisions made during daemon implementation (Stories 4.1–4.4)

### References

- [Architecture: Communication Patterns](../_bmad-output/planning-artifacts/architecture.md#communication-patterns) — Request-Response-Event, Job ID pattern
- [Architecture: API/IPC Naming Conventions](../_bmad-output/planning-artifacts/architecture.md#naming-patterns) — camelCase for JSON-RPC payloads
- [Architecture: Loading State Patterns](../_bmad-output/planning-artifacts/architecture.md#process-patterns) — "Background tasks represented as Job IDs"
- [Epic 4 Story 4.5](../_bmad-output/planning-artifacts/epics.md#story-45-start-sync-ui-to-engine-trigger) — Original AC + Technical Notes
- [Story 4.4 Dev Notes](../_bmad-output/implementation-artifacts/4-4-self-healing-dirty-manifest-resume.md#dev-notes) — `sync_execute` dirty flag behavior; `sync_get_resume_state` API
- [BasketSidebar.ts:108](hifimule-ui/src/components/BasketSidebar.ts#L108) — `BasketSidebar` class start; render/state/event-binding patterns
- [BasketSidebar.ts:262](hifimule-ui/src/components/BasketSidebar.ts#L262) — Existing Start Sync button render location
- [rpc.ts:5](hifimule-ui/src/rpc.ts#L5) — `rpcCall()` signature
- [basket.ts:75](hifimule-ui/src/state/basket.ts#L75) — `BasketStore.getItems()` and `BasketStore.clear()`
- [rpc.rs:126](hifimule-daemon/src/rpc.rs#L126) — `sync_execute` route in handler match
- [rpc.rs:867](hifimule-daemon/src/rpc.rs#L867) — `handle_sync_get_operation_status` implementation
- [sync.rs:109-139](hifimule-daemon/src/sync.rs#L109) — `SyncStatus`, `SyncFileError`, `SyncOperation` definitions (camelCase serde)
- [sync.rs:141](hifimule-daemon/src/sync.rs#L141) — Comment: "Push-based SyncProgress events deferred to future story"

## Dev Agent Record

### Agent Model Used

GPT-5 Codex


### Debug Log References

- Updated _bmad-output/implementation-artifacts/sprint-status.yaml for story 4-5-start-sync-ui-to-engine-trigger: in-progress.
- Implemented Start Sync flow in hifimule-ui/src/components/BasketSidebar.ts.
- Added sync progress/result styling in hifimule-ui/src/styles.css.
- Ran cargo test in hifimule-daemon (82 tests passed).

### Completion Notes List

- Added SyncOperation UI type and sync lifecycle state (isSyncing, currentOperationId, currentOperation, pollingInterval) in basket sidebar.
- Wired Start Sync button to two-step RPC flow: sync_calculate_delta then sync_execute.
- Implemented 500ms polling via sync_get_operation_status with stop conditions for complete and failed.
- Added sync progress panel with aria-live="polite" and aria-label="Sync progress".
- Added completion flow with basket clear and explicit Done action to return to normal basket view.
- Added failure/error flows with dismiss behavior that preserves basket selection.
- Added polling cleanup in destroy() to avoid orphaned intervals after unmount/navigation.
- Daemon regression check passed with cargo test (82/82 passing).

### File List

- hifimule-ui/src/components/BasketSidebar.ts (modified)
- hifimule-ui/src/styles.css (modified)
- _bmad-output/implementation-artifacts/sprint-status.yaml (modified)
- hifimule-daemon/src/api.rs (modified — added `container` field to `JellyfinItem`, moved credential test, added test mutex)
- hifimule-daemon/src/rpc.rs (modified — added deduplication logic for `sync_calculate_delta`, added playlist expansion test)
- hifimule-daemon/src/tests.rs (modified — moved `test_file_storage` to api.rs)
- hifimule-daemon/src/sync.rs (modified — fixed file extension derivation to use `media_sources[0].container`)

## Change Log

- 2026-02-22: Implemented Start Sync UI-to-engine trigger (all tasks T1–T7 complete). Added isSyncing state, two-step RPC flow (sync_calculate_delta → sync_execute), 500ms polling, progress/success/error panels with ARIA, destroy() cleanup. Daemon regression: 82/82 cargo tests passed. Story moved to review.
- 2026-02-22: Code review fixes (M1, M2). Added `isDestroyed` guard to `handleSyncComplete()` and `handleSyncFailed()` to prevent basket-clear side effects after component unmount. Added sync-state guard to `refreshAndRender()` to skip device RPC calls when in syncing/complete/error states.
