//! App Permissions — per-application permission management.
//!
//! Controls which system capabilities each application can access,
//! with prompt-based consent and persistent permission storage.
//!
//! ## Architecture
//!
//! ```text
//! App requests resource
//!   → apppermissions::check(app, permission) → allowed/denied
//!   → apppermissions::prompt(app, permission) → user decision
//!   → apppermissions::grant/revoke(app, permission)
//!
//! Integration:
//!   → appregistry (registered apps)
//!   → appsandbox (sandbox enforcement)
//!   → webcam (camera permission)
//!   → location (location permission)
//!   → speechio (microphone permission)
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

/// System permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    Camera,
    Microphone,
    Location,
    Notifications,
    Contacts,
    Calendar,
    Storage,
    Network,
    Bluetooth,
    BackgroundActivity,
    SystemSettings,
    Accessibility,
}

impl Permission {
    pub fn label(self) -> &'static str {
        match self {
            Self::Camera => "Camera",
            Self::Microphone => "Microphone",
            Self::Location => "Location",
            Self::Notifications => "Notifications",
            Self::Contacts => "Contacts",
            Self::Calendar => "Calendar",
            Self::Storage => "Storage",
            Self::Network => "Network",
            Self::Bluetooth => "Bluetooth",
            Self::BackgroundActivity => "Background Activity",
            Self::SystemSettings => "System Settings",
            Self::Accessibility => "Accessibility",
        }
    }
}

/// Permission decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Permission granted.
    Allowed,
    /// Permission denied.
    Denied,
    /// Ask user each time.
    AskEveryTime,
    /// Not yet decided (first request triggers prompt).
    NotDecided,
}

impl Decision {
    pub fn label(self) -> &'static str {
        match self {
            Self::Allowed => "Allowed",
            Self::Denied => "Denied",
            Self::AskEveryTime => "Ask Every Time",
            Self::NotDecided => "Not Decided",
        }
    }
}

/// Per-app permission entry.
#[derive(Debug, Clone)]
pub struct AppPermission {
    pub app_name: String,
    pub permission: Permission,
    pub decision: Decision,
    pub last_requested_ns: u64,
    pub request_count: u64,
}

/// Permission request log entry.
#[derive(Debug, Clone)]
pub struct PermissionLog {
    pub app_name: String,
    pub permission: Permission,
    pub granted: bool,
    pub timestamp_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_ENTRIES: usize = 500;
const MAX_LOG: usize = 200;

struct State {
    entries: Vec<AppPermission>,
    log: Vec<PermissionLog>,
    total_checks: u64,
    total_grants: u64,
    total_denials: u64,
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
        entries: Vec::new(),
        log: Vec::new(),
        total_checks: 0,
        total_grants: 0,
        total_denials: 0,
        ops: 0,
    });
}

/// Check if an app has a permission (returns Decision).
pub fn check(app_name: &str, permission: Permission) -> Decision {
    let mut guard = STATE.lock();
    let state = match guard.as_mut() {
        Some(s) => s,
        None => return Decision::NotDecided,
    };
    state.ops += 1;
    state.total_checks += 1;
    OPS.store(state.ops, Ordering::Relaxed);

    state.entries.iter().find(|e| e.app_name == app_name && e.permission == permission)
        .map_or(Decision::NotDecided, |e| e.decision)
}

/// Grant a permission to an app.
pub fn grant(app_name: &str, permission: Permission) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        if let Some(existing) = state.entries.iter_mut().find(|e| e.app_name == app_name && e.permission == permission) {
            existing.decision = Decision::Allowed;
            existing.last_requested_ns = now;
        } else {
            if state.entries.len() >= MAX_ENTRIES {
                return Err(KernelError::ResourceExhausted);
            }
            state.entries.push(AppPermission {
                app_name: String::from(app_name),
                permission, decision: Decision::Allowed,
                last_requested_ns: now, request_count: 0,
            });
        }
        state.total_grants += 1;

        if state.log.len() >= MAX_LOG {
            state.log.remove(0);
        }
        state.log.push(PermissionLog {
            app_name: String::from(app_name),
            permission, granted: true, timestamp_ns: now,
        });
        Ok(())
    })
}

