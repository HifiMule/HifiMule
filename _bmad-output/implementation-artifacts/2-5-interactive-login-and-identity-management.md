# Story 2.5: Interactive Login & Identity Management

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a Ritualist (Arthur),
I want a clear, guided login screen where I can select my server and enter my credentials,
So that I can easily connect to my library without manually copying API tokens.

## Acceptance Criteria

1.  **Login View UI:**
    *   **Given** the application is unconfigured or a connection error occurs
    *   **Then** the Login View is displayed as the primary interface.
    *   The view MUST allow entry of:
        *   **Server URL** (e.g., `http://localhost:8096` or `https://jellyfin.example.com`).
        *   **Username**.
        *   **Password**.

2.  **Authentication Logic:**
    *   **When** I click "Connect"
    *   **Then** the daemon MUST attempt to authenticate with the Jellyfin server using the `/Users/AuthenticateByName` API endpoint.
    *   **Payload** MUST include client identification headers (`Client="HifiMule"`, `Device`, `DeviceId`, `Version`).

3.  **Secure Token Storage:**
    *   **When** authentication is successful
    *   **Then** the returned `AccessToken` and `UserId` MUST be securely stored.
    *   **Keyring Usage:** The `AccessToken` MUST be stored in the OS-native keyring (using existing `CredentialManager` logic).
    *   **Config Update:** The `ServerUrl` and `UserId` MUST be stored in the local configuration file.

4.  **State Transition:**
    *   **When** login is successful
    *   **Then** the UI MUST automatically transition to the Main Library Browser.
    *   **And** the `daemon_state` RPC MUST report `serverConnected: true`.

5.  **Error Feedback:**
    *   **When** authentication fails (401 Unauthorized or Connection Error)
    *   **Then** a clear, user-friendly error message MUST be displayed on the Login screen (e.g., "Invalid Username or Password", "Server Unreachable").

## Tasks / Subtasks

- [x] **Backend: Implement Authentication API**
    - [x] Add `authenticate_by_name` method to `JellyfinClient` in `hifimule-daemon/src/api.rs`.
    - [x] Define `AuthenticateByNameRequest` and `AuthenticationResult` structs.
    - [x] Implement proper authorization headers for the initial handshake.
- [x] **Backend: Expose Login RPC**
    - [x] Create `login(url, username, password)` RPC method in `hifimule-daemon/src/rpc.rs`.
    - [x] Wire up `login` to call `client.authenticate_by_name` then `CredentialManager::save_credentials`.
- [x] **Frontend: Build Login UI**
    - [x] Create `hifimule-ui/src/components/LoginView.svelte` (or equivalent Web Component/HTML).
    - [x] Implement form validation (URL format, empty fields).
    - [x] Add visual loading state during authentication request.
- [x] **Frontend: Implement Navigation Guard**
    - [x] Update `main.ts` to check `get_daemon_state` on launch.
    - [x] Redirect to Login View if not connected/configured.
    - [x] Redirect to Main View if already connected.

## Dev Notes

### Architecture & Pattern Compliance
- **Keyring Integration:** The `keyring` crate is already implemented in `hifimule-daemon/src/api.rs`. Reuse `CredentialManager::save_credentials`.
- **API Pattern:** Follow the pattern in `api.rs` for `reqwest` calls. Ensure `rename_all = "PascalCase"` or "camelCase" matches Jellyfin API exactly.
- **RPC Pattern:** Use the established JSON-RPC definitions in `rpc.rs`.

### Technical Specifics
- **Jellyfin Auth API:**
    - Endpoint: `/Users/AuthenticateByName`
    - Method: `POST`
    - Body: `{ "Username": "...", "Pw": "..." }`
    - Header: `Authorization: MediaBrowser Client="HifiMule", Device="Desktop", DeviceId="...", Version="..."`
- **Security:** Do NOT store the password. only the returned `AccessToken`.

### Git Intelligence
- **Recent work (Story 2.3 & 2.4):**
    - `keyring` was added in 2.3.
    - `get_daemon_state` was enhanced in 2.4 to report connection status.
    - `CORS` policy was fixed in 2.4 to allow localhost origins.

### Implementation Notes
- **Frontend divergence**: Implemented `login.ts` (Vanilla/Lit) instead of `LoginView.svelte` to match the existing project structure and avoid introducing a new framework unnecessarily.
- **Device ID**: Implemented a persistent `device_id` in `api.rs` (stored in `config.json`) to uniquely identify the client to the Jellyfin server, replacing the hardcoded "HifiMule-Desktop".

### File Structure
- `hifimule-daemon/src/api.rs`: Auth logic here.
- `hifimule-daemon/src/rpc.rs`: RPC handler here.
- `hifimule-ui/`: Login UI code.

### References
- [Story 2.1 (Security)](file:///c:/Workspaces/HifiMule/_bmad-output/implementation-artifacts/2-1-secure-jellyfin-server-link.md)
- [Jellyfin API Docs - AuthenticateByName](https://api.jellyfin.org/)
