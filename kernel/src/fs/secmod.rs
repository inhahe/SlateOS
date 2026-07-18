//! Security Module Statistics — LSM-like security module monitoring.
//!
//! Tracks security policy decisions, denials, audit events,
//! and per-module hook invocations. Essential for understanding
//! security enforcement overhead and effectiveness.
//!
//! ## Architecture
//!
//! ```text
//! Security module monitoring
//!   → secmod::record_check(module, hook) → policy check
//!   → secmod::record_deny(module, hook) → access denied
//!   → secmod::record_audit(module) → audit log event
//!   → secmod::per_module() → per-module stats
//!
//! Integration:
//!   → secpolicy (security policy)
//!   → audit (audit framework)
//!   → prociso (process isolation)
//!   → authbroker (auth broker)
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

/// Security hook type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookType {
    FileOpen,
    FilePermission,
    InodeCreate,
    InodeUnlink,
    TaskAlloc,
    TaskKill,
    SocketCreate,
    SocketConnect,
}

impl HookType {
    pub fn label(self) -> &'static str {
        match self {
            Self::FileOpen => "file_open",
            Self::FilePermission => "file_perm",
            Self::InodeCreate => "inode_create",
            Self::InodeUnlink => "inode_unlink",
            Self::TaskAlloc => "task_alloc",
            Self::TaskKill => "task_kill",
            Self::SocketCreate => "sock_create",
            Self::SocketConnect => "sock_connect",
        }
    }
    pub fn index(self) -> usize {
        match self {
            Self::FileOpen => 0,
            Self::FilePermission => 1,
            Self::InodeCreate => 2,
            Self::InodeUnlink => 3,
            Self::TaskAlloc => 4,
            Self::TaskKill => 5,
            Self::SocketCreate => 6,
            Self::SocketConnect => 7,
        }
    }
}

const NUM_HOOKS: usize = 8;

/// Per-module stats.
#[derive(Debug, Clone)]
pub struct ModuleStats {
    pub name: String,
    pub enabled: bool,
    pub checks: [u64; NUM_HOOKS],
    pub denials: [u64; NUM_HOOKS],
    pub total_checks: u64,
    pub total_denials: u64,
    pub audit_events: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_MODULES: usize = 16;

struct State {
    modules: Vec<ModuleStats>,
    total_checks: u64,
    total_denials: u64,
    total_audits: u64,
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

/// Initialise the security-module statistics state.
///
/// Starts with NO registered security modules and zero check/denial/audit
/// totals. A module is added through [`register_module`] when an LSM-like
/// security module is actually loaded, and its per-hook check/denial/audit
/// counters advance only through real [`record_check`] / [`record_deny`] /
/// [`record_audit`] calls on the security-hook path. The `/proc/secmod`
/// generator and the `secmod` kshell command surface the module list (and
/// [`per_module`] / [`stats`]) as if it reflects the real security-enforcement
/// activity, so seeding it with phantom modules and access counts would be
/// fabricated procfs data — it would claim hundreds of millions of policy
/// checks and hundreds of thousands of denials that never happened.
///
/// (Previously this seeded two fictional modules — "capability" (per-hook
/// checks [50M, 30M, 5M, 2M, 1M, 100K, 500K, 200K], denials [100K, 50K, 10K,
/// 5K, 2K, 500, 1K, 200], 88.8M checks / 168,700 denials / 50K audits) and
/// "apparmor" (checks [40M, 25M, 4M, 1.5M, 800K, 80K, 400K, 150K], denials
/// [200K, 100K, 20K, 10K, 5K, 1K, 2K, 500], 71.93M checks / 338,500 denials /
/// 100K audits) — plus global totals of 160,730,000 checks / 507,200 denials /
/// 150,000 audits.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        modules: Vec::new(),
        total_checks: 0,
        total_denials: 0,
        total_audits: 0,
        ops: 0,
    });
}

/// Register a security module.
pub fn register_module(name: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.modules.iter().any(|m| m.name == name) { return Err(KernelError::AlreadyExists); }
        if state.modules.len() >= MAX_MODULES { return Err(KernelError::ResourceExhausted); }
        state.modules.push(ModuleStats {
            name: String::from(name), enabled: true,
            checks: [0; NUM_HOOKS], denials: [0; NUM_HOOKS],
            total_checks: 0, total_denials: 0, audit_events: 0,
        });
        Ok(())
    })
}

