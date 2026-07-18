//! Auth Broker — credential and authentication management.
//!
//! Implements a Plan 9 Factotum-inspired authentication broker.
//! Programs never touch passwords or keys directly; they request
//! capabilities through the broker, which handles credential
//! storage, verification, and capability granting.
//!
//! ## Architecture
//!
//! ```text
//! Authentication broker
//!   → authbroker::authenticate(principal, method) → verify identity
//!   → authbroker::store_credential(principal, cred) → store credential
//!   → authbroker::request_capability(principal, resource) → grant cap
//!   → authbroker::revoke(principal) → revoke credentials
//!
//! Integration:
//!   → secpolicy (security policy)
//!   → credentials (credential store)
//!   → acl (access control)
//!   → loginscreen (login screen)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Authentication method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    Password,
    Token,
    Certificate,
    Biometric,
    Kerberos,
    PublicKey,
}

impl AuthMethod {
    pub fn label(self) -> &'static str {
        match self {
            Self::Password => "password",
            Self::Token => "token",
            Self::Certificate => "certificate",
            Self::Biometric => "biometric",
            Self::Kerberos => "kerberos",
            Self::PublicKey => "pubkey",
        }
    }
}

/// Authentication result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthResult {
    Granted,
    Denied,
    Expired,
    Locked,
    NeedsSecondFactor,
}

impl AuthResult {
    pub fn label(self) -> &'static str {
        match self {
            Self::Granted => "granted",
            Self::Denied => "denied",
            Self::Expired => "expired",
            Self::Locked => "locked",
            Self::NeedsSecondFactor => "2fa_required",
        }
    }
}

/// A stored credential.
#[derive(Debug, Clone)]
pub struct Credential {
    pub id: u32,
    pub principal: String,
    pub method: AuthMethod,
    pub hash: String,       // Credential hash (never store plaintext).
    pub created_ns: u64,
    pub expires_ns: u64,    // 0 = never.
    pub locked: bool,
    pub failed_attempts: u32,
    pub max_failures: u32,  // Auto-lock after N failures (0 = unlimited).
}

/// A capability grant record.
#[derive(Debug, Clone)]
pub struct CapGrant {
    pub id: u32,
    pub principal: String,
    pub resource: String,
    pub granted_ns: u64,
    pub expires_ns: u64,
    pub revoked: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_CREDENTIALS: usize = 512;
const MAX_GRANTS: usize = 2048;

struct State {
    credentials: Vec<Credential>,
    grants: Vec<CapGrant>,
    next_cred_id: u32,
    next_grant_id: u32,
    total_auth_attempts: u64,
    total_granted: u64,
    total_denied: u64,
    total_revoked: u64,
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialise the auth-broker state.
///
/// Starts with NO stored credentials and NO capability grants (all auth/grant/
/// deny/revoke totals zero). A credential is added through [`store_credential`]
/// when an account is actually provisioned, and a capability grant through
/// [`grant_capability`]; the per-principal counters advance only through real
/// [`authenticate`] / [`record_failure`] / [`revoke_grant`] calls. The
/// `/proc/authbroker` generator and the `authbroker` kshell command surface the
/// credential list (and [`list_credentials`] / [`list_grants`] / [`stats`]) as
/// if it reflects the real credential store, so seeding it with phantom accounts
/// would be fabricated procfs data — and uniquely dangerous on a security
/// surface, because it would claim the OS holds verified credentials for
/// privileged principals (root, admin, a service account) that nobody ever
/// provisioned. A real credential store is legitimate, but it must be populated
/// by actual account provisioning through [`store_credential`], not invented
/// here.
///
/// (Previously this seeded three fictional credentials — "root" (Password, a
/// placeholder `$argon2id$…` hash, lock after 5 failures), "admin" (PublicKey,
/// a placeholder `ssh-ed25519 AAAAC3…` key, lock after 10) and "service_acct"
/// (Token, a placeholder `bearer:hashed_token_abc123`, 24h expiry) — none
/// backed by a real provisioned account or verified secret.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        credentials: Vec::new(),
        grants: Vec::new(),
        next_cred_id: 1,
        next_grant_id: 1,
        total_auth_attempts: 0,
        total_granted: 0,
        total_denied: 0,
        total_revoked: 0,
        ops: 0,
    });
}

