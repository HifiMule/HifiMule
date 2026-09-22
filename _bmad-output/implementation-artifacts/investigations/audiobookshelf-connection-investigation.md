# Investigation: Audiobookshelf connection and auto-detection

## Hand-off Brief

1. **What happened.** The user reports that a packaged HifiMule build cannot connect to Audiobookshelf and emits nothing to `daemon.log`; code inspection confirms auto-detection currently probes only Subsonic and Jellyfin.
2. **Where the case stands.** Active. The supplied unauthenticated endpoint is confirmed reachable and identifies Audiobookshelf 2.36.1, while the exact URL/provider choice and UI error from the failed attempt are still missing.
3. **What's needed next.** Trace the entered URL through UI, RPC, provider URL construction, and release logging, then reproduce against the confirmed local endpoint.

## Case Info

| Field | Value |
| --- | --- |
| Ticket | N/A |
| Date opened | 2026-09-22 |
| Status | Active |
| System | HifiMule packaged macOS build; Audiobookshelf 2.36.1 at redacted local endpoint `192.0.2.67:13378` |
| Evidence sources | User report, source tree, read-only HTTP status probe |

## Problem Statement

User report (endpoint redacted): “I built the app and tried to connect to a audiobookshelf server, with no success. Nothing on daemon.log. Regarding the auto detection, we not use /audiobookshelf/status to detect a audiobookshelf server. Like http://192.0.2.67:13378/audiobookshelf/status”.

## Evidence Inventory

| Source | Status | Notes |
| --- | --- | --- |
| User-observed packaged-app failure | Partial | No exact entered URL, selected provider, or visible UI error yet. |
| `hifimule-daemon/src/providers/mod.rs` | Available | `probe_url` probes Subsonic then Jellyfin and returns Unknown; no Audiobookshelf status probe. |
| `hifimule-ui/src/login.ts` | Available | Explicit Audiobookshelf selection invokes the dedicated discovery RPC; Auto uses generic connect. |
| `hifimule-daemon/src/main.rs` | Available | File logging is opt-in at explicit `log_to_file` call sites, not automatic request tracing. |
| `GET http://192.0.2.67:13378/audiobookshelf/status` | Available | 200 JSON: `app=audiobookshelf`, `serverVersion=2.36.1`, local auth enabled. |
| `daemon.log` from failed attempt | Missing | User reports no relevant output. |

## Investigation Backlog

| # | Path to Explore | Priority | Status | Notes |
| - | --- | --- | --- | --- |
| 1 | Reproduce explicit and Auto flows against the supplied endpoint | High | Open | Requires exact URL/provider selection or testing both likely forms. |
| 2 | Trace provider URL construction for reverse-proxy base paths | High | Open | Determine whether `/login` or `/audiobookshelf/login` is called. |
| 3 | Audit RPC/provider error logging in release mode | High | Open | Explain the empty `daemon.log`. |
| 4 | Define and test safe Audiobookshelf status detection | Medium | Open | Validate response signature and base URL semantics. |

## Timeline of Events

| Time | Event | Source | Confidence |
| --- | --- | --- | --- |
| 2026-09-22 | User built HifiMule and attempted an Audiobookshelf connection without success. | User report | Confirmed |
| 2026-09-22 11:34:08 GMT | Supplied `/audiobookshelf/status` returned HTTP 200 and identified Audiobookshelf 2.36.1 with local auth. | Read-only HTTP probe | Confirmed |

## Confirmed Findings

### Finding 1: Auto-detection does not probe Audiobookshelf

**Evidence:** `hifimule-daemon/src/providers/mod.rs:624`

**Detail:** `probe_url` checks the Subsonic ping envelope and Jellyfin public information endpoint, then returns `ServerType::Unknown`. There is no Audiobookshelf branch.

### Finding 2: The proposed status endpoint is an unauthenticated positive identifier

**Evidence:** HTTP 200 response observed from the redacted local endpoint `http://192.0.2.67:13378/audiobookshelf/status` on 2026-09-22.

**Detail:** The JSON contains `{"app":"audiobookshelf","serverVersion":"2.36.1",...}` and advertises local authentication.

### Finding 3: Explicit Audiobookshelf selection bypasses generic auto-detection

**Evidence:** `hifimule-ui/src/login.ts:270`

**Detail:** Selecting Audiobookshelf calls `server.audiobookshelf.discover`; only the Auto path calls generic `server.connect`.

## Deduced Conclusions

### Deduction 1: Missing auto-detection is real but may not fully explain explicit-provider failure

**Based on:** Findings 1 and 3.

**Reasoning:** Auto cannot recognize Audiobookshelf, but explicit selection follows a different path. If explicit Audiobookshelf was selected, another issue—most likely URL base-path construction or request failure—must account for the connection failure.

**Conclusion:** Detection and connection must be investigated as separate defects.

## Hypothesized Paths

### Hypothesis 1: Auto-detection fails because no Audiobookshelf status probe exists

**Status:** Confirmed

**Theory:** Auto returns Unknown for Audiobookshelf because it never requests a status endpoint.

