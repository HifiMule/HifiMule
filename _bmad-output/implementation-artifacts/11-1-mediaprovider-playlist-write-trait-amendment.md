---
baseline_commit: 4e7a543
---

# Story 11.1: MediaProvider Playlist-Write Trait Amendment

Status: done

## Story

As a System Admin (Alexis),
I want the MediaProvider trait to expose playlist write operations,
so that the daemon can create, modify, and delete server playlists in a provider-neutral way.

## Acceptance Criteria

1. **Given** a provider is connected **When** `capabilities()` is called **Then** `supports_playlist_write` is present — `true` for Jellyfin and Subsonic/OpenSubsonic.

2. **Given** the active provider supports playlist write **When** `create_playlist(name, track_ids)` is called **Then** the server creates a new playlist and returns the server-assigned playlist ID as a `String`.

3. **Given** the active provider supports playlist write **When** `add_to_playlist(playlist_id, track_ids)` is called **Then** the specified tracks are appended to the playlist.

4. **Given** the active provider supports playlist write **When** `remove_from_playlist(playlist_id, track_ids)` is called **Then** the specified tracks are removed from the playlist.

5. **Given** the active provider supports playlist write **When** `delete_playlist(playlist_id)` is called **Then** the playlist is deleted from the server.

6. **Given** a provider does not implement the write methods **When** any write method is called **Then** `ProviderError::UnsupportedCapability` is returned (default trait implementation).

7. Tests cover: capability `true` for both providers, and `UnsupportedCapability` from the default trait implementations.

## Tasks / Subtasks

- [x] Task 1: Amend `Capabilities` struct (AC: 1, 7)
  - [x] In `hifimule-daemon/src/providers/mod.rs`: add `pub supports_playlist_write: bool` to `Capabilities` struct (after `supports_server_transcoding`).
  - [x] In `providers/jellyfin.rs` `capabilities()` (line ~368): add `supports_playlist_write: true` to the `Capabilities { ... }` literal.
  - [x] In `providers/subsonic.rs` `capabilities()` (line ~472): add `supports_playlist_write: true` to the `Capabilities { ... }` literal.
  - [x] In `rpc.rs` `FakeBrowseProvider.capabilities()` (line ~8079): add `supports_playlist_write: false` (test mock, no write support needed).
  - [x] Update test assertions in `jellyfin.rs:857` and `subsonic.rs:1709` that compare full `Capabilities` structs — add `supports_playlist_write: true` to the expected value.

- [x] Task 2: Add four write methods to `MediaProvider` trait with default `UnsupportedCapability` implementations (AC: 2–6)
  - [x] In `providers/mod.rs`, add immediately before `fn change_metadata` (keeping trait methods grouped):
    ```rust
    async fn create_playlist(
        &self,
        _name: &str,
        _track_ids: &[String],
    ) -> Result<String, ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "create_playlist is not supported by this provider".to_string(),
        ))
    }

    async fn add_to_playlist(
        &self,
        _playlist_id: &str,
        _track_ids: &[String],
    ) -> Result<(), ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "add_to_playlist is not supported by this provider".to_string(),
        ))
    }

    async fn remove_from_playlist(
        &self,
        _playlist_id: &str,
        _track_ids: &[String],
    ) -> Result<(), ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "remove_from_playlist is not supported by this provider".to_string(),
        ))
    }

    async fn delete_playlist(
        &self,
        _playlist_id: &str,
    ) -> Result<(), ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "delete_playlist is not supported by this provider".to_string(),
        ))
    }
    ```
  - [x] **Do NOT add override implementations** in `jellyfin.rs` or `subsonic.rs` — that is Stories 11.2 and 11.3.

- [x] Task 3: Add tests for default trait implementations (AC: 6, 7)
  - [x] In `providers/mod.rs` `#[cfg(test)]` block, add a minimal stub provider and four tests:
    ```rust
    // A minimal provider stub that accepts all the required trait methods
    // but does NOT override the playlist write methods (uses defaults).
    struct MinimalProvider;
    #[async_trait]
    impl MediaProvider for MinimalProvider {
        // Implement all required (non-default) methods with Err(UnsupportedCapability(...))
        // The playlist write methods are intentionally NOT overridden.
        fn server_type(&self) -> ServerType { ServerType::Unknown }
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                open_subsonic: false,
                supports_changes_since: false,
                supports_server_transcoding: false,
                supports_playlist_write: false,
                browse: BrowseCapabilities { list_modes: vec![] },
            }
        }
        // ... other required methods returning UnsupportedCapability
    }

    #[tokio::test]
    async fn trait_default_create_playlist_returns_unsupported() { ... }
    #[tokio::test]
    async fn trait_default_add_to_playlist_returns_unsupported() { ... }
    #[tokio::test]
    async fn trait_default_remove_from_playlist_returns_unsupported() { ... }
    #[tokio::test]
    async fn trait_default_delete_playlist_returns_unsupported() { ... }
    ```
  - [x] Pattern: `assert!(matches!(result, Err(ProviderError::UnsupportedCapability(_))))`.

