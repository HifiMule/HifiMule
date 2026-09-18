# Investigation: Subsonic get_credentials blocks device initialization

## Hand-off Brief

1. **What happened.** Confirmed: a healthy selected Subsonic/OpenSubsonic server can produce a successful `get_credentials` response with empty legacy URL and null `userId`, so `InitDeviceModal` renders the disconnected state.
2. **Where the case stands.** Concluded with high confidence: the RPC bypasses its DB fallback on successful vault lookup, while missing-only enrichment is unsafe because legacy Jellyfin metadata can remain stale after provider switching.
3. **What's needed next.** Repair `handle_get_credentials` so one DB-selected UUID supplies both record metadata and its matching vault secret, with provider-aware user identity and the documented regression matrix.

## Case Info

| Field            | Value |
| ---------------- | ----- |
| Ticket           | N/A |
| Date opened      | 2026-09-18 |
| Status           | Concluded |
| System           | HifiMule repository; reported on Windows `%APPDATA%`, provider type Subsonic/OpenSubsonic |
| Evidence sources | User bug report; current source code; version control and tests pending |

## Problem Statement

The user reports that the Initialize Device modal incorrectly blocks every connected Subsonic/Navidrome server because `get_credentials` returns a Jellyfin-oriented `userId` that remains null. The proposed repair is to enrich incomplete successful credential reads from the selected `server_config` row and to broaden missing-credential error handling.

## Evidence Inventory

| Source   | Status | Notes |
| -------- | ------ | ----- |
| User bug report | Available | Detailed causal chain, observed RPC payloads, workaround, and suggested fix; treated as a hypothesis pending source verification. |
| Current source code | Available | UI gate, RPC success/error arms, provider connection persistence, credential manager, selected-server synchronization, and adjacent provider construction were mapped. |
| Runtime reproduction | Missing | No live server/config/vault fixture supplied. |
| Version control history | Partial | Recent history for the three affected files was inventoried; commit intent has not yet been inspected. |
| Test results | Partial | The named test compiled under the controlled FFmpeg wrapper but cannot execute in this macOS environment because `system-configuration` panics while creating a dynamic store. |
| Static analysis | Missing | Not required to establish the control-flow defect; no dedicated run yet. |

## Investigation Backlog

| # | Path to Explore | Priority | Status | Notes |
| - | --------------- | -------- | ------ | ----- |
| 1 | Trace `InitDeviceModal` gating and credential response contract | High | Done | `userId` is the gate and is passed back as `profileId`. |
| 2 | Trace Subsonic/OpenSubsonic connection persistence | High | Done | DB retains URL/username; vault intentionally stores no Subsonic `user_id`; legacy config selection updates only the UUID. |
| 3 | Inspect `CredentialManager::get_credentials` and RPC fallback | High | Done | Successful reads return config metadata unchanged; DB fallback exists only in the error arm. |
| 4 | Review and extend tests for vault-present and vault-missing states | High | Done | Required regression matrix is defined; execution remains blocked locally by the macOS dynamic-store test panic. |
| 5 | Check Jellyfin behavior and security implications | Medium | Done | Global credential-manager changes would misroute Subsonic credentials into Jellyfin-only callers. |
| 6 | Review relevant git history | Low | Done | Multi-server selection originated in `e423c7e`; Subsonic fallback coverage in `e351217`. |
| 7 | Define implementation seam and test matrix | High | Done | Repair belongs in `handle_get_credentials`, keyed by one selected server UUID. |

## Timeline of Events

| Time | Event | Source | Confidence |
| ---- | ----- | ------ | ---------- |
| 2026-09-18 | Investigation opened from a detailed user report. | Conversation | Confirmed |
| Commit `e423c7e` | Multi-server infrastructure introduced `set_config_selected_server`. | Git history | Confirmed |
| Commit `e351217` | Subsonic support added the named fallback regression test. | Git history | Confirmed |

## Confirmed Findings

### Finding 1: The reported entry points exist in current source

**Evidence:** `hifimule-ui/src/components/InitDeviceModal.ts:99`, `hifimule-daemon/src/rpc.rs:2452`, `hifimule-daemon/src/rpc.rs:8855`

**Detail:** The UI render method, RPC handler, and specifically named fallback test are present, providing an independent stronghold for the investigation.

### Finding 2: The UI treats `userId` as the connection gate

