# Story 2.1: Secure Jellyfin Server Link

Status: done

## Story

As a System Admin (Alexis),
I want to securely store my Jellyfin URL and credentials in the OS-native keyring,
so that I don't have to re-enter them and my tokens are safe from other users.

## Acceptance Criteria

1. **Jellyfin Connection Validation**:
   - The daemon must provide an IPC method to test connection to a Jellyfin server.
   - Validation is performed by calling the `/System/Info` endpoint.
   - Successful call (HTTP 200) with a valid JSON response confirms connection.
2. **Secure Credential Storage**:
   - Jellyfin URL and Token must be stored in a local file `credentials.json` (Fallback for V1).
   - Service Name: `jellyfinsync` (internal reference)
   - Keys: `server_url`, `server_token`
   - **Note**: Keyring storage is postponed to V2.
3. **IPC Exposure**:
   - The daemon must expose JSON-RPC 2.0 methods over HTTP (port 19140 by default, or as configured).
   - Methods:
     - `test_connection(url: String, token: String)` -> Returns success/error.
     - `save_credentials(url: String, token: String)` -> Persists to keyring and returns success.
     - `get_credentials()` -> Retrieves from keyring (safely) and returns to UI.
4. **Error Handling**:
   - Graceful errors for invalid URLs, unreachable servers, or invalid tokens.
   - Keyring access errors should be reported to the user.

## Tasks / Subtasks

- [x] Implement Jellyfin API Client (AC: 1)
  - [x] Add `reqwest` and `serde` logic for `/System/Info` call
- [x] Implement File Credential Storage (Fallback V1) (AC: 2)
  - [x] Use `serde_json` to read/write `credentials.json`
  - [x] Implement `FileCredentialManager` struct matches previous API
- [x] Setup JSON-RPC IPC Server (AC: 3)
  - [x] Add `axum` and `serde_json` to daemon
  - [x] Implement RPC router and handlers
- [x] Integration & Testing (AC: 1, 2, 3, 4)
  - [x] Unit tests for connection logic
  - [x] Manual verification via `curl` or UI mockup

## Dev Notes

- **Jellyfin Auth**: Use `X-Emby-Token` header or `Authorization: MediaBrowser ... Token="..."`.
- **Keyring**: Use `keyring::Entry::new("jellyfinsync", "server_url")` pattern.
- **IPC Port**: 19140 is the designated port for JellyfinSync (randomly selected from private range).
- **Architecture**: The IPC server should run on the Tokio runtime already present in `main.rs`.

### Project Structure Notes

- New code should live in `jellyfinsync-daemon/src/api` or directly in `main.rs` if small.
- For this story, keep it simple but extensible.

### References

- [Keyring Crate](https://docs.rs/keyring/latest/keyring/)
- [Jellyfin API Docs](https://api.jellyfin.org/)
- [Source: _bmad-output/planning-artifacts/architecture.md#Authentication & Security]

## Dev Agent Record

### Agent Model Used

Antigravity (Claude 3.5 Sonnet)

### Debug Log References

### Completion Notes List

- **Architecture Deviation (Approved for V1):** Using file-based credential storage (`credentials.json`) instead of OS-native keyring. This is a temporary V1 implementation to unblock development. Keyring integration is deferred to V2 (Epic 2 follow-up).
- All acceptance criteria implemented with file-based fallback
- JSON-RPC server running on port 19140
- Jellyfin API client tested with mock server

### File List

**New Files:**
- `jellyfinsync-daemon/src/api.rs` - Jellyfin API client and file-based credential manager
- `jellyfinsync-daemon/src/rpc.rs` - JSON-RPC 2.0 server implementation

**Modified Files:**
- `jellyfinsync-daemon/src/main.rs` - Added RPC server startup in tokio runtime
- `jellyfinsync-daemon/src/tests.rs` - Added credential storage tests
- `jellyfinsync-daemon/Cargo.toml` - Added dependencies: reqwest, serde, serde_json, axum, lazy_static, mockito (dev)
- `Cargo.lock` - Dependency lockfile updates
