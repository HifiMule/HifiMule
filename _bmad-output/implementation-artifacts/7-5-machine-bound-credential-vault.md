---
baseline_commit: 88f267d0d591bb0d1d533fa9f954b2df0be20047
---

# Story 7.5 — Machine-Bound Credential Vault

## Story

**As** a HifiMule user,
**I want** my server credentials stored in a hardware-bound encrypted vault,
**So that** my secrets are protected against disk/backup exfiltration without requiring OS-native credential service dependencies (macOS Keychain, D-Bus, Windows Credential Manager).

## Acceptance Criteria

- AC1: `vault.rs` module exists with `encrypt_file(path, plaintext, salt)` and `decrypt_file(path, salt)` functions using `machine-uid` + `blake3` + `chacha20poly1305` (AEAD) with a **random** nonce per write (nonce prepended to ciphertext file).
- AC2: `CredentialManager::load_secrets()` reads from `secrets.enc` via `vault::decrypt_file` (non-test path).
- AC3: `CredentialManager::save_secrets()` writes to `secrets.enc` via `vault::encrypt_file` (non-test path).
- AC4: `CredentialManager::clear_credentials()` deletes `secrets.enc` file (non-test path).
- AC5: `CredentialManager::get_vault_path()` returns `get_app_data_dir()?.join("secrets.enc")`.
- AC6: `Cargo.toml` has no `keyring` dependency; has `machine-uid`, `blake3`, `chacha20poly1305`, `secrecy`, and `rand`/`getrandom` for nonce generation.
- AC7: The `#[cfg(test)]` mock seam (`TEST_SECRETS`, `credential_test_lock`) is preserved unchanged.
- AC8: `cargo build` succeeds with zero errors.
- AC9: `cargo test` passes with no regressions (existing `CredentialManager` test suite passes).
- AC10: Error messages referencing "keyring" in non-test code paths are updated to reference "vault".
- AC11: Planning artifacts (`epics.md`, `architecture.md`) updated with new wording per sprint-change-proposal.

## Tasks / Subtasks

- [x] T1: Create `hifimule-daemon/src/vault.rs` with hardware-bound encrypt/decrypt
  - [x] T1.1: Implement `derive_key(salt: &str) -> Result<[u8; 32], VaultError>` using machine-uid + blake3
  - [x] T1.2: Implement `encrypt_file(path: &Path, plaintext: &str, salt: &str) -> Result<()>` with random nonce (OsRng)
  - [x] T1.3: Implement `decrypt_file(path: &Path, salt: &str) -> Result<String>` reading nonce from file header
  - [x] T1.4: Define `VaultError` enum with HardwareId, Crypto, Io, InvalidPayload variants

- [x] T2: Update `hifimule-daemon/Cargo.toml`
  - [x] T2.1: Remove `keyring = "2.3"` dependency
  - [x] T2.2: Add `machine-uid`, `blake3`, `chacha20poly1305`, `secrecy`, and `rand` (for OsRng)

- [x] T3: Update `CredentialManager` in `hifimule-daemon/src/api.rs`
  - [x] T3.1: Remove `KEYRING_SERVICE` and `KEYRING_SECRETS_ACCOUNT` constants; add `VAULT_APP_SALT`
  - [x] T3.2: Add `mod vault;` declaration and `get_vault_path()` helper
  - [x] T3.3: Replace `load_secrets` non-test path with vault-based implementation
  - [x] T3.4: Replace `save_secrets` non-test path with vault-based implementation
  - [x] T3.5: Replace `clear_credentials` non-test path with `fs::remove_file` on vault path
  - [x] T3.6: Update residual "keyring" strings in error messages (non-test) to reference "vault"

- [x] T4: Register `vault` module in `main.rs`

- [x] T5: Update planning artifacts
  - [x] T5.1: Apply NFR11 + Story 2.1/2.5 AC wording edits to `epics.md`
  - [x] T5.2: Apply 5 wording edits to `architecture.md`

