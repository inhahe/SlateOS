//! Slate OS Credential Manager Service
//!
//! A system-wide password and credential storage service analogous to Windows
//! Credential Manager or GNOME Keyring. Credentials are encrypted at rest using
//! a session key derived from the user's master password. The service exposes an
//! IPC-based API for store, retrieve, search, autofill, and lifecycle operations.
//!
//! Security model:
//! - The session key is derived by iterating SHA-256 [`DEFAULT_KDF_ROUNDS`]
//!   times over the password and salt, so that testing one guess costs the
//!   attacker what one unlock costs the user.
//! - The master password is verified against a value derived from that
//!   *stretched* key, not from the password directly — otherwise a guess
//!   would cost one hash and the stretching would be decorative.
//! - Encryption is a counter-mode stream cipher keyed on SHA-256, with a
//!   fresh nonce per encryption. It is **not authenticated**: ciphertext can
//!   be modified undetectably. See `known-issues.md`.
//! - Auto-lock after a configurable idle timeout.
//! - Rate limiting on failed unlock attempts.
//!
//! None of the above is a substitute for a vetted primitive; see
//! `open-questions.md` C-Q5 for whether this tree should be porting one
//! rather than writing its own.


use std::collections::HashMap;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

// SHA-256 and the constant-time digest comparison used to be written out in
// this file. They were one of twenty-six hand-copied SHA-256s in the tree; the
// algorithm and its FIPS 180-4 vectors now live in `sha2`, once.
use sha2::{eq_constant_time, sha256};

// Likewise the password generator's xorshift, which reduced with `% bound`.
use randrange::Rng;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Salt mixed into the master password before hashing to derive the session key.
///
/// A salt's job is to be *different for every vault*, so that an attacker who
/// precomputes a table of hashed passwords has to redo the work for each
/// victim rather than once for the world. This one is a compile-time constant
/// and so does exactly the opposite — every SlateOS install shares it.
///
/// It is still here because fixing it needs a per-vault random value, and
/// there is no source of unpredictable numbers in userspace yet; see
/// `requests/c-a-userspace-entropy-syscall.md`. When that syscall exists, this
/// constant becomes a per-vault salt generated at `set_master_password` time
/// and stored beside the verifier. Tracked in `known-issues.md` →
/// `C-THE-MASTER-PASSWORD-IS-HASHED-ONCE-WITH-A-SALT-EVERY-INSTALL-SHARES`.
const KEY_DERIVATION_SALT: &str = "slateos_credential_salt";

/// How many hash iterations turn a master password into a key, for a vault
/// created today.
///
/// This is *key stretching*, and it is the entire defence a password-derived
/// key has. A password has perhaps 30–40 bits of real entropy, which is
/// nothing; what makes it survive is that each guess costs the attacker time.
/// A single SHA-256 pass — what this used to do — lets commodity GPU hardware
/// try billions of candidates per second. At 100 000 iterations the same
/// hardware manages tens of thousands: a factor of 10^5, bought once.
///
/// The number is chosen the standard way — by measurement, so that one
/// derivation costs enough to hurt an attacker and not enough for a user to
/// notice. On the development machine one SHA-256 of a ~70-byte input takes
/// 1.28 µs in a release build, so 100 000 rounds is ~130 ms per unlock.
///
/// It is a *default* rather than a constant of the format because the right
/// number rises with hardware, and a vault written under the old number must
/// keep opening after the default moves: the cost is a property of the stored
/// verifier, not of the code that reads it. Every real password-hashing format
/// records its cost parameters alongside the hash for this reason. See
/// [`CredentialStore::kdf_rounds`].
///
/// This is PBKDF2's *structure*, not PBKDF2 (which iterates HMAC, not a bare
/// hash), and it is far weaker than a memory-hard function such as Argon2id —
/// stretching only costs an attacker time, whereas memory-hardness also
/// denies them the parallelism that makes GPUs worth using. Argon2id is the
/// end state; see `open-questions.md` → C-Q5.
const DEFAULT_KDF_ROUNDS: u32 = 100_000;

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
#[derive(Debug, Clone, PartialEq, Eq)]
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
// Cryptography (key derivation + counter-mode stream cipher)
// ---------------------------------------------------------------------------


/// The size of the nonce prefix that [`encrypt`] writes ahead of a ciphertext.
const NONCE_LEN: usize = 8;

/// Derive a 32-byte session key from the master password at a given cost.
///
/// `rounds` comes from the vault ([`CredentialStore::kdf_rounds`]), not from a
/// constant — see [`DEFAULT_KDF_ROUNDS`] for why, and for why the single
/// SHA-256 pass this used to be was not a key derivation function at all.
fn derive_session_key(master_password: &str, rounds: u32) -> [u8; 32] {
    stretch(
        master_password.as_bytes(),
        KEY_DERIVATION_SALT.as_bytes(),
        rounds,
    )
}