**Evidence:** `hifimule-ui/src/components/InitDeviceModal.ts:90`, `hifimule-ui/src/components/InitDeviceModal.ts:100`, `hifimule-ui/src/components/InitDeviceModal.ts:319`

**Detail:** A falsy `userId` renders the disconnected message; a truthy value is later submitted as `profileId` to `device_initialize`.

### Finding 3: Credential metadata and selected-server persistence are provider-asymmetric

**Evidence:** `hifimule-daemon/src/rpc.rs:2119`, `hifimule-daemon/src/rpc.rs:2132`, `hifimule-daemon/src/rpc.rs:2142`, `hifimule-daemon/src/api.rs:1865`, `hifimule-daemon/src/api.rs:1873`

**Detail:** Jellyfin selection writes URL and provider user ID into legacy config, while Subsonic-family selection stores a vault credential with `user_id: None` and changes only `selected_server_id`.

### Finding 4: The existing test does not reach the reported successful-but-incomplete path

**Evidence:** `hifimule-daemon/src/rpc.rs:2453`, `hifimule-daemon/src/rpc.rs:2466`, `hifimule-daemon/src/rpc.rs:8855`, `hifimule-daemon/src/rpc.rs:8858`

**Detail:** The successful credential branch serializes its tuple immediately. The named test points at a missing config file, so it exercises only the error fallback.

### Finding 5: The named test is currently blocked by an environment-specific panic

**Evidence:** Command `npm run build:daemon -- test -p hifimule-daemon test_get_credentials_falls_back_to_server_config_for_subsonic_device_init`, run 2026-09-18

**Detail:** Compilation succeeds under the repository's controlled FFmpeg wrapper, but the test panics in `system-configuration-0.6.1/src/dynamic_store.rs` with `Attempted to create a NULL object` before assertions execute.

## Deduced Conclusions

### Deduction 1: The modal failure is deterministic for a fresh selected Subsonic-family connection

**Based on:** Findings 2–4 and the source trace at `hifimule-daemon/src/api.rs:1994-2001`.

**Reasoning:** A fresh Subsonic connection creates a vault entry and selected UUID but leaves legacy URL empty and user ID null. The vault lookup therefore succeeds, the RPC never enters its DB fallback, and the modal receives null `userId`.

**Conclusion:** The reported root cause is confirmed without requiring a network reproduction.

### Deduction 2: Missing-only enrichment can return a mixed-server credential tuple

**Based on:** `hifimule-daemon/src/api.rs:1865-1868`, `hifimule-daemon/src/api.rs:1995-2001`, and `hifimule-daemon/src/db.rs:684-690`.

**Reasoning:** Selecting Subsonic updates only `selected_server_id`; old Jellyfin URL/user ID remain non-empty. The vault secret is loaded using the new UUID, while missing-only checks preserve old metadata. Independently querying the DB's selected row can also disagree with the file-backed UUID.

**Conclusion:** The suggested fallback placement is directionally correct, but its proposed field-level merge is not safe enough.

### Deduction 3: A Subsonic username satisfies current device-initialization behavior

**Based on:** `hifimule-daemon/src/rpc.rs:6868-6899`, `hifimule-daemon/src/device/mod.rs:707-778`, and `hifimule-daemon/src/db.rs:941-960`.

**Reasoning:** `profileId` is stored verbatim, its presence controls recognized state, and it is not sent to a provider API.

**Conclusion:** Returning a Subsonic username is behaviorally compatible today, though the value also appears in state/tooltip presentation and is not literally used only as a boolean.

## Hypothesized Paths

### Hypothesis 1: Incomplete successful credential reads bypass the selected-server fallback

**Status:** Confirmed

**Theory:** For Subsonic/OpenSubsonic, a vault credential can exist while the legacy `config.url` and `config.user_id` fields remain empty. `CredentialManager::get_credentials()` therefore succeeds with an incomplete payload, and `handle_get_credentials` only consults `server_config` on selected errors, allowing null `userId` to reach the modal.

**Supporting indicators:** The current tree contains the exact RPC handler, UI gate, and fallback test named in the report.

**Would confirm:** Source trace showing Subsonic connection saves a credential with no user ID, successful credential retrieval reads legacy config fields, and DB enrichment occurs only in the error arm.

**Would refute:** Successful retrieval already enriches incomplete fields, or the modal derives connection state independently of `userId`.

