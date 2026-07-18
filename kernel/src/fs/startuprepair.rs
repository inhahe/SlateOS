//! Startup repair — boot diagnostics and automatic repair.
//!
//! Detects and repairs common boot problems: corrupted boot files,
//! missing drivers, filesystem errors, broken system services.
//! Runs automatically after failed boots or on user request.
//!
//! ## Architecture
//!
//! ```text
//! Boot failure detection (failed boot counter)
//!   → startuprepair::auto_diagnose() → repair sequence
//!
//! Recovery environment
//!   → startuprepair::run_all_checks() → comprehensive scan
//!
//! Integration:
//!   → bootcfg (boot configuration repair)
//!   → health (filesystem health check)
//!   → driverupdate (driver rollback)
//!   → restorepoint (restore to working state)
//!   → syslog (repair log entries)
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

/// Repair check category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckCategory {
    BootLoader,
    BootConfig,
    FileSystem,
    SystemFiles,
    Drivers,
    Services,
    Registry,
    Permissions,
}

impl CheckCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::BootLoader => "Boot Loader",
            Self::BootConfig => "Boot Configuration",
            Self::FileSystem => "File System",
            Self::SystemFiles => "System Files",
            Self::Drivers => "Drivers",
            Self::Services => "Services",
            Self::Registry => "Registry/Config",
            Self::Permissions => "Permissions",
        }
    }
}

/// Check result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckResult {
    Pass,
    Warning,
    Fail,
    Repaired,
    Skipped,
}

impl CheckResult {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Warning => "WARN",
            Self::Fail => "FAIL",
            Self::Repaired => "REPAIRED",
            Self::Skipped => "SKIPPED",
        }
    }
}

/// A diagnostic check result.
#[derive(Debug, Clone)]
pub struct DiagCheck {
    /// Check ID.
    pub id: u32,
    /// Category.
    pub category: CheckCategory,
    /// Check description.
    pub description: String,
    /// Result.
    pub result: CheckResult,
    /// Detail message.
    pub detail: String,
    /// Whether auto-repair was attempted.
    pub repair_attempted: bool,
    /// Timestamp (ns).
    pub timestamp_ns: u64,
}

/// Repair session.
#[derive(Debug, Clone)]
pub struct RepairSession {
    pub session_id: u32,
    pub checks: Vec<DiagCheck>,
    pub started_ns: u64,
    pub completed_ns: u64,
    pub auto_triggered: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_SESSIONS: usize = 20;

struct State {
    sessions: Vec<RepairSession>,
    next_session_id: u32,
    next_check_id: u32,
    failed_boots: u32,
    auto_repair_threshold: u32,
    total_repairs: u64,
    total_checks: u64,
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
        sessions: Vec::new(),
        next_session_id: 1,
        next_check_id: 1,
        failed_boots: 0,
        auto_repair_threshold: 3,
        total_repairs: 0,
        total_checks: 0,
        ops: 0,
    });
}

/// Start a new repair session.
pub fn start_session(auto_triggered: bool) -> KernelResult<u32> {
    with_state(|state| {
        let id = state.next_session_id;
        state.next_session_id += 1;
        state.sessions.push(RepairSession {
            session_id: id,
            checks: Vec::new(),
            started_ns: crate::hpet::elapsed_ns(),
            completed_ns: 0,
            auto_triggered,
        });
        while state.sessions.len() > MAX_SESSIONS {
            state.sessions.remove(0);
        }
        Ok(id)
    })
}

/// Add a check result to a session.
pub fn add_check(
    session_id: u32, category: CheckCategory, description: &str,
    result: CheckResult, detail: &str, repair_attempted: bool,
) -> KernelResult<u32> {
    with_state(|state| {
        let session = state.sessions.iter_mut()
            .find(|s| s.session_id == session_id)
            .ok_or(KernelError::NotFound)?;

        let check_id = state.next_check_id;
        state.next_check_id += 1;
        state.total_checks += 1;

        if result == CheckResult::Repaired {
            state.total_repairs += 1;
        }

        session.checks.push(DiagCheck {
            id: check_id, category,
            description: String::from(description),
            result, detail: String::from(detail),
            repair_attempted,
            timestamp_ns: crate::hpet::elapsed_ns(),
        });

        Ok(check_id)
    })
}

/// Complete a session.
pub fn complete_session(session_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let session = state.sessions.iter_mut()
            .find(|s| s.session_id == session_id)
            .ok_or(KernelError::NotFound)?;
        session.completed_ns = crate::hpet::elapsed_ns();
        Ok(())
    })
}

