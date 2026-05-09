# Story 8.5: Subsonic URL Credential Sanitization

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a System Admin (Alexis),
I want Subsonic auth credentials to never appear in log files,
so that my server password and derived auth tokens are not exposed in application logs.

## Acceptance Criteria

1. Given any Subsonic request URL is logged, returned through debug output, or included in an error message, when the URL contains `u=`, `p=`, `t=`, or `s=` query parameters, then those parameter values are replaced with `[REDACTED]` before the text leaves the provider/RPC boundary.
2. Given a Subsonic URL contains non-secret query parameters such as `id`, `format`, `maxBitRate`, `v`, `c`, or `f`, when it is sanitized, then those parameters remain intact and in a parseable URL string.
3. Given `download_url()` or `cover_art_url()` on `SubsonicProvider` returns a signed URL, when any sync, image, RPC, tracing, `println!`, or `eprintln!` path logs or stores an error involving that URL, then raw `u`, `p`, `t`, and `s` values do not appear.
4. Given sanitizer input contains words such as `status=ok` or `type=json`, when sanitization runs, then it must not redact mid-word `s=` or `t=` substrings and must not loop indefinitely.
5. Given provider factory and `server.connect` failures include Subsonic request context, when a JSON-RPC error is returned or formatted, then raw username, password, token, and salt values do not appear.
6. Given the daemon crate is tested, when `rtk cargo test -p hifimule-daemon providers::subsonic providers:: rpc --no-fail-fast` or the full `rtk cargo test -p hifimule-daemon` runs, then sanitizer, provider, factory, RPC, and existing tests pass.

## Tasks / Subtasks

- [x] Add a public Subsonic URL sanitizer in the provider module (AC: 1, 2, 4)
  - [x] Implement `pub fn sanitize_subsonic_url(url: &Url) -> String` in `hifimule-daemon/src/providers/subsonic.rs`.
  - [x] Replace values for only the auth keys `u`, `p`, `t`, and `s` with `[REDACTED]`.
  - [x] Preserve non-secret query parameters and path/query parseability, including `id`, `format`, `maxBitRate`, `v`, `c`, and `f`.
  - [x] Keep the existing `sanitize_message` boundary behavior: single-character keys must match only at query/message separators, not inside words such as `status=ok`.
  - [x] Decide whether to keep `sanitize_message` as a message-level wrapper around `sanitize_subsonic_url` or leave it as a focused text sanitizer; do not maintain two divergent redaction algorithms without tests for both.

- [x] Apply the sanitizer to Subsonic provider URL/error surfaces (AC: 1, 3, 5)
  - [x] Audit `SubsonicClient::signed_url`, `get_envelope_url`, `map_reqwest_error`, `download_url`, `stream_url`, and `cover_art_url`.
  - [x] Ensure any future logging inside `providers/subsonic.rs` uses `sanitize_subsonic_url` before formatting a URL.
  - [x] Ensure `ProviderError::{Auth,Http,Deserialization,Other}` messages created from Subsonic failures cannot include raw `u`, `p`, `t`, `s`, raw password, or derived token/salt values.
  - [x] Do not remove signed auth parameters from returned runtime URLs; callers still need usable URLs for HTTP requests. Only logged/debug/error text is sanitized.

- [x] Audit cross-module log and error paths that can receive Subsonic URLs (AC: 3, 5)
  - [x] Check `hifimule-daemon/src/sync.rs` around stream resolution and per-file `SyncFileError` construction; sanitize any URL-bearing error before storing it in operation status.
  - [x] Check `hifimule-daemon/src/rpc.rs` around `handle_server_connect`, `require_provider`, image proxy, and sync spawning paths; JSON-RPC errors must not leak auth query values.
  - [x] Check `hifimule-daemon/src/main.rs` daemon/auto-sync logging for propagated sync errors.
  - [x] Keep Jellyfin behavior compatible; do not convert this story into a provider-neutral sync migration.

- [x] Add focused redaction tests (AC: 1-6)
  - [x] Unit-test `sanitize_subsonic_url` with a signed `/rest/download.view` URL containing `u`, `p`, `t`, `s`, `id`, `v`, `c`, and `f`.
  - [x] Unit-test `/rest/stream.view` with `format=mp3` and `maxBitRate=192`; verify bitrate/profile params survive unchanged.
  - [x] Unit-test `/rest/getCoverArt.view` with `id=cover1`; verify the artwork ID survives.
  - [x] Unit-test no false positives for `status=ok`, `type=json`, `artist=The Smiths`, and already-redacted values.
  - [x] Unit-test malformed/relative text inputs through `sanitize_message` if that helper remains responsible for non-URL error strings.
  - [x] Add provider/factory/RPC tests proving failed Subsonic connect and failed `server.connect` responses do not contain the supplied username, password, token, or salt.