**Supporting indicators:** Direct source inspection and the verified positive endpoint.

**Would confirm:** Existing source lacks an Audiobookshelf probe and the endpoint returns an identifying response.

**Would refute:** Another code path detects Audiobookshelf before `probe_url` returns.

**Resolution:** Confirmed by `hifimule-daemon/src/providers/mod.rs:624-663` and the live 200 response.

### Hypothesis 2: The failed connection used the host root while Audiobookshelf is mounted at `/audiobookshelf`

**Status:** Open

**Theory:** Discovery appends `/login` and `/api/libraries` to the entered base. Entering `http://192.0.2.67:13378` would target the wrong routes if the actual base is `http://192.0.2.67:13378/audiobookshelf`.

**Supporting indicators:** The verified status URL contains the `/audiobookshelf` prefix.

**Would confirm:** Root `/login` fails while `/audiobookshelf/login` is the valid route, and the failed attempt used the root URL.

**Would refute:** The failed attempt entered the prefixed base URL or root `/login` is valid.

**Resolution:** Open.

### Hypothesis 3: No daemon log entry exists because the discovery path has no file-log call

**Status:** Open

**Theory:** Errors are converted into JSON-RPC responses but never passed to `log_to_file`.

**Supporting indicators:** `handle_audiobookshelf_discover` maps provider errors directly; file logging is a manual helper.

**Would confirm:** Caller-chain inspection finds no log invocation around discovery or RPC failure.

**Would refute:** A shared RPC boundary logs every error to the file.

**Resolution:** Open.

## Missing Evidence

| Gap | Impact | How to Obtain |
| --- | --- | --- |
| Exact URL entered in HifiMule | Distinguishes missing base path from authentication or transport failure. | User confirmation or reproduction. |
| Provider choice used (Auto vs Audiobookshelf) | Determines whether detection was in the failing path. | User confirmation. |
| Visible UI error text | Narrows transport/auth/parse/RPC causes. | User report or screenshot. |
| Browser console / RPC response | Confirms whether the request reached the daemon and the returned error code. | Reproduce in development or inspect packaged webview logs. |

## Source Code Trace

| Element | Detail |
| --- | --- |
| Error origin | Not yet established. |
| Trigger | Submitting the login form in Auto or explicit Audiobookshelf mode. |
| Condition | Auto lacks an Audiobookshelf probe; explicit mode may have a base-path or request issue. |
| Related files | `hifimule-ui/src/login.ts`, `hifimule-ui/src/rpc.ts`, `hifimule-daemon/src/rpc.rs`, `hifimule-daemon/src/providers/mod.rs`, `hifimule-daemon/src/providers/audiobookshelf.rs`, `hifimule-daemon/src/main.rs` |

## Conclusion

**Confidence:** Medium

Auto-detection definitively omits Audiobookshelf even though the supplied status endpoint is a valid positive identifier. The explicit connection failure and absent log remain separate open threads until the exact submitted URL/provider selection and provider/logging caller chains are verified.

## Recommended Next Steps

### Fix direction

Pending causal trace. Likely mechanisms are an Audiobookshelf status probe with strict response validation, canonical base-path handling, and sanitized RPC failure logging.

### Diagnostic

Test root and prefixed status/login routes, trace the exact submitted URL, and audit all error boundaries between provider discovery and `daemon.log`.

## Reproduction Plan

1. Submit the host root with Auto and explicit Audiobookshelf.
2. Submit the `/audiobookshelf` base with Auto and explicit Audiobookshelf.
3. Capture RPC error codes, visible error text, requested paths, and daemon-log output.

## Side Findings

- The status response advertises only local authentication, matching Story 17.2's supported auth mode.

## Follow-up: 2026-09-22

### New Evidence

- The user tested both `http://192.0.2.67:13378` and `http://192.0.2.67:13378/audiobookshelf/` as HifiMule server URLs (endpoint redacted).

### Additional Findings

- Testing the prefixed base means Hypothesis 2 cannot by itself explain every failed attempt. It remains open until the provider choice and visible error are known, because Auto and explicit Audiobookshelf take different RPC paths.

### Updated Hypotheses

- Hypothesis 2 remains **Open**, narrowed: an omitted prefix may explain the root-URL attempt but not the prefixed-URL attempt.
- Hypothesis 3 remains **Open** pending caller-chain inspection.

### Backlog Changes

- Exact submitted URLs are no longer missing evidence.
- Highest-value missing evidence is now the provider selection used for each attempt and the visible UI error text.

### Updated Conclusion

**Confidence:** Medium

The new evidence rules out “the user only entered the wrong base URL” as a complete explanation. Auto-detection omission remains confirmed; diagnosing the prefixed explicit-provider attempt requires the selected provider mode and UI/RPC error.

## Follow-up: 2026-09-22 #2

### New Evidence