/// Run all standard checks (simulated).
pub fn run_all_checks() -> KernelResult<u32> {
    let sid = start_session(false)?;

    add_check(sid, CheckCategory::BootLoader, "Boot loader integrity",
        CheckResult::Pass, "Boot loader files intact", false)?;
    add_check(sid, CheckCategory::BootConfig, "Boot configuration",
        CheckResult::Pass, "Boot entries valid", false)?;
    add_check(sid, CheckCategory::FileSystem, "Root filesystem",
        CheckResult::Pass, "No errors detected", false)?;
    add_check(sid, CheckCategory::SystemFiles, "Critical system files",
        CheckResult::Pass, "All files present and valid", false)?;
    add_check(sid, CheckCategory::Drivers, "Essential drivers",
        CheckResult::Pass, "All drivers loaded", false)?;
    add_check(sid, CheckCategory::Services, "System services",
        CheckResult::Pass, "Services responding", false)?;
    add_check(sid, CheckCategory::Permissions, "File permissions",
        CheckResult::Pass, "Permissions correct", false)?;

    complete_session(sid)?;
    Ok(sid)
}

/// Record a failed boot.
pub fn record_failed_boot() -> KernelResult<bool> {
    with_state(|state| {
        state.failed_boots += 1;
        Ok(state.failed_boots >= state.auto_repair_threshold)
    })
}

/// Reset failed boot counter (called on successful boot).
pub fn reset_failed_boots() -> KernelResult<()> {
    with_state(|state| { state.failed_boots = 0; Ok(()) })
}

/// Get session results.
pub fn get_session(session_id: u32) -> KernelResult<RepairSession> {
    with_state(|state| {
        state.sessions.iter().find(|s| s.session_id == session_id)
            .cloned().ok_or(KernelError::NotFound)
    })
}

/// List sessions.
pub fn list_sessions() -> Vec<RepairSession> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.sessions.clone())
}

/// Statistics: (session_count, total_checks, total_repairs, failed_boots, ops).
pub fn stats() -> (usize, u64, u64, u32, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.sessions.len(), s.total_checks, s.total_repairs, s.failed_boots, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("startuprepair::self_test() — running tests...");
    init_defaults();

    // 1: No sessions initially.
    assert!(list_sessions().is_empty());
    crate::serial_println!("  [1/11] empty initial: OK");

    // 2: Start session.
    let sid = start_session(false).expect("start");
    assert!(sid > 0);
    crate::serial_println!("  [2/11] start session: OK");

    // 3: Add passing check.
    add_check(sid, CheckCategory::BootLoader, "Boot check",
        CheckResult::Pass, "OK", false).expect("add pass");
    crate::serial_println!("  [3/11] pass check: OK");

    // 4: Add failing check.
    add_check(sid, CheckCategory::FileSystem, "FS check",
        CheckResult::Fail, "Corruption detected", false).expect("add fail");
    crate::serial_println!("  [4/11] fail check: OK");

    // 5: Add repaired check.
    add_check(sid, CheckCategory::FileSystem, "FS repair",
        CheckResult::Repaired, "Corruption fixed", true).expect("add repair");
    crate::serial_println!("  [5/11] repaired check: OK");

    // 6: Complete session.
    complete_session(sid).expect("complete");
    let session = get_session(sid).expect("get session");
    assert!(session.completed_ns > 0);
    assert_eq!(session.checks.len(), 3);
    crate::serial_println!("  [6/11] complete session: OK");

    // 7: Run all checks.
    let sid2 = run_all_checks().expect("run all");
    let session2 = get_session(sid2).expect("get session 2");
    assert_eq!(session2.checks.len(), 7);
    assert!(session2.checks.iter().all(|c| c.result == CheckResult::Pass));
    crate::serial_println!("  [7/11] run all checks: OK");

    // 8: Failed boot counter.
    record_failed_boot().expect("fail 1");
    record_failed_boot().expect("fail 2");
    let needs_repair = record_failed_boot().expect("fail 3");
    assert!(needs_repair); // Threshold is 3.
    crate::serial_println!("  [8/11] failed boot counter: OK");

    // 9: Reset counter.
    reset_failed_boots().expect("reset");
    let (_, _, _, failed, _) = stats();
    assert_eq!(failed, 0);
    crate::serial_println!("  [9/11] reset counter: OK");

    // 10: Session list.
    let sessions = list_sessions();
    assert_eq!(sessions.len(), 2);
    crate::serial_println!("  [10/11] session list: OK");

    // 11: Stats.
    let (sessions, checks, repairs, failed, ops) = stats();
    assert_eq!(sessions, 2);
    assert!(checks >= 10);
    assert!(repairs >= 1);
    assert_eq!(failed, 0);
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("startuprepair::self_test() — all 11 tests passed");
}
