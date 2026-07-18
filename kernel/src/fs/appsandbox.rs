//! App sandbox — per-application capability isolation and access control.
//!
//! Manages filesystem access rules, network permissions, device access,
//! and privilege escalation prevention for untrusted applications.
//! Each sandboxed app runs with a restricted set of capabilities.
//!
//! ## Architecture
//!
//! ```text
//! App launch
//!   → appsandbox::create_sandbox(app_id) → new sandbox profile
//!
//! File access request
//!   → appsandbox::check_access(sandbox_id, path, mode)
//!     → allow/deny based on rules
//!
//! Settings panel → Security → App Permissions
//!   → appsandbox::list_sandboxes() → show app profiles
//!   → appsandbox::grant_permission() / revoke_permission()
//!
//! Integration:
//!   → appregistry (app metadata)
//!   → credentials (privilege escalation)
//!   → policy (system-wide security policy)
//!   → syslog (access violation logging)
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

/// Permission type for sandbox rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    FileRead,
    FileWrite,
    NetworkAccess,
    DeviceAccess,
    Clipboard,
    Notifications,
    Camera,
    Microphone,
    Location,
    SystemSettings,
}

impl Permission {
    pub fn label(self) -> &'static str {
        match self {
            Self::FileRead => "File Read",
            Self::FileWrite => "File Write",
            Self::NetworkAccess => "Network",
            Self::DeviceAccess => "Devices",
            Self::Clipboard => "Clipboard",
            Self::Notifications => "Notifications",
            Self::Camera => "Camera",
            Self::Microphone => "Microphone",
            Self::Location => "Location",
            Self::SystemSettings => "System Settings",
        }
    }
}

/// Access decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessDecision {
    Allow,
    Deny,
    Prompt,
}

impl AccessDecision {
    pub fn label(self) -> &'static str {
        match self {
            Self::Allow => "Allow",
            Self::Deny => "Deny",
            Self::Prompt => "Prompt",
        }
    }
}

/// Sandbox trust level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustLevel {
    Untrusted,
    LowTrust,
    Standard,
    Elevated,
    System,
}

impl TrustLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Untrusted => "Untrusted",
            Self::LowTrust => "Low Trust",
            Self::Standard => "Standard",
            Self::Elevated => "Elevated",
            Self::System => "System",
        }
    }
}

/// A permission rule.
#[derive(Debug, Clone)]
pub struct PermissionRule {
    pub permission: Permission,
    pub decision: AccessDecision,
    /// Optional path restriction (for file permissions).
    pub path_prefix: String,
}

/// An application sandbox profile.
#[derive(Debug, Clone)]
pub struct Sandbox {
    /// Sandbox ID.
    pub id: u32,
    /// Application name.
    pub app_name: String,
    /// Trust level.
    pub trust_level: TrustLevel,
    /// Permission rules.
    pub rules: Vec<PermissionRule>,
    /// Whether sandbox is active.
    pub active: bool,
    /// Access attempts.
    pub access_attempts: u64,
    /// Denied accesses.
    pub access_denied: u64,
    /// Created timestamp (ns).
    pub created_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_SANDBOXES: usize = 200;

struct State {
    sandboxes: Vec<Sandbox>,
    next_id: u32,
    total_created: u64,
    total_checks: u64,
    total_denied: u64,
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
    *guard = Some(State {
        sandboxes: Vec::new(),
        next_id: 1,
        total_created: 0,
        total_checks: 0,
        total_denied: 0,
        ops: 0,
    });
}

/// Create a new sandbox for an application.
pub fn create_sandbox(app_name: &str, trust_level: TrustLevel) -> KernelResult<u32> {
    with_state(|state| {
        if state.sandboxes.len() >= MAX_SANDBOXES {
            return Err(KernelError::ResourceExhausted);
        }
        if state.sandboxes.iter().any(|s| s.app_name == app_name) {
            return Err(KernelError::AlreadyExists);
        }

        let id = state.next_id;
        state.next_id += 1;
        state.total_created += 1;

        // Set default rules based on trust level.
        let mut rules = Vec::new();
        match trust_level {
            TrustLevel::Untrusted => {
                rules.push(PermissionRule { permission: Permission::FileRead, decision: AccessDecision::Deny, path_prefix: String::new() });
                rules.push(PermissionRule { permission: Permission::FileWrite, decision: AccessDecision::Deny, path_prefix: String::new() });
                rules.push(PermissionRule { permission: Permission::NetworkAccess, decision: AccessDecision::Deny, path_prefix: String::new() });
            }
            TrustLevel::LowTrust => {
                rules.push(PermissionRule { permission: Permission::FileRead, decision: AccessDecision::Prompt, path_prefix: String::new() });
                rules.push(PermissionRule { permission: Permission::FileWrite, decision: AccessDecision::Deny, path_prefix: String::new() });
                rules.push(PermissionRule { permission: Permission::NetworkAccess, decision: AccessDecision::Prompt, path_prefix: String::new() });
            }
            TrustLevel::Standard => {
                rules.push(PermissionRule { permission: Permission::FileRead, decision: AccessDecision::Allow, path_prefix: String::new() });
                rules.push(PermissionRule { permission: Permission::FileWrite, decision: AccessDecision::Prompt, path_prefix: String::new() });
                rules.push(PermissionRule { permission: Permission::NetworkAccess, decision: AccessDecision::Allow, path_prefix: String::new() });
            }
            TrustLevel::Elevated | TrustLevel::System => {
                rules.push(PermissionRule { permission: Permission::FileRead, decision: AccessDecision::Allow, path_prefix: String::new() });
                rules.push(PermissionRule { permission: Permission::FileWrite, decision: AccessDecision::Allow, path_prefix: String::new() });
                rules.push(PermissionRule { permission: Permission::NetworkAccess, decision: AccessDecision::Allow, path_prefix: String::new() });
            }
        }

        state.sandboxes.push(Sandbox {
            id, app_name: String::from(app_name),
            trust_level, rules, active: true,
            access_attempts: 0, access_denied: 0,
            created_ns: crate::hpet::elapsed_ns(),
        });
        Ok(id)
    })
}

/// Grant a permission to a sandbox.
pub fn grant_permission(id: u32, permission: Permission, path_prefix: &str) -> KernelResult<()> {
    with_state(|state| {
        let sandbox = state.sandboxes.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        // Update existing or add new rule.
        if let Some(rule) = sandbox.rules.iter_mut().find(|r| r.permission == permission && r.path_prefix == path_prefix) {
            rule.decision = AccessDecision::Allow;
        } else {
            sandbox.rules.push(PermissionRule {
                permission, decision: AccessDecision::Allow,
                path_prefix: String::from(path_prefix),
            });
        }
        Ok(())
    })
}

/// Revoke a permission from a sandbox.
pub fn revoke_permission(id: u32, permission: Permission) -> KernelResult<()> {
    with_state(|state| {
        let sandbox = state.sandboxes.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        for rule in sandbox.rules.iter_mut() {
            if rule.permission == permission {
                rule.decision = AccessDecision::Deny;
            }
        }
        Ok(())
    })
}

/// Check access for a sandbox.
pub fn check_access(id: u32, permission: Permission) -> KernelResult<AccessDecision> {
    with_state(|state| {
        let sandbox = state.sandboxes.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        sandbox.access_attempts += 1;
        state.total_checks += 1;

        if !sandbox.active {
            return Ok(AccessDecision::Allow);
        }

        let decision = sandbox.rules.iter()
            .find(|r| r.permission == permission)
            .map(|r| r.decision)
            .unwrap_or(AccessDecision::Deny);

        if decision == AccessDecision::Deny {
            sandbox.access_denied += 1;
            state.total_denied += 1;
        }
        Ok(decision)
    })
}

/// Enable/disable a sandbox.
pub fn set_active(id: u32, active: bool) -> KernelResult<()> {
    with_state(|state| {
        let sandbox = state.sandboxes.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        sandbox.active = active;
        Ok(())
    })
}

/// Remove a sandbox.
pub fn remove_sandbox(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.sandboxes.iter().position(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        state.sandboxes.remove(pos);
        Ok(())
    })
}

