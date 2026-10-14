---
title: Restore Audiobookshelf 2.37.1 direct playback
type: bugfix
created: 2026-10-14
status: done
baseline_commit: 2c19da7
context:
  - docs/audiobookshelf-integration-contract.md
  - docs/audiobookshelf-implementation.md
---

<frozen-after-approval reason="User-authorized playback bug fix">

## Intent

**Problem:** Audiobookshelf 2.37.1 books and podcasts cannot play because direct-play admission requires exactly 2.36.1. A server configured beneath `/audiobookshelf/` also fails admission when its direct media URL is server-relative without that prefix.

**Approach:** Check only Audiobookshelf's major version, accepting the 2.x line including future releases, and resolve valid item-file media paths beneath the configured deployment URL. Preserve the established direct-format, authenticated-range, scoped-identity, and session-cleanup contracts.

## Boundaries & Constraints

**Always:** Keep authentication daemon-side. Accept Audiobookshelf major version 2; assume future minor and patch releases preserve compatibility. Preserve library/item/file/episode validation, direct method 0, supported MP3 and AAC/MP4 formats, query/fragment and foreign-origin rejection, range verification, and cleanup of rejected or retired sessions. Handle root deployments and configured base paths consistently for both media roles.

**Ask First:** Publishing or release operations outside the local fix.

**Never:** Enable HLS/transcoding, generalize compatibility to other major versions, write listening progress in live probes, or record credentials, tokens, actual upstream IDs, or media URLs in committed fixtures.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
| --- | --- | --- | --- |
| Supported major | 2.36.1, 2.37.1, or future 2.x direct session | Book/podcast representation after range validation | Existing decoder validation applies |
| Unsupported major | Major 1 or 3, empty or malformed version | No media representation | Unsupported capability; close session |
| Deployment prefix | Base `/audiobookshelf`, content `/api/items/{item}/file/{ino}` | Fetch underneath configured prefix | Identity validation precedes request |
| Already-prefixed URL | Content includes configured prefix | Preserve prefix exactly once | Reject mismatched item/file |
| Unsafe URL | Foreign origin, network path, query, fragment, traversal, wrong item/file | No authenticated media request | Existing safe failure classification |
| Unsupported method/format | HLS or unknown format | No direct representation | Existing unsupported capability |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/providers/audiobookshelf.rs` — book and podcast session admission, media validation, existing HTTP-double tests.
- `docs/audiobookshelf-integration-contract.md` — additive controlled observation record.
- `docs/audiobookshelf-implementation.md` — implementation compatibility summary.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/providers/audiobookshelf.rs` — reproduce both blockers with provider-level regression tests before implementation; retain old-version coverage, accept future 2.x releases, and reject unsupported majors and unsafe paths.
- [x] `hifimule-daemon/src/providers/audiobookshelf.rs` — share major-version and safe item-file resolution logic between both playback paths.
- [x] `docs/audiobookshelf-integration-contract.md` — append the 2026-10-14 live 2.37.1 evidence and its limits without rewriting historical observations.
- [x] `docs/audiobookshelf-implementation.md` — reflect verified direct-play versions and base-path handling.

**Acceptance Criteria:**
- Given a scoped 2.37.1 book or episode, when resolving direct playback, then an authenticated range-verified original representation is returned and its session is closed on retirement.
- Given a configured deployment prefix and unprefixed item-file path, when resolving media, then requests retain that prefix.
- Given any 2.x version with a valid direct session, when resolving playback, then minor and patch version differences do not prevent admission.
- Given an unsupported major version or invalid media path, when admission fails, then no audio representation is exposed and the created session is retired.

## Spec Change Log

- 2026-10-14: User requested major-version-only compatibility. Expanded the locked intent and acceptance tests from two observed versions to the 2.x line, including future releases. Keep scoped media URL construction, runtime identity/format/range validation, and session cleanup.

## Verification

- `rtk proxy node scripts/build-daemon.mjs test -p hifimule-daemon providers::audiobookshelf` — first observe new regression failures, then pass all provider tests.
- `rtk proxy node scripts/build-daemon.mjs test -p hifimule-daemon` — daemon suite.
- `rtk cargo fmt --all -- --check` — formatting.
- Opt-in provider smoke test against the supplied disposable account — confirm admission/range for both roles; no progress writes.

## Verification Results

- Initial regressions failed on the unchanged version gate and prefix handling. The future-2.x regression also failed against the initial allowlist before the major-version policy was implemented.
- Final provider suite: 79 passed, 0 failed, 1 ignored live smoke test.
- Final daemon suite: 1,483 unit tests and 5 contract tests passed; 11 optional tests ignored.
- Opt-in live provider smoke: 1 passed, exercising both book and podcast admission, authenticated audio ranges, deployment prefix, and successful cleanup. It submits no progress writes and does not establish audible output or installed-player seek behavior.
- Changed provider rustfmt and git diff whitespace checks passed. Whole-workspace formatting has pre-existing failures in unrelated files; those were left unchanged.
- Three independent reviews found no remaining actionable defect after triage. A pre-existing dot-identity normalization issue is recorded in deferred-work.md. The new live test's inverted cleanup assertion was corrected and retested.

## Suggested Review Order

- Check major compatibility while retaining per-session admission checks.
  [audiobookshelf.rs:2182](../../hifimule-daemon/src/providers/audiobookshelf.rs#L2182)
- Resolve exact selected file paths beneath the configured deployment URL.
  [audiobookshelf.rs:2191](../../hifimule-daemon/src/providers/audiobookshelf.rs#L2191)
- Follow podcast and audiobook callers through authenticated range verification.
  [audiobookshelf.rs:1162](../../hifimule-daemon/src/providers/audiobookshelf.rs#L1162)
  [audiobookshelf.rs:3592](../../hifimule-daemon/src/providers/audiobookshelf.rs#L3592)
- Review future-release regressions and the opt-in live verification.
  [audiobookshelf.rs:4115](../../hifimule-daemon/src/providers/audiobookshelf.rs#L4115)
  [audiobookshelf.rs:4180](../../hifimule-daemon/src/providers/audiobookshelf.rs#L4180)
- Read the additive controlled evidence and compatibility policy.
  [audiobookshelf-integration-contract.md:96](../../docs/audiobookshelf-integration-contract.md#L96)