- Auto mode reports “type of server unknown.”
- Explicit Audiobookshelf mode reports “Impossible to connect.”
- Live route inventory: both `/status` and `/audiobookshelf/status` return HTTP 200; GET requests to both `/login` and `/audiobookshelf/login` return HTTP 301 to the corresponding trailing-slash route; OPTIONS reports POST support.
- Static source inventory confirms Auto reaches `probe_url`, explicit Audiobookshelf reaches `handle_audiobookshelf_discover`, and the shared JSON-RPC error return does not itself call `log_to_file`.
- Relevant provider/RPC tests are available, but no test currently exercises live-style status detection or redirect/base-path behavior.
- Recent committed history does not contain Story 17.2 because the implementation is still an uncommitted working-tree change; version-control evidence is therefore partial.

### Additional Findings

- The two UI messages corroborate the source split: Auto fails at detection, while explicit mode reaches the provider-specific path and then receives a generic connection failure.
- Both root and prefixed status routes identify the server, so detection can support either submitted base by appending `/status` to the normalized user URL. Hard-coding `/audiobookshelf/status` would break the already-prefixed input.
- Empty `daemon.log` is now strongly supported as a logging-path gap rather than evidence that the RPC never reached the daemon.

### Updated Hypotheses

- Hypothesis 1 remains **Confirmed**.
- Hypothesis 2 remains **Open** but weakened: both base forms expose valid status and login route families, so base prefix alone is not sufficient.
- Hypothesis 3 moves toward **Confirmed**; the next source-trace pass must rule out logging at the outer RPC/server boundary before changing status.
- New Hypothesis 4 (**Open**): explicit discovery fails because the validated mock contract does not cover the live server's redirect/trailing-slash or response behavior. Confirm by tracing the actual request/response boundary with sanitized diagnostics or a live reproduction.

### Backlog Changes

- Detection response shape and both submitted base URLs are mapped.
- Next priority is the explicit discovery request/response trace, followed by the outer RPC logging refutation pass.

### Updated Conclusion

**Confidence:** Medium-high

Auto failure has a confirmed cause: Audiobookshelf is absent from `probe_url`. Explicit failure is a distinct provider-boundary defect hidden by generic UI messaging and absent daemon error logging; current evidence narrows it to live HTTP contract differences rather than simple user URL omission.

## Follow-up: 2026-09-22 #3

### New Evidence

- `AudiobookshelfProvider::login` deserializes `accessToken` and `refreshToken` from the top level of the login response (`hifimule-daemon/src/providers/audiobookshelf.rs:82-90`, `:198-199`).
- Audiobookshelf's official JWT announcement documents the v2.26+ login response as `{ "user": { "accessToken": ..., "refreshToken": ... }, ... }`; its current web and mobile clients likewise read `response.user.accessToken` and `response.user.refreshToken`.
- The official refresh contract says `/auth/refresh` returns the same response shape as `/login`, while HifiMule's `RefreshResponse` also expects top-level tokens (`hifimule-daemon/src/providers/audiobookshelf.rs:95-103`, `:248-251`).
- Story 17.1's local login fixture flattened the token fields to the top level, so all adapter tests validated the wrong shape and passed.
- A dummy POST to the live prefixed `/login` endpoint returned 401 directly, not a redirect. This refutes redirect handling as the primary explicit-login defect.
- `audiobookshelf_error_to_rpc` maps deserialization and transport errors through its catch-all `AUDIOBOOKSHELF_CONNECTION_FAILED` branch (`hifimule-daemon/src/rpc.rs:2651-2658`), which the UI localizes to the exact reported French message.
- The shared RPC error return at `hifimule-daemon/src/rpc.rs:646-650` performs no file logging; no surrounding Audiobookshelf discovery path calls `log_to_file`.

### Additional Findings

- The explicit connection failure has a deterministic causal chain: valid login response → top-level token deserialization fails → catch-all RPC error → “Impossible de se connecter à Audiobookshelf.”
- Access-token refresh would fail for the same nested-response reason even after login parsing is fixed.
- The absence of `daemon.log` evidence is expected behavior in the current implementation, not proof that the request never reached the daemon.
- Auto-detection should request `{normalized_user_base}/status` and require a successful JSON response whose `app` field is exactly `audiobookshelf`; this handles both root and `/audiobookshelf/` inputs.

### Updated Hypotheses

- Hypothesis 1 remains **Confirmed**: Auto lacks Audiobookshelf probing.
- Hypothesis 2 is **Refuted** as the primary explicit failure. Both base forms expose the routes, and POST does not redirect.
- Hypothesis 3 is **Confirmed**: neither the discovery handler nor the shared RPC error boundary records the failure in `daemon.log`.
- Hypothesis 4 is **Confirmed**: the HifiMule DTO is incompatible with the nested live v2.36.1 login/refresh response.

### Backlog Changes

- Detection, explicit login, refresh parsing, UI error collapse, and daemon logging causes are traced.
- Remaining work is implementation and regression verification rather than further diagnosis.

### Updated Conclusion

**Confidence:** High

Three defects combine: Auto never probes Audiobookshelf; explicit login/refresh deserialize tokens from the wrong JSON level; and the resulting sanitized error is not written to `daemon.log`. The wrong Story 17.1 fixture mirrored the implementation rather than the real v2.36.1 response, allowing the defect through deterministic tests.