/// Record a security check (allow).
pub fn record_check(module: &str, hook: HookType) -> KernelResult<()> {
    with_state(|state| {
        let m = state.modules.iter_mut().find(|m| m.name == module)
            .ok_or(KernelError::NotFound)?;
        m.checks[hook.index()] += 1;
        m.total_checks += 1;
        state.total_checks += 1;
        Ok(())
    })
}

/// Record a denial.
pub fn record_deny(module: &str, hook: HookType) -> KernelResult<()> {
    with_state(|state| {
        let m = state.modules.iter_mut().find(|m| m.name == module)
            .ok_or(KernelError::NotFound)?;
        m.checks[hook.index()] += 1;
        m.denials[hook.index()] += 1;
        m.total_checks += 1;
        m.total_denials += 1;
        state.total_checks += 1;
        state.total_denials += 1;
        Ok(())
    })
}

/// Record an audit event.
pub fn record_audit(module: &str) -> KernelResult<()> {
    with_state(|state| {
        let m = state.modules.iter_mut().find(|m| m.name == module)
            .ok_or(KernelError::NotFound)?;
        m.audit_events += 1;
        state.total_audits += 1;
        Ok(())
    })
}

/// Enable/disable a module.
pub fn set_enabled(module: &str, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let m = state.modules.iter_mut().find(|m| m.name == module)
            .ok_or(KernelError::NotFound)?;
        m.enabled = enabled;
        Ok(())
    })
}

/// Per-module stats.
pub fn per_module() -> Vec<ModuleStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.modules.clone())
}

/// Statistics: (module_count, total_checks, total_denials, total_audits, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.modules.len(), s.total_checks, s.total_denials, s.total_audits, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("secmod::self_test() — running tests...");
    // Start from a clean, empty state so the assertions below are exact and no
    // fixtures leak into the live module table afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no phantom modules, zero totals.
    assert_eq!(per_module().len(), 0);
    let (m0, c0, d0, a0, _) = stats();
    assert_eq!((m0, c0, d0, a0), (0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Register — module appears zeroed; a duplicate is AlreadyExists.
    register_module("test_mod").expect("register");
    assert!(register_module("test_mod").is_err());
    assert_eq!(per_module().len(), 1);
    let m = per_module().into_iter().find(|m| m.name == "test_mod").expect("find");
    assert_eq!((m.total_checks, m.total_denials, m.audit_events), (0, 0, 0));
    crate::serial_println!("  [2/8] register: OK");

    // 3: Check — per-hook + per-module + global check counters advance.
    record_check("test_mod", HookType::FileOpen).expect("check");
    let m = per_module().into_iter().find(|m| m.name == "test_mod").expect("p3");
    assert_eq!((m.total_checks, m.checks[0]), (1, 1));
    assert_eq!(stats().1, 1); // total_checks
    crate::serial_println!("  [3/8] check: OK");

    // 4: Deny — a denial also counts as a check (both per-hook arrays advance).
    record_deny("test_mod", HookType::FileOpen).expect("deny");
    let m = per_module().into_iter().find(|m| m.name == "test_mod").expect("p4");
    assert_eq!((m.total_denials, m.denials[0]), (1, 1));
    assert_eq!((m.total_checks, m.checks[0]), (2, 2)); // deny bumps checks too
    assert_eq!(stats().2, 1); // total_denials
    crate::serial_println!("  [4/8] deny: OK");

    // 5: Audit — per-module and global audit counters advance.
    record_audit("test_mod").expect("audit");
    let m = per_module().into_iter().find(|m| m.name == "test_mod").expect("p5");
    assert_eq!(m.audit_events, 1);
    assert_eq!(stats().3, 1); // total_audits
    crate::serial_println!("  [5/8] audit: OK");

    // 6: Enable/disable — toggling the module's enabled flag.
    set_enabled("test_mod", false).expect("disable");
    let m = per_module().into_iter().find(|m| m.name == "test_mod").expect("p6");
    assert!(!m.enabled);
    crate::serial_println!("  [6/8] enable/disable: OK");

    // 7: Not found — recording into an unregistered module errors.
    assert!(record_check("nonexist", HookType::FileOpen).is_err());
    assert!(record_deny("nonexist", HookType::FileOpen).is_err());
    assert!(record_audit("nonexist").is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Final stats reflect only the real activity above: 1 module, 2 checks
    //    (1 check + 1 deny), 1 denial, 1 audit.
    let (mods, checks, denials, audits, ops) = stats();
    assert_eq!((mods, checks, denials, audits), (1, 2, 1, 1));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("secmod::self_test() — all 8 tests passed");
}
