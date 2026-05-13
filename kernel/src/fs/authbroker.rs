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
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    let now = crate::hpet::elapsed_ns();
    *guard = Some(State {
        credentials: alloc::vec![
            Credential {
                id: 1, principal: String::from("root"), method: AuthMethod::Password,
                hash: String::from("$argon2id$v=19$m=65536,t=3,p=4$..."),
                created_ns: now, expires_ns: 0, locked: false,
                failed_attempts: 0, max_failures: 5,
            },
            Credential {
                id: 2, principal: String::from("admin"), method: AuthMethod::PublicKey,
                hash: String::from("ssh-ed25519 AAAAC3..."),
                created_ns: now, expires_ns: 0, locked: false,
                failed_attempts: 0, max_failures: 10,
            },
            Credential {
                id: 3, principal: String::from("service_acct"), method: AuthMethod::Token,
                hash: String::from("bearer:hashed_token_abc123"),
                created_ns: now, expires_ns: now + 86_400_000_000_000, // 24h.
                locked: false, failed_attempts: 0, max_failures: 0,
            },
        ],
        grants: Vec::new(),
        next_cred_id: 4,
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(list_credentials(None).len(), 3);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Authenticate.
    let r = authenticate("root", AuthMethod::Password).expect("auth");
    assert_eq!(r, AuthResult::Granted);
    let r = authenticate("nobody", AuthMethod::Password).expect("auth2");
    assert_eq!(r, AuthResult::Denied);
    crate::serial_println!("  [2/8] authenticate: OK");

    // 3: Failed attempts + lock.
    for _ in 0..5 {
        record_failure("root", AuthMethod::Password).expect("fail");
    }
    let r = authenticate("root", AuthMethod::Password).expect("auth3");
    assert_eq!(r, AuthResult::Locked);
    crate::serial_println!("  [3/8] lockout: OK");

    // 4: Unlock.
    unlock("root").expect("unlock");
    let r = authenticate("root", AuthMethod::Password).expect("auth4");
    assert_eq!(r, AuthResult::Granted);
    crate::serial_println!("  [4/8] unlock: OK");

    // 5: Store credential.
    let id = store_credential("testuser", AuthMethod::Token, "hash123").expect("store");
    assert!(id >= 4);
    assert_eq!(list_credentials(Some("testuser")).len(), 1);
    crate::serial_println!("  [5/8] store: OK");

    // 6: Grant capability.
    let gid = grant_capability("root", "/dev/sda", 0).expect("grant");
    assert_eq!(list_grants(Some("root")).len(), 1);
    crate::serial_println!("  [6/8] grant: OK");

    // 7: Revoke.
    revoke_grant(gid).expect("revoke");
    assert_eq!(list_grants(Some("root")).len(), 0);
    crate::serial_println!("  [7/8] revoke: OK");

    // 8: Stats.
    let (creds, grants, attempts, granted, denied, revoked, ops) = stats();
    assert!(creds >= 4);
    let _ = grants;
    assert!(attempts >= 4);
    assert!(granted >= 3);
    assert!(denied >= 1);
    assert!(revoked >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("authbroker::self_test() — all 8 tests passed");
}