- [x] Verify and update workflow state (AC: 6)
  - [x] Run focused daemon tests first, then `rtk cargo test -p hifimule-daemon`.
  - [x] Update this story's Dev Agent Record and File List when implementation is complete.

## Dev Notes

### Current Codebase State

- `SubsonicProvider` and its local REST client live in `hifimule-daemon/src/providers/subsonic.rs`. The client signs every request in `SubsonicClient::signed_url()` by appending `u`, `t`, `s`, `v=1.16.1`, `c=hifimule`, `f=json`, then endpoint-specific params. [Source: hifimule-daemon/src/providers/subsonic.rs:474]
- Current `download_url`, `stream_url`, and `cover_art_url` return signed URL strings directly. These URLs are intentionally usable at runtime and currently include auth query values. This story must sanitize logging/error surfaces, not break returned URLs. [Source: hifimule-daemon/src/providers/subsonic.rs:377]
- Existing `sanitize_message()` in `providers/subsonic.rs` already redacts `password`, `u`, `p`, `t`, and `s` in text and includes a separator guard to avoid the previous `status=` false positive. Reuse that learning. [Source: hifimule-daemon/src/providers/subsonic.rs:530]
- `providers/mod.rs` has a separate `sanitize_secret_message()` used by the provider factory's Jellyfin path; it also includes separator guarding and tests for `status=ok`. Keep factory-level and Subsonic-level behavior consistent. [Source: hifimule-daemon/src/providers/mod.rs:240]
- `handle_server_connect()` maps provider errors directly into JSON-RPC `message: error.to_string()`. Any unsanitized `ProviderError` becomes externally visible. [Source: hifimule-daemon/src/rpc.rs:300]
- `sync.rs` is still Jellyfin-specific in `execute_sync()` and calls `JellyfinClient::get_item_stream()`. Story 8.4 explicitly kept full sync migration out of scope; do not make sync provider-neutral here unless required by a found leak. [Source: hifimule-daemon/src/sync.rs:453]

### Architecture Compliance

