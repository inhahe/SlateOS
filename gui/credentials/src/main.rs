//! OurOS Credential Manager Service
//!
//! A system-wide password and credential storage service analogous to Windows
//! Credential Manager or GNOME Keyring. Credentials are encrypted at rest using
//! a session key derived from the user's master password. The service exposes an
//! IPC-based API for store, retrieve, search, autofill, and lifecycle operations.
//!
//! Security model:
//! - Master password verified via stored SHA-256 hash
//! - Session key derived as SHA-256(master_password + salt)
//! - XOR-based stream cipher for demonstration (production would use AES-256-GCM)
//! - Auto-lock after configurable idle timeout
//! - Rate limiting on failed unlock attempts

#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::struct_excessive_bools)]

use std::collections::HashMap;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Salt appended to master password before hashing to derive the session key.
const KEY_DERIVATION_SALT: &str = "ouros_credential_salt";

/// Default auto-lock timeout in seconds (5 minutes).
const DEFAULT_LOCK_TIMEOUT_SECS: u64 = 300;

/// Maximum consecutive failed unlock attempts before lockout.
const MAX_UNLOCK_ATTEMPTS: u32 = 3;

/// Lockout duration in seconds after too many failed attempts.
const LOCKOUT_DURATION_SECS: u64 = 30;

/// Base path for credential storage files.
const CREDENTIAL_STORE_BASE: &str = "/var/credentials";

// ---------------------------------------------------------------------------
// Error Types
// ---------------------------------------------------------------------------

/// Errors that can occur during credential manager operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialError {
    /// The credential store is locked; unlock it first.
    StoreLocked,
    /// The provided master password is incorrect.
    InvalidMasterPassword,
    /// Too many failed unlock attempts; temporarily locked out.
    RateLimited { retry_after_secs: u64 },
    /// The requested credential was not found.
    NotFound { id: u64 },
    /// A credential with the given name already exists.
    DuplicateName { name: String },
    /// The master password has not been set yet.
    MasterPasswordNotSet,
    /// Storage I/O failure.
    StorageError { detail: String },
    /// Encryption or decryption failure.
    CryptoError { detail: String },
    /// Invalid input parameter.
    InvalidInput { detail: String },
}

impl fmt::Display for CredentialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StoreLocked => write!(f, "credential store is locked"),
            Self::InvalidMasterPassword => write!(f, "invalid master password"),
            Self::RateLimited { retry_after_secs } => {
                write!(f, "rate limited, retry after {retry_after_secs}s")
            }
            Self::NotFound { id } => write!(f, "credential not found: {id}"),
            Self::DuplicateName { name } => {
                write!(f, "duplicate credential name: {name}")
            }
            Self::MasterPasswordNotSet => write!(f, "master password not set"),
            Self::StorageError { detail } => write!(f, "storage error: {detail}"),
            Self::CryptoError { detail } => write!(f, "crypto error: {detail}"),
            Self::InvalidInput { detail } => write!(f, "invalid input: {detail}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Credential Types
// ---------------------------------------------------------------------------

/// The type of credential stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialType {
    /// Username/password pair.
    Password,
    /// Bearer token or API token.
    Token,
    /// X.509 certificate (PEM or DER encoded).
    Certificate,
    /// SSH private key.
    SshKey,
    /// API key string.
    ApiKey,
    /// Application-defined credential type.
    Custom(String),
}

impl CredentialType {
    /// Serialize to a string representation for storage.
    #[allow(dead_code)]
    fn to_storage_string(&self) -> String {
        match self {
            Self::Password => "password".to_string(),
            Self::Token => "token".to_string(),
            Self::Certificate => "certificate".to_string(),
            Self::SshKey => "ssh_key".to_string(),
            Self::ApiKey => "api_key".to_string(),
            Self::Custom(s) => format!("custom:{s}"),
        }
    }

    /// Deserialize from a storage string.
    #[allow(dead_code)]
    fn from_storage_string(s: &str) -> Self {
        match s {
            "password" => Self::Password,
            "token" => Self::Token,
            "certificate" => Self::Certificate,
            "ssh_key" => Self::SshKey,
            "api_key" => Self::ApiKey,
            other => {
                if let Some(custom) = other.strip_prefix("custom:") {
                    Self::Custom(custom.to_string())
                } else {
                    Self::Custom(other.to_string())
                }
            }
        }
    }
}

/// A stored credential with encrypted secret data.
#[derive(Debug, Clone)]
pub struct Credential {
    /// Unique identifier.
    pub id: u64,
    /// Human-readable name.
    pub name: String,
    /// Type classification.
    pub credential_type: CredentialType,
    /// Optional associated username.
    pub username: Option<String>,
    /// Target URL or service name this credential applies to.
    pub target: String,
    /// Encrypted secret data (hex-encoded ciphertext).
    pub encrypted_data: Vec<u8>,
    /// Timestamp of creation (seconds since UNIX epoch).
    pub created_at: u64,
    /// Timestamp of last modification.
    pub modified_at: u64,
    /// Timestamp of last access.
    pub last_accessed: u64,
    /// Optional expiration timestamp.
    pub expires_at: Option<u64>,
    /// Freeform tags for organization.
    pub tags: Vec<String>,
}

/// Metadata view of a credential (no secret data exposed).
#[derive(Debug, Clone)]
pub struct CredentialMetadata {
    pub id: u64,
    pub name: String,
    pub credential_type: CredentialType,
    pub username: Option<String>,
    pub target: String,
    pub created_at: u64,
    pub modified_at: u64,
    pub last_accessed: u64,
    pub expires_at: Option<u64>,
    pub tags: Vec<String>,
}

