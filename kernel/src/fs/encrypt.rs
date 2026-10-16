//! Filesystem-level file encryption.
//!
//! Provides per-file encryption using ChaCha20 stream cipher with
//! HMAC-SHA256 authentication (encrypt-then-MAC).  Files are encrypted
//! transparently when rules match, similar to fcompress.
//!
//! ## Cipher: ChaCha20 (RFC 7539)
//!
//! A fast stream cipher that doesn't need AES hardware. 256-bit key,
//! 96-bit nonce, 32-bit counter.  We use it in encrypt-then-MAC mode
//! with HMAC-SHA256 for authentication.
//!
//! ## File Format
//!
//! ```text
//! Offset  Size   Description
//! 0       4      Magic: 0x46 0x45 0x4E 0x43 ("FENC")
//! 4       1      Version (1)
//! 5       1      Cipher (1 = ChaCha20)
//! 6       2      Reserved
//! 8       12     Nonce (random per-file)
//! 20      32     HMAC-SHA256(key, nonce || ciphertext)
//! 52      8      Original plaintext size (little-endian u64)
//! 60      ...    Ciphertext (same length as plaintext)
//! ```
//!
//! Total header: 60 bytes.
//!
//! ## Key Management
//!
//! - Named keystores hold 256-bit symmetric keys
//! - Keys are derived from passphrases via SHA256(passphrase || name): one
//!   fast hash, which a stolen file does not resist (known-issues
//!   `A-FS-ENCRYPT-HAND-ROLLS-ITS-CIPHER-AND-DERIVES-KEYS-WITH-ONE-SHA-256`)
//! - Each file gets a random 12-byte nonce, from the kernel's
//!   random-number generator
//!
//! ## Usage
//!
//! ```text
//! encrypt key add mykey "passphrase"   - derive and store a key
//! encrypt file /path/to/file mykey     - encrypt a file
//! encrypt read /path/to/file mykey     - decrypt and display
//! encrypt dir /path mykey              - encrypt all files in dir
//! ```

#![allow(dead_code)]

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Magic bytes: "FENC"
const MAGIC: [u8; 4] = [0x46, 0x45, 0x4E, 0x43];

/// Header size in bytes.
const HEADER_SIZE: usize = 60;

/// Cipher ID for ChaCha20.
const CIPHER_CHACHA20: u8 = 1;

/// Current format version.
const VERSION: u8 = 1;

/// ChaCha20 key size (256 bits).
const KEY_SIZE: usize = 32;

/// ChaCha20 nonce size (96 bits).
const NONCE_SIZE: usize = 12;

/// HMAC-SHA256 digest size.
const HMAC_SIZE: usize = 32;

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

use crate::crypto::hmac_sha256;

/// How long [`generate_nonce`] waits for the random-number generator to be
/// seeded, as `fs::diskencrypt` waits for its salts.
const RNG_WAIT_NS: u64 = 5_000_000_000;

/// ChaCha20 over `data`, as the FENC format runs it: the keystream from
/// block counter 1, RFC 8439's convention for encryption. Its own inverse.
fn chacha20_crypt(key: &[u8; KEY_SIZE], nonce: &[u8; NONCE_SIZE], data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    crate::crypto::chacha20_xor(key, nonce, 1, &mut out);
    out
}

