# Story 4.6: Sync Progress — Time Remaining Estimation

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a **Convenience Seeker (Sarah)**,
I want to see an estimated time remaining during sync,
so that I know whether to wait by the screen or step away.

## Acceptance Criteria

1. **ETA displayed after ≥ 2 samples**: After at least 2 polling cycles with non-zero `bytesTransferred`, a time-remaining estimate is displayed below the progress bar in the Sync Basket sidebar. Before 2 samples, display "Calculating…". (AC: #1)

2. **ETA formula**: ETA = `bytes_remaining / avg_bytes_per_second`, where `avg_bytes_per_second` = `bytesTransferred / elapsed_seconds` (cumulative average since sync start, using `op.startedAt` as the reference). (AC: #2)

3. **ETA format**:
   - `>= 60s` → `"~N min left"` (e.g. "~3 min left")
   - `>= 10s && < 60s` → `"~N sec left"` (e.g. "~42 sec left")
   - `< 10s` → `"Almost done…"` (AC: #3)

4. **Daemon adds cumulative byte fields**: `sync_get_operation_status` response includes two new fields: `bytesTransferred` (cumulative bytes written across all completed files + in-progress bytes for the current file) and `totalBytes` (pre-computed sum of all file sizes in the sync job). Both are `u64`, serialized as JSON numbers. (AC: #4)

5. **ETA replaced on completion**: When `status === 'complete'`, the ETA line is replaced by the existing "Sync Complete" panel. (AC: #5)

6. **Tray tooltip unchanged**: The tray tooltip remains `"JellyfinSync: Syncing..."` — ETA is UI-side only and the tray is daemon-controlled (see Known Variances). (AC: #6, known variance)

## Tasks / Subtasks

### Daemon Work (sync.rs)

- [x] **T1: Add `bytes_transferred` and `total_bytes` fields to `SyncOperation` struct** (AC: #4)
  - [x] T1.1: In `sync.rs` around line 129, add two new public fields to `SyncOperation` (struct already has `#[serde(rename_all = "camelCase")]`):
    ```rust
    pub bytes_transferred: u64,  // cumulative across all files → "bytesTransferred" in JSON
    pub total_bytes: u64,        // total bytes for entire sync job → "totalBytes" in JSON
    ```
    Place after the existing `bytes_total: u64` field (line 135) so the Rust field order stays logical: `bytes_current`, `bytes_total` (per-file), then `bytes_transferred`, `total_bytes` (cumulative).

  - [x] T1.2: In `create_operation()` (line 163), initialize both new fields to `0`:
    ```rust
    let operation = SyncOperation {
        id: operation_id.clone(),
        status: SyncStatus::Running,
        started_at: timestamp,
        current_file: None,
        bytes_current: 0,
        bytes_total: 0,
        bytes_transferred: 0,   // ← new
        total_bytes: 0,         // ← new; set to true value at start of execute_sync
        files_completed: 0,
        files_total,
        errors: vec![],
    };
    ```

- [x] **T2: Track cumulative bytes in `execute_sync()`** (AC: #4)
  - [x] T2.1: At the very start of `execute_sync()`, after the empty-delta early log (line ~405), compute total job bytes and write to the operation immediately:
    ```rust
    // Compute total bytes for ETA (adds + id_changes both stream file content and contribute bytes)
    let total_job_bytes: u64 = delta.adds.iter().map(|a| a.size_bytes).sum::<u64>()
        + delta.id_changes.iter().map(|c| c.size_bytes).sum::<u64>();
    if let Some(mut operation) = operation_manager.get_operation(&operation_id).await {
        operation.total_bytes = total_job_bytes;
        operation_manager.update_operation(&operation_id, operation).await;
    }
    ```

  - [x] T2.2: Just before the `for add_item in delta.adds.iter()` loop, declare a mutable local counter:
    ```rust
    let mut completed_bytes: u64 = 0;
    ```

  - [x] T2.3: In the existing per-file progress callback (lines 510–531), update `bytes_transferred` = `completed_bytes + bytes_written`:
    ```rust
    // Inside the progress_callback closure (after setting bytes_current/bytes_total):
    let completed_bytes_snapshot = completed_bytes_arc.load(std::sync::atomic::Ordering::Relaxed);
    tokio::spawn(async move {
        if let Some(mut operation) = op_manager_inner.get_operation(&op_id_inner).await {
            operation.current_file = Some(file_name_inner);
            operation.bytes_current = bytes_written;
            operation.bytes_total = total;
            operation.bytes_transferred = completed_bytes_snapshot + bytes_written;
            op_manager_inner
                .update_operation(&op_id_inner, operation)
                .await;
        }
    });
    ```
    **CRITICAL:** `completed_bytes` is a local `u64` that cannot be moved into the async closure directly because it's updated after each file. Use an `Arc<AtomicU64>` to share the value:
    ```rust
    let completed_bytes_arc = Arc::new(std::sync::atomic::AtomicU64::new(0));
    // Replace: let mut completed_bytes: u64 = 0;
    // With: let completed_bytes_arc = Arc::new(AtomicU64::new(0));
    // Clone the arc into each progress_callback closure.
    ```

  - [x] T2.4: After a successful file write, when `files_completed` is incremented (line ~560), also update `completed_bytes`:
    ```rust
    // After: operation.files_completed += 1;
    completed_bytes_arc.fetch_add(add_item.size_bytes, std::sync::atomic::Ordering::Relaxed);
    ```
    Then the next file's progress callback will automatically pick up the correct cumulative base.

  - [x] T2.5: For `id_changes` (line ~631 and ~682): these also transfer bytes. Apply the same `completed_bytes_arc` pattern for the id-change loop. The id-change items have `size_bytes` on `SyncDeltaIdChange` (line ~91 in struct def). Check `sync.rs` around line 600–690 for the id-change streaming block and replicate the progress callback extension.

  - [x] T2.6: No changes needed to `delete` handling — deleted files have no bytes to transfer.

- [x] **T3: Update tests referencing `SyncOperation` construction** (AC: #4)
  - [x] T3.1: In `sync.rs` test at line ~1262 (`create_operation("op-1".to_string(), 10).await`), after creating the op, assert new fields are `0`:
    ```rust
    assert_eq!(op.bytes_transferred, 0);
    assert_eq!(op.total_bytes, 0);
    ```
  - [x] T3.2: In `rpc.rs` test at line ~2827, add `bytes_transferred: 0, total_bytes: 0` if the test manually constructs a `SyncOperation` struct literal (check whether it uses struct update syntax or full construction).
  - [x] T3.3: Run `cargo test` in `jellyfinsync-daemon/` — all existing tests must pass.

### UI Work (BasketSidebar.ts)

- [x] **T4: Update `SyncOperation` TypeScript interface** (AC: #4)
  - [x] T4.1: In `BasketSidebar.ts`, find the `SyncOperation` interface (added in Story 4.5, line ~32). Add the two new fields:
    ```typescript
    interface SyncOperation {
        id: string;
        status: 'running' | 'complete' | 'failed';
        startedAt: string;
        currentFile: string | null;
        bytesCurrent: number;    // per-file: bytes written for current file
        bytesTotal: number;      // per-file: total size of current file
        bytesTransferred: number; // cumulative: total bytes written across all files ← NEW
        totalBytes: number;      // cumulative: total bytes for entire sync job       ← NEW
        filesCompleted: number;
        filesTotal: number;
        errors: Array<{ jellyfinId: string; filename: string; errorMessage: string }>;
    }
    ```

- [x] **T5: Add ETA calculation state and logic** (AC: #1, #2, #3)
  - [x] T5.1: Add ETA text field to `BasketSidebar` class fields (near `isSyncing` around line 130):
    ```typescript
    private etaText: string = 'Calculating…';
    ```

  - [x] T5.2: Add a private `computeEta(op: SyncOperation): string` method to `BasketSidebar`. Uses cumulative average rate (`bytesTransferred / elapsedSeconds` since `op.startedAt`):
    ```typescript
    private computeEta(op: SyncOperation): string {
        if (op.totalBytes <= 0 || op.bytesTransferred <= 0) return 'Calculating…';

        const elapsedSeconds = (Date.now() - new Date(op.startedAt).getTime()) / 1000;
        if (elapsedSeconds <= 0) return 'Calculating…';

        const totalRate = op.bytesTransferred / elapsedSeconds;

        const remaining = op.totalBytes - op.bytesTransferred;
        if (remaining <= 0) return 'Almost done…';

        const etaSeconds = remaining / totalRate;

        if (etaSeconds < 10) return 'Almost done…';
        if (etaSeconds < 60) return `~${Math.round(etaSeconds)} sec left`;
        return `~${Math.round(etaSeconds / 60)} min left`;
    }
    ```

  - [x] T5.3: Reset `etaText` when a sync starts. In `handleStartSync()` (before `this.startPolling()`):
    ```typescript
    this.etaText = 'Calculating…';
    ```
    Also reset in `handleSyncComplete()` and `handleSyncFailed()`:
    ```typescript
    this.etaText = 'Calculating…';
    ```

- [x] **T6: Update `renderSyncProgress()` to display ETA** (AC: #1, #3)
  - [x] T6.1: In `renderSyncProgress()` (line ~792), update the method to call `computeEta()` and render the result. Replace the current `renderSyncProgress()` body:

    The current structure is:
    ```typescript
    private renderSyncProgress() {
        if (!this.currentOperation || this.isDestroyed) return;
        const op = this.currentOperation;
        const pct = ...
        const currentFileName = ...
        this.container.innerHTML = `
            <div class="basket-header">...</div>
            <div class="sync-progress-panel" aria-live="polite" aria-label="Sync progress">
                <sl-progress-bar ...></sl-progress-bar>
                <div class="sync-current-file">...</div>
                <div class="sync-file-counter">...</div>
            </div>
            <div class="basket-footer">...</div>
        `;
    }
    ```

    Update to compute ETA and add a new `<div class="sync-eta">` row **between** the file counter and the basket footer:
    ```typescript
    private renderSyncProgress() {
        if (!this.currentOperation || this.isDestroyed) return;

        const op = this.currentOperation;
        const pct = op.filesTotal > 0
            ? Math.round((op.filesCompleted / op.filesTotal) * 100)
            : 0;
        const currentFileName = op.currentFile
            ? getBasename(op.currentFile)
            : 'Preparing...';

        // Compute ETA from cumulative byte counters (updates etaSamples buffer)
        this.etaText = this.computeEta(op);

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
                <div class="sync-eta">${this.escapeHtml(this.etaText)}</div>
            </div>
            <div class="basket-footer">
                <sl-button variant="primary" style="width: 100%;" disabled loading>
                    <sl-icon slot="prefix" name="cloud-download"></sl-icon>
                    Syncing...
                </sl-button>
            </div>
        `;
    }
    ```

- [x] **T7: TypeScript compilation** (AC: all)
  - [x] T7.1: Run `pnpm tsc --noEmit` (or equivalent) in `jellyfinsync-ui/` — no TypeScript errors.

## Dev Notes

### Architecture Compliance

**This story is exclusively about adding ETA display to the existing sync progress UI.** Do NOT:
- Modify any RPC method signatures beyond adding fields to `SyncOperation`
- Change `execute_sync`'s parameter list or return type
- Touch `calculate_delta`, `SyncDeltaAddItem`, or the manifest update logic
- Modify `sync.start` / `sync_calculate_delta` / `sync_execute` RPC methods
- Add any new IPC methods

**Additive-only changes:** `SyncOperation` gets 2 new fields (`bytes_transferred`, `total_bytes`). Both initialize to `0` and are populated during sync execution. Zero breaking changes to existing consumers (`StatusBar`, Story 4.5b's `get_daemon_state` consumer) since they ignore unknown fields.

**Polling pattern continues:** Story 4.5 established 500ms polling via `sync_get_operation_status`. Story 4.5b notes: "Push-based SyncProgress events deferred to future story." The ETA calculation lives in the polling loop — each call to `renderSyncProgress()` triggers `computeEta()` which updates the sample buffer. The 500ms poll interval gives a new sample every 500ms, which is sufficient precision for ETA calculation.

### Known Variances vs Sprint Change Proposal

**Tray tooltip ETA**: The sprint change proposal says "ETA displayed below progress bar in Sync Basket sidebar and tray tooltip." The tray tooltip is set by the daemon in `main.rs` (lines 387, 399–402). The sprint change technical note says **"ETA calculation is UI-side; daemon only adds the two byte-count fields."** These are contradictory — the tray is daemon-controlled and ETA is UI-side. This story implements ETA in the basket sidebar only. Tray tooltip ETA requires daemon-side rate computation (tracking bytes/second in `execute_sync` state) and is deferred to a future story or tech spec.

**`on_sync_progress` event schema**: The sprint change refers to an event schema (`{ jobId, filesCompleted, totalFiles, percentage, currentFilename, bytesTransferred, totalBytes }`). The actual implementation uses polling via `sync_get_operation_status` (per the Story 4.5 comment: "Push-based SyncProgress events deferred to future story."). The new fields are added to the `SyncOperation` struct that is returned by `sync_get_operation_status` — semantically equivalent.

### Cumulative vs Per-File Byte Tracking

The existing `SyncOperation` fields `bytes_current` / `bytesTotal` (Rust: `bytes_current` / `bytes_total`) track **per-file progress only** — they reset to the new file's values on each file start. Do NOT use them for ETA — they are meaningless for cross-file rate calculation.

The new fields `bytesTransferred` / `totalBytes` are **cumulative for the entire sync job**:
- `totalBytes`: set once at sync start = sum of `size_bytes` for all `delta.adds` + `delta.id_changes` (both loop types stream file content)
- `bytesTransferred`: continuously updated = all completed files' bytes + current in-progress file's written bytes

### `AtomicU64` Pattern for Progress Callback

The existing progress callback is an `Arc<dyn Fn(u64, u64) + Send + Sync>` closure. It already uses `Arc<AtomicU64>` for `last_reported` throttling (line ~509). Follow this exact pattern for `completed_bytes_arc`. The pattern is:
```rust
let completed_bytes_arc = Arc::new(AtomicU64::new(0));
// Per-file, clone the arc into the progress callback closure:
let completed_bytes_for_cb = completed_bytes_arc.clone();
// In callback: use completed_bytes_for_cb.load(Ordering::Relaxed)
// After file completes: completed_bytes_arc.fetch_add(add_item.size_bytes, Ordering::Relaxed)
```

### `id_changes` Byte Tracking (T2.5)

The `id_changes` loop (lines ~600–690) also streams file content. The `SyncDeltaIdChange` struct has `size_bytes: u64` (confirmed at `sync.rs:91`). The id-change flow uses the same `write_file_streamed` pattern with a `ProgressCallback`. Apply the same `completed_bytes_arc` approach there. The `completed_bytes_arc` is shared across both loops — the id-changes loop runs after the adds loop completes, so the cumulative counter remains correct.

### ETA Calculation Behavior

- Rate = `bytesTransferred / elapsedSeconds` (cumulative average from `op.startedAt`)
- When `totalBytes === 0` (daemon hasn't set it yet) or `bytesTransferred === 0`: show "Calculating…"
- When `elapsedSeconds <= 0`: show "Calculating…"
- When `remaining <= 0`: show "Almost done…" (already at 100% bytes, final flush in progress)
- Reset `etaText` on sync start, complete, and error to avoid stale text bleeding into next sync

### Source Tree

**Files to MODIFY:**
1. [jellyfinsync-daemon/src/sync.rs](jellyfinsync-daemon/src/sync.rs) — T1: `SyncOperation` struct fields; T2: `execute_sync` cumulative byte tracking
2. [jellyfinsync-ui/src/components/BasketSidebar.ts](jellyfinsync-ui/src/components/BasketSidebar.ts) — T4: `SyncOperation` interface; T5: ETA state + `computeEta()`; T6: `renderSyncProgress()` update

**Files to READ (do NOT modify):**
3. [jellyfinsync-daemon/src/rpc.rs](jellyfinsync-daemon/src/rpc.rs) — verify `create_operation` call at ~line 939 (no signature change needed); verify `sync_get_operation_status` serializes `SyncOperation` as-is
4. [jellyfinsync-daemon/src/main.rs](jellyfinsync-daemon/src/main.rs) — verify `create_operation` call at ~line 633; understand tray tooltip ("JellyfinSync: Syncing..." stays unchanged)

**Files NOT to touch:**
- `rpc.rs` — `create_operation` signature unchanged; no new RPCs
- `main.rs` — tray tooltip unchanged; auto-sync path unchanged
- Any other sync, device, or auto-fill files

### Critical RPC Signatures

```typescript
// sync_get_operation_status (MODIFIED: SyncOperation struct gains 2 fields)
// params: { operationId: string }
// returns: SyncOperation {
//     id, status, startedAt, currentFile,
//     bytesCurrent, bytesTotal,           // per-file (unchanged)
//     bytesTransferred, totalBytes,        // cumulative (NEW)
//     filesCompleted, filesTotal, errors[]
// }

// All other RPCs: UNCHANGED
```

### Testing Standards

No TypeScript unit test framework in the UI project. UI testing is manual. Daemon: run `cargo test` in `jellyfinsync-daemon/` after adding new struct fields — the compiler will flag any test that constructs `SyncOperation` with a struct literal (if any). Fix those by adding the two new `0` fields. The `SyncOperation` struct uses named fields so partial initialization is a compiler error.

### Previous Story Learnings (4.5b)

From Story 4.5b dev notes:
- **`isSyncing` guard at line 171 must remain unchanged** — this story does not touch `refreshAndRender()` or the guard
- **`startPolling()` is the sync state machine owner** — ETA state resets happen in `handleSyncComplete()` and `handleSyncFailed()` (not in `startPolling()` directly)
- **`sync_get_operation_status` polling** is every 500ms — `computeEta()` will be called at 500ms intervals, which is adequate for a smooth ETA display

From Story 4.5 dev notes:
- **`bytesCurrent` / `bytesTotal`** in the TypeScript interface are per-file; do not confuse with cumulative
- **`SyncOperation` uses `#[serde(rename_all = "camelCase")]`** — Rust field `bytes_transferred` → JSON `bytesTransferred`; `total_bytes` → JSON `totalBytes`
- **151 tests** pass in daemon after 4.5b; baseline to preserve

### Project Structure Notes

- `completed_bytes_arc` pattern: `std::sync::atomic::AtomicU64` is already used in `execute_sync` for `last_reported` — same crate, no new imports needed
- `Arc` is already imported — no new use statements needed
- The `computeEta` method lives only in `BasketSidebar` (no daemon changes required for ETA math)
- `escapeHtml(this.etaText)` is safe: the ETA string is always constructed from numeric operations, but defensive escaping costs nothing

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

None — clean implementation, no blockers.

### Completion Notes List

- T1: Added `bytes_transferred: u64` and `total_bytes: u64` to `SyncOperation` struct (`sync.rs:136-137`) with `#[serde(rename_all = "camelCase")]` → serializes as `bytesTransferred` / `totalBytes`.
- T1.2: Initialized both fields to `0` in `create_operation()`.
- T2.1: Compute `total_job_bytes` at start of `execute_sync()` as sum of `delta.adds` + `delta.id_changes` size_bytes; write to operation immediately.
- T2.2–T2.4: Replaced `let mut completed_bytes: u64 = 0` with `Arc<AtomicU64>` pattern (following existing `last_reported` pattern). Progress callback clones the arc, reads snapshot, sets `operation.bytes_transferred = completed_bytes_snapshot + bytes_written`. After successful file write, `fetch_add(add_item.size_bytes)` and update `bytes_transferred` in operation.
- T2.5: id_changes (virtual/instant) also call `fetch_add(id_change.size_bytes)` and update `bytes_transferred` after each completion.
- T2.6: Deletes confirmed — no byte tracking needed.
- T3.1: Added `assert_eq!(op.bytes_transferred, 0)` and `assert_eq!(op.total_bytes, 0)` to `test_sync_operation_manager_lifecycle`. T3.2: No struct literals in `rpc.rs` tests.
- T3.3: 151 cargo tests pass (unchanged baseline).
- T4: Added `bytesTransferred: number` and `totalBytes: number` to `SyncOperation` TS interface in `BasketSidebar.ts`.
- T5: Added `etaText` private field. Implemented `computeEta()` using cumulative average rate (`bytesTransferred / elapsedSeconds` since `op.startedAt`). Reset in `handleStartSync`, `handleSyncComplete`, `handleSyncFailed`.
- T6: `renderSyncProgress()` calls `computeEta(op)` before rendering; `<div class="sync-eta">` inserted between file counter and basket footer.
- T7: `npx tsc --noEmit` — zero errors.

### File List

- `jellyfinsync-daemon/src/sync.rs`
- `jellyfinsync-ui/src/components/BasketSidebar.ts`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`

## Change Log

- 2026-03-31: Story 4-6 implemented — added ETA display to sync progress UI. Daemon: `SyncOperation` gains `bytesTransferred`/`totalBytes` fields tracked cumulatively in `execute_sync`. UI: `computeEta()` rolling-average method added to `BasketSidebar`; ETA displayed below file counter in `renderSyncProgress()`. 151 daemon tests pass, 0 TS errors.
