---
title: 'Fix Jellyfin 12 authentication compatibility'
type: 'bugfix'
created: '2026-09-09'
status: 'done'
baseline_commit: 'd37f7a06a8b0b3c9fac2ddc713b6222f72e0f110'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** After upgrading to Jellyfin 12.0, HifiMule reports Unknown server type despite a successful password login. Subsequent requests use deprecated authentication that Jellyfin 12 disables by default; downloads also generate a deprecated token query parameter.

**Approach:** Migrate Jellyfin authenticated requests and generated download URLs to the supported authentication formats, with regression coverage for successful login followed by authenticated use. The supported compatibility range for this fix is Jellyfin 10.8 through 12.0, using the same supported protocol without version branching. Supporting releases before 10.8 is out of scope, as agreed by the user during implementation.

## Boundaries & Constraints

**Always:** Keep server communication behind the existing Jellyfin client/provider boundary. Preserve input validation, token confidentiality, server identity, credentials persistence, request paths and existing operation semantics. Use `Authorization` with the `MediaBrowser` scheme for token-bearing API requests and `ApiKey` for URLs consumed without headers. Retain working login client/device metadata. Verify both fresh login and stored-token reuse.

**Ask First:** Changes to server configuration, credential storage, public RPC contracts, or unrelated Jellyfin 12 API migrations.

**Never:** Enable legacy authentication on the user's server, weaken TLS checks, hardcode server credentials, add a version allowlist, or change Subsonic behavior. Generic auto-detection error redesign and changing the metadata endpoint are outside this fix.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|---------------------------|----------------|
| Older Jellyfin | Representative 10.8, 10.9, 10.10 and 10.11 authentication contracts | Fresh login, stored-token reconnect, authenticated browsing and download authorization remain supported through the common protocol | Do not reject servers by version or require users to upgrade |
| Fresh login | Successful password exchange; legacy token headers rejected | Metadata request carries supported token authorization; provider connects and reports version 12.0.0 and server ID | Existing real request errors remain errors |
| Reconnect | Stored valid token | Same supported authorization; no password required | Invalid token cannot produce successful connection |
| Authenticated operations | Browse, playback negotiation, playlist and scrobble requests | Every authenticated client operation uses supported authorization | Preserve existing status/error mapping |
| Direct download | URL has no authentication query | Add encoded `ApiKey` with current token | Preserve base path and existing query parameters |
| Transcoding URL | URL contains `ApiKey`, legacy `api_key`, or unrelated query | Preserve existing supported key; normalize legacy key; add key when absent without duplicates | Preserve unrelated parameters and fragments |
| Invalid token | Invalid header value or missing token | Existing validation rejects invalid values without sending a request | No token disclosure in errors |
| Other providers | Subsonic/OpenSubsonic connection | Existing protocol and order continue to work | Existing unknown-server behavior remains |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/api.rs` — JellyfinClient, repeated legacy header construction, supported password login, API unit tests.
- `hifimule-daemon/src/providers/jellyfin.rs` — provider download URL authentication and extensive operation mocks.
- `hifimule-daemon/src/providers/mod.rs` — password/token connection factory and auto-detection regression tests.
- `hifimule-daemon/src/scrobbler.rs` and `hifimule-daemon/src/rpc.rs` — caller tests currently expecting legacy Jellyfin headers.
- `hifimule-daemon/src/sync.rs` — inspect download URL fixtures for dependencies on old spelling; production downloader must remain provider-neutral.
- `hifimule-daemon/src/auto_fill/mod.rs` — legacy auto-fill HTTP request now uses the shared validated authentication helper, with regression coverage.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/providers/mod.rs`, `hifimule-daemon/src/api.rs`, `hifimule-daemon/src/providers/jellyfin.rs` — verify historical server authentication contracts and add explicit older-version compatibility cases for the matrix. Base fixtures on historical source/API evidence; changing only a mocked version string is insufficient proof. Distinguish mock coverage from live-server validation in the completion report.
- [x] `hifimule-daemon/src/providers/mod.rs` — add failing regression coverage for a 12.0.0 server that accepts password login but requires supported authorization afterward, plus stored-token connection. Assert mocks are reached, metadata retained and provider type correct.
- [x] `hifimule-daemon/src/api.rs` — centralize validated token authorization and migrate every authenticated request; update mocks to enforce the supported scheme and token. Cover an authenticated GET and write operation, as well as invalid token behavior.
- [x] `hifimule-daemon/src/providers/jellyfin.rs` — normalize download authentication to `ApiKey` using structured query handling; cover the URL matrix, preserving unrelated URL components and existing supported tokens. Update provider mocks to exercise the actual authentication contract.
- [x] `hifimule-daemon/src/scrobbler.rs`, `hifimule-daemon/src/rpc.rs`, `hifimule-daemon/src/sync.rs` — update affected caller tests and fixtures only where they represent Jellyfin requests; retain redaction coverage for old and new query spellings.
- [x] `hifimule-daemon/src` — audit for remaining production legacy authentication, run focused and broader daemon checks, and review the final diff for unrelated changes.

