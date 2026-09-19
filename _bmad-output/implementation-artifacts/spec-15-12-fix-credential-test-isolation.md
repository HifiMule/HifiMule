---
title: 'Story 15.12 follow-up: isolate credential tests'
type: 'bugfix'
created: '2026-09-19'
status: 'done'
baseline_commit: '6fe5dd788e7a2be0b08f0d2c0c54afd93d4a0874'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/15-12-access-playback-as-an-always-available-destination.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The parallel daemon suite can fail when `authenticate_by_name` writes a device ID through the process-global test config path while `test_file_storage` is reading or replacing that same temporary config. The first panic poisons `CREDENTIAL_TEST_MUTEX`, producing 30 misleading follow-on failures.

**Approach:** Prevent authentication-only tests from touching shared credential storage, make the test serialization lock recover after an unrelated test panic, and add deterministic coverage for both properties.

## Boundaries & Constraints

**Always:** Preserve production device-ID persistence and authentication headers; keep tests parallel-safe; report the original failing test independently instead of cascading poison errors; retain Story 15.12's current staged work.

**Ask First:** Any production credential-format, vault, config-path, or authentication-protocol change.

**Never:** Serialize the entire Rust suite, weaken credential validation, ignore malformed configuration, add sleeps/retries to hide the race, or treat an empty file as valid configuration.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Parallel authentication and credential storage tests | Authentication tests run while a credential test owns a temporary config path | Authentication uses a deterministic test-only device identity and does not create, truncate, or rewrite the credential test's config | Storage assertions remain authoritative |
| Earlier credential test panics | `CREDENTIAL_TEST_MUTEX` is poisoned | The next test recovers the lock, resets the in-memory test vault, and runs independently | Do not conceal the original panic |
| Production authentication | Normal daemon build | Existing persisted device identity remains in the Jellyfin authorization header | Existing fallback remains unchanged |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/api.rs` -- owns the global test credential lock, config path, persisted device identity, Jellyfin authentication, and the primary failing tests.
- `hifimule-daemon/src/rpc.rs` -- contains many parallel tests using `credential_test_lock`; validates that poison recovery does not alter their isolation contract.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/api.rs` -- isolate authentication test device identity from `CONFIG_FILE_PATH` while leaving non-test behavior unchanged.
- [x] `hifimule-daemon/src/api.rs` -- recover `CREDENTIAL_TEST_MUTEX` poison and reset test vault state on every acquisition so one assertion failure does not manufacture suite-wide failures.
- [x] `hifimule-daemon/src/api.rs` -- add deterministic unit coverage proving authentication test identity performs no config I/O and a poisoned lock can be reacquired.

**Acceptance Criteria:**
- Given the daemon tests execute with normal parallelism, when authentication and credential-storage cases overlap, then `test_file_storage` reads complete valid JSON and no credential lock is poisoned.
- Given a test panics while owning the credential test lock, when a later test acquires it, then the later test executes with clean in-memory vault state while the original panic remains visible.
- Given a non-test daemon build, when Jellyfin authentication constructs its header, then it continues using the persisted machine device ID or the existing fallback.

## Spec Change Log

## Design Notes

The test-only identity belongs at the narrow authentication-header boundary. This avoids broad file-locking changes and does not weaken production atomicity or parsing semantics. Poison recovery is limited to the test mutex; production mutex behavior is untouched.

## Verification

**Commands:**
- `rtk npm run build:daemon -- test -p hifimule-daemon api::tests::` -- expected: all API tests pass with parallel execution.
- `rtk npm run build:daemon -- test -p hifimule-daemon` -- expected: 993 passed, 6 ignored, zero failures.
- `rtk npm run build:daemon -- fmt --all -- --check` -- expected: no formatting diff.
- `rtk git diff --check` -- expected: no whitespace errors.

## Suggested Review Order

**Authentication isolation**

- Separate test identity from production persistence at the narrow header boundary.
  [`api.rs:17`](../../hifimule-daemon/src/api.rs#L17)

- Authentication consumes the isolated identity without changing protocol structure.
  [`api.rs:763`](../../hifimule-daemon/src/api.rs#L763)

**Shared test-state recovery**

- Reset paths and vaults while fully clearing every recovered poison flag.
  [`api.rs:1704`](../../hifimule-daemon/src/api.rs#L1704)

**Regression coverage**

- Prove authentication identity performs no shared config-file I/O.
  [`api.rs:2808`](../../hifimule-daemon/src/api.rs#L2808)

- Poison every shared mutex and verify clean, order-independent recovery.
  [`api.rs:2820`](../../hifimule-daemon/src/api.rs#L2820)