/// Deny a permission to an app.
pub fn deny(app_name: &str, permission: Permission) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        if let Some(existing) = state.entries.iter_mut().find(|e| e.app_name == app_name && e.permission == permission) {
            existing.decision = Decision::Denied;
            existing.last_requested_ns = now;
        } else {
            if state.entries.len() >= MAX_ENTRIES {
                return Err(KernelError::ResourceExhausted);
            }
            state.entries.push(AppPermission {
                app_name: String::from(app_name),
                permission, decision: Decision::Denied,
                last_requested_ns: now, request_count: 0,
            });
        }
        state.total_denials += 1;

        if state.log.len() >= MAX_LOG {
            state.log.remove(0);
        }
        state.log.push(PermissionLog {
            app_name: String::from(app_name),
            permission, granted: false, timestamp_ns: now,
        });
        Ok(())
    })
}

/// Revoke all permissions for an app.
pub fn revoke_all(app_name: &str) -> KernelResult<usize> {
    with_state(|state| {
        let before = state.entries.len();
        state.entries.retain(|e| e.app_name != app_name);
        let removed = before - state.entries.len();
        Ok(removed)
    })
}

/// List permissions for an app.
pub fn list_app_permissions(app_name: &str) -> Vec<AppPermission> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.entries.iter().filter(|e| e.app_name == app_name).cloned().collect()
    })
}

/// List all apps with a specific permission.
pub fn list_by_permission(permission: Permission) -> Vec<AppPermission> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.entries.iter().filter(|e| e.permission == permission).cloned().collect()
    })
}

/// Recent permission log.
pub fn list_log(count: usize) -> Vec<PermissionLog> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let start = s.log.len().saturating_sub(count);
        s.log[start..].to_vec()
    })
}

/// Statistics: (entry_count, total_checks, total_grants, total_denials, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.entries.len(), s.total_checks, s.total_grants, s.total_denials, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("apppermissions::self_test() — running tests...");
    init_defaults();

    // 1: No permissions initially.
    let d = check("testapp", Permission::Camera);
    assert_eq!(d, Decision::NotDecided);
    crate::serial_println!("  [1/8] no permissions: OK");

    // 2: Grant permission.
    grant("testapp", Permission::Camera).expect("grant");
    let d = check("testapp", Permission::Camera);
    assert_eq!(d, Decision::Allowed);
    crate::serial_println!("  [2/8] grant: OK");

    // 3: Deny permission.
    deny("testapp", Permission::Location).expect("deny");
    let d = check("testapp", Permission::Location);
    assert_eq!(d, Decision::Denied);
    crate::serial_println!("  [3/8] deny: OK");

    // 4: List app permissions.
    let perms = list_app_permissions("testapp");
    assert_eq!(perms.len(), 2);
    crate::serial_println!("  [4/8] list app perms: OK");

    // 5: List by permission.
    grant("otherapp", Permission::Camera).expect("grant2");
    let camera_apps = list_by_permission(Permission::Camera);
    assert_eq!(camera_apps.len(), 2);
    crate::serial_println!("  [5/8] list by perm: OK");

    // 6: Revoke all.
    let removed = revoke_all("testapp").expect("revoke");
    assert_eq!(removed, 2);
    let d = check("testapp", Permission::Camera);
    assert_eq!(d, Decision::NotDecided);
    crate::serial_println!("  [6/8] revoke all: OK");

    // 7: Permission log.
    let log = list_log(10);
    assert!(log.len() >= 3);
    crate::serial_println!("  [7/8] permission log: OK");

    // 8: Stats.
    let (entries, checks, grants, denials, ops) = stats();
    assert_eq!(entries, 1); // only otherapp remains
    assert!(checks >= 4);
    assert!(grants >= 2);
    assert!(denials >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("apppermissions::self_test() — all 8 tests passed");
}
