# Sprint Change Proposal — 2026-05-30
## Replace `keyring` with Machine-Bound Hardware Encryption

---

## 1. Issue Summary

**Problem:** The `keyring` crate relies on OS-native credential vaults (macOS Keychain, Windows Credential Manager, Linux Secret Service / D-Bus). For a media credential stored locally by a desktop app, this introduces unnecessary external dependencies: Linux Secret Service requires a running D-Bus session, headless environments and CI can fail to access the vault, and the crate itself has non-trivial platform-specific behaviour differences. This is overkill for the threat model HifiMule actually needs to defend against (offline disk exfiltration, accidental git commits) — not root compromise.

**Proposed solution:** Replace `keyring` with a hardware-bound encryption vault using:
- `machine-uid` to derive a platform-agnostic hardware fingerprint
- `blake3` to mix fingerprint + app salt into a 32-byte key
- `chacha20poly1305` (AEAD) to encrypt/decrypt the `Secrets` JSON blob
- `secrecy` to zeroize key material from memory after use

The encrypted blob is stored as `secrets.enc` in the same app data directory as `config.json` and the daemon log — no OS credential service dependency.

**When discovered:** During implementation review while planning the next development sprint. No user-visible regression; both approaches produce an identically opaque `Secrets` struct at runtime.

---

## 2. Impact Analysis

### Epic Impact
- **Epic 2 (Connection & Verification):** All stories `done`. No story re-opened; this is a retroactive implementation improvement to completed infrastructure.
- **All other epics:** No impact. The change is fully encapsulated in `CredentialManager` (`api.rs`).

### Story Impact
- **Story 2.1** (Secure Media Server Link): Acceptance criteria reference "system Keyring" → update wording to "encrypted local vault".
- **Story 2.5** (Interactive Login & Identity Management): Same — one AC line references "system Keyring" → update wording.
- No story re-opened or moved back to in-progress. These are wording corrections on completed stories.

### Artifact Conflicts
- **`epics.md`:** NFR11, Story 2.1 AC, Story 2.5 AC — 3 targeted line updates.
- **`architecture.md`:** 4 locations referencing `keyring` crate or "OS keyring" — targeted updates.
- **`hifimule-daemon/Cargo.toml`:** Remove `keyring = "2.3"`, add 4 new crates.
- **UI/UX spec:** Zero impact. Credential storage is daemon-internal and invisible to the UI.
- **CI/CD (Epic 6):** Minor positive — removing `keyring` eliminates the `libsecret-1-dev` dependency on Linux, simplifying the `.deb` package and AppImage.

### Technical Impact
- All 3 keyring call sites are in `CredentialManager` in `api.rs` (`load_secrets`, `save_secrets`, `clear_credentials`) — fully isolated.
- The existing `#[cfg(test)]` mock seam (`TEST_SECRETS: Mutex<Option<Secrets>>`) is preserved unchanged.
- Storage file: `secrets.enc` in `get_app_data_dir()` — same directory as `config.json`.
- File format: `[12-byte nonce][ciphertext]` — nonce stored inline, re-generated on every save.

---

## 3. Recommended Approach

**Option 1: Direct Adjustment** — Selected.

Swap the 3 call sites in `CredentialManager` from `keyring::Entry` to a new `VaultManager` (either a small new module `vault.rs` or inline helpers). Update Cargo.toml. Update planning artifact wording.

- **Effort:** Low (< 1 day)
- **Risk:** Low — isolated module, existing test seam preserved, pure Rust with no new OS dependencies
- **Timeline impact:** None — zero sprint disruption

No rollback or MVP review needed.

---

## 4. Detailed Change Proposals

### 4.1 — `hifimule-daemon/Cargo.toml`

**Section:** `[dependencies]`

OLD:
```toml
keyring = "2.3"
```

NEW:
```toml
machine-uid = "0.5"
blake3 = "1.5"
chacha20poly1305 = "0.10"
secrecy = { version = "0.8", features = ["serde"] }
```

**Rationale:** Direct replacement of the OS-native credential crate with the four pure-Rust crates that implement the hardware-bound vault pattern from `encryption.md`.

---

### 4.2 — `hifimule-daemon/src/api.rs` — `CredentialManager`

**Section:** Constants and `load_secrets` / `save_secrets` / `clear_credentials`

OLD (non-test path):
```rust
#[cfg(not(test))]
const KEYRING_SERVICE: &str = "hifimule.github.io";
#[cfg(not(test))]
const KEYRING_SECRETS_ACCOUNT: &str = "secrets";
```