impl From<&Credential> for CredentialMetadata {
    fn from(cred: &Credential) -> Self {
        Self {
            id: cred.id,
            name: cred.name.clone(),
            credential_type: cred.credential_type.clone(),
            username: cred.username.clone(),
            target: cred.target.clone(),
            created_at: cred.created_at,
            modified_at: cred.modified_at,
            last_accessed: cred.last_accessed,
            expires_at: cred.expires_at,
            tags: cred.tags.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Cryptography (demonstration-grade XOR cipher + SHA-256)
// ---------------------------------------------------------------------------

/// Compute SHA-256 hash of the input bytes.
///
/// This is a software implementation of SHA-256 following FIPS 180-4.
fn sha256(data: &[u8]) -> [u8; 32] {
    // Initial hash values (first 32 bits of fractional parts of sqrt of first 8 primes)
    let mut h: [u32; 8] = [
        0x6a09_e667, 0xbb67_ae85, 0x3c6e_f372, 0xa54f_f53a,
        0x510e_527f, 0x9b05_688c, 0x1f83_d9ab, 0x5be0_cd19,
    ];

    // Round constants (first 32 bits of fractional parts of cube roots of first 64 primes)
    const K: [u32; 64] = [
        0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5,
        0x3956_c25b, 0x59f1_11f1, 0x923f_82a4, 0xab1c_5ed5,
        0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3,
        0x72be_5d74, 0x80de_b1fe, 0x9bdc_06a7, 0xc19b_f174,
        0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc,
        0x2de9_2c6f, 0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da,
        0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
        0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967,
        0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc, 0x5338_0d13,
        0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x92722_c85,
        0xa2bf_e8a1, 0xa81a_664b, 0xc24b_8b70, 0xc76c_51a3,
        0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070,
        0x19a4_c116, 0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5,
        0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
        0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208,
        0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7, 0xc671_78f2,
    ];

    // Pre-processing: pad message to multiple of 512 bits (64 bytes)
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    // Process each 512-bit (64-byte) block
    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            let base = i * 4;
            w[i] = u32::from_be_bytes([
                chunk[base],
                chunk[base + 1],
                chunk[base + 2],
                chunk[base + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7)
                ^ w[i - 15].rotate_right(18)
                ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17)
                ^ w[i - 2].rotate_right(19)
                ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut result = [0u8; 32];
    for (i, val) in h.iter().enumerate() {
        let bytes = val.to_be_bytes();
        result[i * 4] = bytes[0];
        result[i * 4 + 1] = bytes[1];
        result[i * 4 + 2] = bytes[2];
        result[i * 4 + 3] = bytes[3];
    }
    result
}

/// Derive a 32-byte session key from the master password using SHA-256.
fn derive_session_key(master_password: &str) -> [u8; 32] {
    let mut input = master_password.as_bytes().to_vec();
    input.extend_from_slice(KEY_DERIVATION_SALT.as_bytes());
    sha256(&input)
}

/// Encrypt plaintext using XOR stream cipher with the given key.
///
/// The key is expanded by repeatedly hashing (key || counter) to produce
/// a keystream of sufficient length. This is a demonstration cipher;
/// production use would employ AES-256-GCM.
pub fn encrypt(plaintext: &[u8], key: &[u8; 32]) -> Vec<u8> {
    let keystream = generate_keystream(key, plaintext.len());
    plaintext
        .iter()
        .zip(keystream.iter())
        .map(|(p, k)| p ^ k)
        .collect()
}

/// Decrypt ciphertext using XOR stream cipher with the given key.
///
/// XOR encryption is symmetric so decryption is the same operation.
pub fn decrypt(ciphertext: &[u8], key: &[u8; 32]) -> Vec<u8> {
    encrypt(ciphertext, key)
}

/// Generate a keystream of the requested length by chaining SHA-256 hashes.
fn generate_keystream(key: &[u8; 32], length: usize) -> Vec<u8> {
    let mut stream = Vec::with_capacity(length);
    let mut counter: u64 = 0;
    while stream.len() < length {
        let mut block_input = key.to_vec();
        block_input.extend_from_slice(&counter.to_le_bytes());
        let block = sha256(&block_input);
        let remaining = length.saturating_sub(stream.len());
        let take = remaining.min(32);
        stream.extend_from_slice(&block[..take]);
        counter = counter.wrapping_add(1);
    }
    stream.truncate(length);
    stream
}

/// Encode bytes as a hex string.
pub fn to_hex(data: &[u8]) -> String {
    let mut hex = String::with_capacity(data.len() * 2);
    for byte in data {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// Decode a hex string into bytes.
pub fn from_hex(hex: &str) -> Result<Vec<u8>, CredentialError> {
    if hex.len() % 2 != 0 {
        return Err(CredentialError::CryptoError {
            detail: "hex string has odd length".to_string(),
        });
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    let mut chars = hex.chars();
    loop {
        let high = match chars.next() {
            Some(c) => c,
            None => break,
        };
        let low = match chars.next() {
            Some(c) => c,
            None => {
                return Err(CredentialError::CryptoError {
                    detail: "unexpected end of hex string".to_string(),
                });
            }
        };
        let byte = hex_char_to_nibble(high)? << 4 | hex_char_to_nibble(low)?;
        bytes.push(byte);
    }
    Ok(bytes)
}

fn hex_char_to_nibble(c: char) -> Result<u8, CredentialError> {
    match c {
        '0'..='9' => Ok(c as u8 - b'0'),
        'a'..='f' => Ok(c as u8 - b'a' + 10),
        'A'..='F' => Ok(c as u8 - b'A' + 10),
        _ => Err(CredentialError::CryptoError {
            detail: format!("invalid hex character: {c}"),
        }),
    }
}

// ---------------------------------------------------------------------------
// Password Generator
// ---------------------------------------------------------------------------

/// Password strength classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PasswordStrength {
    Weak,
    Medium,
    Strong,
    VeryStrong,
}

/// Xorshift64 PRNG state.
struct Xorshift64 {
    state: u64,
}

impl Xorshift64 {
    /// Create a new PRNG seeded from the given value.
    /// The seed must not be zero.
    fn new(seed: u64) -> Self {
        let state = if seed == 0 { 0xDEAD_BEEF_CAFE_BABE } else { seed };
        Self { state }
    }

    /// Generate the next pseudo-random u64.
    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// Generate a random value in [0, bound).
    fn next_bounded(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        self.next_u64() % bound
    }
}

/// Generate a random password with the specified character classes.
pub fn generate_password(
    length: usize,
    include_uppercase: bool,
    include_digits: bool,
    include_symbols: bool,
    seed: u64,
) -> String {
    let mut charset = Vec::new();

    // Always include lowercase
    for c in b'a'..=b'z' {
        charset.push(c as char);
    }

    if include_uppercase {
        for c in b'A'..=b'Z' {
            charset.push(c as char);
        }
    }

    if include_digits {
        for c in b'0'..=b'9' {
            charset.push(c as char);
        }
    }

    if include_symbols {
        for &c in b"!@#$%^&*()-_=+[]{}|;:,.<>?" {
            charset.push(c as char);
        }
    }

    if charset.is_empty() {
        return String::new();
    }

    let mut rng = Xorshift64::new(seed);
    let charset_len = charset.len() as u64;

    (0..length)
        .map(|_| {
            let idx = rng.next_bounded(charset_len) as usize;
            charset.get(idx).copied().unwrap_or('a')
        })
        .collect()
}

/// Estimate password strength based on length and character class diversity.
pub fn estimate_password_strength(password: &str) -> PasswordStrength {
    let len = password.len();
    let has_lower = password.chars().any(|c| c.is_ascii_lowercase());
    let has_upper = password.chars().any(|c| c.is_ascii_uppercase());
    let has_digit = password.chars().any(|c| c.is_ascii_digit());
    let has_symbol = password.chars().any(|c| c.is_ascii_punctuation());

    let class_count =
        u32::from(has_lower) + u32::from(has_upper) + u32::from(has_digit) + u32::from(has_symbol);

    if len < 8 || class_count <= 1 {
        PasswordStrength::Weak
    } else if len < 12 || class_count <= 2 {
        PasswordStrength::Medium
    } else if len < 16 || class_count <= 3 {
        PasswordStrength::Strong
    } else {
        PasswordStrength::VeryStrong
    }
}

// ---------------------------------------------------------------------------
// URL Matching for Autofill
// ---------------------------------------------------------------------------

/// Priority level for URL matching (higher = better match).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatchPriority {
    /// No match at all.
    None,
    /// Wildcard subdomain match (*.example.com).
    Wildcard,
    /// Parent domain match (target is parent of query domain).
    ParentDomain,
    /// Exact domain match.
    ExactDomain,
    /// Exact domain + path prefix match.
    ExactWithPath,
}

/// Extract the domain from a URL string (strips scheme, port, path).
fn extract_domain(url: &str) -> &str {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .or_else(|| url.strip_prefix("ftp://"))
        .unwrap_or(url);

    // Take everything before the first '/' or ':'
    let end = without_scheme
        .find('/')
        .or_else(|| without_scheme.find(':'))
        .unwrap_or(without_scheme.len());

    &without_scheme[..end]
}

/// Extract the path from a URL (everything after the domain).
fn extract_path(url: &str) -> &str {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .or_else(|| url.strip_prefix("ftp://"))
        .unwrap_or(url);

    match without_scheme.find('/') {
        Some(idx) => &without_scheme[idx..],
        None => "/",
    }
}

/// Determine how well a stored credential target matches a query URL.
pub fn match_url(credential_target: &str, query_url: &str) -> MatchPriority {
    let target_domain = extract_domain(credential_target);
    let query_domain = extract_domain(query_url);

    // Check for wildcard pattern (*.example.com)
    if let Some(wildcard_base) = target_domain.strip_prefix("*.") {
        if query_domain == wildcard_base
            || query_domain.ends_with(&format!(".{wildcard_base}"))
        {
            return MatchPriority::Wildcard;
        }
        return MatchPriority::None;
    }

    // Check exact domain match
    if target_domain.eq_ignore_ascii_case(query_domain) {
        // Check if target has a path prefix that also matches
        let target_path = extract_path(credential_target);
        let query_path = extract_path(query_url);

        if target_path != "/" && query_path.starts_with(target_path) {
            return MatchPriority::ExactWithPath;
        }
        return MatchPriority::ExactDomain;
    }

    // Check if query domain is a subdomain of the target domain
    if query_domain.ends_with(&format!(".{target_domain}")) {
        return MatchPriority::ParentDomain;
    }

    MatchPriority::None
}

// ---------------------------------------------------------------------------
// Service API Messages
// ---------------------------------------------------------------------------

/// Filter criteria for listing credentials.
#[derive(Debug, Clone, Default)]
pub struct ListFilter {
    /// Filter by credential type.
    pub credential_type: Option<CredentialType>,
    /// Filter by tag (credential must have this tag).
    pub tag: Option<String>,
    /// Filter by target prefix.
    pub target_prefix: Option<String>,
}

/// Requests sent to the credential manager service.
#[derive(Debug, Clone)]
pub enum CredentialRequest {
    /// Unlock the store with the master password.
    Unlock { master_password: String },
    /// Lock the store (clear session key).
    Lock,
    /// Store a new credential.
    Store {
        name: String,
        credential_type: CredentialType,
        username: Option<String>,
        target: String,
        data: Vec<u8>,
        tags: Vec<String>,
    },
    /// Retrieve a credential by ID (decrypted).
    Retrieve { id: u64 },
    /// Retrieve credentials matching a target URL/service.
    RetrieveByTarget { target: String },
    /// Update fields on an existing credential.
    Update {
        id: u64,
        name: Option<String>,
        username: Option<String>,
        target: Option<String>,
        data: Option<Vec<u8>>,
        tags: Option<Vec<String>>,
    },
    /// Delete a credential by ID.
    Delete { id: u64 },
    /// List credentials (metadata only) with optional filter.
    List { filter: ListFilter },
    /// Search by query string (matches name, target, tags).
    Search { query: String },
    /// Set or change the master password.
    SetMasterPassword {
        old_password: Option<String>,
        new_password: String,
    },
    /// Query for autofill candidates matching a URL.
    AutofillQuery { url: String },
    /// Check whether the store is locked.
    IsLocked,
    /// Configure the auto-lock timeout.
    SetTimeout { seconds: u64 },
}

/// Responses from the credential manager service.
#[derive(Debug, Clone)]
pub enum CredentialResponse {
    /// Operation succeeded with no payload.
    Ok,
    /// Operation returned a credential ID.
    Stored { id: u64 },
    /// A single decrypted credential.
    Credential {
        id: u64,
        name: String,
        credential_type: CredentialType,
        username: Option<String>,
        target: String,
        data: Vec<u8>,
        created_at: u64,
        modified_at: u64,
        expires_at: Option<u64>,
        tags: Vec<String>,
    },
    /// Multiple credentials (decrypted).
    Credentials(Vec<CredentialMetadata>),
    /// Lock status.
    LockStatus { locked: bool },
    /// An error occurred.
    Error(CredentialError),
}

// ---------------------------------------------------------------------------
// Credential Store (Core State)
// ---------------------------------------------------------------------------

/// The main credential store holding all state.
pub struct CredentialStore {
    /// All stored credentials, indexed by ID.
    credentials: HashMap<u64, Credential>,
    /// Next credential ID to assign.
    next_id: u64,
    /// User ID that owns this store.
    uid: u32,
    /// SHA-256 hash of the master password (for verification).
    master_password_hash: Option<[u8; 32]>,
    /// Derived session key (present only when unlocked).
    session_key: Option<[u8; 32]>,
    /// Timestamp of last activity (for auto-lock).
    last_activity: u64,
    /// Auto-lock timeout in seconds.
    lock_timeout_secs: u64,
    /// Number of consecutive failed unlock attempts.
    failed_attempts: u32,
    /// Timestamp when lockout expires (0 = no lockout).
    lockout_until: u64,
}

impl CredentialStore {
    /// Create a new empty credential store for the given user.
    pub fn new(uid: u32) -> Self {
        Self {
            credentials: HashMap::new(),
            next_id: 1,
            uid,
            master_password_hash: None,
            session_key: None,
            last_activity: current_timestamp(),
            lock_timeout_secs: DEFAULT_LOCK_TIMEOUT_SECS,
            failed_attempts: 0,
            lockout_until: 0,
        }
    }

    /// Check whether the store is currently locked.
    pub fn is_locked(&self) -> bool {
        self.session_key.is_none()
    }

    /// Update the last activity timestamp (resets auto-lock timer).
    fn touch(&mut self) {
        self.last_activity = current_timestamp();
    }

    /// Check if auto-lock timeout has elapsed and lock if so.
    pub fn check_auto_lock(&mut self) {
        if self.session_key.is_some() {
            let now = current_timestamp();
            if now.saturating_sub(self.last_activity) >= self.lock_timeout_secs {
                self.lock();
            }
        }
    }

    /// Set the master password (first time or change).
    pub fn set_master_password(
        &mut self,
        old_password: Option<&str>,
        new_password: &str,
    ) -> Result<(), CredentialError> {
        // If a master password is already set, verify the old one
        if let Some(existing_hash) = self.master_password_hash {
            let old_pw = old_password.ok_or(CredentialError::InvalidMasterPassword)?;
            let old_hash = sha256(old_pw.as_bytes());
            if old_hash != existing_hash {
                return Err(CredentialError::InvalidMasterPassword);
            }

            // Re-encrypt all credentials with the new key
            let old_key = derive_session_key(old_pw);
            let new_key = derive_session_key(new_password);

            for cred in self.credentials.values_mut() {
                let plaintext = decrypt(&cred.encrypted_data, &old_key);
                cred.encrypted_data = encrypt(&plaintext, &new_key);
            }

            self.session_key = Some(new_key);
        }

        self.master_password_hash = Some(sha256(new_password.as_bytes()));
        if self.session_key.is_none() {
            self.session_key = Some(derive_session_key(new_password));
        }
        self.touch();
        Ok(())
    }

    /// Unlock the store with the master password.
    pub fn unlock(&mut self, master_password: &str) -> Result<(), CredentialError> {
        // Check rate limiting
        let now = current_timestamp();
        if now < self.lockout_until {
            return Err(CredentialError::RateLimited {
                retry_after_secs: self.lockout_until.saturating_sub(now),
            });
        }

        let hash = self.master_password_hash.ok_or(CredentialError::MasterPasswordNotSet)?;
        let attempt_hash = sha256(master_password.as_bytes());

        if attempt_hash != hash {
            self.failed_attempts = self.failed_attempts.saturating_add(1);
            if self.failed_attempts >= MAX_UNLOCK_ATTEMPTS {
                self.lockout_until = now.saturating_add(LOCKOUT_DURATION_SECS);
                self.failed_attempts = 0;
            }
            return Err(CredentialError::InvalidMasterPassword);
        }

        // Success — reset attempts and derive session key
        self.failed_attempts = 0;
        self.session_key = Some(derive_session_key(master_password));
        self.touch();
        Ok(())
    }

    /// Lock the store (securely clear session key).
    pub fn lock(&mut self) {
        if let Some(ref mut key) = self.session_key {
            // Overwrite key memory before dropping
            for byte in key.iter_mut() {
                *byte = 0;
            }
        }
        self.session_key = None;
    }

    /// Store a new credential. Requires the store to be unlocked.
    pub fn store_credential(
        &mut self,
        name: String,
        credential_type: CredentialType,
        username: Option<String>,
        target: String,
        data: &[u8],
        tags: Vec<String>,
    ) -> Result<u64, CredentialError> {
        let key = self.require_unlocked()?;

        if name.is_empty() {
            return Err(CredentialError::InvalidInput {
                detail: "credential name cannot be empty".to_string(),
            });
        }

        let now = current_timestamp();
        let encrypted_data = encrypt(data, &key);
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);

        let credential = Credential {
            id,
            name,
            credential_type,
            username,
            target,
            encrypted_data,
            created_at: now,
            modified_at: now,
            last_accessed: now,
            expires_at: None,
            tags,
        };

        self.credentials.insert(id, credential);
        self.touch();
        Ok(id)
    }

    /// Retrieve and decrypt a credential by ID.
    pub fn retrieve(&mut self, id: u64) -> Result<(CredentialMetadata, Vec<u8>), CredentialError> {
        let key = self.require_unlocked()?;

        let cred = self
            .credentials
            .get_mut(&id)
            .ok_or(CredentialError::NotFound { id })?;

        cred.last_accessed = current_timestamp();
        let plaintext = decrypt(&cred.encrypted_data, &key);
        let metadata = CredentialMetadata::from(&*cred);

        self.touch();
        Ok((metadata, plaintext))
    }

    /// Find credentials matching a target URL/service name.
    pub fn retrieve_by_target(
        &mut self,
        target: &str,
    ) -> Result<Vec<(CredentialMetadata, Vec<u8>)>, CredentialError> {
        let key = self.require_unlocked()?;
        let now = current_timestamp();

        let mut results: Vec<(CredentialMetadata, Vec<u8>, MatchPriority)> = Vec::new();

        for cred in self.credentials.values_mut() {
            let priority = match_url(&cred.target, target);
            if priority != MatchPriority::None {
                cred.last_accessed = now;
                let plaintext = decrypt(&cred.encrypted_data, &key);
                let metadata = CredentialMetadata::from(&*cred);
                results.push((metadata, plaintext, priority));
            }
        }

        // Sort by priority (highest first)
        results.sort_by(|a, b| b.2.cmp(&a.2));

        self.touch();
        Ok(results.into_iter().map(|(m, d, _)| (m, d)).collect())
    }

    /// Update fields on an existing credential.
    pub fn update(
        &mut self,
        id: u64,
        name: Option<String>,
        username: Option<String>,
        target: Option<String>,
        data: Option<&[u8]>,
        tags: Option<Vec<String>>,
    ) -> Result<(), CredentialError> {
        let key = self.require_unlocked()?;

        let cred = self
            .credentials
            .get_mut(&id)
            .ok_or(CredentialError::NotFound { id })?;

        if let Some(new_name) = name {
            if new_name.is_empty() {
                return Err(CredentialError::InvalidInput {
                    detail: "credential name cannot be empty".to_string(),
                });
            }
            cred.name = new_name;
        }
        if let Some(new_username) = username {
            cred.username = Some(new_username);
        }
        if let Some(new_target) = target {
            cred.target = new_target;
        }
        if let Some(new_data) = data {
            cred.encrypted_data = encrypt(new_data, &key);
        }
        if let Some(new_tags) = tags {
            cred.tags = new_tags;
        }

        cred.modified_at = current_timestamp();
        self.touch();
        Ok(())
    }

    /// Delete a credential by ID.
    pub fn delete(&mut self, id: u64) -> Result<(), CredentialError> {
        let _key = self.require_unlocked()?;

        if self.credentials.remove(&id).is_none() {
            return Err(CredentialError::NotFound { id });
        }

        self.touch();
        Ok(())
    }

    /// List credential metadata (no secrets) with optional filter.
    pub fn list(&self, filter: &ListFilter) -> Vec<CredentialMetadata> {
        self.credentials
            .values()
            .filter(|cred| {
                if let Some(ref ct) = filter.credential_type {
                    if cred.credential_type != *ct {
                        return false;
                    }
                }
                if let Some(ref tag) = filter.tag {
                    if !cred.tags.contains(tag) {
                        return false;
                    }
                }
                if let Some(ref prefix) = filter.target_prefix {
                    if !cred.target.starts_with(prefix.as_str()) {
                        return false;
                    }
                }
                true
            })
            .map(CredentialMetadata::from)
            .collect()
    }

    /// Search credentials by query string (matches name, target, tags).
    pub fn search(&self, query: &str) -> Vec<CredentialMetadata> {
        let query_lower = query.to_ascii_lowercase();

        self.credentials
            .values()
            .filter(|cred| {
                cred.name.to_ascii_lowercase().contains(&query_lower)
                    || cred.target.to_ascii_lowercase().contains(&query_lower)
                    || cred.tags.iter().any(|t| {
                        t.to_ascii_lowercase().contains(&query_lower)
                    })
                    || cred
                        .username
                        .as_ref()
                        .map_or(false, |u| u.to_ascii_lowercase().contains(&query_lower))
            })
            .map(CredentialMetadata::from)
            .collect()
    }

    /// Find autofill candidates for a URL, sorted by match priority.
    pub fn autofill_query(&self, url: &str) -> Vec<CredentialMetadata> {
        let mut matches: Vec<(CredentialMetadata, MatchPriority)> = self
            .credentials
            .values()
            .filter_map(|cred| {
                let priority = match_url(&cred.target, url);
                if priority != MatchPriority::None {
                    Some((CredentialMetadata::from(cred), priority))
                } else {
                    None
                }
            })
            .collect();

        matches.sort_by(|a, b| b.1.cmp(&a.1));
        matches.into_iter().map(|(m, _)| m).collect()
    }

    /// Set the auto-lock timeout.
    pub fn set_timeout(&mut self, seconds: u64) {
        self.lock_timeout_secs = seconds;
    }

    /// Get the storage path for this user's credential file.
    pub fn storage_path(&self) -> String {
        format!("{CREDENTIAL_STORE_BASE}/{}.json", self.uid)
    }

    /// Require that the store is unlocked, returning the session key.
    fn require_unlocked(&self) -> Result<[u8; 32], CredentialError> {
        self.session_key.ok_or(CredentialError::StoreLocked)
    }

    /// Handle a request and produce a response.
    pub fn handle_request(&mut self, request: CredentialRequest) -> CredentialResponse {
        // Check auto-lock before processing
        self.check_auto_lock();

        match request {
            CredentialRequest::IsLocked => {
                CredentialResponse::LockStatus { locked: self.is_locked() }
            }
            CredentialRequest::Unlock { master_password } => {
                match self.unlock(&master_password) {
                    Ok(()) => CredentialResponse::Ok,
                    Err(e) => CredentialResponse::Error(e),
                }
            }
            CredentialRequest::Lock => {
                self.lock();
                CredentialResponse::Ok
            }
            CredentialRequest::SetMasterPassword { old_password, new_password } => {
                match self.set_master_password(old_password.as_deref(), &new_password) {
                    Ok(()) => CredentialResponse::Ok,
                    Err(e) => CredentialResponse::Error(e),
                }
            }
            CredentialRequest::Store {
                name,
                credential_type,
                username,
                target,
                data,
                tags,
            } => match self.store_credential(name, credential_type, username, target, &data, tags) {
                Ok(id) => CredentialResponse::Stored { id },
                Err(e) => CredentialResponse::Error(e),
            },
            CredentialRequest::Retrieve { id } => match self.retrieve(id) {
                Ok((meta, data)) => CredentialResponse::Credential {
                    id: meta.id,
                    name: meta.name,
                    credential_type: meta.credential_type,
                    username: meta.username,
                    target: meta.target,
                    data,
                    created_at: meta.created_at,
                    modified_at: meta.modified_at,
                    expires_at: meta.expires_at,
                    tags: meta.tags,
                },
                Err(e) => CredentialResponse::Error(e),
            },
            CredentialRequest::RetrieveByTarget { target } => {
                match self.retrieve_by_target(&target) {
                    Ok(results) => {
                        let metadata: Vec<CredentialMetadata> =
                            results.into_iter().map(|(m, _)| m).collect();
                        CredentialResponse::Credentials(metadata)
                    }
                    Err(e) => CredentialResponse::Error(e),
                }
            }
            CredentialRequest::Update {
                id,
                name,
                username,
                target,
                data,
                tags,
            } => match self.update(id, name, username, target, data.as_deref(), tags) {
                Ok(()) => CredentialResponse::Ok,
                Err(e) => CredentialResponse::Error(e),
            },
            CredentialRequest::Delete { id } => match self.delete(id) {
                Ok(()) => CredentialResponse::Ok,
                Err(e) => CredentialResponse::Error(e),
            },
            CredentialRequest::List { filter } => {
                CredentialResponse::Credentials(self.list(&filter))
            }
            CredentialRequest::Search { query } => {
                CredentialResponse::Credentials(self.search(&query))
            }
            CredentialRequest::AutofillQuery { url } => {
                CredentialResponse::Credentials(self.autofill_query(&url))
            }
            CredentialRequest::SetTimeout { seconds } => {
                self.set_timeout(seconds);
                CredentialResponse::Ok
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Identity Verification with Debounce
// ---------------------------------------------------------------------------

/// Sensitivity level for credential operations.
/// Higher sensitivity requires more recent verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SensitivityLevel {
    /// Reading metadata (list, search) — no verification needed.
    Low,
    /// Retrieving decrypted credentials — verification needed.
    Medium,
    /// Modifying or deleting credentials — verification needed.
    High,
    /// Changing master password, exporting store — strictest verification.
    Critical,
}

impl SensitivityLevel {
    /// Returns the default debounce window for this sensitivity level.
    /// More sensitive operations have shorter debounce windows.
    fn default_debounce_secs(self) -> u64 {
        match self {
            Self::Low => 0,       // No verification required
            Self::Medium => 60,   // 1 minute
            Self::High => 30,     // 30 seconds
            Self::Critical => 0,  // Always re-verify
        }
    }
}

/// Result of an identity verification check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationResult {
    /// Verification passed (either fresh or within debounce window).
    Verified,
    /// Verification is required — the caller must prompt the user.
    VerificationRequired {
        /// Why verification is needed.
        reason: String,
        /// The sensitivity level that triggered the check.
        level: SensitivityLevel,
    },
    /// Verification failed (wrong password).
    Failed,
    /// Temporarily locked out due to too many failed attempts.
    LockedOut { retry_after_secs: u64 },
}

/// Configuration for the identity verification system.
#[derive(Debug, Clone)]
pub struct VerificationConfig {
    /// Debounce window per sensitivity level (seconds).
    /// If the user verified within this many seconds, skip re-verification.
    pub debounce_secs: [u64; 4], // indexed by SensitivityLevel ordinal
    /// Maximum failed verification attempts before lockout.
    pub max_attempts: u32,
    /// Lockout duration in seconds.
    pub lockout_secs: u64,
    /// Whether to require verification for medium-sensitivity operations.
    pub require_for_medium: bool,
    /// Whether to require verification for high-sensitivity operations.
    pub require_for_high: bool,
    /// Whether verification is globally enabled.
    pub enabled: bool,
}

impl Default for VerificationConfig {
    fn default() -> Self {
        Self {
            debounce_secs: [
                SensitivityLevel::Low.default_debounce_secs(),
                SensitivityLevel::Medium.default_debounce_secs(),
                SensitivityLevel::High.default_debounce_secs(),
                SensitivityLevel::Critical.default_debounce_secs(),
            ],
            max_attempts: 3,
            lockout_secs: 30,
            require_for_medium: true,
            require_for_high: true,
            enabled: true,
        }
    }
}

impl VerificationConfig {
    /// Get the debounce window for a given sensitivity level.
    fn debounce_for(&self, level: SensitivityLevel) -> u64 {
        let idx = level as usize;
        if idx < self.debounce_secs.len() {
            self.debounce_secs[idx]
        } else {
            0
        }
    }
}

/// Tracks identity verification state with per-level debounce.
#[derive(Debug)]
pub struct IdentityVerifier {
    /// Configuration for verification behavior.
    config: VerificationConfig,
    /// Timestamp of last successful verification per sensitivity level.
    /// Verification at a higher level also counts for lower levels.
    last_verified: [u64; 4],
    /// Consecutive failed attempts in the current session.
    failed_attempts: u32,
    /// Timestamp when lockout expires (0 = no lockout).
    lockout_until: u64,
    /// Total successful verifications (for audit/metrics).
    total_verifications: u64,
    /// Total failed verifications (for audit/metrics).
    total_failures: u64,
}

impl IdentityVerifier {
    /// Create a new verifier with default configuration.
    pub fn new() -> Self {
        Self {
            config: VerificationConfig::default(),
            last_verified: [0; 4],
            failed_attempts: 0,
            lockout_until: 0,
            total_verifications: 0,
            total_failures: 0,
        }
    }

    /// Create a new verifier with custom configuration.
    pub fn with_config(config: VerificationConfig) -> Self {
        Self {
            config,
            last_verified: [0; 4],
            failed_attempts: 0,
            lockout_until: 0,
            total_verifications: 0,
            total_failures: 0,
        }
    }

    /// Check whether verification is needed for the given sensitivity level.
    ///
    /// Returns `Verified` if within the debounce window, or
    /// `VerificationRequired` if the user must re-authenticate.
    pub fn check(&self, level: SensitivityLevel, now: u64) -> VerificationResult {
        // If verification is globally disabled, always pass
        if !self.config.enabled {
            return VerificationResult::Verified;
        }

        // Low sensitivity never requires verification
        if level == SensitivityLevel::Low {
            return VerificationResult::Verified;
        }

        // Check if this level is configured to require verification
        if level == SensitivityLevel::Medium && !self.config.require_for_medium {
            return VerificationResult::Verified;
        }
        if level == SensitivityLevel::High && !self.config.require_for_high {
            return VerificationResult::Verified;
        }

        // Check lockout
        if now < self.lockout_until {
            return VerificationResult::LockedOut {
                retry_after_secs: self.lockout_until.saturating_sub(now),
            };
        }

        // Critical always requires fresh verification
        if level == SensitivityLevel::Critical {
            return VerificationResult::VerificationRequired {
                reason: "This operation requires identity verification.".to_string(),
                level,
            };
        }

        // Check debounce: was there a recent verification at this level or higher?
        let debounce_window = self.config.debounce_for(level);
        let level_idx = level as usize;

        // Check this level and all higher levels (a Critical verification
        // satisfies Medium and High checks too)
        for check_idx in level_idx..self.last_verified.len() {
            let last = self.last_verified[check_idx];
            if last > 0 && now.saturating_sub(last) < debounce_window {
                return VerificationResult::Verified;
            }
        }

        // No recent verification — require one
        let reason = match level {
            SensitivityLevel::Medium => {
                "Viewing credential secrets requires identity verification.".to_string()
            }
            SensitivityLevel::High => {
                "Modifying credentials requires identity verification.".to_string()
            }
            _ => "Identity verification required.".to_string(),
        };

        VerificationResult::VerificationRequired { reason, level }
    }

    /// Record a successful verification.
    ///
    /// Verifying at a given level also satisfies all lower levels.
    pub fn record_success(&mut self, level: SensitivityLevel, now: u64) {
        let level_idx = level as usize;
        // Set the timestamp for this level and all lower levels
        for idx in 0..=level_idx {
            self.last_verified[idx] = now;
        }
        self.failed_attempts = 0;
        self.total_verifications = self.total_verifications.saturating_add(1);
    }

    /// Record a failed verification attempt.
    ///
    /// Returns the resulting lockout state.
    pub fn record_failure(&mut self, now: u64) -> VerificationResult {
        self.failed_attempts = self.failed_attempts.saturating_add(1);
        self.total_failures = self.total_failures.saturating_add(1);

        if self.failed_attempts >= self.config.max_attempts {
            self.lockout_until = now.saturating_add(self.config.lockout_secs);
            self.failed_attempts = 0;
            VerificationResult::LockedOut {
                retry_after_secs: self.config.lockout_secs,
            }
        } else {
            VerificationResult::Failed
        }
    }

    /// Verify the user's identity by checking their master password against
    /// the stored hash. On success, records the verification with debounce.
    pub fn verify(
        &mut self,
        password: &str,
        master_password_hash: &[u8; 32],
        level: SensitivityLevel,
        now: u64,
    ) -> VerificationResult {
        // Check lockout first
        if now < self.lockout_until {
            return VerificationResult::LockedOut {
                retry_after_secs: self.lockout_until.saturating_sub(now),
            };
        }

        let attempt_hash = sha256(password.as_bytes());
        if attempt_hash == *master_password_hash {
            self.record_success(level, now);
            VerificationResult::Verified
        } else {
            self.record_failure(now)
        }
    }

    /// Clear all verification state (e.g., on store lock).
    pub fn clear(&mut self) {
        self.last_verified = [0; 4];
        // Don't clear failed_attempts or lockout — those persist across locks
    }

    /// Get the debounce configuration.
    pub fn config(&self) -> &VerificationConfig {
        &self.config
    }

    /// Update the debounce window for a specific sensitivity level.
    pub fn set_debounce(&mut self, level: SensitivityLevel, seconds: u64) {
        let idx = level as usize;
        if idx < self.config.debounce_secs.len() {
            self.config.debounce_secs[idx] = seconds;
        }
    }

    /// Enable or disable verification globally.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.config.enabled = enabled;
    }

    /// Get verification statistics.
    pub fn stats(&self) -> (u64, u64) {
        (self.total_verifications, self.total_failures)
    }

    /// Check if currently locked out.
    pub fn is_locked_out(&self, now: u64) -> bool {
        now < self.lockout_until
    }

    /// Seconds remaining in lockout (0 if not locked out).
    pub fn lockout_remaining(&self, now: u64) -> u64 {
        if now < self.lockout_until {
            self.lockout_until.saturating_sub(now)
        } else {
            0
        }
    }

    /// Seconds since last verification at the given level.
    /// Returns `None` if never verified at that level.
    pub fn time_since_verification(&self, level: SensitivityLevel, now: u64) -> Option<u64> {
        let idx = level as usize;
        if idx < self.last_verified.len() && self.last_verified[idx] > 0 {
            Some(now.saturating_sub(self.last_verified[idx]))
        } else {
            None
        }
    }
}

/// Classify a credential operation by its sensitivity level.
pub fn classify_operation(request: &CredentialRequest) -> SensitivityLevel {
    match request {
        // Read-only metadata operations
        CredentialRequest::IsLocked
        | CredentialRequest::List { .. }
        | CredentialRequest::Search { .. }
        | CredentialRequest::SetTimeout { .. } => SensitivityLevel::Low,

        // Viewing decrypted secrets
        CredentialRequest::Retrieve { .. }
        | CredentialRequest::RetrieveByTarget { .. }
        | CredentialRequest::AutofillQuery { .. } => SensitivityLevel::Medium,

        // Modifying credentials
        CredentialRequest::Store { .. }
        | CredentialRequest::Update { .. }
        | CredentialRequest::Delete { .. }
        | CredentialRequest::Lock
        | CredentialRequest::Unlock { .. } => SensitivityLevel::High,

        // Critical security operations
        CredentialRequest::SetMasterPassword { .. } => SensitivityLevel::Critical,
    }
}

// ---------------------------------------------------------------------------
// Utility Functions
// ---------------------------------------------------------------------------

/// Get current timestamp as seconds since UNIX epoch.
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Entry Point
// ---------------------------------------------------------------------------

fn main() {
    // The credential manager runs as a system service, receiving IPC requests.
    // For now, perform a self-test to validate core functionality.
    let mut store = CredentialStore::new(1000);

    // Set master password
    if let Err(e) = store.set_master_password(None, "initial_master_password") {
        eprintln!("Failed to set master password: {e}");
        return;
    }

    println!("OurOS Credential Manager v0.1.0");
    println!("Store initialized for uid={}", store.uid);
    println!("Storage path: {}", store.storage_path());
    println!("Auto-lock timeout: {}s", store.lock_timeout_secs);
    println!("Status: {}", if store.is_locked() { "locked" } else { "unlocked" });
    println!("\nCredential Manager service ready. Awaiting IPC requests...");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Crypto tests --

    #[test]
    fn test_sha256_empty() {
        let hash = sha256(b"");
        let expected = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(to_hex(&hash), expected);
    }

    #[test]
    fn test_sha256_hello() {
        let hash = sha256(b"hello");
        let expected = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        assert_eq!(to_hex(&hash), expected);
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = derive_session_key("test_password");
        let plaintext = b"super secret credential data";
        let ciphertext = encrypt(plaintext, &key);

        // Ciphertext should differ from plaintext
        assert_ne!(&ciphertext[..], &plaintext[..]);

        let decrypted = decrypt(&ciphertext, &key);
        assert_eq!(&decrypted[..], &plaintext[..]);
    }

    #[test]
    fn test_encrypt_decrypt_empty() {
        let key = derive_session_key("key");
        let plaintext = b"";
        let ciphertext = encrypt(plaintext, &key);
        assert!(ciphertext.is_empty());
        let decrypted = decrypt(&ciphertext, &key);
        assert!(decrypted.is_empty());
    }

    #[test]
    fn test_wrong_key_fails_decrypt() {
        let key1 = derive_session_key("password1");
        let key2 = derive_session_key("password2");
        let plaintext = b"sensitive data";
        let ciphertext = encrypt(plaintext, &key1);
        let wrong_decrypt = decrypt(&ciphertext, &key2);
        assert_ne!(&wrong_decrypt[..], &plaintext[..]);
    }

    #[test]
    fn test_hex_roundtrip() {
        let data = vec![0x00, 0x0f, 0xde, 0xad, 0xbe, 0xef, 0xff];
        let hex = to_hex(&data);
        assert_eq!(hex, "000fdeadbeefff");
        let decoded = from_hex(&hex).expect("valid hex");
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_hex_invalid() {
        assert!(from_hex("0g").is_err());
        assert!(from_hex("abc").is_err()); // odd length
    }

    // -- Store tests --

    #[test]
    fn test_store_locked_by_default_after_new() {
        let store = CredentialStore::new(1000);
        // No master password set yet, so session_key is None
        assert!(store.is_locked());
    }

    #[test]
    fn test_set_master_password_unlocks() {
        let mut store = CredentialStore::new(1000);
        store
            .set_master_password(None, "my_password")
            .expect("should succeed");
        assert!(!store.is_locked());
    }

    #[test]
    fn test_lock_and_unlock() {
        let mut store = CredentialStore::new(1000);
        store.set_master_password(None, "pw123").expect("set pw");
        assert!(!store.is_locked());

        store.lock();
        assert!(store.is_locked());

        store.unlock("pw123").expect("unlock");
        assert!(!store.is_locked());
    }

    #[test]
    fn test_unlock_wrong_password() {
        let mut store = CredentialStore::new(1000);
        store.set_master_password(None, "correct").expect("set pw");
        store.lock();

        let result = store.unlock("wrong");
        assert_eq!(result, Err(CredentialError::InvalidMasterPassword));
    }

    #[test]
    fn test_rate_limiting() {
        let mut store = CredentialStore::new(1000);
        store.set_master_password(None, "secure").expect("set pw");
        store.lock();

        // Fail 3 times to trigger lockout
        for _ in 0..MAX_UNLOCK_ATTEMPTS {
            let _ = store.unlock("wrong");
        }

        // Next attempt should be rate-limited
        let result = store.unlock("secure");
        assert!(matches!(result, Err(CredentialError::RateLimited { .. })));
    }

    #[test]
    fn test_store_and_retrieve_credential() {
        let mut store = CredentialStore::new(1000);
        store.set_master_password(None, "master").expect("set pw");

        let id = store
            .store_credential(
                "GitHub Token".to_string(),
                CredentialType::Token,
                Some("octocat".to_string()),
                "https://github.com".to_string(),
                b"ghp_1234567890abcdef",
                vec!["dev".to_string(), "vcs".to_string()],
            )
            .expect("store");

        let (meta, data) = store.retrieve(id).expect("retrieve");
        assert_eq!(meta.name, "GitHub Token");
        assert_eq!(meta.credential_type, CredentialType::Token);
        assert_eq!(meta.username, Some("octocat".to_string()));
        assert_eq!(&data, b"ghp_1234567890abcdef");
    }

    #[test]
    fn test_retrieve_while_locked() {
        let mut store = CredentialStore::new(1000);
        store.set_master_password(None, "master").expect("set pw");

        let id = store
            .store_credential(
                "Test".to_string(),
                CredentialType::Password,
                None,
                "example.com".to_string(),
                b"secret",
                vec![],
            )
            .expect("store");

        store.lock();
        let result = store.retrieve(id);
        assert_eq!(result, Err(CredentialError::StoreLocked));
    }

    #[test]
    fn test_delete_credential() {
        let mut store = CredentialStore::new(1000);
        store.set_master_password(None, "master").expect("set pw");

        let id = store
            .store_credential(
                "Temp".to_string(),
                CredentialType::ApiKey,
                None,
                "api.example.com".to_string(),
                b"key123",
                vec![],
            )
            .expect("store");

        store.delete(id).expect("delete");
        let result = store.retrieve(id);
        assert_eq!(result, Err(CredentialError::NotFound { id }));
    }

    #[test]
    fn test_search_credentials() {
        let mut store = CredentialStore::new(1000);
        store.set_master_password(None, "master").expect("set pw");

        store
            .store_credential(
                "Work Email".to_string(),
                CredentialType::Password,
                Some("alice@work.com".to_string()),
                "https://mail.work.com".to_string(),
                b"pass1",
                vec!["work".to_string()],
            )
            .expect("store");

        store
            .store_credential(
                "Personal Email".to_string(),
                CredentialType::Password,
                Some("alice@home.com".to_string()),
                "https://mail.home.com".to_string(),
                b"pass2",
                vec!["personal".to_string()],
            )
            .expect("store");

        let results = store.search("work");
        assert_eq!(results.len(), 2); // Matches name "Work Email" and tag "work" + target "mail.work.com"

        let results = store.search("personal");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Personal Email");
    }

    // -- URL matching tests --

    #[test]
    fn test_url_match_exact_domain() {
        let priority = match_url("github.com", "https://github.com/login");
        assert_eq!(priority, MatchPriority::ExactDomain);
    }

    #[test]
    fn test_url_match_with_path() {
        let priority = match_url("https://example.com/app", "https://example.com/app/settings");
        assert_eq!(priority, MatchPriority::ExactWithPath);
    }

    #[test]
    fn test_url_match_wildcard() {
        let priority = match_url("*.example.com", "https://login.example.com/auth");
        assert_eq!(priority, MatchPriority::Wildcard);
    }

    #[test]
    fn test_url_match_parent_domain() {
        let priority = match_url("example.com", "https://sub.example.com/page");
        assert_eq!(priority, MatchPriority::ParentDomain);
    }

    #[test]
    fn test_url_no_match() {
        let priority = match_url("github.com", "https://gitlab.com/repo");
        assert_eq!(priority, MatchPriority::None);
    }

    // -- Password generator tests --

    #[test]
    fn test_generate_password_length() {
        let pw = generate_password(20, true, true, true, 42);
        assert_eq!(pw.len(), 20);
    }

    #[test]
    fn test_generate_password_lowercase_only() {
        let pw = generate_password(16, false, false, false, 123);
        assert!(pw.chars().all(|c| c.is_ascii_lowercase()));
    }

    #[test]
    fn test_generate_password_all_classes() {
        // With a large enough password, we should get all classes
        let pw = generate_password(100, true, true, true, 999);
        let has_lower = pw.chars().any(|c| c.is_ascii_lowercase());
        let has_upper = pw.chars().any(|c| c.is_ascii_uppercase());
        let has_digit = pw.chars().any(|c| c.is_ascii_digit());
        let has_symbol = pw.chars().any(|c| c.is_ascii_punctuation());
        assert!(has_lower);
        assert!(has_upper);
        assert!(has_digit);
        assert!(has_symbol);
    }

    #[test]
    fn test_password_strength_weak() {
        assert_eq!(estimate_password_strength("abc"), PasswordStrength::Weak);
        assert_eq!(estimate_password_strength("abcdefg"), PasswordStrength::Weak);
    }

    #[test]
    fn test_password_strength_medium() {
        assert_eq!(
            estimate_password_strength("Abcdefgh"),
            PasswordStrength::Medium
        );
    }

    #[test]
    fn test_password_strength_strong() {
        assert_eq!(
            estimate_password_strength("Abcdefgh1234"),
            PasswordStrength::Strong
        );
    }

    #[test]
    fn test_password_strength_very_strong() {
        assert_eq!(
            estimate_password_strength("Abcdefgh1234!@#$"),
            PasswordStrength::VeryStrong
        );
    }

    // -- Change master password test --

    #[test]
    fn test_change_master_password_reencrypts() {
        let mut store = CredentialStore::new(1000);
        store.set_master_password(None, "old_pw").expect("set pw");

        let id = store
            .store_credential(
                "Test Cred".to_string(),
                CredentialType::Password,
                None,
                "example.com".to_string(),
                b"my_secret",
                vec![],
            )
            .expect("store");

        // Change master password
        store
            .set_master_password(Some("old_pw"), "new_pw")
            .expect("change pw");

        // Verify can still retrieve with new key active
        let (_, data) = store.retrieve(id).expect("retrieve");
        assert_eq!(&data, b"my_secret");

        // Lock, unlock with new password
        store.lock();
        store.unlock("new_pw").expect("unlock with new pw");
        let (_, data) = store.retrieve(id).expect("retrieve after relock");
        assert_eq!(&data, b"my_secret");
    }

    // -- Handle request integration test --

    #[test]
    fn test_handle_request_lifecycle() {
        let mut store = CredentialStore::new(1000);

        // Set master password
        let resp = store.handle_request(CredentialRequest::SetMasterPassword {
            old_password: None,
            new_password: "master123".to_string(),
        });
        assert!(matches!(resp, CredentialResponse::Ok));

        // Store credential
        let resp = store.handle_request(CredentialRequest::Store {
            name: "SSH Key".to_string(),
            credential_type: CredentialType::SshKey,
            username: Some("root".to_string()),
            target: "server.internal".to_string(),
            data: b"-----BEGIN OPENSSH PRIVATE KEY-----".to_vec(),
            tags: vec!["infra".to_string()],
        });
        let stored_id = match resp {
            CredentialResponse::Stored { id } => id,
            other => panic!("expected Stored, got {other:?}"),
        };

        // Lock
        let resp = store.handle_request(CredentialRequest::Lock);
        assert!(matches!(resp, CredentialResponse::Ok));

        // Attempt retrieve while locked
        let resp = store.handle_request(CredentialRequest::Retrieve { id: stored_id });
        assert!(matches!(
            resp,
            CredentialResponse::Error(CredentialError::StoreLocked)
        ));

        // Check lock status
        let resp = store.handle_request(CredentialRequest::IsLocked);
        assert!(matches!(resp, CredentialResponse::LockStatus { locked: true }));

        // Unlock
        let resp = store.handle_request(CredentialRequest::Unlock {
            master_password: "master123".to_string(),
        });
        assert!(matches!(resp, CredentialResponse::Ok));

        // Retrieve successfully
        let resp = store.handle_request(CredentialRequest::Retrieve { id: stored_id });
        match resp {
            CredentialResponse::Credential { name, data, .. } => {
                assert_eq!(name, "SSH Key");
                assert_eq!(&data, b"-----BEGIN OPENSSH PRIVATE KEY-----");
            }
            other => panic!("expected Credential, got {other:?}"),
        }

        // Delete
        let resp = store.handle_request(CredentialRequest::Delete { id: stored_id });
        assert!(matches!(resp, CredentialResponse::Ok));

        // Verify gone
        let resp = store.handle_request(CredentialRequest::Retrieve { id: stored_id });
        assert!(matches!(
            resp,
            CredentialResponse::Error(CredentialError::NotFound { .. })
        ));
    }

    // -- Autofill test --

    #[test]
    fn test_autofill_query_priority_ordering() {
        let mut store = CredentialStore::new(1000);
        store.set_master_password(None, "master").expect("set pw");

        // Wildcard match
        store
            .store_credential(
                "Wildcard".to_string(),
                CredentialType::Password,
                None,
                "*.example.com".to_string(),
                b"wild",
                vec![],
            )
            .expect("store");

        // Exact domain match
        store
            .store_credential(
                "Exact".to_string(),
                CredentialType::Password,
                None,
                "login.example.com".to_string(),
                b"exact",
                vec![],
            )
            .expect("store");

        let results = store.autofill_query("https://login.example.com/auth");
        assert_eq!(results.len(), 2);
        // Exact domain should come first (higher priority)
        assert_eq!(results[0].name, "Exact");
        assert_eq!(results[1].name, "Wildcard");
    }

    // -- Identity Verification with Debounce tests --

    #[test]
    fn test_verifier_low_sensitivity_always_passes() {
        let verifier = IdentityVerifier::new();
        let result = verifier.check(SensitivityLevel::Low, 1000);
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn test_verifier_medium_requires_verification_initially() {
        let verifier = IdentityVerifier::new();
        let result = verifier.check(SensitivityLevel::Medium, 1000);
        assert!(matches!(result, VerificationResult::VerificationRequired { .. }));
    }

    #[test]
    fn test_verifier_high_requires_verification_initially() {
        let verifier = IdentityVerifier::new();
        let result = verifier.check(SensitivityLevel::High, 1000);
        assert!(matches!(result, VerificationResult::VerificationRequired { .. }));
    }

    #[test]
    fn test_verifier_critical_always_requires_verification() {
        let mut verifier = IdentityVerifier::new();
        // Even after a recent verification, Critical always requires fresh
        verifier.record_success(SensitivityLevel::Critical, 1000);
        let result = verifier.check(SensitivityLevel::Critical, 1001);
        assert!(matches!(result, VerificationResult::VerificationRequired { .. }));
    }

    #[test]
    fn test_verifier_debounce_medium_within_window() {
        let mut verifier = IdentityVerifier::new();
        // Default medium debounce = 60s
        verifier.record_success(SensitivityLevel::Medium, 1000);
        // 30 seconds later — within debounce window
        let result = verifier.check(SensitivityLevel::Medium, 1030);
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn test_verifier_debounce_medium_expired() {
        let mut verifier = IdentityVerifier::new();
        verifier.record_success(SensitivityLevel::Medium, 1000);
        // 61 seconds later — past 60s debounce window
        let result = verifier.check(SensitivityLevel::Medium, 1061);
        assert!(matches!(result, VerificationResult::VerificationRequired { .. }));
    }

    #[test]
    fn test_verifier_debounce_high_within_window() {
        let mut verifier = IdentityVerifier::new();
        // Default high debounce = 30s
        verifier.record_success(SensitivityLevel::High, 1000);
        let result = verifier.check(SensitivityLevel::High, 1020);
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn test_verifier_debounce_high_expired() {
        let mut verifier = IdentityVerifier::new();
        verifier.record_success(SensitivityLevel::High, 1000);
        // 31 seconds later — past 30s debounce
        let result = verifier.check(SensitivityLevel::High, 1031);
        assert!(matches!(result, VerificationResult::VerificationRequired { .. }));
    }

    #[test]
    fn test_verifier_higher_level_satisfies_lower() {
        let mut verifier = IdentityVerifier::new();
        // Verify at High level
        verifier.record_success(SensitivityLevel::High, 1000);
        // Medium should also be satisfied (High > Medium)
        let result = verifier.check(SensitivityLevel::Medium, 1010);
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn test_verifier_lower_level_does_not_satisfy_higher() {
        let mut verifier = IdentityVerifier::new();
        // Verify at Medium level
        verifier.record_success(SensitivityLevel::Medium, 1000);
        // High should still require verification (Medium < High)
        let result = verifier.check(SensitivityLevel::High, 1010);
        assert!(matches!(result, VerificationResult::VerificationRequired { .. }));
    }

    #[test]
    fn test_verifier_password_verification_success() {
        let mut verifier = IdentityVerifier::new();
        let master_hash = sha256(b"correct_password");
        let result = verifier.verify("correct_password", &master_hash, SensitivityLevel::Medium, 1000);
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn test_verifier_password_verification_failure() {
        let mut verifier = IdentityVerifier::new();
        let master_hash = sha256(b"correct_password");
        let result = verifier.verify("wrong_password", &master_hash, SensitivityLevel::Medium, 1000);
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn test_verifier_lockout_after_max_attempts() {
        let mut verifier = IdentityVerifier::new();
        let master_hash = sha256(b"correct_password");
        // 3 failed attempts
        verifier.verify("wrong1", &master_hash, SensitivityLevel::Medium, 1000);
        verifier.verify("wrong2", &master_hash, SensitivityLevel::Medium, 1001);
        let result = verifier.verify("wrong3", &master_hash, SensitivityLevel::Medium, 1002);
        assert!(matches!(result, VerificationResult::LockedOut { .. }));
    }

    #[test]
    fn test_verifier_lockout_blocks_check() {
        let mut verifier = IdentityVerifier::new();
        let master_hash = sha256(b"correct_password");
        // Trigger lockout
        verifier.verify("wrong1", &master_hash, SensitivityLevel::Medium, 1000);
        verifier.verify("wrong2", &master_hash, SensitivityLevel::Medium, 1001);
        verifier.verify("wrong3", &master_hash, SensitivityLevel::Medium, 1002);
        // Even check() should report lockout
        let result = verifier.check(SensitivityLevel::Medium, 1005);
        assert!(matches!(result, VerificationResult::LockedOut { .. }));
    }

    #[test]
    fn test_verifier_lockout_expires() {
        let mut verifier = IdentityVerifier::new();
        let master_hash = sha256(b"correct_password");
        // Trigger lockout (default 30s)
        verifier.verify("wrong1", &master_hash, SensitivityLevel::Medium, 1000);
        verifier.verify("wrong2", &master_hash, SensitivityLevel::Medium, 1001);
        verifier.verify("wrong3", &master_hash, SensitivityLevel::Medium, 1002);
        // After lockout expires (30s), should be able to verify again
        let result = verifier.verify("correct_password", &master_hash, SensitivityLevel::Medium, 1035);
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn test_verifier_success_resets_failed_count() {
        let mut verifier = IdentityVerifier::new();
        let master_hash = sha256(b"correct_password");
        // 2 failed attempts
        verifier.verify("wrong1", &master_hash, SensitivityLevel::Medium, 1000);
        verifier.verify("wrong2", &master_hash, SensitivityLevel::Medium, 1001);
        // Success resets counter
        verifier.verify("correct_password", &master_hash, SensitivityLevel::Medium, 1002);
        // Another failure should not trigger lockout (counter was reset)
        let result = verifier.verify("wrong3", &master_hash, SensitivityLevel::Medium, 1003);
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn test_verifier_disabled_bypasses_all() {
        let mut config = VerificationConfig::default();
        config.enabled = false;
        let verifier = IdentityVerifier::with_config(config);
        // Even Critical should pass when disabled
        let result = verifier.check(SensitivityLevel::Critical, 1000);
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn test_verifier_medium_requirement_can_be_disabled() {
        let mut config = VerificationConfig::default();
        config.require_for_medium = false;
        let verifier = IdentityVerifier::with_config(config);
        let result = verifier.check(SensitivityLevel::Medium, 1000);
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn test_verifier_high_requirement_can_be_disabled() {
        let mut config = VerificationConfig::default();
        config.require_for_high = false;
        let verifier = IdentityVerifier::with_config(config);
        let result = verifier.check(SensitivityLevel::High, 1000);
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn test_verifier_custom_debounce() {
        let mut verifier = IdentityVerifier::new();
        verifier.set_debounce(SensitivityLevel::Medium, 120); // 2 minutes
        verifier.record_success(SensitivityLevel::Medium, 1000);
        // 90 seconds later — within custom 120s window
        let result = verifier.check(SensitivityLevel::Medium, 1090);
        assert_eq!(result, VerificationResult::Verified);
        // 121 seconds later — past custom window
        let result = verifier.check(SensitivityLevel::Medium, 1121);
        assert!(matches!(result, VerificationResult::VerificationRequired { .. }));
    }

    #[test]
    fn test_verifier_clear_resets_timestamps() {
        let mut verifier = IdentityVerifier::new();
        verifier.record_success(SensitivityLevel::High, 1000);
        verifier.clear();
        // Should require verification again after clear
        let result = verifier.check(SensitivityLevel::Medium, 1010);
        assert!(matches!(result, VerificationResult::VerificationRequired { .. }));
    }

    #[test]
    fn test_verifier_stats() {
        let mut verifier = IdentityVerifier::new();
        let master_hash = sha256(b"correct_password");
        verifier.verify("correct_password", &master_hash, SensitivityLevel::Medium, 1000);
        verifier.verify("wrong", &master_hash, SensitivityLevel::Medium, 1001);
        verifier.verify("correct_password", &master_hash, SensitivityLevel::Medium, 1002);
        let (successes, failures) = verifier.stats();
        assert_eq!(successes, 2);
        assert_eq!(failures, 1);
    }

    #[test]
    fn test_verifier_lockout_remaining() {
        let mut verifier = IdentityVerifier::new();
        let master_hash = sha256(b"correct_password");
        verifier.verify("wrong1", &master_hash, SensitivityLevel::Medium, 1000);
        verifier.verify("wrong2", &master_hash, SensitivityLevel::Medium, 1001);
        verifier.verify("wrong3", &master_hash, SensitivityLevel::Medium, 1002);
        // Lockout duration is 30s from timestamp 1002
        assert!(verifier.is_locked_out(1010));
        assert_eq!(verifier.lockout_remaining(1010), 22); // 1032 - 1010
        assert!(!verifier.is_locked_out(1035));
        assert_eq!(verifier.lockout_remaining(1035), 0);
    }

    #[test]
    fn test_verifier_time_since_verification() {
        let mut verifier = IdentityVerifier::new();
        assert_eq!(verifier.time_since_verification(SensitivityLevel::Medium, 1000), None);
        verifier.record_success(SensitivityLevel::Medium, 1000);
        assert_eq!(verifier.time_since_verification(SensitivityLevel::Medium, 1045), Some(45));
    }

    #[test]
    fn test_classify_operation_low() {
        let request = CredentialRequest::IsLocked;
        assert_eq!(classify_operation(&request), SensitivityLevel::Low);

        let request = CredentialRequest::List { filter: ListFilter::default() };
        assert_eq!(classify_operation(&request), SensitivityLevel::Low);

        let request = CredentialRequest::Search { query: "test".to_string() };
        assert_eq!(classify_operation(&request), SensitivityLevel::Low);
    }

    #[test]
    fn test_classify_operation_medium() {
        let request = CredentialRequest::Retrieve { id: 1 };
        assert_eq!(classify_operation(&request), SensitivityLevel::Medium);

        let request = CredentialRequest::AutofillQuery { url: "https://example.com".to_string() };
        assert_eq!(classify_operation(&request), SensitivityLevel::Medium);
    }

    #[test]
    fn test_classify_operation_high() {
        let request = CredentialRequest::Delete { id: 1 };
        assert_eq!(classify_operation(&request), SensitivityLevel::High);

        let request = CredentialRequest::Store {
            name: "test".to_string(),
            credential_type: CredentialType::Password,
            username: None,
            target: "test.com".to_string(),
            data: vec![],
            tags: vec![],
        };
        assert_eq!(classify_operation(&request), SensitivityLevel::High);
    }

    #[test]
    fn test_classify_operation_critical() {
        let request = CredentialRequest::SetMasterPassword {
            old_password: Some("old".to_string()),
            new_password: "new".to_string(),
        };
        assert_eq!(classify_operation(&request), SensitivityLevel::Critical);
    }

    #[test]
    fn test_sensitivity_level_ordering() {
        assert!(SensitivityLevel::Low < SensitivityLevel::Medium);
        assert!(SensitivityLevel::Medium < SensitivityLevel::High);
        assert!(SensitivityLevel::High < SensitivityLevel::Critical);
    }

    #[test]
    fn test_verification_result_reason_messages() {
        let verifier = IdentityVerifier::new();
        if let VerificationResult::VerificationRequired { reason, level } =
            verifier.check(SensitivityLevel::Medium, 1000)
        {
            assert!(reason.contains("secrets"));
            assert_eq!(level, SensitivityLevel::Medium);
        } else {
            panic!("expected VerificationRequired");
        }

        if let VerificationResult::VerificationRequired { reason, level } =
            verifier.check(SensitivityLevel::High, 1000)
        {
            assert!(reason.contains("Modifying"));
            assert_eq!(level, SensitivityLevel::High);
        } else {
            panic!("expected VerificationRequired");
        }
    }

    #[test]
    fn test_verifier_set_enabled_toggle() {
        let mut verifier = IdentityVerifier::new();
        // Initially enabled — requires verification
        assert!(matches!(
            verifier.check(SensitivityLevel::High, 1000),
            VerificationResult::VerificationRequired { .. }
        ));
        // Disable
        verifier.set_enabled(false);
        assert_eq!(verifier.check(SensitivityLevel::High, 1000), VerificationResult::Verified);
        // Re-enable
        verifier.set_enabled(true);
        assert!(matches!(
            verifier.check(SensitivityLevel::High, 1000),
            VerificationResult::VerificationRequired { .. }
        ));
    }

    #[test]
    fn test_default_debounce_values() {
        assert_eq!(SensitivityLevel::Low.default_debounce_secs(), 0);
        assert_eq!(SensitivityLevel::Medium.default_debounce_secs(), 60);
        assert_eq!(SensitivityLevel::High.default_debounce_secs(), 30);
        assert_eq!(SensitivityLevel::Critical.default_debounce_secs(), 0);
    }
}
