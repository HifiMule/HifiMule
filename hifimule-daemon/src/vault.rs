use chacha20poly1305::{
    ChaCha20Poly1305, Nonce,
    aead::{Aead, KeyInit},
};
use rand::{RngCore, rngs::OsRng};
use secrecy::{ExposeSecret, Secret};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum VaultError {
    #[error("Hardware ID extraction failed: {0}")]
    HardwareId(String),
    #[error("Cryptographic operation failed")]
    Crypto,
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid vault payload")]
    InvalidPayload,
}

// BLAKE3 is appropriate here: the hardware UID has UUID-grade entropy, so there is no
// dictionary to brute-force. Threat model: disk/backup exfiltration only.
fn derive_key(app_salt: &str) -> Result<Secret<[u8; 32]>, VaultError> {
    let hw_uid = machine_uid::get().map_err(|e| VaultError::HardwareId(e.to_string()))?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(hw_uid.as_bytes());
    hasher.update(app_salt.as_bytes());
    Ok(Secret::new(*hasher.finalize().as_bytes()))
}

#[cfg(unix)]
fn create_secure_file(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn create_secure_file(path: &Path) -> std::io::Result<File> {
    File::create(path)
}

/// Encrypts `plaintext` and writes `[12-byte random nonce][ciphertext+tag]` to `path`.
/// Uses write-to-temp-then-rename to prevent a corrupt vault on interrupted writes.
pub fn encrypt_file(path: &Path, plaintext: &str, app_salt: &str) -> Result<(), VaultError> {
    let key = derive_key(app_salt)?;
    let cipher = ChaCha20Poly1305::new(key.expose_secret().into());

    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|_| VaultError::Crypto)?;

    let parent = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;

    let tmp_path = path.with_extension("tmp");
    {
        let mut file = create_secure_file(&tmp_path)?;
        file.write_all(&nonce_bytes)?;
        file.write_all(&ciphertext)?;
    }
    fs::rename(&tmp_path, path)?;
    Ok(())
}

/// Reads `[12-byte nonce][ciphertext+16-byte tag]` from `path` and decrypts it.
pub fn decrypt_file(path: &Path, app_salt: &str) -> Result<String, VaultError> {
    let data = fs::read(path)?;
    // Minimum valid size: 12-byte nonce + 16-byte AEAD tag (for empty plaintext)
    if data.len() < 28 {
        return Err(VaultError::InvalidPayload);
    }
    let (nonce_bytes, ciphertext) = data.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);

    let key = derive_key(app_salt)?;
    let cipher = ChaCha20Poly1305::new(key.expose_secret().into());

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| VaultError::Crypto)?;

    String::from_utf8(plaintext).map_err(|_| VaultError::InvalidPayload)
}
