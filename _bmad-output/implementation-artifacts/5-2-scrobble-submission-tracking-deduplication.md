# Story 5.2: Scrobble Submission Tracking (Deduplication)

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a **System Admin (Alexis)**,
I want the engine to check whether each `.scrobbler.log` entry has already been submitted before calling the Jellyfin API,
so that reconnecting the same device multiple times never creates duplicate play-count entries on the server.

## Acceptance Criteria

1. **Pre-submission dedup check**: For every "L"-rated entry in `.scrobbler.log`, the engine checks `scrobble_history` (keyed on `device_id + artist + album + title + timestamp_unix`) **before** calling `search_audio_items()` or `report_item_played()`. If the record exists, the entry is skipped — no Jellyfin API call is made. (AC: #1)

2. **`skipped_duplicate` counter**: Each skipped-due-to-dedup entry increments a new `skipped_duplicate` counter in `ScrobblerResult`. The field appears as `skippedDuplicate` in the RPC JSON response. (AC: #2)

3. **Accounting invariant**: After processing, `submitted + skipped_rating + skipped_duplicate + unmatched + failed == total_entries` for every run. (AC: #3)

4. **No regression on new entries**: Entries that are NOT already in `scrobble_history` continue to be processed and submitted exactly as in Story 5.1 — dedup check is purely additive and non-destructive. (AC: #4)

5. **DB check failure is non-fatal**: If `is_scrobble_recorded()` returns an error (DB lock failure, etc.), the entry is processed normally (treated as "not yet recorded"). The error is logged but does not abort processing or increment `failed`. (AC: #5)

## Tasks / Subtasks

- [x] **T1: Add `is_scrobble_recorded()` to `db.rs`** (AC: #1, #5)
  - [x] T1.1: Add method to `impl Database`:
    ```rust
    pub fn is_scrobble_recorded(
        &self,
        device_id: &str,
        artist: &str,
        album: &str,
        title: &str,
        timestamp_unix: i64,
    ) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM scrobble_history
             WHERE device_id=?1 AND artist=?2 AND album=?3 AND title=?4 AND timestamp_unix=?5",
            params![device_id, artist, album, title, timestamp_unix],
            |row| row.get(0),
        ).map_err(|e| anyhow!("Failed to check scrobble record: {}", e))?;
        Ok(count > 0)
    }
    ```
    - Uses the same `idx_scrobble_unique` index created in Story 5.1 — the query hits the index directly (no full table scan).
    - Lock, query, release — do NOT hold lock across `await` (none here; this is sync).
  - [x] T1.2: Add unit tests in `db.rs` mod tests:
    - `test_is_scrobble_recorded_false`: call `is_scrobble_recorded()` on an empty DB → returns `false`.
    - `test_is_scrobble_recorded_true`: call `record_scrobble()` then `is_scrobble_recorded()` with the same params → returns `true`.
    - `test_is_scrobble_recorded_different_timestamp`: same track but different `timestamp_unix` → returns `false` (distinct listen events are not deduped against each other).

- [x] **T2: Add `skipped_duplicate` to `ScrobblerResult` in `scrobbler.rs`** (AC: #2, #3)
  - [x] T2.1: Add field to the struct:
    ```rust
    pub skipped_duplicate: usize,
    ```
    Place after `skipped_rating` for logical grouping. Serde `rename_all = "camelCase"` gives JSON key `skippedDuplicate` automatically.
  - [x] T2.2: Initialize to `0` in **all three** early-return `ScrobblerResult` construction sites:
    - The "no `.scrobbler.log`" early return.
    - The "failed to read `.scrobbler.log`" early return.
    - The final return at the bottom of the processing loop.
  - [x] T2.3: Update the existing `test_scrobbler_result_submitted_excludes_db_failures` invariant test:
    - Add `skipped_duplicate: 0` to the struct literal.
    - Update the assertion to: `submitted + skipped_rating + skipped_duplicate + unmatched + failed == total_entries`.

- [x] **T3: Implement dedup pre-check in `process_device_scrobbles()`** (AC: #1, #4, #5)
  - [x] T3.1: Add `let mut skipped_duplicate = 0usize;` alongside the existing counters.
  - [x] T3.2: Insert the dedup check **immediately after** the `entry.rating != "L"` gate and **before** `client.search_audio_items()`:
    ```rust
    // Dedup check — skip if already submitted (Story 5.2)
    match db.is_scrobble_recorded(&device_id, &entry.artist, &entry.album, &entry.title, entry.timestamp_unix) {
        Ok(true) => {
            println!("[Scrobbler] Skipping duplicate: '{}' by '{}'", entry.title, entry.artist);
            skipped_duplicate += 1;
            continue;
        }
        Ok(false) => {}
        Err(e) => {
            // Non-fatal: log and proceed with submission attempt
            println!("[Scrobbler] Warning: dedup check failed for '{}': {} — will attempt submission", entry.title, e);
        }
    }
    ```
  - [x] T3.3: Include `skipped_duplicate` in the final `ScrobblerResult { ... }` construction.

- [x] **T4: Add unit test for dedup behavior in `scrobbler.rs`** (AC: #1, #2, #3)
  - [x] T4.1: Add `test_process_device_skips_already_scrobbled` test:
    - Create a temp dir with a `.scrobbler.log` containing 2 "L" entries + 1 "S" entry (use `SAMPLE_LOG` constant already defined in the file).
    - Write the sample log to `temp_dir.path().join(".scrobbler.log")`.
    - Create a `Database::memory()` and pre-populate `scrobble_history` with both "L" entries (Pink Floyd "Money" and Led Zeppelin "Stairway to Heaven").
    - Call `process_device_scrobbles(temp_dir.path(), db, Arc::new(JellyfinClient::new()), "http://localhost:8096", "token-placeholder", "user-placeholder").await`.
    - Since the only "L" entries are already in the DB, the function exits before making any HTTP calls (no reqwest connection attempted → no panic on connection refused).
    - Assert: `total_entries == 3`, `skipped_duplicate == 2`, `skipped_rating == 1`, `submitted == 0`, `unmatched == 0`, `failed == 0`.
    - This test is **network-free** because the dedup check short-circuits all Jellyfin API calls.

- [x] **T5: Verification** (AC: all)
  - [x] T5.1: `cargo test` in `jellyfinsync-daemon/` — all existing 91 tests pass + 4 new tests pass (T1.2 ×3, T4.1 ×1). Result: 95 tests passed.
  - [ ] T5.2: Manual — reconnect the same iPod twice. Second connection: `scrobbler_get_last_result` returns `skippedDuplicate > 0` and `submitted == 0`. No new play-count entries added on Jellyfin server.

## Dev Notes

### Architecture Compliance

**CRITICAL PATTERNS — MANDATORY:**

- **`is_scrobble_recorded()` is synchronous**: It takes a standard Mutex lock on `Arc<Mutex<Connection>>`, queries, and releases. This is exactly the pattern used by `record_scrobble()` and `get_scrobble_count()` in the same file. Do NOT add `async` to this method.

- **No new Cargo.toml dependencies**: All needed crates are available (`rusqlite`, `anyhow`, `params!`). This story adds zero new dependencies.

- **Non-fatal DB error on dedup check (AC #5)**: If `is_scrobble_recorded()` returns `Err`, the correct behavior is to proceed with the submission attempt — not to skip or fail the entry. Rationale: a DB error means we can't confirm whether it was scrobbled. Better to risk a duplicate than to silently drop a valid track. Increment NO counter for this case; just log a warning. The subsequent `record_scrobble()` call will fail or succeed independently.

- **`skipped_duplicate` position in the processing loop**: The dedup check goes AFTER the `entry.rating != "L"` check. "S" entries are still skipped first (no need to DB-query entries we'd skip anyway). Order: `rating != "L"` → `is_scrobble_recorded` → `search_audio_items` → `filter` → `report_item_played` → `record_scrobble`.

- **No changes to `record_scrobble()`**: Story 5.1 already uses `INSERT OR IGNORE` as a safety net. Story 5.2's pre-check adds the behavioral dedup (prevents API calls). Both layers remain in place — the DB-level IGNORE is the last-resort safety net for any race conditions or code paths that bypass the pre-check.

- **No `rpc.rs` or `main.rs` changes required**: `ScrobblerResult` is serialized directly. Adding `skipped_duplicate: usize` to the struct automatically adds `skippedDuplicate` to the JSON output. No handler changes needed.

- **No UI changes**: This story is entirely daemon-side. No TypeScript files are modified.

- **`anyhow::Result` throughout**: `is_scrobble_recorded()` returns `Result<bool>` using `anyhow::Result`. Consistent with all other `db.rs` methods.

### SQLite Query Pattern Reference

The `is_scrobble_recorded()` query hits the existing unique index from Story 5.1:
```sql
-- Index from Story 5.1:
CREATE UNIQUE INDEX IF NOT EXISTS idx_scrobble_unique
ON scrobble_history(device_id, artist, album, title, timestamp_unix)
```

The dedup check query:
```sql
SELECT COUNT(*) FROM scrobble_history
WHERE device_id=?1 AND artist=?2 AND album=?3 AND title=?4 AND timestamp_unix=?5
```

This is an index-backed point lookup — O(log n) at worst. For typical scrobbler logs (50–500 entries), this is negligible overhead per entry. No optimization needed.

### RPC Response Change

After Story 5.2, `scrobbler_get_last_result` will return a new field:

```json
{
  "totalEntries": 100,
  "submitted": 5,
  "skippedRating": 10,
  "skippedDuplicate": 83,    ← NEW in Story 5.2
  "unmatched": 1,
  "failed": 1,
  "errors": [],
  "deviceId": "/Volumes/IPOD",
  "totalScrobbled": 88
}
```

This is an additive, non-breaking change. Any existing client that calls `scrobbler_get_last_result` simply ignores the new field. There are no TypeScript callers yet for this RPC method (it was added in Story 5.1 as a foundation for future UI use).

### Source Tree Components to Touch

**Files to MODIFY:**
1. [jellyfinsync-daemon/src/db.rs](jellyfinsync-daemon/src/db.rs) — Add `is_scrobble_recorded()` method + 3 unit tests
2. [jellyfinsync-daemon/src/scrobbler.rs](jellyfinsync-daemon/src/scrobbler.rs) — Add `skipped_duplicate` field to `ScrobblerResult`, add dedup check in `process_device_scrobbles()`, update invariant test, add dedup scenario test

**Files NOT to modify:**
- `main.rs`, `rpc.rs`, `api.rs`, `sync.rs`, `paths.rs` — no changes needed
- Any TypeScript / frontend files — no changes needed
- `Cargo.toml` — no new dependencies

### Testing Standards Summary

- **All new db tests**: Synchronous `#[test]` (not `#[tokio::test]`) — `is_scrobble_recorded()` is sync. Use `Database::memory()`.
- **New scrobbler test** (`test_process_device_skips_already_scrobbled`): Async `#[tokio::test]`. Uses `Database::memory()` + `JellyfinClient::new()`. Network-safe: all "L" entries pre-populated → zero API calls made.
- **Cargo test target**: `cargo test` in `jellyfinsync-daemon/` — all 91 existing tests + 4 new = 95 total.
- **No mockito required**: The dedup test verifies the short-circuit behavior; it never reaches network code.

### Project Structure Notes

**Alignment with Unified Structure:**
- `is_scrobble_recorded()` follows the established `db.rs` pattern: `pub fn method_name(&self, ...) -> Result<T>`, lock `conn`, query, return.
- `skipped_duplicate` field follows the `ScrobblerResult` pattern: snake_case Rust field, camelCase JSON via serde derive.
- The dedup check fits naturally in the existing processing loop at [jellyfinsync-daemon/src/scrobbler.rs:128](jellyfinsync-daemon/src/scrobbler.rs#L128).

**Detected Conflicts/Variances:**
- None. Story 5.2 is a clean extension of Story 5.1 with no architectural conflicts. The `idx_scrobble_unique` index and `INSERT OR IGNORE` pattern were designed specifically to support this story.

### Previous Story Intelligence (Story 5.1 → 5.2)

From Story 5.1 dev notes and completion record:
- **Foundation already laid**: `scrobble_history` table exists with the exact unique index needed for O(log n) dedup lookups. `record_scrobble()` already uses `INSERT OR IGNORE`. Story 5.2 is completing the design intent.
- **Test count baseline**: 91 tests pass as of Story 5.1 review completion (e6ffb91 "Review 5.1"). Story 5.2 adds 4, targeting 95 total.
- **`ScrobblerResult` camelCase serde**: `skipped_duplicate` (Rust) → `skippedDuplicate` (JSON). Matches the established pattern (`total_entries` → `totalEntries`, `skipped_rating` → `skippedRating`).
- **Non-fatal error design from Story 5.1**: The existing pattern is: collect errors into `Vec<String>`, never abort on individual entry failures. Story 5.2 adds a non-fatal DB warning path (proceed on check error) that is consistent with this philosophy.
- **Existing invariant test will break**: `test_scrobbler_result_submitted_excludes_db_failures` at [jellyfinsync-daemon/src/scrobbler.rs:339](jellyfinsync-daemon/src/scrobbler.rs#L339) constructs `ScrobblerResult` without `skipped_duplicate`. Adding the field will cause a compile error if not updated. **T2.3 handles this explicitly.**
- **`Database::memory()` is `#[cfg(test)]` only**: Correct — no changes to visibility needed. All new tests use it.

### Git Intelligence

Recent commits:
- `e6ffb91 Review 5.1` — Fixed: submitted counter bug, dead code, artist pre-filter, and 3 missing test paths. Source files are in final reviewed state.
- `6b48e53 Code 5.1` — Initial Story 5.1 implementation. `scrobble_history` table and `record_scrobble()` were introduced here.

Uncommitted changes in working tree (`git status`):
- `jellyfinsync-daemon/src/api.rs`, `scrobbler.rs`, `sync.rs` — modified. These are the Story 5.1 review fixes (not yet committed at story start). Do NOT revert any of these changes.
- `_bmad-output/implementation-artifacts/5-1-rockbox-scrobbler-bridge.md` — story doc updated with review notes.

No open technical debt affecting Story 5.2 scope.

### References

- [Source: epics.md#story-52-scrobble-submission-tracking-deduplication] — Story requirements and original AC
- [Source: epics.md#epic-5-ecosystem-lifecycle--advanced-tools] — Epic 5 objectives
- [Source: architecture.md#data-architecture] — SQLite (rusqlite) for daemon state and scrobble history — confirms `scrobble_history` is the persistence layer
- [Source: architecture.md#safety--atomicity-patterns] — Transaction wrapping patterns (not needed for this story's single-row lookups, but relevant context)
- [jellyfinsync-daemon/src/db.rs:97](jellyfinsync-daemon/src/db.rs#L97) — `record_scrobble()` method — `is_scrobble_recorded()` goes directly below it
- [jellyfinsync-daemon/src/db.rs:115](jellyfinsync-daemon/src/db.rs#L115) — `get_scrobble_count()` — pattern reference for `is_scrobble_recorded()`
- [jellyfinsync-daemon/src/db.rs:68](jellyfinsync-daemon/src/db.rs#L68) — `idx_scrobble_unique` index definition — used by the dedup query
- [jellyfinsync-daemon/src/scrobbler.rs:19](jellyfinsync-daemon/src/scrobbler.rs#L19) — `ScrobblerResult` struct — add `skipped_duplicate` field here
- [jellyfinsync-daemon/src/scrobbler.rs:122](jellyfinsync-daemon/src/scrobbler.rs#L122) — Counter initialization block — add `let mut skipped_duplicate = 0usize;` here
- [jellyfinsync-daemon/src/scrobbler.rs:128](jellyfinsync-daemon/src/scrobbler.rs#L128) — Top of the entry processing loop — dedup check goes after line 131 (the `rating != "L"` gate)
- [jellyfinsync-daemon/src/scrobbler.rs:214](jellyfinsync-daemon/src/scrobbler.rs#L214) — Final `ScrobblerResult { ... }` construction — add `skipped_duplicate` field here
- [jellyfinsync-daemon/src/scrobbler.rs:339](jellyfinsync-daemon/src/scrobbler.rs#L339) — `test_scrobbler_result_submitted_excludes_db_failures` — update struct literal and assertion

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

None — clean implementation with no debug iterations required.

### Completion Notes List

- **T1**: Added `is_scrobble_recorded()` to `db.rs` after `record_scrobble()`. Synchronous method using same `Arc<Mutex<Connection>>` lock pattern. Hits `idx_scrobble_unique` index directly via 5-param WHERE clause. Returns `Result<bool>` via `anyhow`.
- **T1 tests**: Added 3 unit tests: `test_is_scrobble_recorded_false` (empty DB → false), `test_is_scrobble_recorded_true` (record then check → true), `test_is_scrobble_recorded_different_timestamp` (same track, different timestamp → false).
- **T2**: Added `pub skipped_duplicate: usize` to `ScrobblerResult` after `skipped_rating`. Serde `rename_all = "camelCase"` yields `skippedDuplicate` in JSON automatically. Added `skipped_duplicate: 0` to both early-return sites and final result construction. Updated invariant test to include field in struct literal and sum assertion.
- **T3**: Added `let mut skipped_duplicate = 0usize;` counter. Inserted dedup pre-check block after `rating != "L"` gate, before `search_audio_items()`. `Ok(true)` → log + increment + `continue`. `Ok(false)` → fall through. `Err(e)` → log warning, proceed with submission (non-fatal, AC #5 compliant). `skipped_duplicate` included in final `ScrobblerResult`.
- **T4**: Added `test_process_device_skips_already_scrobbled` — async, uses `Database::memory()` pre-populated with both "L" entries from `SAMPLE_LOG`. Network-free: dedup check short-circuits before any `reqwest` calls. Asserts: `total_entries=3, skipped_duplicate=2, skipped_rating=1, submitted=0, unmatched=0, failed=0`.
- **T5**: `cargo test` — 95 passed (91 prior + 3 db tests + 1 scrobbler test). 0 failures, 0 regressions.

### File List

- jellyfinsync-daemon/src/db.rs
- jellyfinsync-daemon/src/scrobbler.rs

## Senior Developer Review (AI)

**Reviewer:** Alexis (AI) — 2026-02-28
**Outcome:** Approved with fixes applied

### Findings Fixed

**[MEDIUM] M1 — Missing `errors.is_empty()` assertion in dedup test** (`scrobbler.rs`)
Added `assert!(result.errors.is_empty())` to `test_process_device_skips_already_scrobbled`. Inconsistency with `test_process_device_no_log_file` which already asserted this.

**[MEDIUM] M2 — AC #5 (non-fatal dedup error path) had no test coverage** (`db.rs`, `scrobbler.rs`)
Added `#[cfg(test)] drop_scrobble_table_for_test()` to `Database` and new test `test_process_device_dedup_error_is_nonfatal` that drops `scrobble_history` to trigger `Err(e)` in the dedup match, then asserts `skipped_duplicate == 0` and `failed == 2` (entries attempted normal submission). 96 total tests now pass.

**[MEDIUM] M3 — Dead code in `test_process_device_unreadable_log`** (`scrobbler.rs`)
Removed `temp_dir` creation and `.scrobbler.log` write (lines 329–331) that were never used — the test already used `bad_dir` as its path. Cleaned up misleading comments.

**[MEDIUM] M4 — `record_scrobble` failure after successful API call was a silent dedup blind spot** (`scrobbler.rs`)
Improved error message in the `record_scrobble` failure branch to explicitly state: *"track was submitted to Jellyfin but will not be deduplicated on next sync"*. Updated the invariant test's error string to match.

### Low-Severity Notes (Not Fixed — Design Decisions)

- **L1**: `COUNT(*)` vs `EXISTS` in `is_scrobble_recorded` — `EXISTS` would be marginally cleaner but SQLite's query planner handles both identically on a unique index at typical log sizes.
- **L2**: T5.2 manual test (`[ ]`) — physical device reconnect test; outside automated test scope. Story marked done since all ACs are verified by automated tests.
- **L3**: `device_id` as raw path string — pre-existing architecture decision; path normalization is a future concern.
- **L4**: `println!` logging — pre-existing systemic pattern.

## Change Log

- 2026-02-28: Story 5.2 implemented — added `is_scrobble_recorded()` to `db.rs`, `skipped_duplicate` field to `ScrobblerResult`, dedup pre-check in `process_device_scrobbles()`, 4 new tests (95 total). Status: review.
- 2026-02-28: Code review — 4 medium issues fixed: errors assertion added to dedup test, AC#5 non-fatal path now tested, dead code removed from unreadable-log test, record_scrobble error message clarified. 96 total tests pass. Status: done.
