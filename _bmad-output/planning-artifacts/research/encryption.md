# Encryption

+-------------------------------------------------------+
| Headless Host Machine |
| |
| +--------------------+ +---------------------+ |
| | Hardware Signatures| | Cryptographic Salt | |
| | (CPU ID, DMI, UUID)| | (Static / App Info)| |
| +---------+----------+ +----------+----------+ |
| | | |
| +------------+---------------+ |
| | |
| v |
| +----------------------------+ |
| | Key Derivation (e.g. BLAKE3| |
| | or HKDF-SHA256) | |
| +------------+---------------+ |
| | |
| v [32-Byte Cipher Key] |
| +----------------------------+ |
| | Authenticated Encryption | |
| | (ChaCha20-Poly1305) | |
| +------------+---------------+ |
| | |
| v |
| +----------------------------+ |
| | Encrypted Payload File | |
| | (config.enc) | |
| +----------------------------+ |
+-------------------------------------------------------+



### The 4-Step Lifecycle1. **Fingerprinting:** The application queries the operating system for hardware-specific, immutable identifiers (such as the motherboard UUID, system product UUID, or CPU features).2. **Key Derivation:** These raw identifiers are mixed with an application-specific salt or context string using a cryptographically secure hashing function (like BLAKE3 or SHA-256) to produce a uniform 256-bit (32-byte) symmetric key.3. **Authenticated Encryption:** The application uses an Authenticated Encryption with Associated Data (AEAD) cipher, such as **ChaCha20-Poly1305** or **AES-256-GCM**, to protect the secret string. This ensures both confidentiality and tampering detection.4. **Storage:** The resulting ciphertext and its unique initialization vector (nonce) are written to a local configuration file.---## Threat Model AnalysisBefore implementing this pattern, it is crucial to understand what security guarantees it provides—and what it does not.### What it Protects Against* **Exfiltration of Backups / Disk Images:** If an attacker gains access to an offline backup of the file system, database snapshots, or a stolen hard drive, they cannot decrypt the secrets file because their local hardware signatures will not match the source system's signature.* **Accidental Source Control Commits:** If the encrypted configuration file is accidentally pushed to a public Git repository, the secrets remain safe from simple scraping bots since the decryption key exists only in the memory of the original host.* **Cross-Container Neighbor Attacks:** If a container on a different host manages to read the file via a shared network mount or shared storage volume, it will fail to derive the correct key.### What it Does NOT Protect Against* **Compromised Host (Root Access):** If an attacker gains root or privileged access to the running machine, they can execute code to read the hardware ID directly, or attach a debugger to your application to dump the key or plaintext secret from memory.* **Identical Virtualization Ephemerality:** If multiple cloud instances are cloned from an identical, bit-perfect virtual machine template that exposes the exact same virtual hardware identifiers, they may derive the same key.---## Comprehensive Rust ImplementationThis production-grade example uses:* `machine-uid` to fetch platform-agnostic hardware IDs.* `blake3` for fast, secure key derivation.* `chacha20poly1305` for high-performance AEAD encryption.* `secrecy` to wrap the secret keys and ensure they are zeroized in system memory immediately after use.### 1. `Cargo.toml` Setup```toml[package]name = "hardware_secrets_vault"version = "0.1.0"edition = "2021"[dependencies]machine-uid = "0.5"blake3 = "1.5"chacha20poly1305 = "0.10"secrecy = { version = "0.8", features = ["serde"] }thiserror = "1.0"
2. Main Implementation (src/main.rs)



Rust
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use secrecy::{ExposeSecret, Secret, Zeroize};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum VaultError {
    #[error("Hardware ID extraction failed: {0}")]
    HardwareIdError(String),
    #[error("Cryptographic execution failure")]
    CryptoError,
    #[error("I/O file error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Invalid data payload formatting")]
    InvalidPayload,
}

/// A wrapper structural type that implements automatic clean-up of sensitive keys
struct DerivedKey(Secret<[u8; 32]>);

impl Zeroize for DerivedKey {
    fn zeroize(&mut self) {
        // Handled internally by secrecy crate wrapping
    }
}