/// Iterated SHA-256 over `password` and `salt` — PBKDF2's shape.
///
/// The password and salt are folded back in on *every* round rather than the
/// accumulator merely being hashed with itself. That matters: a chain of the
/// form `h = SHA-256(h)` is the same chain for every password, so an attacker
/// could walk it once and then test candidates against any point on it. Mixing
/// the password in each round makes the whole chain password-specific, which
/// is what forces the attacker to pay the full cost per guess.
fn stretch(password: &[u8], salt: &[u8], rounds: u32) -> [u8; 32] {
    let mut buf = Vec::with_capacity(32usize.saturating_add(password.len()).saturating_add(salt.len()));
    buf.extend_from_slice(password);
    buf.extend_from_slice(salt);
    let mut acc = sha256(&buf);

    for _ in 0..rounds {
        buf.clear();
        buf.extend_from_slice(&acc);
        buf.extend_from_slice(password);
        buf.extend_from_slice(salt);
        acc = sha256(&buf);
    }
    acc
}

/// The value stored to check a master password against, derived from the
/// *stretched* key rather than from the password.
///
/// This distinction is the whole point. The store used to keep
/// `SHA-256(password)`, which meant an attacker holding a copy of the vault
/// could test a candidate password for the price of one SHA-256 and never
/// touch [`derive_session_key`] at all — so stretching the key would have
/// bought exactly nothing. Deriving the verifier from the key instead puts the
/// full [`KEY_DERIVATION_ROUNDS`] between the attacker and each guess.
///
/// The extra label keeps this from being a value the key itself could collide
/// with: the verifier is public (it is stored beside the ciphertext) and must
/// not be usable as, or derivable into, the key.
fn verifier_for(key: &[u8; 32]) -> [u8; 32] {
    let label = b"slateos-credential-verifier";
    let mut buf = Vec::with_capacity(32usize.saturating_add(label.len()));
    buf.extend_from_slice(key);
    buf.extend_from_slice(label);
    sha256(&buf)
}


/// Encrypt plaintext under `key`, using `nonce` once and only once.
///
/// The returned blob is `nonce ‖ ciphertext`: the nonce travels with the
/// ciphertext so that [`decrypt`] needs nothing but the key, and so that a
/// caller cannot store the ciphertext and forget the nonce.
///
/// **The caller must never reuse a nonce under the same key.** This is a
/// stream cipher: the keystream is a function of `(key, nonce)` alone, so two
/// messages sharing both are XORed with identical material, and XORing the two
/// ciphertexts together cancels the key entirely and leaves the two plaintexts
/// XORed with each other — recoverable by hand for anything text-like. That is
/// precisely the bug this parameter exists to fix; before it, *every* record
/// in a vault shared one keystream. [`CredentialStore`] supplies nonces from a
/// counter that only ever increases.
///
/// Still not authenticated: an attacker who cannot read the ciphertext can
/// nonetheless flip any bit of the plaintext by flipping the same bit of the
/// ciphertext, undetectably. Fixing that needs an AEAD or an HMAC — see
/// `open-questions.md` → C-Q5.
pub fn encrypt(plaintext: &[u8], key: &[u8; 32], nonce: u64) -> Vec<u8> {
    let keystream = generate_keystream(key, nonce, plaintext.len());
    let mut out = Vec::with_capacity(NONCE_LEN.saturating_add(plaintext.len()));
    out.extend_from_slice(&nonce.to_be_bytes());
    out.extend(plaintext.iter().zip(keystream).map(|(p, k)| p ^ k));
    out
}