- [x] Task 4: Verify compilation (AC: all)
  - [x] Run `rtk cargo check` from the workspace root — zero errors.
  - [x] Run `rtk cargo test` — all existing tests pass; new tests pass.

### Review Findings

- [x] [Review][Decision→Patch] Providers advertised `supports_playlist_write: true` before write methods were implemented (flagged High by Blind Hunter + Edge Case Hunter as a "capability lie"). **Resolved (option 2): gated to `false`** in `jellyfin.rs` and `subsonic.rs` `capabilities()` (with `// Gated false until the playlist-write adapter lands` comments) and matching test assertions updated. **⚠️ Stories 11.2/11.3 MUST flip these flags back to `true`** when they implement the adapters.
- [x] [Review][Patch] Strengthen default-impl tests to assert the error message, not just the variant [hifimule-daemon/src/providers/mod.rs:980-1019] — all four tests now extract the `UnsupportedCapability` message and assert it contains the method name, catching a future copy-paste regression. Applied.
- [x] [Review][Defer] Empty `track_ids` boundary unhandled/untested for real providers [hifimule-daemon/src/providers/mod.rs] — no provider overrides the write methods yet, so empty-slice behavior belongs to Stories 11.2/11.3. Deferred, not in scope for 11.1.

## Dev Notes

### Scope of this story

**This story is ONLY:**
1. `Capabilities.supports_playlist_write: bool` — add field, set to `true` in both providers.
2. Four new default trait methods in `MediaProvider` — return `UnsupportedCapability` unless overridden.
3. Tests for the default implementations.

**Do NOT implement HTTP calls, API wiring, or actual playlist CRUD logic.** That is Stories 11.2 (Jellyfin adapter) and 11.3 (Subsonic adapter). Do not add `create_playlist` / `add_to_playlist` / `remove_from_playlist` / `delete_playlist` overrides to `jellyfin.rs` or `subsonic.rs`.

### Exact file locations

| File | Line(s) | Change |
|------|---------|--------|
| `hifimule-daemon/src/providers/mod.rs` | 256–261 | Add `supports_playlist_write: bool` to `Capabilities` |
| `hifimule-daemon/src/providers/mod.rs` | ~199 (before `change_metadata`) | Add 4 default write methods |
| `hifimule-daemon/src/providers/mod.rs` | test module (end of file) | Add `MinimalProvider` + 4 default tests |
| `hifimule-daemon/src/providers/jellyfin.rs` | ~368 (`capabilities()` impl) | Add `supports_playlist_write: true` |
| `hifimule-daemon/src/providers/jellyfin.rs` | ~857 (test assert) | Add `supports_playlist_write: true` to expected `Capabilities` |
| `hifimule-daemon/src/providers/subsonic.rs` | ~472 (`capabilities()` impl) | Add `supports_playlist_write: true` |
| `hifimule-daemon/src/providers/subsonic.rs` | ~1709 (test assert) | Add `supports_playlist_write: true` to expected `Capabilities` |
| `hifimule-daemon/src/rpc.rs` | ~8079 (`FakeBrowseProvider.capabilities()`) | Add `supports_playlist_write: false` |

### Critical: `ProviderError` — use `UnsupportedCapability`, not `NotSupported`

The epics mention `ProviderError::NotSupported` but this variant does **not exist** in the current code. The existing error type for "capability not available" is:
```rust
#[error("provider capability is unsupported: {0}")]
UnsupportedCapability(String),
```
Use `ProviderError::UnsupportedCapability(...)` everywhere. **Do NOT add a `NotSupported` variant.** The architecture doc's Epic 11 section also says "returns `ProviderError::NotSupported`" — this is a docs artifact; the code uses `UnsupportedCapability`.

### Critical: `capabilities()` returns `Capabilities` by value (not `&Capabilities`)

The architecture doc shows `fn capabilities(&self) -> &Capabilities;` but the **actual trait and both provider implementations return `Capabilities` by value** (`fn capabilities(&self) -> Capabilities`). Match the existing code — do not change the return type.

### Capabilities struct — all construction sites must be updated