/// Authenticate a principal.
pub fn authenticate(principal: &str, method: AuthMethod) -> KernelResult<AuthResult> {
    with_state(|state| {
        state.total_auth_attempts += 1;
        let cred = state.credentials.iter_mut()
            .find(|c| c.principal == principal && c.method == method);
        let cred = match cred {
            Some(c) => c,
            None => { state.total_denied += 1; return Ok(AuthResult::Denied); }
        };
        if cred.locked {
            state.total_denied += 1;
            return Ok(AuthResult::Locked);
        }
        let now = crate::hpet::elapsed_ns();
        if cred.expires_ns > 0 && now > cred.expires_ns {
            state.total_denied += 1;
            return Ok(AuthResult::Expired);
        }
        // Simulated auth: always succeeds if credential exists and is valid.
        cred.failed_attempts = 0;
        state.total_granted += 1;
        Ok(AuthResult::Granted)
    })
}

/// Record a failed authentication attempt.
pub fn record_failure(principal: &str, method: AuthMethod) -> KernelResult<()> {
    with_state(|state| {
        let cred = state.credentials.iter_mut()
            .find(|c| c.principal == principal && c.method == method)
            .ok_or(KernelError::NotFound)?;
        cred.failed_attempts += 1;
        if cred.max_failures > 0 && cred.failed_attempts >= cred.max_failures {
            cred.locked = true;
        }
        state.total_denied += 1;
        Ok(())
    })
}

/// Store a new credential.
pub fn store_credential(principal: &str, method: AuthMethod, hash: &str) -> KernelResult<u32> {
    with_state(|state| {
        if state.credentials.len() >= MAX_CREDENTIALS { return Err(KernelError::ResourceExhausted); }
        let now = crate::hpet::elapsed_ns();
        let id = state.next_cred_id;
        state.next_cred_id += 1;
        state.credentials.push(Credential {
            id, principal: String::from(principal), method,
            hash: String::from(hash), created_ns: now, expires_ns: 0,
            locked: false, failed_attempts: 0, max_failures: 5,
        });
        Ok(id)
    })
}

/// Remove a credential.
pub fn remove_credential(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.credentials.len();
        state.credentials.retain(|c| c.id != id);
        if state.credentials.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Unlock a locked credential.
pub fn unlock(principal: &str) -> KernelResult<()> {
    with_state(|state| {
        let cred = state.credentials.iter_mut()
            .find(|c| c.principal == principal)
            .ok_or(KernelError::NotFound)?;
        cred.locked = false;
        cred.failed_attempts = 0;
        Ok(())
    })
}

/// Grant a capability to a principal for a resource.
pub fn grant_capability(principal: &str, resource: &str, ttl_ns: u64) -> KernelResult<u32> {
    with_state(|state| {
        if state.grants.len() >= MAX_GRANTS { return Err(KernelError::ResourceExhausted); }
        let now = crate::hpet::elapsed_ns();
        let id = state.next_grant_id;
        state.next_grant_id += 1;
        let expires = if ttl_ns > 0 { now + ttl_ns } else { 0 };
        state.grants.push(CapGrant {
            id, principal: String::from(principal), resource: String::from(resource),
            granted_ns: now, expires_ns: expires, revoked: false,
        });
        Ok(id)
    })
}

/// Revoke a capability grant.
pub fn revoke_grant(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let g = state.grants.iter_mut().find(|g| g.id == id)
            .ok_or(KernelError::NotFound)?;
        if g.revoked { return Err(KernelError::AlreadyExists); }
        g.revoked = true;
        state.total_revoked += 1;
        Ok(())
    })
}

/// List credentials for a principal (or all).
pub fn list_credentials(principal: Option<&str>) -> Vec<Credential> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        match principal {
            Some(p) => s.credentials.iter().filter(|c| c.principal == p).cloned().collect(),
            None => s.credentials.clone(),
        }
    })
}