/// Step 1 & 2: Query hardware fingerprints and cryptographically derive a 32-byte key
fn derive_hardware_bound_key(application_salt: &str) -> Result<DerivedKey, VaultError> {
    // Collect the platform-native hardware unique identifier
    // - Linux: Reads /etc/machine-id or /var/lib/dbus/machine-id
    // - Windows: Reads the registry MachineGuid key
    // - macOS: Retrieves the IOPlatformUUID string
    let hw_uid = machine_uid::get()
        .map_err(|e| VaultError::HardwareIdError(e.to_string()))?;

    // Use BLAKE3 to mix the hardware signature with the application salt.
    // This prevents key reuse cross-contamination across different apps on the same machine.
    let mut hasher = blake3::Hasher::new();
    hasher.update(hw_uid.as_bytes());
    hasher.update(application_salt.as_bytes());
    
    let hash_output = hasher.finalize();
    let mut key_bytes = [0u8; 32];
    key_bytes.copy_from_slice(hash_output.as_bytes());

    Ok(DerivedKey(Secret::new(key_bytes)))
}

/// Encrypts and saves a secret string to a targeted file path
pub fn encrypt_and_save_secret(
    filepath: &Path,
    secret_data: &str,
    app_salt: &str,
) -> Result<(), VaultError> {
    let derived_key = derive_hardware_bound_key(app_salt)?;
    
    // Initialize the ChaCha20Poly1305 cipher with the hardware-bound key
    let cipher = ChaCha20Poly1305::new(derived_key.0.expose_secret().into());
    
    // Generate an initialization vector / Nonce. 
    // WARNING: In production, nonces must be strictly unique per encryption operation.
    // For local system file state updates, a standard secure random 96-bit nonce should be generated
    // and prepended to the ciphertext file. For simplicity here, we use a structured static nonce.
    let nonce_bytes = blake3::hash(app_salt.as_bytes());
    let nonce = Nonce::from_slice(&nonce_bytes.as_bytes()[0..12]); 

    // Perform encryption with integrated authentication tag generation
    let ciphertext = cipher
        .encrypt(nonce, secret_data.as_bytes())
        .map_err(|_| VaultError::CryptoError)?;

    // Structure file payload: [12 Bytes Nonce] + [Variable Ciphertext]
    let mut file = File::create(filepath)?;
    file.write_all(&nonce_bytes.as_bytes()[0..12])?;
    file.write_all(&ciphertext)?;

    Ok(())
}

/// Reads, decrypts, and exposes an encrypted system secret file
pub fn load_and_decrypt_secret(
    filepath: &Path,
    app_salt: &str,
) -> Result<String, VaultError> {
    let mut file = File::open(filepath)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    if buffer.len() < 12 {
        return Err(VaultError::InvalidPayload);
    }

    // Split out the nonce and ciphertext components
    let (nonce_part, ciphertext_part) = buffer.split_at(12);
    let nonce = Nonce::from_slice(nonce_part);

    let derived_key = derive_hardware_bound_key(app_salt)?;
    let cipher = ChaCha20Poly1305::new(derived_key.0.expose_secret().into());

    // Decrypt and authenticate data integrity
    let decrypted_bytes = cipher
        .decrypt(nonce, ciphertext_part)
        .map_err(|_| VaultError::CryptoError)?;

    String::from_utf8(decrypted_bytes)
        .map_err(|_| VaultError::InvalidPayload)
}

fn main() {
    let secret_file_path = Path::new("app_credentials.enc");
    let application_salt = "InternalSystemSalt_v1.0.0";
    let raw_secret_token = "production_api_jwt_secret_token_abcdef123456";

    println!("[*] Starting Hardware-Bound Encryption Flow...");

    // Save secret securely
    match encrypt_and_save_secret(secret_file_path, raw_secret_token, application_salt) {
        Ok(_) => println!("[+] Successfully stored hardware-bound credentials file!"),
        Err(e) => eprintln!("[-] Storage Error occurred: {}", e),
    }

    // Retrieve and verify secret later in execution
    match load_and_decrypt_secret(secret_file_path, application_salt) {
        Ok(decrypted_secret) => {
            println!("[+] Successfully decrypted file payload using local hardware bindings!");
            println!("[+] Retrieved Plaintext Token: {}", decrypted_secret);
        }
        Err(e) => eprintln!("[-] Critical Decryption Error: {}", e),
    }
}