---
title: 'Make album start deserialization and supersession reliable'
type: 'bugfix'
created: '2026-09-18'
status: 'done'
baseline_commit: '77aa6584c16b6de6a3c39fef9b930fbba3b67d5e'
context:
  - '{project-root}/_bmad-output/planning-artifacts/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/15-10-preserve-an-album-s-relative-loudness-with-consistent-gain.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Starting an OpenSubsonic album fails with `provider response deserialization failed: invalid type: newtype struct, expected any valid JSON value`. Story 15.10 stores `replayGain` as `RawValue`, but the response path first converts the body to `serde_json::Value` and then deserializes the typed envelope; `RawValue` requires the original JSON deserializer and fails during the second conversion. A rapid second album click is also rejected as “An album resolution is pending” instead of replacing the obsolete fetch.

**Approach:** Deserialize the typed Subsonic envelope directly from response bytes, then perform existing failed-status/error mapping against the typed envelope. Let a fresh album command cancel and supersede an older pending resolution, and treat that expected supersession as silent in the album-play UI while preserving stale-owner fencing.

## Boundaries & Constraints

**Always:** Preserve existing Subsonic authentication, HTTP-status mapping, API-error sanitization, generic response bodies, classic Subsonic compatibility, Story 15.10 evidence behavior, command-ID reuse protection, generation/revision validation and mutation-guard release. A newer accepted album request must cancel provider work for the older reservation before becoming pending. Add regression tests through production response and RPC paths.

**Ask First:** Any broad public RPC/provider contract change or removal of the extreme-exponent tolerance requirement.

**Never:** Enable serde_json `arbitrary_precision` globally, accept malformed ReplayGain as valid evidence, expose raw metadata publicly, weaken credential/error redaction, allow a cancelled reservation to commit, or show an error toast for expected album supersession.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Valid album | OpenSubsonic album response containing numeric `albumGain` and `albumPeak` | Album deserializes and retains qualified evidence | N/A |
| Extreme optional number | `albumGain: 1e400` inside `replayGain` | Album and songs deserialize; evidence is typed as rejected | Playback remains available at unity |
| Failed API response | Typed failed response with an error code/message | Existing auth/not-found/HTTP mapping remains unchanged | Message remains sanitized |
| Rapid replacement | Album B is clicked while album A provider resolution is pending | A is cancelled and B becomes the sole pending resolution | A reports typed `ALBUM_SUPERSEDED`; UI suppresses it |
| Late obsolete result | Album A returns after B superseded it | A cannot commit or alter B/current playback | Owner token rejects stale commit |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/providers/subsonic.rs` -- HTTP response parsing, typed envelope/error mapping, ReplayGain DTO parsing and provider tests.
- `hifimule-daemon/src/playback/session/album_admission.rs` -- serialized reservation ownership, cancellation and stale-token commit fencing.
- `hifimule-daemon/src/playback/session/album_admission_tests.rs` -- pending reservation supersession and atomicity coverage.
- `hifimule-daemon/src/rpc.rs` and `hifimule-daemon/src/rpc/album_tests.rs` -- provider-fetch cancellation and typed supersession error behavior.
- `hifimule-ui/src/rpc.ts` -- album-play wrapper suppresses only the expected typed supersession result.
- `Cargo.toml` -- keeps the narrowly scoped serde_json `raw_value` feature required for bounded optional metadata parsing.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/providers/subsonic.rs` -- parse `SubsonicEnvelope<T>` directly from response bytes and map API failures from its typed fields.
- [x] `hifimule-daemon/src/providers/subsonic.rs` -- add valid and overflow ReplayGain regression coverage through the real `get_album` HTTP fixture path.
- [x] `hifimule-daemon/src/playback/session/album_admission.rs` -- validate a new album request, cancel the prior pending reservation, and reserve the new request without weakening token fencing.
- [x] `hifimule-daemon/src/playback/session/album_admission_tests.rs` and `hifimule-daemon/src/rpc/album_tests.rs` -- prove cancellation, latest-request ownership, late-result rejection and distinct same/different album replacement.
- [x] `hifimule-daemon/src/rpc.rs` and `hifimule-ui/src/rpc.ts` -- expose and silently consume a specific `ALBUM_SUPERSEDED` outcome while continuing to surface unrelated conflicts.
- [x] `_bmad-output/implementation-artifacts/15-10-preserve-an-album-s-relative-loudness-with-consistent-gain.md` -- record the review correction and validation evidence.