- [x] T6: Build and test validation
  - [x] T6.1: `cargo build` succeeds, zero `keyring` imports remain
  - [x] T6.2: `cargo test` passes (no regressions) — 378 tests passed
  - [x] T6.3: `cargo tree | grep keyring` returns nothing

### Review Findings

- [x] [Review][Patch] Add comment to `derive_key` — BLAKE3 accepted for key derivation (hardware UID has UUID-grade entropy; KDF brute-force resistance is not applicable). Document this scope in a one-line comment. [`vault.rs:derive_key`]
- [x] [Review][Patch] Document hardware UID instability as known limitation — credentials are irrecoverably lost if hardware fingerprint changes (VM migration, hardware swap, OS reinstall); add a note to architecture.md acknowledging this scope boundary. [`architecture.md`]
- [x] [Review][Patch] Non-atomic file write destroys vault on interrupted save — `File::create` truncates the existing file immediately before both `write_all` calls succeed; a crash/OOM/SIGKILL between the two writes leaves a corrupt file and all credentials permanently lost. Fix: write to a temp file then `fs::rename`. [`vault.rs:51-54`]
- [x] [Review][Patch] `clear_credentials` silently swallows `remove_file` errors — `let _ = fs::remove_file(...)` discards I/O failures; callers get `Ok(())` while the vault file was not deleted. [`api.rs:1547`]
- [x] [Review][Patch] Key material in stack buffer before `Secret<>` wrapping — `key_bytes` is a plain `[u8; 32]` on the stack before `Secret::new` takes ownership; also `ChaCha20Poly1305::new(key.expose_secret().into())` copies the key into a `GenericArray` that may not be zeroized after cipher construction. [`vault.rs:30-38`]
- [x] [Review][Patch] Vault file world-readable on Unix — `File::create` creates with `umask`-masked `0o666` (typically `0o644`); any local user can read `secrets.enc`. Fix: use `OpenOptions` with `.mode(0o600)`. [`vault.rs:51`]
- [x] [Review][Patch] `save_server_secret`/`save_credentials` silently erase all secrets on vault read failure — both call `load_secrets().unwrap_or_default()`, so any `VaultError` (wrong key, corruption) is treated as "no secrets"; the vault is then overwritten with only the new secret, irrecoverably destroying all others. [`api.rs:1510,1521`]
- [x] [Review][Patch] AEAD length guard too permissive — `data.len() < 12` passes files of 12–27 bytes (nonce present but AEAD tag truncated); minimum valid size is 28 bytes (12 nonce + 16 tag). Fix: change guard to `data.len() < 28`. [`vault.rs:60`]
- [x] [Review][Patch] `secrecy` serde feature enables accidental key serialization — `secrecy = { features = ["serde"] }` makes `Secret<T: Serialize>` serializable; remove the feature if `Secret<>` is never stored in a serialized struct. [`Cargo.toml:32`]
- [x] [Review][Patch] Stale "keyring" references in planning docs — architecture.md and epics.md still contain unreplaced "keyring" prose references outside the sections updated in this story. [`architecture.md`, `epics.md`]
- [x] [Review][Defer] Concurrent `save_secrets` race condition [`vault.rs:51-54`, `api.rs`] — deferred, pre-existing pattern; fix requires broader mutex infrastructure across the CredentialManager
- [x] [Review][Defer] `derive_key` called twice per encrypt/decrypt round-trip [`vault.rs:37,66`] — deferred, pre-existing; performance concern only, not a correctness issue
- [x] [Review][Defer] `get_app_data_dir()` CWD fallback when HOME unset [`paths.rs`] — deferred, pre-existing in paths.rs, not introduced by this change

## Dev Notes