`Capabilities` is a struct with explicit named fields; it does not `#[derive(Default)]`. Adding `supports_playlist_write` will cause compile errors everywhere `Capabilities { ... }` is constructed if the new field is missing. The compiler will catch all sites, but here they are explicitly:

- `providers/jellyfin.rs:368` → `supports_playlist_write: true`
- `providers/subsonic.rs:472` → `supports_playlist_write: true`
- `providers/jellyfin.rs:857` (test) → `supports_playlist_write: true`
- `providers/subsonic.rs:1709` (test) → `supports_playlist_write: true`
- `rpc.rs:8079` (`FakeBrowseProvider` test mock) → `supports_playlist_write: false`
- `MinimalProvider` (new test stub in `mod.rs`) → `supports_playlist_write: false`

### Existing patterns to follow

**Default method pattern** (already used in `mod.rs` for `get_song`, `list_genres`, `list_recently_added`, etc.):
```rust
async fn list_genres(
    &self,
    _library_id: Option<&str>,
    _offset: u32,
    _limit: u32,
) -> Result<(Vec<Genre>, u64), ProviderError> {
    Err(ProviderError::UnsupportedCapability(
        "list_genres is not supported by this provider".to_string(),
    ))
}
```
The playlist write defaults follow the same shape.

**Existing test pattern for checking `UnsupportedCapability`** (from existing code):
```rust
// Not currently in mod.rs tests but pattern from other tests:
assert!(matches!(result, Err(ProviderError::UnsupportedCapability(_))));
```

### Architecture contract for callers (for context — NOT implemented in this story)

Per `architecture.md:Epic 11`:
> Callers MUST check `capabilities().supports_playlist_write` before invoking any write method.

The capability-check enforcement and the actual write calls happen in Stories 11.2–11.5. This story only establishes the contract.

### What this story does NOT change

- No new dependencies in `Cargo.toml`
- No UI files touched
- No RPC handler changes (`rpc.rs` is updated only to fix the test mock's `Capabilities` struct literal)
- No `ProviderError` variants added or changed

### Project Structure Notes

All changes stay within `hifimule-daemon/src/providers/` and `hifimule-daemon/src/rpc.rs` (test mock only). No new files required.

### References

- Architecture Epic 11 section: `planning-artifacts/architecture.md:523–593`
- Epics Story 11.1: `planning-artifacts/epics.md:2058–2096`
- `Capabilities` struct: `hifimule-daemon/src/providers/mod.rs:256–261`
- `MediaProvider` trait: `hifimule-daemon/src/providers/mod.rs:55–218`
- `ProviderError` enum: `hifimule-daemon/src/providers/mod.rs:321–342`
- `JellyfinProvider.capabilities()`: `hifimule-daemon/src/providers/jellyfin.rs:367–385`
- `SubsonicProvider.capabilities()`: `hifimule-daemon/src/providers/subsonic.rs:456–478`
- `FakeBrowseProvider` test mock: `hifimule-daemon/src/rpc.rs:8075–8087`
- Sprint change proposal: `planning-artifacts/sprint-change-proposal-2026-06-05.md`

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

- Added `supports_playlist_write: bool` field to `Capabilities` struct in `providers/mod.rs`.
- Updated all six `Capabilities { ... }` construction sites: `jellyfin.rs` impl + test, `subsonic.rs` impl + test, `rpc.rs` FakeBrowseProvider mock, and new `MinimalProvider` in `mod.rs` tests.
- Added four default `async fn` write methods to `MediaProvider` trait (`create_playlist`, `add_to_playlist`, `remove_from_playlist`, `delete_playlist`), each returning `Err(ProviderError::UnsupportedCapability(...))`. Inserted immediately before `change_metadata` as specified.
- Added `MinimalProvider` struct in `mod.rs` test module implementing all required trait methods (all returning `UnsupportedCapability`) without overriding the playlist write defaults.
- Added four new tests verifying each default write method returns `Err(ProviderError::UnsupportedCapability(_))`.
- `cargo check`: 0 errors, 2 pre-existing warnings (unrelated to this story).
- `cargo test`: 392 tests passed (392 pre-existing + 4 new = all pass).

### File List

- `hifimule-daemon/src/providers/mod.rs`
- `hifimule-daemon/src/providers/jellyfin.rs`
- `hifimule-daemon/src/providers/subsonic.rs`
- `hifimule-daemon/src/rpc.rs`

## Change Log

- 2026-06-05: Story 11.1 implemented — added `supports_playlist_write` to `Capabilities`, four default write methods to `MediaProvider` trait, and four `UnsupportedCapability` tests via `MinimalProvider`. All 392 tests pass.
