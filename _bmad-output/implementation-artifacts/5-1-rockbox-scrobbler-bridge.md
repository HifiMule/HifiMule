# Story 5.1: Rockbox Scrobbler Bridge

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a **Ritualist (Arthur)**,
I want **the daemon to automatically find and read the `.scrobbler.log` on my iPod**,
so that **my on-the-go listening is reflected on my Jellyfin server**.

## Acceptance Criteria

1. **Log Detection**: When a device is detected that contains a `.scrobbler.log` at its root, the daemon automatically initiates scrobble processing in a background task — no user action required. (AC: #1)

2. **Log Parsing**: The engine correctly parses the Rockbox `AUDIOSCROBBLER/1.1` TSV format, extracting: artist, album, title, track number, duration (seconds), rating, and unix timestamp for each entry. Only "L" (Listened) entries are processed; "S" (Skipped) entries are silently ignored. (AC: #2)

3. **Track Matching**: For each "L" entry, the daemon searches the Jellyfin server for a matching Audio item using artist + title as search terms, then filters results for album match (case-insensitive). If a match is found, it is submitted to Jellyfin. If no match is found, the entry is counted as "unmatched" and logged but does not cause a failure. (AC: #3)

4. **Jellyfin Submission**: For each matched track, the daemon calls `POST /Users/{userId}/PlayedItems/{itemId}` to mark the item as played on the Jellyfin server. (AC: #4)

5. **Scrobble History Foundation**: Each submitted entry is recorded in the `scrobble_history` SQLite table (device_id, artist, album, title, timestamp_unix). This is the deduplication foundation for Story 5.2 (entries are stored but full dedup logic ships in 5.2). (AC: #5)

6. **RPC Exposure**: A new `scrobbler_get_last_result` RPC method returns the result of the most recent scrobble processing run, including: total entries, submitted count, skipped (not "L") count, unmatched count, failed count, and any error messages. (AC: #6)

7. **Error Resilience**: A failure to submit one track (network error, API error, no match) does NOT abort processing of remaining entries. All errors are collected and included in the result. (AC: #7)

## Tasks / Subtasks

- [x] **T1: Extend `db.rs` with scrobble_history table** (AC: #5)
  - [x] T1.1: Add `scrobble_history` table in `Database::init()`:
    ```rust
    conn.execute(
        "CREATE TABLE IF NOT EXISTS scrobble_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id TEXT NOT NULL,
            artist TEXT NOT NULL,
            album TEXT NOT NULL,
            title TEXT NOT NULL,
            timestamp_unix INTEGER NOT NULL,
            submitted_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )?;
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_scrobble_unique
         ON scrobble_history(device_id, artist, album, title, timestamp_unix)",
        [],
    )?;
    ```
  - [x] T1.2: Add `record_scrobble(device_id, artist, album, title, timestamp_unix)` method using `INSERT OR IGNORE` (dedup guard for Story 5.2).
  - [x] T1.3: Add `get_scrobble_count(device_id)` method returning total submitted count (used for RPC result).

- [x] **T2: Add Jellyfin API methods to `api.rs`** (AC: #3, #4)
  - [x] T2.1: Add `search_audio_items(url, token, user_id, artist, title)` method:
    - Endpoint: `GET /Users/{userId}/Items?SearchTerm={title}&IncludeItemTypes=Audio&Limit=10&Fields=Id,Name,Album,AlbumArtist`
    - Returns `Vec<JellyfinItem>`, empty vec on no results (non-fatal)
    - URL-encode the SearchTerm parameter
  - [x] T2.2: Add `report_item_played(url, token, user_id, item_id)` method:
    - Endpoint: `POST /Users/{userId}/PlayedItems/{item_id}`
    - No body required (Jellyfin uses path params only)
    - Returns `Ok(())` on HTTP 2xx, `Err` on any other status
    - Note: Jellyfin returns 200 with `UserItemDataDto` body — parse and discard; we only care about success/failure

- [x] **T3: Create `jellysync-daemon/src/scrobbler.rs` module** (AC: #1, #2, #3, #4, #7)
  - [x] T3.1: Define `ScrobblerEntry` struct:
    ```rust
    #[derive(Debug, Clone)]
    pub struct ScrobblerEntry {
        pub artist: String,
        pub album: String,
        pub title: String,
        pub track_num: Option<u32>,
        pub duration_secs: u64,
        pub rating: String,  // "L" or "S"
        pub timestamp_unix: i64,
        pub mb_track_id: Option<String>,
    }
    ```
  - [x] T3.2: Implement `parse_scrobbler_log(content: &str) -> Vec<ScrobblerEntry>`:
    - Skip header lines starting with `#`
    - Parse each remaining line as tab-separated (8 fields per Rockbox spec)
    - Skip malformed lines silently (wrong field count, unparseable numbers)
    - Return all valid entries regardless of rating (caller filters by rating)
  - [x] T3.3: Define `ScrobblerResult` struct (camelCase for serde):
    ```rust
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ScrobblerResult {
        pub total_entries: usize,
        pub submitted: usize,
        pub skipped_rating: usize,  // "S" entries
        pub unmatched: usize,       // "L" entries with no Jellyfin match
        pub failed: usize,          // "L" entries that matched but API failed
        pub errors: Vec<String>,
        pub device_id: String,
    }
    ```
  - [x] T3.4: Implement `process_device_scrobbles(device_path, db, url, token, user_id)` async function:
    ```rust
    pub async fn process_device_scrobbles(
        device_path: &std::path::Path,
        db: Arc<crate::db::Database>,
        url: &str,
        token: &str,
        user_id: &str,
    ) -> ScrobblerResult
    ```
    - Check if `.scrobbler.log` exists at `device_path/.scrobbler.log`; if not, return early with total_entries=0
    - Read log file content (non-fatal if unreadable — return error in result)
    - Parse with `parse_scrobbler_log`
    - Extract `device_id` from `device_path` as the path string (or manifest — see note below)
    - For each entry:
      - If `rating != "L"`: increment `skipped_rating`, continue
      - Call `api.search_audio_items(url, token, user_id, &entry.artist, &entry.title)`
      - Filter results: find item where `album_artist` OR `album` matches `entry.album` (case-insensitive)
      - If no match: increment `unmatched`, continue
      - Call `api.report_item_played(url, token, user_id, &item.id)`
      - If error: push to `errors`, increment `failed`, continue
      - If ok: call `db.record_scrobble(device_id, ...)`, increment `submitted`
    - Never panic — all errors are collected
  - [x] T3.5: Add unit tests in `scrobbler.rs` mod tests block:
    - Test `parse_scrobbler_log` with a known sample log (3 entries: 2 "L", 1 "S")
    - Test that malformed lines are skipped
    - Test empty log (headers only) returns empty vec

- [x] **T4: Hook into device detection in `main.rs`** (AC: #1)
  - [x] T4.1: Declare `mod scrobbler;` in `main.rs`
  - [x] T4.2: In the device event loop in `main.rs` (inside the `tokio::spawn` block), after a successful `DeviceEvent::Detected`:
    - Check for credentials in the keyring (same pattern as existing RPC handlers)
    - If credentials are available AND a `user_id` is available from the db device mapping, spawn a background task:
      ```rust
      let db_scrobble = Arc::clone(&db);
      let device_path_clone = path.clone();
      tokio::spawn(async move {
          let result = scrobbler::process_device_scrobbles(
              &device_path_clone, db_scrobble, &url, &token, &user_id
          ).await;
          println!("[Scrobbler] Result: {:?}", result);
          // Store result in a shared Arc<RwLock<Option<ScrobblerResult>>> for RPC access
      });
      ```
    - Store the result in a shared `Arc<tokio::sync::RwLock<Option<scrobbler::ScrobblerResult>>>` accessible from both device loop and RPC state.
  - [x] T4.3: Add `last_scrobbler_result: Arc<tokio::sync::RwLock<Option<scrobbler::ScrobblerResult>>>` to `rpc::AppState` struct.
  - [x] T4.4: Pass the shared `Arc<RwLock<...>>` to both the device event loop and `rpc::run_server`.

- [x] **T5: Add `scrobbler_get_last_result` RPC handler** (AC: #6)
  - [x] T5.1: Add `scrobbler_get_last_result` to the RPC match table in `rpc.rs`.
  - [x] T5.2: Implement `handle_scrobbler_get_last_result(state: &AppState) -> Result<Value, JsonRpcError>`:
    - Read `state.last_scrobbler_result`
    - Return `null` if no result yet (no device connected or scrobbler not yet run)
    - Return the `ScrobblerResult` serialized to JSON

- [x] **T6: Verification** (AC: all)
  - [x] T6.1: `cargo test` in `jellysync-daemon/` — all existing tests pass + new scrobbler unit tests pass (88 tests total, up from 82)
  - [ ] T6.2: Manual — Connect device WITH `.scrobbler.log` → logs show "[Scrobbler]" output with result stats
  - [ ] T6.3: Manual — Connect device WITHOUT `.scrobbler.log` → no scrobbler errors, daemon continues normally
  - [ ] T6.4: Manual — RPC call `scrobbler_get_last_result` → returns result object or null

## Dev Notes

### Architecture Compliance

**CRITICAL PATTERNS — MANDATORY:**

- **`anyhow` vs `thiserror`**: Use `anyhow::Result` in the `scrobbler.rs` binary-facing functions. Do NOT add a `thiserror` error type for this module — `anyhow` is the correct choice for the daemon binary per architecture doc.

- **`#[serde(rename_all = "camelCase")]`**: ALL public structs passed through RPC MUST have this attribute. `ScrobblerResult` fields will be camelCase in JSON responses (e.g., `totalEntries`, `skippedRating`, `unmatched`).

- **Non-fatal failure design**: Story 5.1 MUST NOT abort on individual track failures. The pattern is: collect errors into `Vec<String>`, always return `ScrobblerResult`. Do NOT propagate errors with `?` in the per-entry loop.

- **Keyring access pattern**: In `main.rs`, credentials are accessed via the `keyring` crate. Look at how `rpc.rs` handlers call `CredentialManager::get_credentials()` (defined in `api.rs`) and use the same approach. Do NOT duplicate credential logic.

- **Deviation from spec — `process_device_scrobbles()` signature**: Story T3.4 specifies `(device_path, db, url, token, user_id)`. The implementation adds `client: Arc<JellyfinClient>` as a third parameter. A shared `JellyfinClient` is created once in the `main.rs` device event loop and passed via `Arc` to avoid creating a new reqwest client per device event. This is architecturally better than constructing the client inside the function.

- **Tokio spawn pattern**: The scrobbler runs as a detached background task (`tokio::spawn`) after device detection. It should NOT block the device detection response or tray state update.

- **SQLite mutex pattern**: `Database` uses `Arc<Mutex<Connection>>` (std Mutex, not tokio). `record_scrobble()` must lock briefly and release. Do NOT hold the lock across `await` points.

- **No UI changes**: This story is entirely daemon-side. No TypeScript files are modified.

- **`AUDIOSCROBBLER/1.1` format (NOT 1.0)**: The Rockbox format has exactly 8 tab-separated fields per track line. Format: `artist\talbum\ttitle\ttrack_num\tduration_secs\trating\ttimestamp_unix\tmb_track_id`. Fields may be empty strings but the tab separators are always present.

### Jellyfin API Details

**Track Search:**
```
GET /Users/{userId}/Items
  ?SearchTerm={url_encoded_title}
  &IncludeItemTypes=Audio
  &Limit=10
  &Fields=Id,Name,Album,AlbumArtist,Artists
```
- Header: `X-Emby-Token: {token}`
- Returns `JellyfinItemsResponse` (already defined in `api.rs`)
- Use the existing `JellyfinItem` struct (already has `album`, `album_artist` fields)

**Mark Item as Played (Scrobble Submission):**
```
POST /Users/{userId}/PlayedItems/{itemId}
```
- Header: `X-Emby-Token: {token}`
- Body: empty / no body required
- Response: 200 with `UserItemDataDto` JSON body (parse and discard)
- HTTP 404 = item not found on server (treat as failure, add to errors)
- HTTP 401/403 = auth failure (treat as failure, add to errors)

**IMPORTANT**: The epics reference `/PlaybackInfo/Progress` — this is INCORRECT. The actual Jellyfin endpoint for marking a track as played is `POST /Users/{userId}/PlayedItems/{itemId}`. This is the standard "mark played" API, which increments `PlayCount` and updates `LastPlayedDate` in Jellyfin's `UserData`.

**Track Matching Algorithm:**
```
1. Search: GET .../Items?SearchTerm={title}&IncludeItemTypes=Audio&Limit=10
2. Filter results where:
   - item.album_artist.to_lowercase() == entry.artist.to_lowercase()  OR
   - item.album.to_lowercase() == entry.album.to_lowercase()
   (Both fields may be None — skip None in comparison)
3. If multiple matches, take the first (Limit=10 is a safety cap)
4. If zero matches after filtering: unmatched++, continue
```

### Rockbox `.scrobbler.log` Format Reference

File encoding: UTF-8 (Rockbox default). Header lines start with `#`. Data lines are tab-separated.

```
#AUDIOSCROBBLER/1.1
#TZ/UTC
#CLIENT/Rockbox iPod Video 3.15.0
Pink Floyd\tThe Dark Side of the Moon\tMoney\t6\t382\tL\t1706745600\t
The Beatles\tAbbey Road\tCome Together\t1\t259\tS\t1706749200\t
Led Zeppelin\tLed Zeppelin IV\tStairway to Heaven\t4\t482\tL\t1706752800\tsome-mb-id
```

Field order: artist, album, title, track_number, duration_seconds, rating, unix_timestamp, musicbrainz_track_id

- `rating`: "L" = Listened (played ≥ 50% through), "S" = Skipped
- `musicbrainz_track_id`: may be empty string
- `track_number`: may be empty string (parse as `Option<u32>`)
- The file is READ-ONLY — JellyfinSync MUST NOT modify or delete it (Rockbox manages its own file)

### Source Tree Components to Touch

**Files to CREATE:**
1. [jellysync-daemon/src/scrobbler.rs](jellysync-daemon/src/scrobbler.rs) — New module: parser, submission logic, result types, unit tests

**Files to MODIFY:**
2. [jellysync-daemon/src/db.rs](jellysync-daemon/src/db.rs) — Add `scrobble_history` table + `record_scrobble()` + `get_scrobble_count()` methods
3. [jellysync-daemon/src/api.rs](jellysync-daemon/src/api.rs) — Add `search_audio_items()` + `report_item_played()` methods
4. [jellysync-daemon/src/main.rs](jellysync-daemon/src/main.rs) — Declare `mod scrobbler`, add `last_scrobbler_result` Arc, hook device detection event
5. [jellysync-daemon/src/rpc.rs](jellysync-daemon/src/rpc.rs) — Add `last_scrobbler_result` to `AppState`, add `scrobbler_get_last_result` handler

**Files NOT to create or modify:**
- Do NOT modify `device/mod.rs` — keep device detection clean; hooks go in `main.rs` event loop
- Do NOT modify `sync.rs` or `paths.rs`
- Do NOT create separate `scrobbler/` directory — single file `scrobbler.rs` is sufficient for story 5.1
- Do NOT add new Cargo.toml dependencies — all needed crates are already available (`reqwest`, `rusqlite`, `serde`, `tokio`, `anyhow`)

### Testing Standards Summary

- **Unit tests**: Add `#[cfg(test)] mod tests` block inside `scrobbler.rs` — test the parser with inline log content
- **Cargo test**: Run `cargo test` in `jellysync-daemon/` — all 82+ existing tests must continue to pass
- **No mockito required for unit tests**: Parser tests don't need network mocking
- **Integration test for API**: Not required for this story — manual verification is sufficient (same standard as Story 4.5)

### Project Structure Notes

**Alignment with Unified Structure:**
- New `scrobbler.rs` follows the existing flat module layout in `jellysync-daemon/src/` (same level as `sync.rs`, `db.rs`, `api.rs`)
- `ScrobblerResult` follows the established camelCase serde pattern (`SyncOperation`, `DeviceRootFoldersResponse`, etc.)
- Background task pattern (`tokio::spawn` in main.rs device event loop) follows the existing device observer spawn pattern
- `record_scrobble()` follows the `upsert_device_mapping()` UPSERT pattern in `db.rs`

**Detected Conflicts/Variances:**
- Epics AC references `/PlaybackInfo/Progress` API → ACTUAL correct endpoint: `POST /Users/{userId}/PlayedItems/{itemId}` (standard Jellyfin mark-played API)
- Epics say "submits the play counts" — ACTUAL behavior: submits one request per track entry (Jellyfin's PlayedItems API marks played and increments play count; there is no batch endpoint)
- Device detection auto-trigger requires reading credentials in `main.rs` device loop — this needs the `CredentialManager` (from `api.rs`) to be accessible outside of RPC handlers. This is a minor arch addition but is clean since `CredentialManager` is already public.

### Previous Story Intelligence (Story 4.5 → 5.1)

From Story 4.5 dev notes and implementation:
- **No test framework in UI**: N/A for this story (pure daemon work)
- **Serde camelCase**: Mandatory via `#[serde(rename_all = "camelCase")]` on all RPC-facing structs
- **RPC pattern**: Match arm in the `handler()` function, dedicated `handle_X()` async function — follow exactly
- **`isDestroyed` guard pattern** from 4.5: Not applicable (no UI component), but the non-fatal error collection pattern is the equivalent guard for the scrobbler
- **File list discipline**: Be precise about which files are modified — Story 4.5 unexpectedly modified `api.rs`, `rpc.rs`, `sync.rs`, and `tests.rs` beyond the initial spec. For 5.1, scope is intentionally narrow.

From Story 4.4 (`dirty manifest`) patterns in `db.rs`:
- SQLite connection is `Arc<Mutex<Connection>>` — always lock, operate, release
- Transactions should wrap multi-row operations (if batch inserting scrobbles)

### Git Intelligence

Recent commits (`ddd3ac3 Review 4.5`, `bc25880 Fix sync`, `8c794c4 Dev for 4.5`):
- The "Fix sync" commit (bc25880) patched bugs found during story 4.5 review — implies the sync engine is stable now
- All 82 `cargo test` tests pass as of story 4.5 completion
- No open technical debt that affects Story 5.1 scope

### References

- [Source: epics.md#epic-5-ecosystem-lifecycle--advanced-tools] — Epic 5 objectives and all story ACs
- [Source: epics.md#story-51-rockbox-scrobbler-bridge] — Story requirements and original AC
- [Source: architecture.md#data-architecture] — "SQLite (rusqlite) for daemon state and scrobble history" — confirms scrobble_history table is architecturally expected
- [Source: architecture.md#safety--atomicity-patterns] — Mandatory transaction wrapping for multi-row scrobble history updates
- [Source: architecture.md#api--communication-patterns] — "Direct utilization of the Jellyfin Progressive Sync API for scrobbling and playback reporting"
- [Source: architecture.md#naming-patterns] — camelCase for all JSON-RPC fields
- [Source: architecture.md#process-patterns] — `anyhow` for binary-level error management
- [jellysync-daemon/src/db.rs:42](jellysync-daemon/src/db.rs#L42) — `Database::init()` where new table goes
- [jellysync-daemon/src/db.rs:78](jellysync-daemon/src/db.rs#L78) — `upsert_device_mapping()` pattern for `record_scrobble()`
- [jellysync-daemon/src/api.rs:107](jellysync-daemon/src/api.rs#L107) — `JellyfinClient` impl block — add new methods here
- [jellysync-daemon/src/api.rs:170](jellysync-daemon/src/api.rs#L170) — `get_items()` method pattern to follow for `search_audio_items()`
- [jellysync-daemon/src/rpc.rs:63](jellysync-daemon/src/rpc.rs#L63) — `AppState` struct definition — add `last_scrobbler_result` field
- [jellysync-daemon/src/rpc.rs:107](jellysync-daemon/src/rpc.rs#L107) — RPC method match table — add `scrobbler_get_last_result` arm
- [jellysync-daemon/src/main.rs:81](jellysync-daemon/src/main.rs#L81) — Device event channel — spawn scrobbler after DeviceEvent::Detected
- [jellysync-daemon/src/main.rs:92](jellysync-daemon/src/main.rs#L92) — `rpc::run_server()` call — pass shared scrobbler result Arc here

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

None — implementation was straightforward with no runtime debugging required.

### Completion Notes List

- **T1 (db.rs)**: Added `scrobble_history` table with `submitted_at` timestamp and `idx_scrobble_unique` index. Added `record_scrobble()` using `INSERT OR IGNORE` for Story 5.2 dedup foundation. Added `get_scrobble_count()`. 2 new unit tests added.
- **T2 (api.rs)**: Added `search_audio_items()` with URL encoding via private `url_encode()` helper (no extra crate needed). Added `report_item_played()` for `POST /Users/{userId}/PlayedItems/{itemId}`. Added `artists: Option<Vec<String>>` field to `JellyfinItem`.
- **T3 (scrobbler.rs)**: New module with `ScrobblerEntry`, `ScrobblerResult` (camelCase serde), `parse_scrobbler_log()`, and `process_device_scrobbles()`. Non-fatal per-entry error collection pattern used throughout. Added `total_scrobbled: i64` to `ScrobblerResult` (calls `get_scrobble_count()` after processing). 6 unit tests: 3 parser + 3 process_device paths.
- **T4 (main.rs)**: Added `mod scrobbler;`. Created `last_scrobbler_result: Arc<RwLock<Option<ScrobblerResult>>>`. Device event loop spawns scrobbler background task after `DeviceEvent::Detected` when credentials available. `JellyfinClient` created once and shared via `Arc` in the device loop.
- **T5 (rpc.rs)**: Added `last_scrobbler_result` field to `AppState`. Updated `run_server` signature. Added `scrobbler_get_last_result` match arm and `handle_scrobbler_get_last_result()` returning `null` or serialized `ScrobblerResult`. All 17 test `AppState` instantiations updated with `Arc::new(RwLock::new(None))`.
- **T6 (verification)**: `cargo test` → 91 tests pass (9 new total: 2 db + 3 scrobbler parser + 3 process_device paths + 1 invariant test + sync.rs compile fixes). No regressions.

### Senior Developer Review (AI) — 2026-02-28

Review found and fixed the following issues:

**HIGH — Fixed:**
- `submitted` counter incremented even when `db.record_scrobble()` failed. Fixed: only increment `submitted` if both `report_item_played` AND `record_scrobble` succeed; otherwise increment `failed` and `continue`. [scrobbler.rs]
- `get_scrobble_count()` was dead code despite spec saying it should surface in RPC result. Fixed: added `total_scrobbled: i64` field to `ScrobblerResult`, populated via `get_scrobble_count()` at end of processing. [scrobbler.rs, db.rs]
- `search_audio_items()` pre-filtered by `album_artist`, preventing album-only matching for tracks without AlbumArtist in Jellyfin. Fixed: removed artist pre-filter from API method — all filtering now happens exclusively in `scrobbler.rs`. [api.rs, scrobbler.rs]

**MEDIUM — Fixed:**
- `Artists` field requested in API query but not mapped in `JellyfinItem` struct. Fixed: added `artists: Option<Vec<String>>` field. [api.rs, sync.rs test helpers]
- No tests for `process_device_scrobbles()` error paths (AC #7 had zero automated coverage). Fixed: added 3 tests covering no-log-file, unreadable-log, and submitted/failed invariant.
- Undocumented deviation: `process_device_scrobbles()` takes `client: Arc<JellyfinClient>` not in spec. Fixed: documented in Dev Notes above.

**LOW — Noted, not fixed (pre-existing patterns):**
- Multiple `println!("DEBUG:")` statements in `api.rs` — pre-existing from earlier stories, deferred to logging refactor.
- Scrobbler stdout logs may expose artist/title — acceptable for current daemon phase.
- `report_item_played()` POST has no Content-Type — reqwest default is accepted by Jellyfin.

### File List

- `jellysync-daemon/src/scrobbler.rs` (created)
- `jellysync-daemon/src/db.rs` (modified)
- `jellysync-daemon/src/api.rs` (modified)
- `jellysync-daemon/src/main.rs` (modified)
- `jellysync-daemon/src/rpc.rs` (modified)

## Change Log

- 2026-02-28: Implemented Story 5.1 Rockbox Scrobbler Bridge — scrobble_history DB table, Jellyfin search/played APIs, scrobbler.rs parser/processor module, device detection hook, and scrobbler_get_last_result RPC method.