/// Decrypt a blob produced by [`encrypt`], reading the nonce off its front.
///
/// No longer the same operation as `encrypt`, because the output carries a
/// nonce the input did not. A blob too short to hold one is malformed rather
/// than empty — an empty plaintext still encrypts to `NONCE_LEN` bytes, so
/// anything shorter never came from `encrypt`.
pub fn decrypt(blob: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, CredentialError> {
    let (prefix, body) = blob.split_at_checked(NONCE_LEN).ok_or_else(|| {
        CredentialError::CryptoError {
            detail: format!(
                "ciphertext is {} bytes, too short to carry its {NONCE_LEN}-byte nonce",
                blob.len()
            ),
        }
    })?;
    let nonce_bytes: [u8; NONCE_LEN] =
        prefix
            .try_into()
            .map_err(|_| CredentialError::CryptoError {
                detail: "malformed nonce prefix".to_string(),
            })?;
    let nonce = u64::from_be_bytes(nonce_bytes);
    let keystream = generate_keystream(key, nonce, body.len());
    Ok(body.iter().zip(keystream).map(|(c, k)| c ^ k).collect())
}

/// Generate `length` bytes of keystream for `(key, nonce)`.
///
/// SHA-256 used as a counter-mode pseudo-random function: block `i` is
/// `SHA-256(key ‖ nonce ‖ i)`. The nonce is what makes two encryptions under
/// one key produce different keystreams; the counter is what lets one
/// encryption exceed 32 bytes.
fn generate_keystream(key: &[u8; 32], nonce: u64, length: usize) -> Vec<u8> {
    let mut stream = Vec::with_capacity(length);
    let mut counter: u64 = 0;
    while stream.len() < length {
        let mut block_input = Vec::with_capacity(48);
        block_input.extend_from_slice(key);
        block_input.extend_from_slice(&nonce.to_be_bytes());
        block_input.extend_from_slice(&counter.to_be_bytes());
        let block = sha256(&block_input);
        let remaining = length.saturating_sub(stream.len());
        stream.extend(block.into_iter().take(remaining));
        counter = counter.wrapping_add(1);
    }
    stream
}

/// Encode bytes as a hex string.
pub fn to_hex(data: &[u8]) -> String {
    let mut hex = String::with_capacity(data.len().saturating_mul(2));
    for byte in data {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// Decode a hex string into bytes.
pub fn from_hex(hex: &str) -> Result<Vec<u8>, CredentialError> {
    // There used to be an `if hex.len() % 2 != 0` guard above this loop, with
    // its own "odd length" error, alongside the mid-pair check below. The two
    // did not measure the same thing: `len` is a count of *bytes* and the loop
    // consumes *characters*, so `"éa"` — two characters, three bytes — got the
    // odd-length error while `"ééa"` slipped past the guard on an even byte
    // count and hit the loop's error instead. Nothing downstream could be
    // corrupted by that (every character `hex_char_to_nibble` accepts is
    // ASCII, so on valid input the two counts agree), but one of the two
    // checks had to be the real one, and it is this one — it counts what is
    // actually being consumed.
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    let mut chars = hex.chars();
    while let Some(high) = chars.next() {
        let Some(low) = chars.next() else {
            return Err(CredentialError::CryptoError {
                detail: "hex string has odd length".to_string(),
            });
        };
        let byte = hex_char_to_nibble(high)? << 4 | hex_char_to_nibble(low)?;
        bytes.push(byte);
    }
    Ok(bytes)
}

fn hex_char_to_nibble(c: char) -> Result<u8, CredentialError> {
    // `to_digit` is the same three ranges written once, by the standard
    // library, and it returns the value rather than an offset the caller has
    // to subtract — which is where the hand-written version's three separate
    // `- b'0'` / `- b'a' + 10` expressions each had to be got right.
    c.to_digit(16)
        .and_then(|value| u8::try_from(value).ok())
        .ok_or_else(|| CredentialError::CryptoError {
            detail: format!("invalid hex character: {c}"),
        })
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

/// Generate a random password with the specified character classes.
///
/// # This does not produce unguessable passwords
///
/// `seed` is supplied by the caller, and in practice that caller passes a
/// timestamp — so the output is a deterministic function of roughly twenty
/// bits of real entropy, and an attacker who knows the day a password was
/// generated can enumerate every password it could have been. The generator
/// itself is sound as a generator; it is a *pseudo*-random one, which is the
/// wrong tool for the one job on this page that needs unpredictability.
///
/// This cannot be fixed inside lane C: entropy comes from the kernel. See
/// `requests/c-a-userspace-entropy-syscall.md` and `known-issues.md` →
/// `C-THERE-IS-NO-RANDOMNESS-SOURCE-FOR-USERSPACE`. When that syscall lands,
/// `seed` goes away and this function reads from it directly.
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

    let mut rng = Rng::new(seed);
    (0..length)
        .map(|_| {
            // `choose` reduces by taking the high half of a widening
            // multiply, which is very nearly unbiased. The `% charset.len()`
            // this replaced skewed towards the front of the alphabet by about
            // one part in 2^58 — far too small to matter, and not why the
            // change was made. It was made because there is no reason for a
            // second private generator to exist when `randrange` is right
            // there, and every private copy is a place for the *serious*
            // version of this bug to reappear (see `randrange`'s module docs:
            // `% bound` on a power-of-two bound returns a short cycle).
            rng.choose(&charset).copied().unwrap_or('a')
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

    let class_count = [has_lower, has_upper, has_digit, has_symbol]
        .into_iter()
        .filter(|present| *present)
        .count();

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
    /// Verifier for the master password: `SHA-256(session key ‖ tag)`.
    ///
    /// Deliberately derived from the *stretched* key rather than from the
    /// password directly. A verifier that is a single hash of the password
    /// would let an attacker holding the vault test guesses at one SHA-256
    /// each and never touch the slow derivation at all — which would make the
    /// stretching in `derive_session_key` decorative. Checking a password now
    /// costs the same [`Self::kdf_rounds`] as using it.
    master_password_verifier: Option<[u8; 32]>,
    /// Iteration count this vault's verifier and session key were derived at.
    ///
    /// **A persistence layer must round-trip this**, for the same reason every
    /// password-hashing format stores its cost alongside its hash: when
    /// [`DEFAULT_KDF_ROUNDS`] is raised, a vault written under the old number
    /// still has to open, and it can only do that if it remembers what the old
    /// number was.
    kdf_rounds: u32,
    /// Next nonce to hand to [`encrypt`]. Only ever increases.
    ///
    /// **A persistence layer must round-trip this.** The one rule a stream
    /// cipher's nonce has is that it is never reused under a given key; a
    /// vault reloaded from disk with this reset to zero would re-issue nonces
    /// it has already used and reintroduce exactly the two-time pad this
    /// exists to prevent. There is no persistence layer yet, so today it
    /// cannot happen — this comment is here so it does not start happening.
    next_nonce: u64,
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
        Self::with_kdf_rounds(uid, DEFAULT_KDF_ROUNDS)
    }

    /// Create a store whose master password is stretched by `rounds`
    /// iterations rather than [`DEFAULT_KDF_ROUNDS`].
    ///
    /// This exists so a vault loaded from disk can be reconstructed at the
    /// cost it was written at, and so tests need not pay ~130 ms per unlock.
    /// It is **not** a knob for lowering the cost of a live vault: a smaller
    /// `rounds` is a proportionally cheaper offline guess at the user's master
    /// password, and nothing else.
    #[must_use]
    pub fn with_kdf_rounds(uid: u32, rounds: u32) -> Self {
        Self {
            credentials: HashMap::new(),
            next_id: 1,
            uid,
            master_password_verifier: None,
            kdf_rounds: rounds,
            next_nonce: 0,
            session_key: None,
            last_activity: current_timestamp(),
            lock_timeout_secs: DEFAULT_LOCK_TIMEOUT_SECS,
            failed_attempts: 0,
            lockout_until: 0,
        }
    }

    /// The iteration count this vault's master password is stretched by.
    #[must_use]
    pub fn kdf_rounds(&self) -> u32 {
        self.kdf_rounds
    }

    /// The stored master-password verifier, or `None` if no master password
    /// has been set.
    ///
    /// Public because [`IdentityVerifier::verify`] re-checks the master
    /// password without unlocking the store, and must check it against the
    /// same value and at the same cost that [`Self::unlock`] would.
    #[must_use]
    pub fn master_password_verifier(&self) -> Option<[u8; 32]> {
        self.master_password_verifier
    }

    /// Check whether the store is currently locked.
    pub fn is_locked(&self) -> bool {
        self.session_key.is_none()
    }

    /// Update the last activity timestamp (resets auto-lock timer).
    fn touch(&mut self) {
        self.last_activity = current_timestamp();
    }

    /// Take the next `count` nonces, advancing the counter past them.
    ///
    /// Returned as a base rather than one at a time so that a caller already
    /// holding a mutable borrow of `credentials` — re-encrypting the whole
    /// vault, say — can still get fresh nonces without a second borrow of
    /// `self`. Gaps are harmless: nonces must be unique, not contiguous.
    fn take_nonces(&mut self, count: u64) -> u64 {
        let base = self.next_nonce;
        self.next_nonce = self.next_nonce.saturating_add(count);
        base
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
        let new_key = derive_session_key(new_password, self.kdf_rounds);

        // If a master password is already set, verify the old one
        if let Some(existing) = self.master_password_verifier {
            let old_pw = old_password.ok_or(CredentialError::InvalidMasterPassword)?;
            let old_key = derive_session_key(old_pw, self.kdf_rounds);
            if !eq_constant_time(&verifier_for(&old_key), &existing) {
                return Err(CredentialError::InvalidMasterPassword);
            }

            // Re-encrypt all credentials with the new key. Fresh nonces are
            // taken even though the key has changed (so reuse would be
            // harmless): a counter that only ever moves forward is one fewer
            // invariant to reason about than one with exceptions.
            let count = u64::try_from(self.credentials.len()).unwrap_or(u64::MAX);
            let base = self.take_nonces(count);
            let mut failures = Vec::new();
            for (i, cred) in self.credentials.values_mut().enumerate() {
                let nonce = base.saturating_add(u64::try_from(i).unwrap_or(u64::MAX));
                match decrypt(&cred.encrypted_data, &old_key) {
                    Ok(plaintext) => cred.encrypted_data = encrypt(&plaintext, &new_key, nonce),
                    // Re-encryption is a batch: one unreadable record must not
                    // abandon the rest half-converted under two different keys.
                    // Convert what can be converted, then report.
                    Err(_) => failures.push(cred.id),
                }
            }
            if !failures.is_empty() {
                return Err(CredentialError::CryptoError {
                    detail: format!(
                        "{} credential(s) could not be re-encrypted and remain under the old key: {failures:?}",
                        failures.len()
                    ),
                });
            }
        }

        self.master_password_verifier = Some(verifier_for(&new_key));
        self.session_key = Some(new_key);
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

        let expected = self
            .master_password_verifier
            .ok_or(CredentialError::MasterPasswordNotSet)?;

        // Derive once and reuse: the derivation is deliberately expensive
        // (~130 ms), so doing it a second time to produce the session key
        // would double the cost of every legitimate unlock for no benefit.
        let key = derive_session_key(master_password, self.kdf_rounds);

        if !eq_constant_time(&verifier_for(&key), &expected) {
            self.failed_attempts = self.failed_attempts.saturating_add(1);
            if self.failed_attempts >= MAX_UNLOCK_ATTEMPTS {
                self.lockout_until = now.saturating_add(LOCKOUT_DURATION_SECS);
                self.failed_attempts = 0;
            }
            return Err(CredentialError::InvalidMasterPassword);
        }

        // Success — reset attempts and adopt the key we just derived.
        self.failed_attempts = 0;
        self.session_key = Some(key);
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
        let encrypted_data = encrypt(data, &key, self.take_nonces(1));
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
        let plaintext = decrypt(&cred.encrypted_data, &key)?;
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
                let plaintext = decrypt(&cred.encrypted_data, &key)?;
                let metadata = CredentialMetadata::from(&*cred);
                results.push((metadata, plaintext, priority));
            }
        }

        // Sort by priority (highest first)
        results.sort_by_key(|r| std::cmp::Reverse(r.2));

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

        // Reserve the nonce *before* borrowing the credential: `take_nonces`
        // needs `&mut self`, and `get_mut` below holds that borrow for the
        // rest of the function. Reserved only when there is new data to
        // encrypt — a nonce spent on nothing is a nonce that can never be
        // reused, but there is no reason to burn one.
        let nonce = if data.is_some() {
            Some(self.take_nonces(1))
        } else {
            None
        };

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
        if let Some((new_data, n)) = data.zip(nonce) {
            cred.encrypted_data = encrypt(new_data, &key, n);
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
                        .is_some_and(|u| u.to_ascii_lowercase().contains(&query_lower))
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

        matches.sort_by_key(|m| std::cmp::Reverse(m.1));
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
    /// Every level, weakest first.
    ///
    /// The ordering is load-bearing: "verifying at a level satisfies all lower
    /// levels" and "a check is satisfied by any verification at this level or
    /// higher" are both expressed by comparing against this order.
    pub const ALL: [Self; 4] = [Self::Low, Self::Medium, Self::High, Self::Critical];

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

/// One `T` per [`SensitivityLevel`].
///
/// This replaces a `[u64; 4]` indexed by `level as usize`. That array appeared
/// in two structs and was read or written at five call sites, each of which
/// wrote its own `if idx < arr.len()` guard and, when the guard failed,
/// silently returned zero or did nothing.
///
/// The guard can only fail if `SensitivityLevel` grows a fifth variant — and
/// that is exactly the case it handles wrongly. A new level would compile
/// cleanly and then, at five separate sites, quietly fail to record or
/// consult its own verification state: an operation classified at the new
/// level would be treated as never verified and, in `set_debounce`, would
/// silently discard the window the caller set. Here the mapping is a `match`
/// over the enum, so a fifth variant is a compile error in one place and
/// nowhere else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerLevel<T> {
    low: T,
    medium: T,
    high: T,
    critical: T,
}

impl<T> PerLevel<T> {
    /// The value stored for `level`. Total — there is no missing case.
    pub const fn get(&self, level: SensitivityLevel) -> &T {
        match level {
            SensitivityLevel::Low => &self.low,
            SensitivityLevel::Medium => &self.medium,
            SensitivityLevel::High => &self.high,
            SensitivityLevel::Critical => &self.critical,
        }
    }

    /// The value stored for `level`, mutably.
    pub const fn get_mut(&mut self, level: SensitivityLevel) -> &mut T {
        match level {
            SensitivityLevel::Low => &mut self.low,
            SensitivityLevel::Medium => &mut self.medium,
            SensitivityLevel::High => &mut self.high,
            SensitivityLevel::Critical => &mut self.critical,
        }
    }
}

impl<T: Copy> PerLevel<T> {
    /// The same value for every level.
    pub const fn splat(value: T) -> Self {
        Self {
            low: value,
            medium: value,
            high: value,
            critical: value,
        }
    }

    /// One value per level, computed from the level.
    pub fn from_fn(mut f: impl FnMut(SensitivityLevel) -> T) -> Self {
        Self {
            low: f(SensitivityLevel::Low),
            medium: f(SensitivityLevel::Medium),
            high: f(SensitivityLevel::High),
            critical: f(SensitivityLevel::Critical),
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
    pub debounce_secs: PerLevel<u64>,
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
            debounce_secs: PerLevel::from_fn(SensitivityLevel::default_debounce_secs),
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
        *self.debounce_secs.get(level)
    }
}

/// Tracks identity verification state with per-level debounce.
#[derive(Debug)]
pub struct IdentityVerifier {
    /// Configuration for verification behavior.
    config: VerificationConfig,
    /// Timestamp of last successful verification per sensitivity level.
    /// Verification at a higher level also counts for lower levels.
    last_verified: PerLevel<u64>,
    /// Consecutive failed attempts in the current session.
    failed_attempts: u32,
    /// Timestamp when lockout expires (0 = no lockout).
    lockout_until: u64,
    /// Total successful verifications (for audit/metrics).
    total_verifications: u64,
    /// Total failed verifications (for audit/metrics).
    total_failures: u64,
}

impl Default for IdentityVerifier {
    fn default() -> Self {
        Self::new()
    }
}

impl IdentityVerifier {
    /// Create a new verifier with default configuration.
    pub fn new() -> Self {
        Self::with_config(VerificationConfig::default())
    }

    /// Create a new verifier with custom configuration.
    pub fn with_config(config: VerificationConfig) -> Self {
        Self {
            config,
            last_verified: PerLevel::splat(0),
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

        // Check this level and all higher levels (a Critical verification
        // satisfies Medium and High checks too).
        for higher in SensitivityLevel::ALL.into_iter().filter(|l| *l >= level) {
            let last = *self.last_verified.get(higher);
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
        // Set the timestamp for this level and all lower levels.
        for lower in SensitivityLevel::ALL.into_iter().filter(|l| *l <= level) {
            *self.last_verified.get_mut(lower) = now;
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
    /// the store's verifier. On success, records the verification with
    /// debounce.
    ///
    /// `verifier` and `rounds` come from the store
    /// ([`CredentialStore::master_password_verifier`] and
    /// [`CredentialStore::kdf_rounds`]) — the same value `unlock` checks,
    /// derived the same way and at the same cost. This used to take a bare
    /// `SHA-256(password)`, which made re-verification a second, far cheaper
    /// oracle for the master password than the unlock path it was supposed to
    /// mirror.
    pub fn verify(
        &mut self,
        password: &str,
        verifier: &[u8; 32],
        rounds: u32,
        level: SensitivityLevel,
        now: u64,
    ) -> VerificationResult {
        // Check lockout first
        if now < self.lockout_until {
            return VerificationResult::LockedOut {
                retry_after_secs: self.lockout_until.saturating_sub(now),
            };
        }

        let attempt = verifier_for(&derive_session_key(password, rounds));
        if eq_constant_time(&attempt, verifier) {
            self.record_success(level, now);
            VerificationResult::Verified
        } else {
            self.record_failure(now)
        }
    }

    /// Clear all verification state (e.g., on store lock).
    pub fn clear(&mut self) {
        self.last_verified = PerLevel::splat(0);
        // Don't clear failed_attempts or lockout — those persist across locks
    }

    /// Get the debounce configuration.
    pub fn config(&self) -> &VerificationConfig {
        &self.config
    }

    /// Update the debounce window for a specific sensitivity level.
    pub fn set_debounce(&mut self, level: SensitivityLevel, seconds: u64) {
        *self.config.debounce_secs.get_mut(level) = seconds;
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
        match *self.last_verified.get(level) {
            0 => None,
            last => Some(now.saturating_sub(last)),
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

    println!("Slate OS Credential Manager v0.1.0");
    println!("Store initialized for uid={}", store.uid);
    println!("Storage path: {}", store.storage_path());
    println!("Auto-lock timeout: {}s", store.lock_timeout_secs);
    println!("Status: {}", if store.is_locked() { "locked" } else { "unlocked" });
    println!("\nCredential Manager service ready. Awaiting IPC requests...");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// The five defensive lints the workspace turns on are for production code: a
// test that indexes a fixed-size fixture, or unwraps a value it just
// constructed, is *asserting*, and an assertion that fails by panicking is a
// test doing its job rather than a robustness hole.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
#[cfg(test)]
mod tests {

    use super::*;

    /// Stretching cost for tests.
    ///
    /// [`DEFAULT_KDF_ROUNDS`] is deliberately expensive — ~130 ms per
    /// derivation in release, and roughly seven times that in the debug build
    /// tests run under. Several hundred derivations across this module at that
    /// price would put the suite in the minutes. The rounds parameter is not
    /// what these tests are checking, so they pay a token amount of it;
    /// `default_kdf_rounds_are_usable` below covers the real number once.
    const TEST_KDF_ROUNDS: u32 = 4;

    /// A store cheap enough to unlock in a test. See [`TEST_KDF_ROUNDS`].
    fn test_store() -> CredentialStore {
        CredentialStore::with_kdf_rounds(1000, TEST_KDF_ROUNDS)
    }

    /// A key derived at the test cost.
    fn test_key(password: &str) -> [u8; 32] {
        derive_session_key(password, TEST_KDF_ROUNDS)
    }

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
        let key = test_key("test_password");
        let plaintext = b"super secret credential data";
        let ciphertext = encrypt(plaintext, &key, 7);

        // Ciphertext should differ from plaintext
        assert_ne!(&ciphertext[..], &plaintext[..]);

        let decrypted = decrypt(&ciphertext, &key).expect("decrypt");
        assert_eq!(&decrypted[..], &plaintext[..]);
    }

    #[test]
    fn test_encrypt_decrypt_empty() {
        let key = test_key("key");
        let plaintext = b"";
        let ciphertext = encrypt(plaintext, &key, 0);
        // Not empty: an empty plaintext still carries its nonce.
        assert_eq!(ciphertext.len(), NONCE_LEN);
        let decrypted = decrypt(&ciphertext, &key).expect("decrypt");
        assert!(decrypted.is_empty());
    }

    #[test]
    fn test_wrong_key_fails_decrypt() {
        let key1 = test_key("password1");
        let key2 = test_key("password2");
        let plaintext = b"sensitive data";
        let ciphertext = encrypt(plaintext, &key1, 1);
        let wrong_decrypt = decrypt(&ciphertext, &key2).expect("decrypt");
        assert_ne!(&wrong_decrypt[..], &plaintext[..]);
    }

    #[test]
    fn the_same_plaintext_twice_does_not_produce_the_same_ciphertext() {
        // The two-time-pad regression. Before nonces existed, the keystream
        // was a function of the key alone, so every record in a vault was
        // XORed with identical material and XORing two ciphertexts together
        // cancelled the key outright. Two encryptions of *the same bytes*
        // under *the same key* differing is the cheapest observable proof
        // that no longer holds.
        let key = test_key("pw");
        let plaintext = b"the same message, twice";
        let first = encrypt(plaintext, &key, 0);
        let second = encrypt(plaintext, &key, 1);
        assert_ne!(first, second);

        // Specifically: the bodies differ, not merely the nonce prefixes.
        assert_ne!(first[NONCE_LEN..], second[NONCE_LEN..]);

        // And both still decrypt.
        assert_eq!(decrypt(&first, &key).expect("first"), plaintext);
        assert_eq!(decrypt(&second, &key).expect("second"), plaintext);
    }

    #[test]
    fn a_blob_too_short_to_hold_a_nonce_is_rejected_not_misread() {
        let key = test_key("pw");
        for len in 0..NONCE_LEN {
            let truncated = vec![0u8; len];
            assert!(
                decrypt(&truncated, &key).is_err(),
                "{len}-byte blob should be rejected"
            );
        }
        // Exactly NONCE_LEN bytes is the encryption of the empty plaintext.
        assert!(decrypt(&[0u8; NONCE_LEN], &key).is_ok());
    }

    #[test]
    fn stretching_more_rounds_gives_a_different_key() {
        // Guards against the rounds parameter being accepted and ignored —
        // which would silently reduce every vault to the cost of whatever was
        // hard-coded instead.
        assert_ne!(
            derive_session_key("pw", 1),
            derive_session_key("pw", 2),
            "the iteration count must actually reach the hash"
        );
    }

    #[test]
    fn the_verifier_is_not_the_key_it_verifies() {
        // The verifier is stored in the clear beside the ciphertext. If it
        // were the key, or trivially convertible to it, the vault would be
        // readable without the password at all.
        let key = test_key("pw");
        assert_ne!(verifier_for(&key), key);
    }

    #[test]
    fn default_kdf_rounds_are_usable() {
        // The rest of the module runs at TEST_KDF_ROUNDS, so this is the one
        // place the shipped cost is exercised end to end. It is also a crude
        // budget check: if this test ever becomes slow enough to notice, the
        // number is too high for an interactive unlock.
        let mut store = CredentialStore::new(1000);
        assert_eq!(store.kdf_rounds(), DEFAULT_KDF_ROUNDS);
        store.set_master_password(None, "real cost").expect("set pw");
        store.lock();
        store.unlock("real cost").expect("unlock");
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

    #[test]
    fn from_hex_rejects_multi_byte_characters_whatever_the_byte_count() {
        // `from_hex` used to length-check `hex.len()`, which counts bytes,
        // and then consume `hex.chars()`. These four inputs are the two
        // parities of character count crossed with the two parities of byte
        // count, which the old code could not tell apart. All four are
        // invalid hex and all four must be rejected.
        for input in ["é", "éa", "éé", "ééa"] {
            assert!(
                from_hex(input).is_err(),
                "{input:?} ({} chars, {} bytes) was accepted as hex",
                input.chars().count(),
                input.len()
            );
        }
    }

    // -- Store tests --

    #[test]
    fn test_store_locked_by_default_after_new() {
        let store = test_store();
        // No master password set yet, so session_key is None
        assert!(store.is_locked());
    }

    #[test]
    fn test_set_master_password_unlocks() {
        let mut store = test_store();
        store
            .set_master_password(None, "my_password")
            .expect("should succeed");
        assert!(!store.is_locked());
    }

    #[test]
    fn test_lock_and_unlock() {
        let mut store = test_store();
        store.set_master_password(None, "pw123").expect("set pw");
        assert!(!store.is_locked());

        store.lock();
        assert!(store.is_locked());

        store.unlock("pw123").expect("unlock");
        assert!(!store.is_locked());
    }

    #[test]
    fn test_unlock_wrong_password() {
        let mut store = test_store();
        store.set_master_password(None, "correct").expect("set pw");
        store.lock();

        let result = store.unlock("wrong");
        assert_eq!(result, Err(CredentialError::InvalidMasterPassword));
    }

    #[test]
    fn test_rate_limiting() {
        let mut store = test_store();
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
        let mut store = test_store();
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
        let mut store = test_store();
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
        let mut store = test_store();
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
        let mut store = test_store();
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
        // Only one record ("Work Email") matches "work" — search returns
        // distinct credentials, not distinct match reasons.  The Work Email
        // record happens to match three ways (name, tag, target), but it
        // still appears once in the result.
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Work Email");

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
        let mut store = test_store();
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
        let mut store = test_store();

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
        let mut store = test_store();
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
        let master_hash = verifier_for(&test_key("correct_password"));
        let result = verifier.verify("correct_password", &master_hash, TEST_KDF_ROUNDS, SensitivityLevel::Medium, 1000);
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn test_verifier_password_verification_failure() {
        let mut verifier = IdentityVerifier::new();
        let master_hash = verifier_for(&test_key("correct_password"));
        let result = verifier.verify("wrong_password", &master_hash, TEST_KDF_ROUNDS, SensitivityLevel::Medium, 1000);
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn test_verifier_lockout_after_max_attempts() {
        let mut verifier = IdentityVerifier::new();
        let master_hash = verifier_for(&test_key("correct_password"));
        // 3 failed attempts
        verifier.verify("wrong1", &master_hash, TEST_KDF_ROUNDS, SensitivityLevel::Medium, 1000);
        verifier.verify("wrong2", &master_hash, TEST_KDF_ROUNDS, SensitivityLevel::Medium, 1001);
        let result = verifier.verify("wrong3", &master_hash, TEST_KDF_ROUNDS, SensitivityLevel::Medium, 1002);
        assert!(matches!(result, VerificationResult::LockedOut { .. }));
    }

    #[test]
    fn test_verifier_lockout_blocks_check() {
        let mut verifier = IdentityVerifier::new();
        let master_hash = verifier_for(&test_key("correct_password"));
        // Trigger lockout
        verifier.verify("wrong1", &master_hash, TEST_KDF_ROUNDS, SensitivityLevel::Medium, 1000);
        verifier.verify("wrong2", &master_hash, TEST_KDF_ROUNDS, SensitivityLevel::Medium, 1001);
        verifier.verify("wrong3", &master_hash, TEST_KDF_ROUNDS, SensitivityLevel::Medium, 1002);
        // Even check() should report lockout
        let result = verifier.check(SensitivityLevel::Medium, 1005);
        assert!(matches!(result, VerificationResult::LockedOut { .. }));
    }

    #[test]
    fn test_verifier_lockout_expires() {
        let mut verifier = IdentityVerifier::new();
        let master_hash = verifier_for(&test_key("correct_password"));
        // Trigger lockout (default 30s)
        verifier.verify("wrong1", &master_hash, TEST_KDF_ROUNDS, SensitivityLevel::Medium, 1000);
        verifier.verify("wrong2", &master_hash, TEST_KDF_ROUNDS, SensitivityLevel::Medium, 1001);
        verifier.verify("wrong3", &master_hash, TEST_KDF_ROUNDS, SensitivityLevel::Medium, 1002);
        // After lockout expires (30s), should be able to verify again
        let result = verifier.verify("correct_password", &master_hash, TEST_KDF_ROUNDS, SensitivityLevel::Medium, 1035);
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn test_verifier_success_resets_failed_count() {
        let mut verifier = IdentityVerifier::new();
        let master_hash = verifier_for(&test_key("correct_password"));
        // 2 failed attempts
        verifier.verify("wrong1", &master_hash, TEST_KDF_ROUNDS, SensitivityLevel::Medium, 1000);
        verifier.verify("wrong2", &master_hash, TEST_KDF_ROUNDS, SensitivityLevel::Medium, 1001);
        // Success resets counter
        verifier.verify("correct_password", &master_hash, TEST_KDF_ROUNDS, SensitivityLevel::Medium, 1002);
        // Another failure should not trigger lockout (counter was reset)
        let result = verifier.verify("wrong3", &master_hash, TEST_KDF_ROUNDS, SensitivityLevel::Medium, 1003);
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn test_verifier_disabled_bypasses_all() {
        let config = VerificationConfig {
            enabled: false,
            ..VerificationConfig::default()
        };
        let verifier = IdentityVerifier::with_config(config);
        // Even Critical should pass when disabled
        let result = verifier.check(SensitivityLevel::Critical, 1000);
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn test_verifier_medium_requirement_can_be_disabled() {
        let config = VerificationConfig {
            require_for_medium: false,
            ..VerificationConfig::default()
        };
        let verifier = IdentityVerifier::with_config(config);
        let result = verifier.check(SensitivityLevel::Medium, 1000);
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn test_verifier_high_requirement_can_be_disabled() {
        let config = VerificationConfig {
            require_for_high: false,
            ..VerificationConfig::default()
        };
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
        let master_hash = verifier_for(&test_key("correct_password"));
        verifier.verify("correct_password", &master_hash, TEST_KDF_ROUNDS, SensitivityLevel::Medium, 1000);
        verifier.verify("wrong", &master_hash, TEST_KDF_ROUNDS, SensitivityLevel::Medium, 1001);
        verifier.verify("correct_password", &master_hash, TEST_KDF_ROUNDS, SensitivityLevel::Medium, 1002);
        let (successes, failures) = verifier.stats();
        assert_eq!(successes, 2);
        assert_eq!(failures, 1);
    }

    #[test]
    fn test_verifier_lockout_remaining() {
        let mut verifier = IdentityVerifier::new();
        let master_hash = verifier_for(&test_key("correct_password"));
        verifier.verify("wrong1", &master_hash, TEST_KDF_ROUNDS, SensitivityLevel::Medium, 1000);
        verifier.verify("wrong2", &master_hash, TEST_KDF_ROUNDS, SensitivityLevel::Medium, 1001);
        verifier.verify("wrong3", &master_hash, TEST_KDF_ROUNDS, SensitivityLevel::Medium, 1002);
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

    #[test]
    fn per_level_gives_each_level_its_own_slot() {
        // The point of the newtype is that no two levels alias, and that
        // `ALL` really does list every one of them — if a fifth variant is
        // added and left out of `ALL`, this fails rather than silently
        // dropping that level's state.
        let mut values = PerLevel::splat(0_u64);
        for (n, level) in SensitivityLevel::ALL.into_iter().enumerate() {
            *values.get_mut(level) = n as u64 + 1;
        }
        for (n, level) in SensitivityLevel::ALL.into_iter().enumerate() {
            assert_eq!(*values.get(level), n as u64 + 1, "{level:?} aliases");
        }
        assert_eq!(values, PerLevel::from_fn(|l| *values.get(l)));
    }

    #[test]
    fn setting_a_debounce_does_not_disturb_the_other_levels() {
        // The old `set_debounce` indexed `[u64; 4]` behind a bounds check and
        // silently did nothing if the check failed. Pin the whole mapping so
        // a write to one level is visible at that level and nowhere else.
        let mut verifier = IdentityVerifier::new();
        verifier.set_debounce(SensitivityLevel::High, 999);
        assert_eq!(verifier.config().debounce_for(SensitivityLevel::High), 999);
        for level in SensitivityLevel::ALL {
            if level != SensitivityLevel::High {
                assert_eq!(
                    verifier.config().debounce_for(level),
                    level.default_debounce_secs(),
                    "{level:?} changed"
                );
            }
        }
    }
}
