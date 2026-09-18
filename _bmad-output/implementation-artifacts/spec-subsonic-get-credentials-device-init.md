---
title: 'Make device initialization credentials provider-aware'
type: 'bugfix'
created: '2026-09-18'
status: 'done'
baseline_commit: '4c886ce386ef1404e5d3fd73666072f88d324a4f'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/investigations/subsonic-get-credentials-device-init-investigation.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** `get_credentials` returns legacy Jellyfin-session metadata even when the selected server is Subsonic/OpenSubsonic. A valid vault password can therefore be paired with an empty or stale URL and a null or stale Jellyfin `userId`, causing the Initialize Device modal to report that no server is connected.

**Approach:** Make the `get_credentials` RPC construct a provider-aware response from one authoritative database-selected server and the vault credential keyed by that same local server UUID. Preserve `CredentialManager::get_credentials` as a Jellyfin-only legacy API for its existing callers and for config-only sessions with no selected database row.

## Boundaries & Constraints

**Always:** Keep metadata and secrets bound to one server UUID; preserve a valid vault token/password; return the vault user ID for Jellyfin and database username for Subsonic/OpenSubsonic; preserve server type/version fields where DB metadata is used; distinguish an absent credential from vault/config storage failures.

**Ask First:** Any database schema or selection-invariant change; any change to missing-vault behavior that would intentionally block existing DB-only Subsonic device initialization; any UI or device-schema rename.

**Never:** Make `CredentialManager::get_credentials` provider-neutral; substitute a Jellyfin login username for its provider user UUID; conditionally merge stale legacy fields into another server's response; swallow vault decryption, parsing, or I/O failures as logged-out state.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Fresh Subsonic | Selected DB row and matching vault password; legacy URL empty and user ID null | DB URL, vault password, DB username, server metadata | N/A |
| Cross-provider switch | Selected Subsonic row/vault entry; stale non-empty Jellyfin legacy fields | Entire response belongs to Subsonic; no stale Jellyfin values | N/A |
| Jellyfin | Selected Jellyfin row and matching vault entry | DB URL, vault token, vault provider user UUID | Missing provider UUID remains null; never use login username |
| Selection mismatch | DB selection differs from legacy config UUID | DB-selected metadata and credential from that same DB UUID | Never combine both selections |
| Missing selected credential | Selected DB row but no matching vault entry | Preserve existing expected-state response: DB metadata, null token; Subsonic username remains available for device initialization | Do not return `ERR_STORAGE_ERROR` for this exact absence |
| Storage failure | Vault/config decryption, parsing, or I/O fails | No fabricated credential response | Return `ERR_STORAGE_ERROR` |
| Legacy session | No selected DB row; valid config-only Jellyfin session | Existing legacy tuple is returned unchanged | Existing expected-state handling remains |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/rpc.rs` -- `handle_get_credentials`, provider-aware response assembly, and RPC regression tests.
- `hifimule-daemon/src/api.rs` -- Jellyfin-session credential contract, UUID-keyed vault APIs, and explicit malformed-vault parse errors.
- `hifimule-daemon/src/db.rs` -- selected `ServerConfig` source for URL, provider type, username, version, and local UUID.
- `hifimule-ui/src/components/InitDeviceModal.ts` -- unchanged consumer that gates initialization on `userId`.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/api.rs` -- stop converting malformed decrypted vault JSON into an empty vault; return a diagnosable parse error while retaining deliberate legacy-vault migration behavior.
- [x] `hifimule-daemon/src/rpc.rs` -- resolve the selected DB record first, load its exact UUID-keyed vault entry, and construct a provider-aware response while retaining legacy fallback behavior.
- [x] `hifimule-daemon/src/api.rs`, `hifimule-daemon/src/rpc.rs` -- add regression tests covering malformed vault data, successful Subsonic lookup, stale Jellyfin metadata, Jellyfin UUID preservation, selection mismatch, missing vault entries, and legacy no-row behavior.

**Acceptance Criteria:**
- Given a connected selected Subsonic/OpenSubsonic server, when the Initialize Device modal requests credentials, then it receives a truthy username and can render the initialization form.
- Given a provider switch or inconsistent legacy config, when credentials are requested, then URL, secret, and identity are never sourced from different server UUIDs.
- Given a selected Jellyfin server, when credentials are requested, then its provider user UUID remains unchanged.
- Given a real credential-storage failure, when credentials are requested, then the RPC reports `ERR_STORAGE_ERROR` rather than masking it as an unconfigured state.

## Spec Change Log

- **2026-09-18, review loop 1:** Acceptance review found that malformed decrypted vault JSON was silently converted into an empty vault, causing the new selected-server path to misclassify corruption as a missing credential. Expanded the Code Map and tasks to require explicit vault parse errors and regression coverage, avoiding a null-token response that violates the frozen storage-failure contract. **KEEP:** DB-selected UUID binding; provider-specific Jellyfin/Subsonic identity; exact missing-entry behavior; legacy no-row fallback; focused credential test isolation.

## Design Notes

The DB-selected row is authoritative for the provider-facing RPC because `server.list` and the active server manager derive selection from the database. The credential lookup must use `selected.id`; provider identity is `credential.user_id` for Jellyfin and `selected.username` for Subsonic/OpenSubsonic. If no selected row exists, delegate to the current legacy credential-manager flow so older config-only Jellyfin sessions continue to work. Vault loading must distinguish a genuinely absent UUID entry from malformed decrypted JSON; legacy-shape handling remains the responsibility of the existing migration path rather than silent defaulting during normal reads.

## Verification

**Commands:**
- `npm run build:daemon -- test -p hifimule-daemon get_credentials` -- expected: all focused credential RPC tests pass.
- `npm run build:daemon -- test -p hifimule-daemon` -- expected: daemon test suite passes, subject to the documented macOS dynamic-store test-environment limitation.
- `cargo fmt --all -- --check` -- expected: no formatting changes required.

## Suggested Review Order

**Provider-aware resolution**

- Start with the DB-selected UUID boundary and provider-specific identity mapping.
  [`rpc.rs:2452`](../../hifimule-daemon/src/rpc.rs#L2452)

- See how the public RPC prefers selected DB state before legacy sessions.
  [`rpc.rs:2489`](../../hifimule-daemon/src/rpc.rs#L2489)

- Review the explicit config-only fallback response shape.
  [`rpc.rs:2478`](../../hifimule-daemon/src/rpc.rs#L2478)

**Credential integrity**

- Strict parsing prevents malformed or legacy vault JSON becoming an empty vault.
  [`api.rs:1700`](../../hifimule-daemon/src/api.rs#L1700)

- Optional UUID lookup cleanly separates missing credentials from storage failures.
  [`api.rs:1962`](../../hifimule-daemon/src/api.rs#L1962)

- Legacy lookup preserves old sessions while propagating config and vault errors.
  [`api.rs:2040`](../../hifimule-daemon/src/api.rs#L2040)

**Regression coverage**

- Subsonic coverage proves DB username and matching vault secret are returned.
  [`rpc.rs:8865`](../../hifimule-daemon/src/rpc.rs#L8865)

- Cross-provider coverage rejects stale Jellyfin metadata after server switching.
  [`rpc.rs:8902`](../../hifimule-daemon/src/rpc.rs#L8902)

- Jellyfin coverage preserves the provider UUID instead of the login username.
  [`rpc.rs:8942`](../../hifimule-daemon/src/rpc.rs#L8942)

- Vault parsing coverage locks malformed-data behavior to an explicit error.
  [`api.rs:2830`](../../hifimule-daemon/src/api.rs#L2830)