- Subsonic URL sanitization is a security requirement: all Subsonic URLs must be sanitized before logging, and stream/download URLs must never appear in logs with credentials intact. [Source: _bmad-output/planning-artifacts/architecture.md#Subsonic-URL-Sanitization-Security-Requirement]
- All AI agents must call `sanitize_subsonic_url()` before passing Subsonic URLs to `tracing::` macros or file-based logging. [Source: _bmad-output/planning-artifacts/architecture.md#Enforcement-Guidelines]
- Raw Subsonic passwords must remain inside `providers/subsonic.rs` and must not be stored in `AppState`, returned through RPC, or written to logs. [Source: _bmad-output/planning-artifacts/architecture.md#Subsonic-Auth-Internals]
- Continue routing media server communication through `Arc<dyn MediaProvider>` and provider modules; do not construct Subsonic REST URLs outside `providers/subsonic.rs`. [Source: _bmad-output/planning-artifacts/project-context.md#Core-Principles]

### Story Boundaries

- In scope: sanitizer utility, Subsonic provider/error redaction, provider factory/RPC error redaction, tests proving raw auth query values do not leak.
- In scope if encountered: small message-sanitizer refactor to remove duplicated redaction logic, provided behavior is covered by tests.
- Out of scope: API-key authentication support, provider-neutral sync migration, browse RPC migration, UI changes, and Story 8.6 album-level fallback.
- Do not add the `opensubsonic` crate back. Story 8.3 removed it after implementing the local client.

### Previous Story Intelligence

- Story 8.3 deferred comprehensive `t=` and `s=` sanitization in all error/log paths to this story. It also established that raw passwords stay inside `providers/subsonic.rs` and `CredentialKind::Password` debug output is redacted. [Source: _bmad-output/implementation-artifacts/8-3-subsonicprovider-adapter.md#Review-Findings]
- Story 8.4 fixed a previous `sanitize_secret_message` bug class: single-character keys (`u`, `p`, `t`, `s`) can match inside normal words unless guarded by separators. Keep tests for this exact failure mode. [Source: _bmad-output/implementation-artifacts/8-4-runtime-server-type-detection-factory.md#Review-Findings]
- Story 8.4 left sync, auto-fill, scrobble, browse, and image proxy paths mostly Jellyfin-first for compatibility. Avoid broad migration while doing the security sweep. [Source: _bmad-output/implementation-artifacts/8-4-runtime-server-type-detection-factory.md#Story-Boundaries]
- Recent commits are `784250f Change domain`, `50a0d79 Review 8.4`, `1766ca4 Dev 8.4`, `a3c2f52 Story 8.4`, and `63f6f69 Review 8.3`; implementation should build on the provider/factory changes rather than reworking older Jellyfin flows.

### Latest Technical Context

- OpenSubsonic's common API parameters include `u`, `p`, `t`, `s`, `v`, `c`, and `f`; either `p` or both `t` and `s` authenticate traditional Subsonic requests, while `v`, `c`, and `f` are normal non-secret protocol/client/format parameters. [Source: https://opensubsonic.netlify.app/docs/api-reference/]
- The classic Subsonic API recommends token authentication since API 1.13.0: generate a salt per REST call, send it as `s`, and send `t=md5(password + salt)`. This confirms both token and salt must be treated as sensitive in logs. [Source: https://www.subsonic.org/pages/api.jsp]
- OpenSubsonic API-key authentication exists as an extension using `apiKey`, but HifiMule's current implementation uses password-derived token/salt auth. API-key support is out of scope for this story unless already present locally. [Source: https://opensubsonic.netlify.app/docs/extensions/apikeyauth/]

### Testing Guidance

- Keep sanitizer unit tests in `providers/subsonic.rs` so private helpers and signed URL generation are easy to exercise.
- Prefer `reqwest::Url` parsing for URL sanitizer tests; do not use ad hoc string splitting for production code if `Url` APIs can preserve the URL safely.
- Use the existing `mockito` dev dependency for provider/RPC HTTP failure tests; do not add `wiremock` or snapshot tooling for this story.
- Assert secrets are absent from formatted `ProviderError`, JSON-RPC `JsonRpcError.message`, and any stored `SyncFileError.error_message` introduced or touched here.
- Test exact preservation for non-secret query params where it matters, but avoid depending on query parameter ordering.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story-8.5-Subsonic-URL-Credential-Sanitization]
- [Source: _bmad-output/planning-artifacts/architecture.md#Subsonic-URL-Sanitization-Security-Requirement]
- [Source: _bmad-output/planning-artifacts/architecture.md#Subsonic-Auth-Internals]
- [Source: _bmad-output/planning-artifacts/prd.md#Server-Profile-Management]
- [Source: _bmad-output/implementation-artifacts/8-3-subsonicprovider-adapter.md]
- [Source: _bmad-output/implementation-artifacts/8-4-runtime-server-type-detection-factory.md]
- [Source: hifimule-daemon/src/providers/subsonic.rs]
- [Source: hifimule-daemon/src/providers/mod.rs]
- [Source: hifimule-daemon/src/rpc.rs]
- [Source: hifimule-daemon/src/sync.rs]
- [Source: hifimule-daemon/src/main.rs]

## Dev Agent Record

### Agent Model Used

GPT-5 Codex

### Debug Log References

- `rtk cargo test -p hifimule-daemon providers::subsonic providers:: rpc --no-fail-fast` could not run literally because Cargo accepts one test filter; split focused runs were used instead.
- `rtk cargo test -p hifimule-daemon subsonic --no-fail-fast` - passed, 32 tests.
- `rtk cargo test -p hifimule-daemon providers --no-fail-fast` - passed, 54 tests.
- `rtk cargo test -p hifimule-daemon rpc --no-fail-fast` - passed, 35 tests.
- `rtk cargo fmt --check` - passed after `rtk cargo fmt`.
- `rtk cargo test -p hifimule-daemon` - passed, 271 tests.

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- Added `sanitize_subsonic_url(&Url)` to redact only `u`, `p`, `t`, and `s` query values while keeping non-secret parameters parseable.
- Replaced Subsonic provider message redaction with `sanitize_subsonic_message`, using `[REDACTED]` and separator-guarded matching for auth keys and password text.
- Sanitized Subsonic/auto `server.connect` JSON-RPC failure messages before returning them to callers.
- Audited sync/main/RPC paths; sync remains Jellyfin-specific, and no provider-neutral migration was introduced.
- Added sanitizer, provider factory, and RPC tests proving username, password, token, and salt values do not leak through formatted errors.

### File List

- hifimule-daemon/src/providers/subsonic.rs
- hifimule-daemon/src/providers/mod.rs
- hifimule-daemon/src/rpc.rs
- _bmad-output/implementation-artifacts/sprint-status.yaml
- _bmad-output/implementation-artifacts/8-5-subsonic-url-credential-sanitization.md

### Change Log

- 2026-05-09: Implemented Subsonic URL/message credential sanitization and redaction coverage; story ready for review.
