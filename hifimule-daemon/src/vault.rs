use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use rand::{rngs::OsRng, RngCore};
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

fn derive_key(app_salt: &str) -> Result<Secret<[u8; 32]>, VaultError> {
    let hw_uid = machine_uid::get().map_err(|e| VaultError::HardwareId(e.to_string()))?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(hw_uid.as_bytes());
    hasher.update(app_salt.as_bytes());
    let hash = hasher.finalize();
    let mut key_bytes = [0u8; 32];
    key_bytes.copy_from_slice(hash.as_bytes());
    Ok(Secret::new(key_bytes))
}

/// Encrypts `plaintext` and writes `[12-byte random nonce][ciphertext]` to `path`.
pub fn encrypt_file(path: &Path, plaintext: &str, app_salt: &str) -> Result<(), VaultError> {
    let key = derive_key(app_salt)?;
    let cipher = ChaCha20Poly1305::new(key.expose_secret().into());

    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|_| VaultError::Crypto)?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = File::create(path)?;
    file.write_all(&nonce_bytes)?;
    file.write_all(&ciphertext)?;
    Ok(())
}

/// Reads `[12-byte nonce][ciphertext]` from `path` and decrypts it.
pub fn decrypt_file(path: &Path, app_salt: &str) -> Result<String, VaultError> {
    let data = fs::read(path)?;
    if data.len() < 12 {
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