/// A nonce for one file, from the kernel's random-number generator. A
/// ChaCha20 nonce must never repeat under one key, and a key here is
/// derived from a passphrase -- the same on every boot -- so a counter or a
/// clock, which restart with the machine, will not do.
///
/// # Errors
///
/// `WouldBlock` if the generator is still not seeded after [`RNG_WAIT_NS`].
fn generate_nonce() -> KernelResult<[u8; NONCE_SIZE]> {
    if !crate::rng::is_ready() && !crate::rng::wait_until_ready(RNG_WAIT_NS) {
        return Err(KernelError::WouldBlock);
    }
    let mut nonce = [0u8; NONCE_SIZE];
    crate::rng::fill(&mut nonce);
    Ok(nonce)
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A named encryption key.
struct KeyEntry {
    name: String,
    key: [u8; KEY_SIZE],
}

/// Public info about a stored key.
#[derive(Debug, Clone)]
pub struct KeyInfo {
    pub name: String,
}

/// Information about an encrypted file.
#[derive(Debug, Clone)]
pub struct EncryptInfo {
    pub encrypted: bool,
    pub cipher: &'static str,
    pub original_size: u64,
    pub stored_size: u64,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Maximum number of stored keys.
const MAX_KEYS: usize = 64;

struct EncryptInner {
    keys: BTreeMap<String, KeyEntry>,
    files_encrypted: u64,
    files_decrypted: u64,
}

static STATE: Mutex<EncryptInner> = Mutex::new(EncryptInner {
    keys: BTreeMap::new(),
    files_encrypted: 0,
    files_decrypted: 0,
});

// ---------------------------------------------------------------------------
// Key management
// ---------------------------------------------------------------------------

/// Derive a 256-bit key from a passphrase and store it under a name.
///
/// Key derivation: SHA256(passphrase || name || "FENC_KDF_V1").
/// This is simplified; a production system would use Argon2id.
pub fn add_key(name: &str, passphrase: &str) -> KernelResult<()> {
    if name.is_empty() || name.len() > 64 {
        return Err(KernelError::InvalidArgument);
    }

    let mut state = STATE.lock();
    if state.keys.len() >= MAX_KEYS {
        return Err(KernelError::DiskFull);
    }

    // Derive key.
    let key = derive_key(passphrase, name);

    state.keys.insert(
        String::from(name),
        KeyEntry {
            name: String::from(name),
            key,
        },
    );

    Ok(())
}

/// Add a raw 256-bit key (for testing or key import).
pub fn add_raw_key(name: &str, key: [u8; KEY_SIZE]) -> KernelResult<()> {
    if name.is_empty() || name.len() > 64 {
        return Err(KernelError::InvalidArgument);
    }

    let mut state = STATE.lock();
    if state.keys.len() >= MAX_KEYS {
        return Err(KernelError::DiskFull);
    }

    state.keys.insert(
        String::from(name),
        KeyEntry {
            name: String::from(name),
            key,
        },
    );

    Ok(())
}

/// Remove a key from the keystore.
pub fn remove_key(name: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.keys.remove(name).is_none() {
        return Err(KernelError::NotFound);
    }
    Ok(())
}

/// List all key names.
pub fn list_keys() -> Vec<KeyInfo> {
    let state = STATE.lock();
    state
        .keys
        .values()
        .map(|k| KeyInfo {
            name: k.name.clone(),
        })
        .collect()
}

/// Check if a key exists.
pub fn has_key(name: &str) -> bool {
    STATE.lock().keys.contains_key(name)
}

/// Get key count.
pub fn key_count() -> usize {
    STATE.lock().keys.len()
}

// ---------------------------------------------------------------------------
// Encryption / Decryption
// ---------------------------------------------------------------------------

/// Encrypt data using a named key.
///
/// Returns the encrypted file content (header + ciphertext) ready to
/// be written to disk.
pub fn encrypt(data: &[u8], key_name: &str) -> KernelResult<Vec<u8>> {
    let state = STATE.lock();
    let entry = state.keys.get(key_name).ok_or(KernelError::NotFound)?;
    let key = entry.key;
    drop(state);

    // Generate random nonce.
    let nonce = generate_nonce()?;

    // Encrypt.
    let ciphertext = chacha20_crypt(&key, &nonce, data);

    // Compute HMAC over nonce || ciphertext.
    let mut hmac_input = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
    hmac_input.extend_from_slice(&nonce);
    hmac_input.extend_from_slice(&ciphertext);
    let mac = hmac_sha256(&key, &hmac_input);

    // Build output.
    let mut output = Vec::with_capacity(HEADER_SIZE + ciphertext.len());
    output.extend_from_slice(&MAGIC); // 0-3: magic
    output.push(VERSION); // 4: version
    output.push(CIPHER_CHACHA20); // 5: cipher
    output.push(0); // 6-7: reserved
    output.push(0);
    output.extend_from_slice(&nonce); // 8-19: nonce
    output.extend_from_slice(&mac); // 20-51: HMAC
    output.extend_from_slice(&(data.len() as u64).to_le_bytes()); // 52-59: orig size
    output.extend_from_slice(&ciphertext); // 60+: ciphertext

    // One acquisition, not two. `STATE.lock().x = STATE.lock().x + 1` deadlocks
    // on a non-reentrant spinlock: Rust evaluates the right-hand side first, but
    // the guard it produces is a temporary that lives until the end of the
    // statement, so the left-hand side asks for a lock the same thread is still
    // holding and spins forever. This hung the boot the first time anything
    // called `encrypt()`.
    let mut state = STATE.lock();
    state.files_encrypted = state.files_encrypted.saturating_add(1);
    drop(state);

    Ok(output)
}

/// Decrypt data using a named key.
///
/// Verifies the HMAC before decrypting.  Returns the original plaintext
/// on success, or an error if authentication fails.
pub fn decrypt(data: &[u8], key_name: &str) -> KernelResult<Vec<u8>> {
    if !is_encrypted(data) {
        return Err(KernelError::InvalidArgument);
    }

    if data[5] != CIPHER_CHACHA20 {
        return Err(KernelError::NotSupported);
    }

    let state = STATE.lock();
    let entry = state.keys.get(key_name).ok_or(KernelError::NotFound)?;
    let key = entry.key;
    drop(state);

    // Extract header fields.
    let mut nonce = [0u8; NONCE_SIZE];
    nonce.copy_from_slice(&data[8..20]);

    let mut stored_mac = [0u8; HMAC_SIZE];
    stored_mac.copy_from_slice(&data[20..52]);

    let mut size_bytes = [0u8; 8];
    size_bytes.copy_from_slice(&data[52..60]);
    let _original_size = u64::from_le_bytes(size_bytes);

    let ciphertext = &data[HEADER_SIZE..];

    // Verify HMAC (authenticate-then-decrypt).
    let mut hmac_input = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
    hmac_input.extend_from_slice(&nonce);
    hmac_input.extend_from_slice(ciphertext);
    let computed_mac = hmac_sha256(&key, &hmac_input);

    // Constant-time comparison to prevent timing attacks.
    if !constant_time_eq(&stored_mac, &computed_mac) {
        return Err(KernelError::PermissionDenied);
    }

    // Decrypt.
    let plaintext = chacha20_crypt(&key, &nonce, ciphertext);

    // Single acquisition — see the note in `encrypt()` above.
    let mut state = STATE.lock();
    state.files_decrypted = state.files_decrypted.saturating_add(1);
    drop(state);

    Ok(plaintext)
}

/// Check if data starts with the FENC magic header.
pub fn is_encrypted(data: &[u8]) -> bool {
    data.len() >= HEADER_SIZE && data[..4] == MAGIC
}

/// Get info about encrypted file data.
pub fn file_info(data: &[u8]) -> EncryptInfo {
    if !is_encrypted(data) {
        return EncryptInfo {
            encrypted: false,
            cipher: "none",
            original_size: data.len() as u64,
            stored_size: data.len() as u64,
        };
    }

    let mut size_bytes = [0u8; 8];
    size_bytes.copy_from_slice(&data[52..60]);
    let original_size = u64::from_le_bytes(size_bytes);

    EncryptInfo {
        encrypted: true,
        cipher: "chacha20",
        original_size,
        stored_size: data.len() as u64,
    }
}

/// Get encryption stats.
pub fn stats() -> (u64, u64, usize) {
    let state = STATE.lock();
    (
        state.files_encrypted,
        state.files_decrypted,
        state.keys.len(),
    )
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Derive a key from passphrase and salt.
fn derive_key(passphrase: &str, salt: &str) -> [u8; KEY_SIZE] {
    use crate::crypto::Sha256;

    let mut hasher = Sha256::new();
    hasher.update(passphrase.as_bytes());
    hasher.update(salt.as_bytes());
    hasher.update(b"FENC_KDF_V1");
    hasher.finalize()
}

/// Constant-time byte array comparison (prevents timing attacks).
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[encrypt] Running self-test...");

    test_chacha20_basic();
    test_hmac_sha256();
    test_encrypt_decrypt();
    test_wrong_key();
    test_tamper_detection();
    test_key_management();
    test_nonce_uniqueness();
    test_file_info();

    serial_println!("[encrypt] Self-test passed (8 tests).");
    Ok(())
}

fn test_chacha20_basic() {
    // Verify ChaCha20 is its own inverse (encrypt then decrypt = original).
    let key = [0x42u8; KEY_SIZE];
    let nonce = [0x01u8; NONCE_SIZE];
    let plaintext = b"Hello, ChaCha20 encryption test!";

    let ciphertext = chacha20_crypt(&key, &nonce, plaintext);
    assert_ne!(&ciphertext, plaintext.as_ref(), "should not be plaintext");

    let decrypted = chacha20_crypt(&key, &nonce, &ciphertext);
    assert_eq!(&decrypted, plaintext.as_ref(), "decrypt should match");

    serial_println!("[encrypt]   chacha20 basic: ok");
}

fn test_hmac_sha256() {
    // Basic HMAC-SHA256 consistency check.
    let key = [0xAB; KEY_SIZE];
    let msg = b"test message for HMAC";

    let mac1 = hmac_sha256(&key, msg);
    let mac2 = hmac_sha256(&key, msg);
    assert_eq!(mac1, mac2, "same input → same MAC");

    // Different message → different MAC.
    let mac3 = hmac_sha256(&key, b"different message");
    assert_ne!(mac1, mac3, "different input → different MAC");

    // Different key → different MAC.
    let key2 = [0xCD; KEY_SIZE];
    let mac4 = hmac_sha256(&key2, msg);
    assert_ne!(mac1, mac4, "different key → different MAC");

    serial_println!("[encrypt]   hmac-sha256: ok");
}

fn test_encrypt_decrypt() {
    // Full round-trip: add key, encrypt, decrypt.
    add_raw_key("test_enc_1", [0x55; KEY_SIZE]).expect("add key");

    let plaintext = b"Secret data that must be protected from unauthorized access.";
    let encrypted = encrypt(plaintext, "test_enc_1").expect("encrypt");

    assert!(is_encrypted(&encrypted));
    assert_ne!(&encrypted[HEADER_SIZE..], plaintext.as_ref());

    let decrypted = decrypt(&encrypted, "test_enc_1").expect("decrypt");
    assert_eq!(&decrypted, plaintext.as_ref());

    remove_key("test_enc_1").expect("remove key");
    serial_println!("[encrypt]   encrypt/decrypt round-trip: ok");
}

fn test_wrong_key() {
    // Decrypting with wrong key should fail (HMAC mismatch).
    add_raw_key("test_enc_2a", [0x11; KEY_SIZE]).expect("add key a");
    add_raw_key("test_enc_2b", [0x22; KEY_SIZE]).expect("add key b");

    let plaintext = b"encrypted with key A";
    let encrypted = encrypt(plaintext, "test_enc_2a").expect("encrypt");

    // Try to decrypt with key B.
    let result = decrypt(&encrypted, "test_enc_2b");
    assert!(result.is_err(), "wrong key should fail");

    remove_key("test_enc_2a").expect("rm");
    remove_key("test_enc_2b").expect("rm");
    serial_println!("[encrypt]   wrong key rejection: ok");
}

fn test_tamper_detection() {
    // Modifying ciphertext should fail HMAC verification.
    add_raw_key("test_enc_3", [0x33; KEY_SIZE]).expect("add key");

    let plaintext = b"tamper detection test data";
    let mut encrypted = encrypt(plaintext, "test_enc_3").expect("encrypt");

    // Tamper with a ciphertext byte.
    if encrypted.len() > HEADER_SIZE + 5 {
        encrypted[HEADER_SIZE + 5] ^= 0xFF;
    }

    let result = decrypt(&encrypted, "test_enc_3");
    assert!(result.is_err(), "tampered data should fail");

    remove_key("test_enc_3").expect("rm");
    serial_println!("[encrypt]   tamper detection: ok");
}

fn test_key_management() {
    // Test add/list/remove/has operations.
    assert!(!has_key("test_km"));

    add_key("test_km", "my passphrase").expect("add");
    assert!(has_key("test_km"));

    let keys = list_keys();
    assert!(keys.iter().any(|k| k.name == "test_km"));

    remove_key("test_km").expect("rm");
    assert!(!has_key("test_km"));

    // Removing non-existent should fail.
    assert!(remove_key("test_km").is_err());

    serial_println!("[encrypt]   key management: ok");
}

fn test_nonce_uniqueness() {
    // Generate multiple nonces — they should all be different.
    let n1 = generate_nonce().expect("the RNG is seeded");
    let n2 = generate_nonce().expect("the RNG is seeded");
    let n3 = generate_nonce().expect("the RNG is seeded");

    // 96 random bits each: any two alike is a broken generator.
    assert!(n1 != n2 && n2 != n3 && n1 != n3, "nonces are distinct");

    serial_println!("[encrypt]   nonce uniqueness: ok");
}

fn test_file_info() {
    add_raw_key("test_enc_info", [0x77; KEY_SIZE]).expect("add key");

    let data = b"file info test";
    let encrypted = encrypt(data, "test_enc_info").expect("encrypt");

    let info = file_info(&encrypted);
    assert!(info.encrypted);
    assert_eq!(info.cipher, "chacha20");
    assert_eq!(info.original_size, data.len() as u64);

    // Unencrypted data.
    let info2 = file_info(b"not encrypted");
    assert!(!info2.encrypted);

    remove_key("test_enc_info").expect("rm");
    serial_println!("[encrypt]   file info: ok");
}