**Acceptance Criteria:**
- Given a valid OpenSubsonic album response with ReplayGain, when album playback resolves the provider response, then deserialization succeeds and the album policy receives the numeric pair.
- Given overflow or malformed optional ReplayGain, when the same response is resolved, then the album remains playable with rejected evidence and unity fallback.
- Given an API-level failed response, when it is parsed directly, then existing sanitized provider error classification is preserved.
- Given any album resolution is pending, when a fresh valid album command arrives, then the old fetch is cancelled, the new album becomes the only committable reservation and no pending-resolution popup appears.
- Given a superseded provider result arrives late, when it attempts to commit, then it receives `ALBUM_SUPERSEDED`/stale-token rejection and cannot change the queue, policy or active newer request.
- Given another playback conflict or provider failure, when album start fails, then the UI still shows the error.

## Spec Change Log

## Verification

**Commands:**
- `rtk npm run build:daemon -- test -p hifimule-daemon providers::subsonic::tests` -- all Subsonic provider tests pass.
- `rtk npm run build:daemon -- test -p hifimule-daemon` -- full daemon suite passes.
- `rtk tsc --noEmit` through the repository UI check -- album supersession handling type-checks.
- `rtk cargo fmt --all -- --check` -- formatting passes.
- `rtk git diff --check` -- no whitespace errors.

**Results:** Subsonic provider suite: 61 passed. Focused album admission, RPC and persistence regressions passed. Full daemon suite: 955 passed, 6 ignored. UI production build, Rust formatting and diff checks passed. Review fixes also gate ReplayGain to OpenSubsonic, preserve tolerant failed-response mapping, prioritize supersession over obsolete provider results and validate persisted album membership with a bounded digest.

## Suggested Review Order

**Album admission and supersession**

- Start here: resolution, policy selection and commit share one fenced request lifecycle.
  [`rpc.rs:895`](../../hifimule-daemon/src/rpc.rs#L895)

- Fresh valid commands cancel prior work while stale commands leave it untouched.
  [`album_admission.rs:143`](../../hifimule-daemon/src/playback/session/album_admission.rs#L143)

- The UI silently consumes only the expected typed supersession result.
  [`rpc.ts:262`](../../hifimule-ui/src/rpc.ts#L262)

**Provider decoding and policy**

- Raw response preservation keeps extreme optional numbers from breaking album decoding.
  [`subsonic.rs:1247`](../../hifimule-daemon/src/providers/subsonic.rs#L1247)

- Capability gating prevents classic Subsonic metadata from activating album gain.
  [`subsonic.rs:339`](../../hifimule-daemon/src/providers/subsonic.rs#L339)

- One deterministic resolver freezes the album-wide scalar or an explicit unity reason.
  [`loudness.rs:81`](../../hifimule-daemon/src/playback/loudness.rs#L81)

**Durability and audio application**

- Frozen context binds policy to a compact digest of exact ordered album membership.
  [`model.rs:470`](../../hifimule-daemon/src/playback/model.rs#L470)

- Restore validation rejects altered member sources before playback resumes.
  [`persistence.rs:131`](../../hifimule-daemon/src/playback/persistence.rs#L131)

- Packed f32 samples receive the immutable scalar once after conversion.
  [`decoder.rs:157`](../../hifimule-daemon/src/playback/decoder.rs#L157)

**Regression evidence**

- Provider fixtures cover valid, overflowing, classic and malformed-error responses.
  [`subsonic.rs:2329`](../../hifimule-daemon/src/providers/subsonic.rs#L2329)

- RPC coverage proves latest-request ownership for same and different albums.
  [`album_tests.rs:253`](../../hifimule-daemon/src/rpc/album_tests.rs#L253)
