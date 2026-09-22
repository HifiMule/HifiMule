---
title: 'Fix Audiobookshelf detection and authentication'
type: 'bugfix'
created: '2026-09-22'
status: 'done'
baseline_commit: '1b7d723a02df1cf14e8c662469b2f7135599cf77'
context:
  - '_bmad-output/implementation-artifacts/investigations/audiobookshelf-connection-investigation.md'
  - 'docs/audiobookshelf-integration-contract.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Auto mode cannot identify Audiobookshelf, while explicit Audiobookshelf login rejects valid v2.36.1 responses because HifiMule expects JWT fields at the response top level instead of under `user`. The catch-all error is localized but never written to `daemon.log`, leaving packaged-app failures opaque.

**Approach:** Add strict status-based detection, parse the real nested login/refresh token envelope, route Auto submissions into the existing two-stage library picker when Audiobookshelf is detected, and log only sanitized failure classifications.

## Boundaries & Constraints

**Always:** Append `/status` to the normalized user-supplied base URL and require HTTP success plus JSON `app === "audiobookshelf"`; support both root and `/audiobookshelf/` bases; keep tokens, passwords, upstream IDs, raw response bodies, and submitted URLs out of RPC results and logs; preserve Jellyfin/Subsonic detection and connect behavior; retain the existing one-library commit flow.

**Ask First:** Any change to persisted schema, vault format, credential model, public RPC payloads, or support beyond local username/password authentication.

**Never:** Hard-code `/audiobookshelf/status`; log credentials, tokens, request bodies, raw responses, or server URLs; fall back to legacy `user.token`; automatically persist a server before library selection; broaden into catalogue/playback support.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Auto root base | `http://host:port`, `/status` identifies Audiobookshelf | Enter authenticated discovery and show the one-library picker | No generic `server.connect` persistence |
| Auto prefixed base | `http://host:port/audiobookshelf/` | Probe `/audiobookshelf/status`, then discover at the same normalized base | Preserve prefix exactly once |
| Explicit login | Valid response with `user.accessToken` and `user.refreshToken` | Discover accessible Books/Podcasts libraries | Missing/empty nested token is a sanitized response-shape error |
| Token refresh | `/auth/refresh` returns the login envelope | Replace access token and optional rotated refresh token once | A second 401 remains authentication failure |
| Non-ABS status | 404, invalid JSON, or another `app` value | Continue existing Subsonic/Jellyfin detection | Return Unknown if no provider matches |
| Provider failure | Transport, HTTP, or deserialize error | UI receives localized structured error | `daemon.log` records only method/provider class and safe status/category |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/providers/audiobookshelf.rs` -- Login/refresh DTOs, token extraction, and adapter tests.
- `hifimule-daemon/src/providers/mod.rs` -- Provider-neutral URL probing and detection tests.
- `hifimule-daemon/src/rpc.rs` -- Audiobookshelf error classification/logging and RPC regression coverage.
- `hifimule-ui/src/login.ts` -- Auto submission routing into discovery/picker.
- `hifimule-ui/tests/audiobookshelfSetup.test.mjs` -- Source-level UI regression assertions.
- `hifimule-daemon/tests/fixtures/audiobookshelf/2.36.1/` -- Corrected fixture evidence for nested JWT fields.
- `docs/audiobookshelf-integration-contract.md` -- Documents the nested v2.26+ login/refresh token envelope.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/providers/audiobookshelf.rs` and v2.36.1 fixtures -- model nested `user` JWT fields for login and refresh; add rejecting/accepting regression cases.
- [x] `hifimule-daemon/src/providers/mod.rs` -- add strict `{base}/status` detection before existing probes, with root/prefix/non-match tests.
- [x] `hifimule-ui/src/login.ts` -- re-probe Auto on submit and invoke Audiobookshelf discovery/picker when detected, without changing explicit selection.
- [x] `hifimule-daemon/src/rpc.rs` -- write sanitized Audiobookshelf failure categories to daemon logging before structured localization.
- [x] `hifimule-ui/tests/audiobookshelfSetup.test.mjs` and Rust tests -- cover Auto routing, nested tokens, refresh, logging redaction, and provider non-regression.
- [x] `hifimule-ui/src-tauri/bundled-libs/.gitkeep` -- restore the packaging-generated unrelated delta.