### Architecture
- `vault.rs` is a small standalone module in `hifimule-daemon/src/`. It has no dependency on the rest of the codebase — only on `machine-uid`, `blake3`, `chacha20poly1305`, `secrecy`, and `rand`.
- File format: `[12-byte random nonce][ciphertext+tag]` — nonce stored inline, regenerated on every write.
- Key derivation: `machine_uid::get()` bytes + `app_salt` bytes → `blake3::Hasher` → 32-byte key wrapped in `secrecy::Secret<[u8; 32]>`.
- The `#[cfg(test)]` mock seam in `api.rs` (`TEST_SECRETS`, `CREDENTIAL_TEST_MUTEX`, `credential_test_lock`, `clear_test_secrets`) must be left completely untouched.
- `get_vault_path()` should not be `#[cfg(not(test))]` — it's used from within a `#[cfg(not(test))]` block, but can be a plain `fn`.

### Key References
- Sprint change proposal: `_bmad-output/planning-artifacts/sprint-change-proposal-2026-05-30-keyring-to-machine-bound-encryption.md`
- Encryption reference: `_bmad-output/planning-artifacts/research/encryption.md`
- Existing `CredentialManager`: `hifimule-daemon/src/api.rs` lines ~1395–1640
- `paths::get_app_data_dir()`: `hifimule-daemon/src/paths.rs`

### Important Note on Nonce
The reference `encryption.md` uses a static (deterministic) nonce derived from `blake3::hash(app_salt)` — **this is only for illustration**. Production code MUST use a random nonce via `rand::rngs::OsRng` + `chacha20poly1305::aead::OsRng` / `rand::RngCore::fill_bytes`. See proposal section 4.2 note.

### Cargo.toml Versions
- `machine-uid = "0.5"`
- `blake3 = "1.5"`
- `chacha20poly1305 = "0.10"`
- `secrecy = { version = "0.8", features = ["serde"] }`
- Check if `rand` already in Cargo.toml; if not, add `rand = "0.8"`

## Dev Agent Record

### Implementation Plan
Replace the three keyring call sites in `CredentialManager` (load_secrets, save_secrets, clear_credentials) with a new `vault.rs` module that implements hardware-bound ChaCha20-Poly1305 AEAD encryption. The test seam (`TEST_SECRETS`) is preserved unchanged.

### Debug Log

### Completion Notes

- Created `vault.rs` with `VaultError`, `derive_key`, `encrypt_file`, `decrypt_file`. Uses `machine-uid` + `blake3` key derivation, `ChaCha20Poly1305` AEAD, random `OsRng` nonce per write prepended to file. `create_dir_all` ensures vault directory exists before first write.
- Replaced all 3 keyring call sites in `CredentialManager` (`load_secrets`, `save_secrets`, `clear_credentials`). `#[cfg(test)]` mock seam (`TEST_SECRETS`, `credential_test_lock`) left completely unchanged.
- `VAULT_APP_SALT = "hifimule.github.io/secrets/v1"` — stable salt ensuring key isolation.
- Updated 2 residual "keyring" error message strings ("No token found in vault", "No server secret found in vault").
- All 378 existing tests pass with no regressions.

## File List

- `hifimule-daemon/src/vault.rs` (new)
- `hifimule-daemon/src/main.rs` (modified — added `mod vault;`)
- `hifimule-daemon/src/api.rs` (modified — CredentialManager vault migration)
- `hifimule-daemon/Cargo.toml` (modified — removed keyring, added vault deps)
- `_bmad-output/planning-artifacts/epics.md` (modified — NFR11, Story 2.1/2.5 AC wording)
- `_bmad-output/planning-artifacts/architecture.md` (modified — 5 wording updates)
- `_bmad-output/implementation-artifacts/sprint-status.yaml` (modified — status in-progress → review)
- `_bmad-output/implementation-artifacts/7-5-machine-bound-credential-vault.md` (this file)

## Change Log

- 2026-05-30: Replaced `keyring` crate with hardware-bound ChaCha20-Poly1305 vault (`vault.rs`). Updated CredentialManager call sites, Cargo.toml, planning artifacts. 378 tests passing.

## Status
done