**Resolution:** Confirmed at `hifimule-daemon/src/rpc.rs:2452-2466`: the successful tuple is returned immediately, and DB fallback begins only in the error arm. Subsonic persistence at `hifimule-daemon/src/rpc.rs:2132-2142` creates the incomplete successful state.

### Hypothesis 2: Filling only empty URL/null user ID from the selected DB row is a safe complete fix

**Status:** Refuted

**Theory:** The RPC can preserve all successful credential values and fill only absent metadata from `get_server_config()`.

**Supporting indicators:** This repairs the fresh Subsonic case and preserves the vault token.

**Would confirm:** All non-empty config metadata is guaranteed to belong to the UUID used for the vault lookup, and DB/file selection cannot diverge.

**Would refute:** A supported path leaves stale non-empty metadata or the two selections are independently persisted.

**Resolution:** Refuted. `set_config_selected_server` changes only the UUID and preserves prior metadata; DB selection and config selection are separate stores. Jellyfin → Subsonic switching can pair an old Jellyfin identity with the new Subsonic secret.

### Hypothesis 3: The Subsonic username is an acceptable device `profileId`

**Status:** Confirmed

**Theory:** Current device initialization treats the value as provider-neutral opaque recognition metadata despite Jellyfin-specific names.

**Supporting indicators:** No format validation or provider call occurs.

**Would confirm:** All reads only use presence/state/display semantics.

**Would refute:** Any Jellyfin request or UUID-specific comparison consumes the stored value.

**Resolution:** Confirmed with a qualification: the value is forwarded in `DeviceRecognized.profile_id` and shown in the tray tooltip, but no provider API consumes it.

### Hypothesis 4: `CredentialManager::get_credentials` should become provider-neutral

**Status:** Refuted

**Theory:** Fix the issue centrally by returning DB/provider-aware credentials from the credential manager.

**Supporting indicators:** A central repair might appear to reduce duplication.

**Would confirm:** All callers treat the tuple generically or guard the provider before use.

**Would refute:** Any caller sends the tuple directly to Jellyfin APIs without a reliable provider guard.

**Resolution:** Refuted. Multiple RPC, auto-fill, proxy, exclusion, and background-scrobbler paths assume Jellyfin tuple semantics. In particular, `hifimule-daemon/src/main.rs:383-408` launches the Jellyfin scrobbler whenever the tuple succeeds with a user ID, without checking provider type.

## Missing Evidence

| Gap | Impact | How to Obtain |
| --- | ------ | ------------- |
| Live/runtime reproduction | Would independently confirm the exact payload on a real selected Navidrome/Subsonic server | Reproduce with an installed server and inspect `get_credentials`. |
| Executable regression test on this host | Prevents direct confirmation of current and proposed assertions | Isolate/disable the macOS network watcher during the RPC unit test or run in CI/Linux. |
| Commit-level intent | May explain why device initialization consumes a Jellyfin-session API | Inspect the introducing commits for the modal and fallback. |
| Cross-provider stale-config fixture | Determines whether missing-only enrichment can mix old Jellyfin metadata with a Subsonic token | Add a switch Jellyfin → Subsonic test using explicit config/vault/DB state. |
| End-to-end reconnect assertion | Would verify a Subsonic username persists and produces `DeviceRecognized` | Extend device initialization coverage after the RPC fix. |

## Source Code Trace

| Element       | Detail |
| ------------- | ------ |
| Error origin  | `hifimule-daemon/src/rpc.rs:2452`, `handle_get_credentials` successful arm |
| Trigger       | Initialize Device modal requests credentials. |
| Condition     | Selected Subsonic-family server has a vault entry, while legacy config metadata is absent or stale. |
| Related files | `hifimule-ui/src/components/InitDeviceModal.ts`; `hifimule-daemon/src/rpc.rs`; `hifimule-daemon/src/api.rs`; `hifimule-daemon/src/db.rs`; `hifimule-daemon/src/device/mod.rs`. |

### Implementation boundary

The repair belongs in `hifimule-daemon/src/rpc.rs:2452` only. `CredentialManager::get_credentials` is explicitly a legacy Jellyfin-session API and has numerous Jellyfin-only callers (`rpc.rs:3853`, `3905`, `3960`, `4008`, `4088`, `5111`, `7159`; `auto_fill/mod.rs:104`; `main.rs:385`). Making it provider-neutral would risk sending Subsonic passwords/usernames to Jellyfin endpoints.

