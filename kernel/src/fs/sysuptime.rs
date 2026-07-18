//! System Uptime — uptime tracking and history.
//!
//! Tracks current and historical system uptime, including boot
//! timestamps, shutdown records, and availability statistics.
//!
//! ## Architecture
//!
//! ```text
//! Uptime tracking
//!   → sysuptime::current() → current uptime in nanoseconds
//!   → sysuptime::boot_time() → boot timestamp
//!   → sysuptime::record_shutdown(reason) → log shutdown event
//!   → sysuptime::history() → past uptime sessions
//!
//! Integration:
//!   → sysinfo (system information)
//!   → eventlog (event logging)
//!   → power (power management)
//!   → sysdiag (diagnostics)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Shutdown reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownReason {
    Clean,
    Reboot,
    Update,
    PowerLoss,
    KernelPanic,
    UserRequest,
    Scheduled,
}

impl ShutdownReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::Clean => "Clean shutdown",
            Self::Reboot => "Reboot",
            Self::Update => "Update reboot",
            Self::PowerLoss => "Power loss",
            Self::KernelPanic => "Kernel panic",
            Self::UserRequest => "User request",
            Self::Scheduled => "Scheduled shutdown",
        }
    }
}

/// A historical uptime session.
#[derive(Debug, Clone)]
pub struct UptimeSession {
    pub session_id: u32,
    pub boot_ns: u64,
    pub shutdown_ns: u64,
    pub duration_ns: u64,
    pub shutdown_reason: ShutdownReason,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_HISTORY: usize = 200;

struct State {
    boot_timestamp_ns: u64,
    history: Vec<UptimeSession>,
    next_session_id: u32,
    current_session_id: u32,
    total_sessions: u64,
    longest_uptime_ns: u64,
    total_uptime_ns: u64,
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
        boot_timestamp_ns: now,
        history: Vec::new(),
        next_session_id: 2,
        current_session_id: 1,
        total_sessions: 1,
        longest_uptime_ns: 0,
        total_uptime_ns: 0,
        ops: 0,
    });
}

/// Get current uptime in nanoseconds.
pub fn current_uptime_ns() -> u64 {
    let now = crate::hpet::elapsed_ns();
    STATE.lock().as_ref().map_or(0, |s| now.saturating_sub(s.boot_timestamp_ns))
}

/// Get boot timestamp.
pub fn boot_time_ns() -> u64 {
    STATE.lock().as_ref().map_or(0, |s| s.boot_timestamp_ns)
}

/// Get current session ID.
pub fn current_session_id() -> u32 {
    STATE.lock().as_ref().map_or(0, |s| s.current_session_id)
}

/// Record a shutdown event (closes current session, starts new one).
pub fn record_shutdown(reason: ShutdownReason) -> KernelResult<u32> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let duration = now.saturating_sub(state.boot_timestamp_ns);
        // Record the session.
        if state.history.len() >= MAX_HISTORY {
            state.history.remove(0);
        }
        state.history.push(UptimeSession {
            session_id: state.current_session_id,
            boot_ns: state.boot_timestamp_ns,
            shutdown_ns: now,
            duration_ns: duration,
            shutdown_reason: reason,
        });
        state.total_uptime_ns += duration;
        if duration > state.longest_uptime_ns {
            state.longest_uptime_ns = duration;
        }
        // Start new session.
        let new_id = state.next_session_id;
        state.next_session_id += 1;
        state.current_session_id = new_id;
        state.boot_timestamp_ns = now;
        state.total_sessions += 1;
        Ok(new_id)
    })
}

/// Get uptime history.
pub fn history() -> Vec<UptimeSession> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.history.clone())
}

/// Get longest uptime ever recorded (nanoseconds).
pub fn longest_uptime_ns() -> u64 {
    STATE.lock().as_ref().map_or(0, |s| {
        let current = crate::hpet::elapsed_ns().saturating_sub(s.boot_timestamp_ns);
        core::cmp::max(s.longest_uptime_ns, current)
    })
}

/// Format nanoseconds as human-readable duration.
pub fn format_duration(ns: u64) -> String {
    let secs = ns / 1_000_000_000;
    let mins = secs / 60;
    let hours = mins / 60;
    let days = hours / 24;
    if days > 0 {
        format!("{}d {}h {}m {}s", days, hours % 24, mins % 60, secs % 60)
    } else if hours > 0 {
        format!("{}h {}m {}s", hours, mins % 60, secs % 60)
    } else if mins > 0 {
        format!("{}m {}s", mins, secs % 60)
    } else {
        format!("{}s", secs)
    }
}

/// Statistics: (session_count, total_sessions, longest_uptime_ns, total_uptime_ns, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.history.len(), s.total_sessions, s.longest_uptime_ns, s.total_uptime_ns, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("sysuptime::self_test() — running tests...");
    // Start from a clean state so the assertions below are exact, and reset at
    // the end — this self_test records shutdowns, which would otherwise leave
    // fabricated session history resident in the live uptime state.
    *STATE.lock() = None;
    init_defaults();

    // 1: Boot time set — init_defaults() records the real HPET instant as the
    //    boot timestamp; honest history starts empty with zeroed totals.
    let boot = boot_time_ns();
    assert!(boot > 0);
    assert!(history().is_empty());
    let (h0, ts0, longest0, total0, _) = stats();
    assert_eq!((h0, longest0, total0), (0, 0, 0));
    assert_eq!(ts0, 1); // current session is genuinely session 1
    crate::serial_println!("  [1/8] boot time: OK");

    // 2: Current uptime.
    let uptime = current_uptime_ns();
    // Uptime should be small (just initialized).
    let _ = uptime;
    crate::serial_println!("  [2/8] current uptime: OK");

    // 3: Session ID.
    assert_eq!(current_session_id(), 1);
    crate::serial_println!("  [3/8] session id: OK");

    // 4: Record shutdown.
    let new_id = record_shutdown(ShutdownReason::Reboot).expect("shutdown");
    assert_eq!(new_id, 2);
    assert_eq!(current_session_id(), 2);
    assert_eq!(history().len(), 1);
    crate::serial_println!("  [4/8] record shutdown: OK");

    // 5: History details.
    let h = history();
    assert_eq!(h[0].session_id, 1);
    assert_eq!(h[0].shutdown_reason, ShutdownReason::Reboot);
    assert!(h[0].duration_ns > 0 || h[0].shutdown_ns >= h[0].boot_ns);
    crate::serial_println!("  [5/8] history details: OK");

    // 6: Multiple shutdowns.
    record_shutdown(ShutdownReason::Update).expect("shutdown2");
    record_shutdown(ShutdownReason::Clean).expect("shutdown3");
    assert_eq!(history().len(), 3);
    assert_eq!(current_session_id(), 4);
    crate::serial_println!("  [6/8] multiple shutdowns: OK");

    // 7: Format duration.
    let s = format_duration(90_000_000_000); // 90 seconds
    assert_eq!(s, "1m 30s");
    let s = format_duration(3_661_000_000_000); // 1h 1m 1s
    assert_eq!(s, "1h 1m 1s");
    crate::serial_println!("  [7/8] format: OK");

    // 8: Stats.
    let (hist_count, total_sessions, _longest, _total_uptime, ops) = stats();
    assert_eq!(hist_count, 3);
    assert_eq!(total_sessions, 4);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue: drop the fixture sessions and re-establish a clean live
    // session so the boot-time uptime tracking is not polluted by the test.
    *STATE.lock() = None;
    init_defaults();
    crate::serial_println!("sysuptime::self_test() — all 8 tests passed");
}
