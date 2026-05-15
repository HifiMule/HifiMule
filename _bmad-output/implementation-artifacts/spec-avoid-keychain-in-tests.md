---
title: 'Avoid keychain access in tests'
type: 'bugfix'
created: '2026-05-15'
status: 'done'
route: 'one-shot'
---

## Intent

**Problem:** On macOS, `cargo test` triggers system permission dialogs because `CredentialManager::load_secrets()` and `save_secrets()` always call the real keyring crate (macOS Keychain), even in test builds — blocking test runs until the user clicks "Allow".

**Approach:** Gate all keyring crate calls behind `#[cfg(not(test))]` and introduce an in-memory `TEST_SECRETS` static (`Mutex<Option<Secrets>>`) for test builds. A `credential_test_lock()` helper atomically acquires `CREDENTIAL_TEST_MUTEX` and resets `TEST_SECRETS`, ensuring isolation between tests.

## Suggested Review Order

1. [api.rs:944–956](../../hifimule-daemon/src/api.rs#L944) — `TEST_SECRETS` static and `credential_test_lock()` helper
2. [api.rs:967–999](../../hifimule-daemon/src/api.rs#L967) — `load_secrets` / `save_secrets` cfg-gated branches
3. [api.rs:1073–1090](../../hifimule-daemon/src/api.rs#L1073) — `clear_credentials` and `clear_test_secrets`
4. [rpc.rs:3139](../../hifimule-daemon/src/rpc.rs#L3139) — updated import in test module