NEW:
```rust
const VAULT_APP_SALT: &str = "hifimule.github.io/secrets/v1";
```

---

OLD `load_secrets` (non-test block):
```rust
let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_SECRETS_ACCOUNT)
    .map_err(|e| anyhow!("Failed to access keyring: {}", e))?;
return match entry.get_password() {
    Ok(json) => serde_json::from_str(&json)
        .map_err(|e| anyhow!("Failed to parse secrets blob: {}", e)),
    Err(_) => Ok(Secrets::default()),
};
```

NEW `load_secrets` (non-test block):
```rust
let path = Self::get_vault_path()?;
if !path.exists() {
    return Ok(Secrets::default());
}
let json = vault::decrypt_file(&path, VAULT_APP_SALT)
    .map_err(|e| anyhow!("Failed to decrypt secrets vault: {}", e))?;
return serde_json::from_str(&json)
    .map_err(|e| anyhow!("Failed to parse secrets blob: {}", e));
```

---

OLD `save_secrets` (non-test block):
```rust
let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_SECRETS_ACCOUNT)
    .map_err(|e| anyhow!("Failed to access keyring: {}", e))?;
let json = serde_json::to_string(secrets)?;
return entry
    .set_password(&json)
    .map_err(|e| anyhow!("Failed to save secrets to keyring: {}", e));
```

NEW `save_secrets` (non-test block):
```rust
let path = Self::get_vault_path()?;
let json = serde_json::to_string(secrets)?;
return vault::encrypt_file(&path, &json, VAULT_APP_SALT)
    .map_err(|e| anyhow!("Failed to save secrets vault: {}", e));
```

---

OLD `clear_credentials` (non-test block):
```rust
#[cfg(not(test))]
if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_SECRETS_ACCOUNT) {
    let _ = entry.delete_password();
}
```

NEW `clear_credentials` (non-test block):
```rust
#[cfg(not(test))]
{
    let vault_path = Self::get_vault_path().ok();
    if let Some(p) = vault_path {
        if p.exists() {
            let _ = fs::remove_file(&p);
        }
    }
}
```

---

**New helper on `CredentialManager`:**
```rust
fn get_vault_path() -> Result<PathBuf> {
    Ok(crate::paths::get_app_data_dir()?.join("secrets.enc"))
}
```

---

**New module `hifimule-daemon/src/vault.rs`** (or inline in `api.rs`):

Implements `encrypt_file` and `decrypt_file` following the pattern in `encryption.md`:
1. Derive key: `machine_uid::get()` → mix with `app_salt` via `blake3::Hasher` → 32-byte key wrapped in `secrecy::Secret`
2. Encrypt: `ChaCha20Poly1305` with a randomly-generated 12-byte nonce prepended to the ciphertext file
3. Decrypt: read first 12 bytes as nonce, decrypt remainder

**Note:** Use `rand::thread_rng()` (or `OsRng`) for nonce generation — **not** a deterministic hash of the salt (the `encryption.md` reference code uses a static nonce for illustration; production code must use a random nonce per write). Add `rand` or `getrandom` if not already in Cargo.toml.

---

### 4.3 — `_bmad-output/planning-artifacts/epics.md`

**Change 1 — NFR11 (line 53):**

OLD:
```
NFR11: Encrypted credential storage via OS-native vaults using the `keyring` crate.
```
NEW:
```
NFR11: Encrypted credential storage using hardware-bound encryption (machine-uid + blake3 + ChaCha20-Poly1305). Secrets are stored as `secrets.enc` in the app data directory, bound to the host machine's hardware fingerprint.
```

---

**Change 2 — Story 2.1 acceptance criteria (lines 171–172):**

OLD:
```
**And** for Jellyfin: I enter a Username and Password → daemon authenticates and stores the access token in the system Keyring.
**And** for Subsonic/Navidrome: I enter a Username and Password → daemon stores the password (encrypted) in the system Keyring for per-request MD5 signing.
```
NEW:
```
**And** for Jellyfin: I enter a Username and Password → daemon authenticates and stores the access token in the encrypted local vault (`secrets.enc`).
**And** for Subsonic/Navidrome: I enter a Username and Password → daemon stores the password in the encrypted local vault (`secrets.enc`) for per-request MD5 signing.
```

---

**Change 3 — Story 2.5 acceptance criteria (line 247):**

OLD:
```
**And** the token or password is securely stored in the system Keyring (replacing any existing credential).
```
NEW:
```
**And** the token or password is securely stored in the encrypted local vault (replacing any existing credential).
```

---

### 4.4 — `_bmad-output/planning-artifacts/architecture.md`