**Acceptance Criteria:**
- Given either tested base URL, when Auto is submitted, then HifiMule detects Audiobookshelf and opens the existing library picker without creating an unscoped server.
- Given a real v2.36.1 nested login or refresh response, when parsed, then Bearer discovery succeeds and secrets remain memory-only.
- Given a discovery failure, when RPC returns an error, then `daemon.log` contains a sanitized category/status but no URL, username, password, token, or body.
- Given Jellyfin or Subsonic, when probed or connected, then existing behavior and tests remain unchanged.

## Spec Change Log

- 2026-09-22: Review hardening added bounded non-redirecting auth requests, re-auth logging, setup cleanup, credential compensation reporting, and display-name preservation. The pre-existing case-folded URL normalization concern was recorded in `deferred-work.md` because correcting portable identity requires a separate migration decision.

## Verification

**Commands:**
- `rtk npm run build:daemon -- test -p hifimule-daemon` -- all daemon and Audiobookshelf contract tests pass.
- `rtk proxy node --test hifimule-ui/tests/audiobookshelfSetup.test.mjs` -- focused UI routing and redaction tests pass.
- `rtk npm run build --prefix hifimule-ui` -- TypeScript and production Vite build pass.
- `rtk proxy rustfmt --edition 2024 --check <touched Rust files>` -- touched Rust files are formatted.
- `rtk git diff --check` -- no whitespace errors.

**Results:** 1,059 daemon unit tests passed (6 ignored), 5 Audiobookshelf contract tests passed, 5 focused UI tests passed, and the production UI build completed successfully. The initial sandboxed daemon run could not bind local mock-server ports; the same suite passed outside that restriction.

## Suggested Review Order

**Detection and authentication**

- Start with strict base-relative status detection and unchanged legacy fallbacks.
  [`mod.rs:625`](../../hifimule-daemon/src/providers/mod.rs#L625)

- Parse the real nested token envelope with bounded, non-redirecting requests.
  [`audiobookshelf.rs:89`](../../hifimule-daemon/src/providers/audiobookshelf.rs#L89)

- Refresh once, reject malformed tokens, and classify auth endpoints accurately.
  [`audiobookshelf.rs:236`](../../hifimule-daemon/src/providers/audiobookshelf.rs#L236)

**RPC safety and persistence**

- Emit classification-only diagnostics without URLs, usernames, bodies, or secrets.
  [`rpc.rs:2615`](../../hifimule-daemon/src/rpc.rs#L2615)

- Keep discovery ephemeral until an explicit one-library commit.
  [`rpc.rs:2693`](../../hifimule-daemon/src/rpc.rs#L2693)

- Precompute fallible state and compensate credentials before publishing the provider.
  [`rpc.rs:2801`](../../hifimule-daemon/src/rpc.rs#L2801)

- Apply the same safe logging contract to scoped re-authentication.
  [`rpc.rs:2944`](../../hifimule-daemon/src/rpc.rs#L2944)

**UI routing and lifecycle**

- Express Auto-versus-explicit Audiobookshelf routing as a testable decision.
  [`audiobookshelfSetup.ts:9`](../../hifimule-ui/src/audiobookshelfSetup.ts#L9)

- Re-probe Auto submissions and enter the existing library picker when detected.
  [`login.ts:277`](../../hifimule-ui/src/login.ts#L277)

- Cancel abandoned setups and serialize Back against an in-flight commit.
  [`login.ts:315`](../../hifimule-ui/src/login.ts#L315)

**Regression evidence**

- Cover root and prefixed status endpoints plus exact-marker fallback behavior.
  [`mod.rs:848`](../../hifimule-daemon/src/providers/mod.rs#L848)

- Reject the obsolete flattened token response shape explicitly.
  [`audiobookshelf.rs:639`](../../hifimule-daemon/src/providers/audiobookshelf.rs#L639)

- Verify diagnostic output excludes upstream error details.
  [`rpc.rs:8672`](../../hifimule-daemon/src/rpc.rs#L8672)

- Exercise the Auto routing decision and retain source-level wiring guards.
  [`audiobookshelfSetup.test.mjs:18`](../../hifimule-ui/tests/audiobookshelfSetup.test.mjs#L18)