/// Get sandbox by ID.
pub fn get_sandbox(id: u32) -> KernelResult<Sandbox> {
    with_state(|state| {
        state.sandboxes.iter().find(|s| s.id == id).cloned().ok_or(KernelError::NotFound)
    })
}

/// List all sandboxes.
pub fn list_sandboxes() -> Vec<Sandbox> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.sandboxes.clone())
}

/// Statistics: (sandbox_count, total_created, total_checks, total_denied, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.sandboxes.len(), s.total_created, s.total_checks, s.total_denied, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("appsandbox::self_test() — running tests...");
    init_defaults();

    // 1: Empty initial.
    assert!(list_sandboxes().is_empty());
    crate::serial_println!("  [1/11] empty initial: OK");

    // 2: Create untrusted sandbox.
    let id1 = create_sandbox("malware_test", TrustLevel::Untrusted).expect("create untrusted");
    assert!(id1 > 0);
    crate::serial_println!("  [2/11] create untrusted: OK");

    // 3: Create standard sandbox.
    let id2 = create_sandbox("text_editor", TrustLevel::Standard).expect("create standard");
    assert_eq!(list_sandboxes().len(), 2);
    crate::serial_println!("  [3/11] create standard: OK");

    // 4: Duplicate rejected.
    let r = create_sandbox("text_editor", TrustLevel::Standard);
    assert!(r.is_err());
    crate::serial_println!("  [4/11] duplicate rejected: OK");

    // 5: Check access — untrusted denied.
    let decision = check_access(id1, Permission::FileRead).expect("check untrusted");
    assert_eq!(decision, AccessDecision::Deny);
    crate::serial_println!("  [5/11] untrusted denied: OK");

    // 6: Check access — standard allowed.
    let decision = check_access(id2, Permission::FileRead).expect("check standard");
    assert_eq!(decision, AccessDecision::Allow);
    crate::serial_println!("  [6/11] standard allowed: OK");

    // 7: Grant permission.
    grant_permission(id1, Permission::FileRead, "").expect("grant");
    let decision = check_access(id1, Permission::FileRead).expect("check granted");
    assert_eq!(decision, AccessDecision::Allow);
    crate::serial_println!("  [7/11] grant permission: OK");

    // 8: Revoke permission.
    revoke_permission(id2, Permission::FileRead).expect("revoke");
    let decision = check_access(id2, Permission::FileRead).expect("check revoked");
    assert_eq!(decision, AccessDecision::Deny);
    crate::serial_println!("  [8/11] revoke permission: OK");

    // 9: Disable sandbox.
    set_active(id1, false).expect("disable");
    let decision = check_access(id1, Permission::NetworkAccess).expect("check disabled");
    assert_eq!(decision, AccessDecision::Allow); // Inactive = allow all.
    crate::serial_println!("  [9/11] disable sandbox: OK");

    // 10: Remove sandbox.
    remove_sandbox(id1).expect("remove");
    assert_eq!(list_sandboxes().len(), 1);
    crate::serial_println!("  [10/11] remove sandbox: OK");

    // 11: Stats.
    let (count, created, checks, denied, ops) = stats();
    assert_eq!(count, 1);
    assert_eq!(created, 2);
    assert!(checks > 0);
    assert!(denied > 0);
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("appsandbox::self_test() — all 11 tests passed");
}