**Change 1 — Core Architectural Decisions (line ~69):**

OLD:
```
- **Secure Storage:** `keyring` crate for OS-native credential management.
```
NEW:
```
- **Secure Storage:** Hardware-bound encryption vault (`machine-uid` + `blake3` + `chacha20poly1305`) — credentials stored as `secrets.enc` in the app data directory, bound to the host machine's hardware fingerprint.
```

---

**Change 2 — Authentication & Security section (line ~154):**

OLD:
```
- **Credential Management:** Server credentials are stored in the OS-native secure vault (Windows Credential Manager, macOS Keychain, Linux Secret Service) using the `keyring` crate.
  - **Jellyfin:** Stores a rotatable access token. Re-authenticates on 401.
  - **Subsonic/OpenSubsonic:** Stores the user password (encrypted). Auth is stateless — credentials are sent on every request as `t=md5(password+salt)` + `s=salt`. The password is used only to compute per-request tokens; it is never stored in plaintext.
```
NEW:
```
- **Credential Management:** Server credentials are stored in a hardware-bound encrypted vault (`secrets.enc`) in the app data directory. The encryption key is derived from the host machine's hardware fingerprint (via `machine-uid`) mixed with an app-specific salt using `blake3`, then used with `chacha20poly1305` (AEAD). This protects against offline disk/backup exfiltration; root compromise is out of scope.
  - **Jellyfin:** Stores a rotatable access token. Re-authenticates on 401.
  - **Subsonic/OpenSubsonic:** Stores the user password (encrypted at rest). Auth is stateless — credentials are sent on every request as `t=md5(password+salt)` + `s=salt`. The password is used only to compute per-request tokens; it is never stored in plaintext.
```

---

**Change 3 — Server Config Persistence section (line ~483):**

OLD:
```
Server URL, detected type, and username are persisted in SQLite so the daemon can reconnect on restart. Credentials remain exclusively in the OS keyring.
```
NEW:
```
Server URL, detected type, and username are persisted in SQLite so the daemon can reconnect on restart. Credentials remain exclusively in the encrypted local vault (`secrets.enc`).
```

---

**Change 4 — Server Config Persistence / startup paragraph (line ~496):**

OLD:
```
On daemon startup: if a `server_config` row exists, the daemon calls `connect()` with the stored URL, fetches credentials from keyring, and restores the active provider before the RPC server starts accepting requests.
```
NEW:
```
On daemon startup: if a `server_config` row exists, the daemon calls `connect()` with the stored URL, fetches credentials from the encrypted local vault (`secrets.enc`), and restores the active provider before the RPC server starts accepting requests.
```

---

**Change 5 — Subsonic Auth Internals (line ~500):**

OLD:
```
`SubsonicProvider` fetches the password from keyring **once at construction time** and holds it in memory for the session lifetime of the struct.
```
NEW:
```
`SubsonicProvider` fetches the password from the encrypted local vault **once at construction time** and holds it in memory for the session lifetime of the struct.
```

---

## 5. Implementation Handoff

**Scope classification:** Minor — can be implemented directly by the Developer agent.

**Deliverables for Developer agent:**

1. Create `hifimule-daemon/src/vault.rs` implementing `encrypt_file(path, plaintext, salt)` and `decrypt_file(path, salt)` per the pattern in `_bmad-output/planning-artifacts/research/encryption.md`, using a **random nonce** (not a deterministic hash) per write.
2. Update `hifimule-daemon/Cargo.toml`: remove `keyring = "2.3"`, add `machine-uid`, `blake3`, `chacha20poly1305`, `secrecy`. Add `rand` if not already present (for nonce generation).
3. Update `CredentialManager` in `hifimule-daemon/src/api.rs`: remove `keyring` import, add `vault` module call sites per proposals 4.2 above. Preserve the `#[cfg(test)]` mock seam unchanged.
4. Apply wording edits to `epics.md` (proposals 4.3).
5. Apply wording edits to `architecture.md` (proposals 4.4).

**Success criteria:**
- `cargo build` succeeds with no `keyring` import remaining
- `cargo test` passes (existing `CredentialManager` test suite passes unchanged)
- `secrets.enc` is written to `get_app_data_dir()` on first save, alongside `config.json`
- An existing `secrets.enc` from a fresh install is not readable on a different machine (manual verification)
- No `keyring` dependency remains in the dependency tree (`cargo tree | grep keyring` returns nothing)

**Handoff recipient:** Developer agent (`/bmad-dev-story`)

---

## 6. Approval

Requested from: Alexis
Date: 2026-05-30
Status: **Approved**
