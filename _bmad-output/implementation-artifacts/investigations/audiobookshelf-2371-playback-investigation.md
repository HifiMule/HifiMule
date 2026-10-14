# Investigation: Audiobookshelf 2.37.1 playback

## Hand-off Brief

Audiobook and podcast playback rejected Audiobookshelf 2.37.1 because both adapters required exactly 2.36.1; prefixed deployments also rejected unprefixed media routes. Both blockers are fixed using major-version-2 admission and exact item/file URL construction beneath the configured deployment prefix. Regression tests, the full daemon suite, and live provider verification for both media roles pass.

## Case Info

| Field | Value |
| --- | --- |
| Date opened | 2026-10-14 |
| Status | Concluded; implemented and verified |
| System | Windows; Audiobookshelf 2.37.1 |
| Evidence sources | Provider source, controlled test server, existing contract, git history |

## Problem Statement

User reports that audio cannot be read from Audiobookshelf 2.37.1 for books or podcasts. The supplied disposable account authorizes controlled playback testing. Credentials, tokens, item IDs, and session IDs are omitted from this record.

## Evidence Inventory

| Source | Status | Notes |
| --- | --- | --- |
| Provider source | Available | Both playback adapters explicitly gate on 2.36.1 |
| Test server | Available | Book and podcast sessions and one-byte authenticated ranges verified |
| Prior contract | Available | Existing fixtures document 2.36.1 only |
| Installed application reproduction | Missing | Live server transport verified; complete decoder playback still to verify |

## Confirmed Findings

1. `hifimule-daemon/src/providers/audiobookshelf.rs:1112` and `:3533` reject 2.37.1 before streaming.
2. At baseline commit `2c19da7`, the adapter uses `base.join` on a leading-slash media path, then compares with an item endpoint that retains the configured prefix (`hifimule-daemon/src/providers/audiobookshelf.rs:1169`, `:3581`). With `/audiobookshelf`, the origins match but item paths differ, so admission fails.
3. Live probes on 2026-10-14 returned `serverVersion: 2.37.1`, `playMethod: 0`, correct library/item/episode identity, AAC with `audio/mp4` for a book and MP3 with `audio/mpeg` for a podcast. Media paths were unprefixed `/api/items/{item}/file/{ino}`. Both configured-prefix and origin-root media requests returned 206. Configured-prefix probes returned matching content type, `Accept-Ranges: bytes`, valid `Content-Range`, and one byte. Sessions were closed with HTTP 200; no progress was written.

## Hypothesized Paths

| Hypothesis | Status | Resolution |
| --- | --- | --- |
| Server upgrade breaks version admission | Confirmed | Exact equality gates reject observed version |
| Media itself is unavailable | Refuted for sampled items | Both formats returned valid authenticated ranges |
| Prefix handling independently blocks playback | Confirmed | Observed content path loses configured prefix under existing URL join |

## Conclusion

**Confidence: High** for the two admission blockers. Full installed-player output has not been observed. Following the user's compatibility direction, accept major version 2 including future minor/patch releases, and resolve verified item-file paths beneath the configured base URL while preserving identity, origin, format, and range checks.

## Reproduction Plan

Exercise real provider resolution against deterministic HTTP doubles for 2.36.1, 2.37.1, and future 2.x releases with root and prefixed URLs, including rejected unsupported major versions and unsafe URLs. Verify against the supplied server using an opt-in provider smoke test. Run daemon tests via the project's audio-runtime wrapper.

## Verification Outcome

The regressions failed against the initial implementation and pass after the fix. Final verification passed 79 provider tests and 1,488 daemon/contract tests. The opt-in live provider smoke test resolved both media roles, verified ranges and the configured prefix, and closed both sessions successfully without progress writes. Source-file formatting passed; the broader workspace formatting check reports pre-existing unrelated failures.