For the RPC response, resolve one selected `server_config` record and use that record's `id` to load its vault credential. Return metadata from that same record: vault `user_id` for Jellyfin, database `username` for Subsonic/OpenSubsonic. Keep the current credential-manager behavior only as a legacy fallback when there is no selected database row.

### Error classification

The exact expected vault-entry miss is `No credential found in vault for server: {id}` at `hifimule-daemon/src/api.rs:1944`. The current `No token found` match has no producer in the examined credential path. A minimal repair can recognize the exact missing-entry prefix, but a typed `CredentialError::CredentialNotFound` would be safer than continuing string classification. Vault decryption, config parse, and I/O failures must remain storage errors.

## Conclusion

**Confidence:** High

The report's primary diagnosis is correct. Subsonic/OpenSubsonic persistence creates a valid UUID-keyed vault entry but does not populate legacy `config.url` or `config.user_id`; `CredentialManager::get_credentials` therefore succeeds with incomplete legacy metadata, and `handle_get_credentials` returns it without reaching the DB fallback. The modal then treats null `userId` as disconnected.

The proposed move of DB recovery into the successful path is directionally correct but not sufficient as written. Filling only empty/null fields can preserve stale non-empty Jellyfin metadata after switching to Subsonic, and consulting `get_server_config()` without binding it to the credential UUID can pair metadata and secrets from different servers. The correct invariant is one server UUID per response.

The implementation should remain localized to the provider-facing RPC handler. Changing `CredentialManager::get_credentials` would alter a deliberately Jellyfin-shaped API used by Jellyfin RPCs, auto-fill, proxying, exclusion expansion, and an unguarded background scrobbler.

## Recommended Next Steps

### Fix direction

1. Keep `CredentialManager::get_credentials` Jellyfin-specific.
2. In `handle_get_credentials`, prefer the DB-selected record as the authoritative RPC server.
3. Load the vault credential by that exact record ID; never merge file metadata and a differently selected DB row.
4. Derive `userId` by provider: Jellyfin vault `user_id`; Subsonic/OpenSubsonic database username.
5. Preserve the valid vault secret on successful lookup.
6. Retain the legacy manager path only when no DB-selected record exists.
7. Classify only an absent vault entry as an expected missing-credential state; preserve storage errors for decryption/read/parse failures.

This is a non-trivial compatibility repair rather than a one-line fallback move because it must preserve Jellyfin UUID semantics and legacy config-only sessions.

### Diagnostic

Add targeted fixtures for:

- Selected OpenSubsonic row + matching vault password + empty legacy metadata → DB URL, DB username, vault password.
- Jellyfin → Subsonic switch with stale non-empty Jellyfin config → entirely Subsonic response.
- Selected Jellyfin row + matching vault credential → preserve provider UUID, never substitute login username.
- DB/config selection mismatch → metadata and secret still come from the same DB-selected UUID.
- Missing Subsonic vault entry → explicitly preserve or revise the current DB-only device-init behavior.
- Missing Jellyfin vault entry → never manufacture a Jellyfin user ID from the login username.
- Vault decryption/read/parse failure → `ERR_STORAGE_ERROR`.
- No selected DB row + valid legacy Jellyfin session → unchanged legacy response.

## Reproduction Plan

Create a selected OpenSubsonic server row and matching UUID-keyed vault credential while leaving legacy `config.url` empty and `config.user_id` null; call `handle_get_credentials`; assert DB URL, DB username, and vault secret. Repeat after seeding stale Jellyfin URL/user ID in legacy config. Then initialize a device with the returned username and verify persistence plus `DeviceRecognized` on reconnect.

## Side Findings

- **Confirmed:** `server_config.selected` has no schema-level uniqueness constraint, and `set_selected` clears then sets without a transaction (`hifimule-daemon/src/db.rs:699-711`). This permits zero-selection states and makes defensive UUID consistency valuable.
- **Confirmed:** The stored device field is not purely a boolean. Its presence controls recognition, but the value is also forwarded as `DeviceRecognized.profile_id` and displayed in the tray tooltip (`hifimule-daemon/src/main.rs:1117-1122`).
- **Confirmed:** The named regression test cannot run on the current macOS host because `system-configuration` panics while creating a dynamic store; this is an environment/test-isolation issue, not evidence against the bug.
