//! Credential store — secure per-application password and secret storage.
//!
//! Provides an OS-level credential vault for applications to store
//! and retrieve usernames, passwords, API keys, and tokens.  Similar
//! to Windows Credential Manager or macOS Keychain.
//!
//! ## Design Reference
//!
//! design.txt line 758: "api feature to define that something is a
//! username or password field - so the OS can autopopulate it with the
//! user's username and password for that program"
//!
//! design.txt line 759: "api call to verify a user's identity by
//! having them type in their OS user password - with optional debouncer"
//!
//! ## Architecture
//!
//! ```text
//! Application (via capability)
//!   → credentials::store(app_id, service, credential)
//!   → credentials::retrieve(app_id, service)
//!
//! Login dialog / autofill engine
//!   → credentials::lookup_autofill(app_id, field_type)
//!
//! Identity verification
//!   → credentials::verify_identity(user_id)
//!   → credentials::check_debounce(user_id) — skips if recent
//! ```
//!
//! ## Security
//!
//! - Credentials are scoped per-app: an app can only access its own.
//! - A "master unlock" is required before reading secrets (debounced).
//! - Secrets are stored in memory (would be encrypted at rest on disk).

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum stored credentials.
const MAX_CREDENTIALS: usize = 4096;

/// Maximum credentials per application.
const MAX_PER_APP: usize = 256;

/// Maximum autofill rules.
const MAX_AUTOFILL: usize = 1024;

/// Identity verification debounce window (nanoseconds = 5 minutes).
const VERIFY_DEBOUNCE_NS: u64 = 5 * 60 * 1_000_000_000;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Type of credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CredentialKind {
    /// Username + password pair.
    Password,
    /// API key or token.
    ApiKey,
    /// OAuth/bearer token (may have expiry).
    Token,
    /// SSH key reference.
    SshKey,
    /// Certificate reference.
    Certificate,
    /// Generic secret.
    Generic,
}

impl CredentialKind {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Password => "Password",
            Self::ApiKey => "API Key",
            Self::Token => "Token",
            Self::SshKey => "SSH Key",
            Self::Certificate => "Certificate",
            Self::Generic => "Generic",
        }
    }

    /// Parse from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "password" | "pass" => Some(Self::Password),
            "apikey" | "api" | "key" => Some(Self::ApiKey),
            "token" | "oauth" | "bearer" => Some(Self::Token),
            "ssh" | "sshkey" => Some(Self::SshKey),
            "cert" | "certificate" => Some(Self::Certificate),
            "generic" | "secret" => Some(Self::Generic),
            _ => None,
        }
    }
}

/// A stored credential.
#[derive(Debug, Clone)]
pub struct Credential {
    /// Owning application ID.
    pub app_id: String,
    /// Service/site identifier (e.g., "github.com", "smtp.gmail.com").
    pub service: String,
    /// Username (if applicable).
    pub username: String,
    /// The secret value (password, token, key, etc.).
    /// In a real implementation this would be encrypted.
    pub secret: String,
    /// Kind of credential.
    pub kind: CredentialKind,
    /// When this credential was stored (nanoseconds).
    pub created_ns: u64,
    /// When it was last accessed (nanoseconds).
    pub accessed_ns: u64,
    /// When it was last modified (nanoseconds).
    pub modified_ns: u64,
    /// Optional expiry timestamp (0 = no expiry).
    pub expires_ns: u64,
    /// Human-readable label / note.
    pub label: String,
}

/// An autofill rule: maps a field type in an app to a credential.
#[derive(Debug, Clone)]
pub struct AutofillRule {
    /// Application ID this rule applies to.
    pub app_id: String,
    /// Field type (e.g., "username", "password", "email").
    pub field_type: String,
    /// Service key to look up the credential.
    pub service: String,
    /// Whether to autofill automatically or prompt user.
    pub auto: bool,
}

/// Summary of credentials for listing (without exposing secrets).
#[derive(Debug, Clone)]
pub struct CredentialSummary {
    /// Application ID.
    pub app_id: String,
    /// Service identifier.
    pub service: String,
    /// Username (if any).
    pub username: String,
    /// Kind.
    pub kind: CredentialKind,
    /// Label.
    pub label: String,
    /// When created.
    pub created_ns: u64,
    /// Whether expired.
    pub expired: bool,
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

/// Composite key: (app_id, service).
type CredKey = (String, String);

struct CredentialStore {
    /// (app_id, service) → Credential.
    credentials: BTreeMap<CredKey, Credential>,
    /// Autofill rules.
    autofill: Vec<AutofillRule>,
    /// Last identity verification per user_id (nanoseconds).
    verify_times: BTreeMap<String, u64>,
    /// Whether the store is unlocked.
    unlocked: bool,
}

impl CredentialStore {
    const fn new() -> Self {
        Self {
            credentials: BTreeMap::new(),
            autofill: Vec::new(),
            verify_times: BTreeMap::new(),
            unlocked: false,
        }
    }