/// List active grants for a principal.
pub fn list_grants(principal: Option<&str>) -> Vec<CapGrant> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        match principal {
            Some(p) => s.grants.iter().filter(|g| g.principal == p && !g.revoked).cloned().collect(),
            None => s.grants.iter().filter(|g| !g.revoked).cloned().collect(),
        }
    })
}

/// Statistics: (cred_count, grant_count, auth_attempts, granted, denied, revoked, ops).
pub fn stats() -> (usize, usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (
            s.credentials.len(), s.grants.len(),
            s.total_auth_attempts, s.total_granted, s.total_denied, s.total_revoked, s.ops
        ),
        None => (0, 0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("authbroker::self_test() — running tests...");
    // Start from a clean, empty state so the assertions below are exact and no
    // fixtures leak into the live credential store afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no phantom credentials or grants, zero totals.
    assert_eq!(list_credentials(None).len(), 0);
    assert_eq!(list_grants(None).len(), 0);
    let (c0, g0, at0, gr0, dn0, rv0, _) = stats();
    assert_eq!((c0, g0, at0, gr0, dn0, rv0), (0, 0, 0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Store + authenticate — first credential gets id 1; auth of a stored
    //    principal is Granted, auth of an unknown principal is Denied.
    let uid = store_credential("alice", AuthMethod::Password, "hash_a").expect("store");
    assert_eq!(uid, 1);
    assert_eq!(list_credentials(Some("alice")).len(), 1);
    assert_eq!(authenticate("alice", AuthMethod::Password).expect("auth"), AuthResult::Granted);
    assert_eq!(authenticate("nobody", AuthMethod::Password).expect("auth2"), AuthResult::Denied);
    crate::serial_println!("  [2/8] store + authenticate: OK");

    // 3: Failed attempts + lock — alice has max_failures=5 (the store default),
    //    so the fifth failure locks the account.
    for _ in 0..5 {
        record_failure("alice", AuthMethod::Password).expect("fail");
    }
    assert_eq!(authenticate("alice", AuthMethod::Password).expect("auth3"), AuthResult::Locked);
    crate::serial_println!("  [3/8] lockout: OK");

    // 4: Unlock — clears the lock and the failure counter; auth succeeds again.
    unlock("alice").expect("unlock");
    assert_eq!(authenticate("alice", AuthMethod::Password).expect("auth4"), AuthResult::Granted);
    crate::serial_println!("  [4/8] unlock: OK");

    // 5: Second credential — gets id 2; per-principal listing is exact.
    let bid = store_credential("bob", AuthMethod::Token, "hash_b").expect("store2");
    assert_eq!(bid, 2);
    assert_eq!(list_credentials(None).len(), 2);
    assert_eq!(list_credentials(Some("bob")).len(), 1);
    crate::serial_println!("  [5/8] second credential: OK");

    // 6: Grant capability — first grant gets id 1; active-grant listing is exact.
    let gid = grant_capability("alice", "/dev/sda", 0).expect("grant");
    assert_eq!(gid, 1);
    assert_eq!(list_grants(Some("alice")).len(), 1);
    crate::serial_println!("  [6/8] grant: OK");

    // 7: Revoke — the grant drops out of the active listing; double revoke errors.
    revoke_grant(gid).expect("revoke");
    assert_eq!(list_grants(Some("alice")).len(), 0);
    assert!(revoke_grant(gid).is_err());
    crate::serial_println!("  [7/8] revoke: OK");

    // 8: Final stats reflect only the real activity above: 2 credentials, 1
    //    grant slot, and the auth/grant/deny/revoke counters from this test.
    //    attempts = 4 auths (alice granted, nobody denied, locked, granted);
    //    granted = 2; denied = 1 (nobody) + 5 (record_failure) + 1 (locked) = 7;
    //    revoked = 1.
    let (creds, grants, attempts, granted, denied, revoked, ops) = stats();
    assert_eq!((creds, grants, attempts, granted, denied, revoked), (2, 1, 4, 2, 7, 1));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("authbroker::self_test() — all 8 tests passed");
}