**Acceptance Criteria:**
- Given Jellyfin 12 with legacy authorization disabled and valid credentials, when HifiMule connects automatically, then login and metadata retrieval succeed and the provider exposes the returned version and stable ID.
- Given an established Jellyfin session, when browsing or downloading music, then HifiMule uses authentication accepted without enabling server compatibility settings.
- Given representative Jellyfin 10.8, 10.9, 10.10 and 10.11 authentication contracts, when fresh login, stored-token reconnect, browsing and download authorization are exercised, then the common authentication implementation remains compatible without requiring a server upgrade or a manual compatibility toggle.
- Given Subsonic fixtures, when regression tests execute, then connection/provider behavior remains unchanged.

## Spec Change Log

## Design Notes

User logs show successful authentication, a newly issued token, then an authentication challenge. Code inspection finds login already sends `Authorization: MediaBrowser ...`, followed by `/System/Info` with `X-Emby-Token`. Auto mode masks this failure as Unknown server type. No connection version gate exists. This is a confirmed protocol incompatibility consistent with the report, not a live reproduction against the user's server.

Jellyfin's [authentication migration](https://github.com/jellyfin/jellyfin/pull/13306) specifies supported header and query formats; its [default change](https://github.com/jellyfin/jellyfin/pull/15559) disables legacy methods on existing installations. The [12.0 release notes](https://jellyfin.org/posts/jellyfin-release-12.0/) confirm this ships in 12.0. Supported authentication also works on older releases. Do not change metadata endpoints merely to hide rejection of token headers.

## Verification

### Results — 2026-09-09

- Reproduced the original Unknown server type failure with successful login followed by a strict supported-authentication metadata mock before fixing the headers.
- Reproduced download URL normalization/fragment and ApiKey redaction regressions before their fixes. The final auto-fill regression failed before migrating its remaining legacy header.
- Final full daemon test run: `rtk cargo test -p hifimule-daemon` — **640 passed, 0 failed**.
- `rtk cargo fmt --all -- --check` and `rtk git -c safe.directory=C:/Workspaces/HifiMule diff --check` — passed.
- Production-source audit: remaining `X-Emby-Token` occurrences are assertions that the legacy header is absent; legacy query spellings remain only for normalization, redaction and test inputs.
- Historical parser source independently inspected at tags [10.8.13](https://raw.githubusercontent.com/jellyfin/jellyfin/v10.8.13/Jellyfin.Server.Implementations/Security/AuthorizationContext.cs), [10.9.11](https://raw.githubusercontent.com/jellyfin/jellyfin/v10.9.11/Jellyfin.Server.Implementations/Security/AuthorizationContext.cs), [10.10.7](https://raw.githubusercontent.com/jellyfin/jellyfin/v10.10.7/Jellyfin.Server.Implementations/Security/AuthorizationContext.cs) and [10.11.0](https://raw.githubusercontent.com/jellyfin/jellyfin/v10.11.0/Jellyfin.Server.Implementations/Security/AuthorizationContext.cs). All accept `Authorization` with the `MediaBrowser` scheme and `ApiKey`; parameter values are URL-decoded. Header token values therefore use percent encoding after original-value validation. In 10.11 only legacy alternatives are gated by EnableLegacyAuthorization.
- Compatibility tests exercise the shared source-verified protocol with representative 10.8–12.0 metadata. These are mock HTTP tests, not live-server integration tests. The user's upgraded server has not been accessed.

- `rtk cargo test -p hifimule-daemon providers::tests` — factory regressions fail before migration and pass afterward.
- `rtk cargo test -p hifimule-daemon api::tests` — client authentication and API tests pass.
- `rtk cargo test -p hifimule-daemon` — daemon suite passes, including provider, RPC, scrobble and URL tests.
- `rtk cargo fmt --all -- --check` — formatting passes.
- If environment dependencies prevent execution, record the exact failure and do not claim tests passed. Live verification on the upgraded server remains a separate confirmation after building the fix.

### Review outcome

Three independent reviews completed: acceptance auditor, edge-case hunter and blind diff review. The duplicate empty-ApiKey finding was classified as a patch: normalization now skips empty credentials before selecting a supported, legacy or stored token. Added three cases, observed failure, applied fix and reran the full suite: 640 passed. Strengthened the header encoding test to decode the value and assert token identity. The historical compatibility concern is resolved by the source verification above, with live-test limitations retained. The credential configuration override observation reflects the existing shared test harness convention; no failing interaction remained in the full suite, and a general harness redesign is outside this authentication fix.

## Suggested Review Order

- Use one validated token header throughout authenticated requests.
  [api.rs:13](../../hifimule-daemon/src/api.rs#L13)

- Normalize download credentials while preserving query values and fragments.
  [jellyfin.rs:506](../../hifimule-daemon/src/providers/jellyfin.rs#L506)

- Keep both supported and legacy query tokens redacted.
  [mod.rs:556](../../hifimule-daemon/src/providers/mod.rs#L556)

- Verify source-backed authentication contracts across the supported release range.
  [mod.rs:611](../../hifimule-daemon/src/providers/mod.rs#L611)

- Reproduce successful login followed by strict Jellyfin 12 token validation.
  [mod.rs:733](../../hifimule-daemon/src/providers/mod.rs#L733)

- Cover the remaining auto-fill HTTP request.
  [mod.rs:531](../../hifimule-daemon/src/auto_fill/mod.rs#L531)