    fn count_for_app(&self, app_id: &str) -> usize {
        self.credentials.keys()
            .filter(|(a, _)| a == app_id)
            .count()
    }
}

static STORE: Mutex<CredentialStore> = Mutex::new(CredentialStore::new());
static STORE_OPS: AtomicU64 = AtomicU64::new(0);
static RETRIEVE_OPS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Lock/unlock
// ---------------------------------------------------------------------------

/// Unlock the credential store (requires user authentication).
/// In a real system this would verify the user's master password.
pub fn unlock() {
    STORE.lock().unlocked = true;
}

/// Lock the credential store.
pub fn lock() {
    STORE.lock().unlocked = false;
}

/// Check if the store is unlocked.
pub fn is_unlocked() -> bool {
    STORE.lock().unlocked
}

// ---------------------------------------------------------------------------
// Store / retrieve
// ---------------------------------------------------------------------------

/// Store a credential. Overwrites existing for same (app_id, service).
pub fn store(app_id: &str, service: &str, username: &str,
             secret: &str, kind: CredentialKind) -> KernelResult<()> {
    if app_id.is_empty() || service.is_empty() || secret.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    STORE_OPS.fetch_add(1, Ordering::Relaxed);
    let now = crate::timekeeping::clock_monotonic();
    let mut state = STORE.lock();

    let key = (String::from(app_id), String::from(service));

    // Check limits only if this is a new entry.
    if !state.credentials.contains_key(&key) {
        if state.credentials.len() >= MAX_CREDENTIALS {
            return Err(KernelError::ResourceExhausted);
        }
        if state.count_for_app(app_id) >= MAX_PER_APP {
            return Err(KernelError::ResourceExhausted);
        }
    }

    state.credentials.insert(key, Credential {
        app_id: String::from(app_id),
        service: String::from(service),
        username: String::from(username),
        secret: String::from(secret),
        kind,
        created_ns: now,
        accessed_ns: now,
        modified_ns: now,
        expires_ns: 0,
        label: String::new(),
    });

    Ok(())
}

/// Store with full options.
pub fn store_full(app_id: &str, service: &str, username: &str,
                  secret: &str, kind: CredentialKind,
                  label: &str, expires_ns: u64) -> KernelResult<()> {
    store(app_id, service, username, secret, kind)?;
    let mut state = STORE.lock();
    let key = (String::from(app_id), String::from(service));
    if let Some(cred) = state.credentials.get_mut(&key) {
        cred.label = String::from(label);
        cred.expires_ns = expires_ns;
    }
    Ok(())
}

/// Retrieve a credential. Requires store to be unlocked.
/// The app can only retrieve its own credentials.
pub fn retrieve(app_id: &str, service: &str) -> KernelResult<Credential> {
    RETRIEVE_OPS.fetch_add(1, Ordering::Relaxed);
    let now = crate::timekeeping::clock_monotonic();
    let mut state = STORE.lock();
    if !state.unlocked {
        return Err(KernelError::PermissionDenied);
    }
    let key = (String::from(app_id), String::from(service));
    let cred = state.credentials.get_mut(&key)
        .ok_or(KernelError::NotFound)?;

    // Check expiry.
    if cred.expires_ns > 0 && now > cred.expires_ns {
        return Err(KernelError::TimedOut);
    }

    cred.accessed_ns = now;
    Ok(cred.clone())
}

/// Delete a credential.
pub fn delete(app_id: &str, service: &str) -> KernelResult<()> {
    let mut state = STORE.lock();
    let key = (String::from(app_id), String::from(service));
    state.credentials.remove(&key).ok_or(KernelError::NotFound)?;
    Ok(())
}

/// Delete all credentials for an application.
pub fn delete_app(app_id: &str) -> usize {
    let mut state = STORE.lock();
    let keys_to_remove: Vec<CredKey> = state.credentials.keys()
        .filter(|(a, _)| a == app_id)
        .cloned()
        .collect();
    let count = keys_to_remove.len();
    for key in &keys_to_remove {
        state.credentials.remove(key);
    }
    count
}

/// List credentials for an app (summaries, no secrets).
pub fn list_for_app(app_id: &str) -> Vec<CredentialSummary> {
    let now = crate::timekeeping::clock_monotonic();
    let state = STORE.lock();
    state.credentials.values()
        .filter(|c| c.app_id == app_id)
        .map(|c| CredentialSummary {
            app_id: c.app_id.clone(),
            service: c.service.clone(),
            username: c.username.clone(),
            kind: c.kind,
            label: c.label.clone(),
            created_ns: c.created_ns,
            expired: c.expires_ns > 0 && now > c.expires_ns,
        })
        .collect()
}

/// List all credentials (summaries, no secrets).
pub fn list_all() -> Vec<CredentialSummary> {
    let now = crate::timekeeping::clock_monotonic();
    let state = STORE.lock();
    state.credentials.values()
        .map(|c| CredentialSummary {
            app_id: c.app_id.clone(),
            service: c.service.clone(),
            username: c.username.clone(),
            kind: c.kind,
            label: c.label.clone(),
            created_ns: c.created_ns,
            expired: c.expires_ns > 0 && now > c.expires_ns,
        })
        .collect()
}

/// Update username on an existing credential.
pub fn update_username(app_id: &str, service: &str, username: &str) -> KernelResult<()> {
    let now = crate::timekeeping::clock_monotonic();
    let mut state = STORE.lock();
    let key = (String::from(app_id), String::from(service));
    let cred = state.credentials.get_mut(&key)
        .ok_or(KernelError::NotFound)?;
    cred.username = String::from(username);
    cred.modified_ns = now;
    Ok(())
}

/// Update secret on an existing credential.
pub fn update_secret(app_id: &str, service: &str, secret: &str) -> KernelResult<()> {
    if secret.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let now = crate::timekeeping::clock_monotonic();
    let mut state = STORE.lock();
    let key = (String::from(app_id), String::from(service));
    let cred = state.credentials.get_mut(&key)
        .ok_or(KernelError::NotFound)?;
    cred.secret = String::from(secret);
    cred.modified_ns = now;
    Ok(())
}

/// Set label on a credential.
pub fn set_label(app_id: &str, service: &str, label: &str) -> KernelResult<()> {
    let mut state = STORE.lock();
    let key = (String::from(app_id), String::from(service));
    let cred = state.credentials.get_mut(&key)
        .ok_or(KernelError::NotFound)?;
    cred.label = String::from(label);
    Ok(())
}

// ---------------------------------------------------------------------------
// Autofill
// ---------------------------------------------------------------------------

/// Add an autofill rule.
pub fn add_autofill(app_id: &str, field_type: &str,
                    service: &str, auto: bool) -> KernelResult<()> {
    if app_id.is_empty() || field_type.is_empty() || service.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = STORE.lock();
    if state.autofill.len() >= MAX_AUTOFILL {
        return Err(KernelError::ResourceExhausted);
    }
    // Check for duplicates.
    let exists = state.autofill.iter().any(|r|
        r.app_id == app_id && r.field_type == field_type && r.service == service);
    if exists {
        return Err(KernelError::AlreadyExists);
    }
    state.autofill.push(AutofillRule {
        app_id: String::from(app_id),
        field_type: String::from(field_type),
        service: String::from(service),
        auto,
    });
    Ok(())
}

/// Remove an autofill rule.
pub fn remove_autofill(app_id: &str, field_type: &str) -> KernelResult<()> {
    let mut state = STORE.lock();
    let before = state.autofill.len();
    state.autofill.retain(|r| !(r.app_id == app_id && r.field_type == field_type));
    if state.autofill.len() == before {
        return Err(KernelError::NotFound);
    }
    Ok(())
}

/// Look up autofill value for a field. Returns (username, secret) if found.
pub fn lookup_autofill(app_id: &str, field_type: &str) -> KernelResult<(String, String)> {
    let state = STORE.lock();
    if !state.unlocked {
        return Err(KernelError::PermissionDenied);
    }
    let rule = state.autofill.iter()
        .find(|r| r.app_id == app_id && r.field_type == field_type)
        .ok_or(KernelError::NotFound)?;
    let key = (String::from(app_id), rule.service.clone());
    let cred = state.credentials.get(&key)
        .ok_or(KernelError::NotFound)?;
    Ok((cred.username.clone(), cred.secret.clone()))
}

/// List autofill rules for an app.
pub fn list_autofill(app_id: &str) -> Vec<AutofillRule> {
    STORE.lock().autofill.iter()
        .filter(|r| r.app_id == app_id)
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------
// Identity verification (design.txt line 759)
// ---------------------------------------------------------------------------

/// Verify user identity. Returns true if debounce is still active
/// (user verified recently), false if re-verification is needed.
///
/// In a real system, if this returns false, the UI would prompt
/// for the user's OS password and then call `confirm_identity`.
pub fn check_debounce(user_id: &str) -> bool {
    let now = crate::timekeeping::clock_monotonic();
    let state = STORE.lock();
    if let Some(&last) = state.verify_times.get(user_id) {
        now.saturating_sub(last) < VERIFY_DEBOUNCE_NS
    } else {
        false
    }
}

/// Confirm identity verification (called after user enters password).
pub fn confirm_identity(user_id: &str) {
    let now = crate::timekeeping::clock_monotonic();
    STORE.lock().verify_times.insert(String::from(user_id), now);
}

/// Get time since last verification for a user (nanoseconds, 0 if never).
pub fn time_since_verify(user_id: &str) -> u64 {
    let now = crate::timekeeping::clock_monotonic();
    let state = STORE.lock();
    state.verify_times.get(user_id)
        .map(|&t| now.saturating_sub(t))
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

/// Search credentials by service name (substring, case-insensitive).
pub fn search(query: &str) -> Vec<CredentialSummary> {
    let now = crate::timekeeping::clock_monotonic();
    let query_lower = query.to_ascii_lowercase();
    let state = STORE.lock();
    state.credentials.values()
        .filter(|c| {
            c.service.to_ascii_lowercase().contains(&query_lower)
                || c.username.to_ascii_lowercase().contains(&query_lower)
                || c.label.to_ascii_lowercase().contains(&query_lower)
        })
        .map(|c| CredentialSummary {
            app_id: c.app_id.clone(),
            service: c.service.clone(),
            username: c.username.clone(),
            kind: c.kind,
            label: c.label.clone(),
            created_ns: c.created_ns,
            expired: c.expires_ns > 0 && now > c.expires_ns,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (credential_count, autofill_count, store_ops, retrieve_ops).
pub fn stats() -> (usize, usize, u64, u64) {
    let state = STORE.lock();
    (
        state.credentials.len(),
        state.autofill.len(),
        STORE_OPS.load(Ordering::Relaxed),
        RETRIEVE_OPS.load(Ordering::Relaxed),
    )
}

/// Reset counters.
pub fn reset_stats() {
    STORE_OPS.store(0, Ordering::Relaxed);
    RETRIEVE_OPS.store(0, Ordering::Relaxed);
}

/// Clear all data.
pub fn clear_all() {
    let mut state = STORE.lock();
    state.credentials.clear();
    state.autofill.clear();
    state.verify_times.clear();
    state.unlocked = false;
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the credential store.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();
    reset_stats();

    // Test 1: Store and retrieve.
    serial_println!("  credentials::test 1: store and retrieve");
    store("browser", "github.com", "user@example.com", "s3cret!", CredentialKind::Password)?;
    // Must unlock first.
    assert!(retrieve("browser", "github.com").is_err()); // Locked.
    unlock();
    let cred = retrieve("browser", "github.com")?;
    assert_eq!(cred.username, "user@example.com");
    assert_eq!(cred.secret, "s3cret!");
    assert_eq!(cred.kind, CredentialKind::Password);

    // Test 2: App isolation — list only own credentials.
    serial_println!("  credentials::test 2: per-app isolation");
    store("email", "smtp.gmail.com", "me@gmail.com", "mailpass", CredentialKind::Password)?;
    store("browser", "gitlab.com", "dev", "gl_tok_123", CredentialKind::Token)?;
    let browser_creds = list_for_app("browser");
    assert_eq!(browser_creds.len(), 2);
    let email_creds = list_for_app("email");
    assert_eq!(email_creds.len(), 1);

    // Test 3: Update and delete.
    serial_println!("  credentials::test 3: update and delete");
    update_secret("browser", "github.com", "new_password!")?;
    let updated = retrieve("browser", "github.com")?;
    assert_eq!(updated.secret, "new_password!");
    delete("browser", "github.com")?;
    assert!(retrieve("browser", "github.com").is_err());

    // Test 4: Autofill.
    serial_println!("  credentials::test 4: autofill");
    add_autofill("browser", "username", "gitlab.com", true)?;
    add_autofill("browser", "password", "gitlab.com", true)?;
    let (user, _secret) = lookup_autofill("browser", "username")?;
    assert_eq!(user, "dev");
    let rules = list_autofill("browser");
    assert_eq!(rules.len(), 2);
    remove_autofill("browser", "username")?;
    assert_eq!(list_autofill("browser").len(), 1);

    // Test 5: Identity verification with debounce.
    serial_println!("  credentials::test 5: identity debounce");
    assert!(!check_debounce("user1"));
    confirm_identity("user1");
    assert!(check_debounce("user1")); // Just verified, debounce active.

    // Test 6: Search.
    serial_println!("  credentials::test 6: search");
    let results = search("gmail");
    assert_eq!(results.len(), 1);
    assert_eq!(results.first().map(|r| r.service.as_str()), Some("smtp.gmail.com"));

    // Test 7: Bulk operations.
    serial_println!("  credentials::test 7: bulk ops");
    let count = delete_app("email");
    assert_eq!(count, 1);
    assert!(list_for_app("email").is_empty());
    lock();
    assert!(!is_unlocked());
    assert!(retrieve("browser", "gitlab.com").is_err()); // Locked again.

    // Cleanup.
    clear_all();
    reset_stats();

    serial_println!("  credentials: all tests passed");
    Ok(())
}
