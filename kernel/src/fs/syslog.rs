//! System log viewer — structured log aggregation and query.
//!
//! Collects kernel and system service log entries, provides filtering
//! by severity/source/time, and supports real-time tailing.
//!
//! ## Architecture
//!
//! ```text
//! Kernel subsystems / services
//!   → syslog::log(severity, source, message)
//!
//! Settings panel → Logs / System Diagnostics
//!   → syslog::query(filter) → filtered entries
//!   → syslog::tail(count) → most recent entries
//!
//! Integration:
//!   → sysdiag (system diagnostics)
//!   → crashreport (crash log correlation)
//!   → audit (security audit events)
//!   → journal (filesystem journal events)
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

/// Log severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Debug,
    Info,
    Notice,
    Warning,
    Error,
    Critical,
    Alert,
    Emergency,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Notice => "NOTICE",
            Self::Warning => "WARN",
            Self::Error => "ERROR",
            Self::Critical => "CRIT",
            Self::Alert => "ALERT",
            Self::Emergency => "EMERG",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "debug" | "dbg" => Some(Self::Debug),
            "info" => Some(Self::Info),
            "notice" => Some(Self::Notice),
            "warn" | "warning" => Some(Self::Warning),
            "error" | "err" => Some(Self::Error),
            "crit" | "critical" => Some(Self::Critical),
            "alert" => Some(Self::Alert),
            "emerg" | "emergency" => Some(Self::Emergency),
            _ => None,
        }
    }
}

/// A log entry.
#[derive(Debug, Clone)]
pub struct LogEntry {
    /// Sequential ID.
    pub id: u64,
    /// Severity level.
    pub severity: Severity,
    /// Source subsystem/service.
    pub source: String,
    /// Log message.
    pub message: String,
    /// Timestamp (ns since boot).
    pub timestamp_ns: u64,
    /// Process ID (0 = kernel).
    pub pid: u32,
}

/// Log query filter.
#[derive(Debug, Clone, Default)]
pub struct LogFilter {
    /// Minimum severity (inclusive).
    pub min_severity: Option<Severity>,
    /// Source substring match.
    pub source: Option<String>,
    /// Message substring match.
    pub message: Option<String>,
    /// After this timestamp.
    pub after_ns: Option<u64>,
    /// Max results.
    pub limit: Option<usize>,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_ENTRIES: usize = 10_000;

struct State {
    entries: Vec<LogEntry>,
    next_id: u64,
    total_logged: u64,
    dropped: u64,
    /// Min severity to keep (entries below this are dropped).
    min_keep: Severity,
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

    // Seed with a boot message.
    let boot_entry = LogEntry {
        id: 1,
        severity: Severity::Info,
        source: String::from("kernel"),
        message: String::from("System log initialized"),
        timestamp_ns: crate::hpet::elapsed_ns(),
        pid: 0,
    };

    *guard = Some(State {
        entries: alloc::vec![boot_entry],
        next_id: 2,
        total_logged: 1,
        dropped: 0,
        min_keep: Severity::Debug,
        ops: 0,
    });
}

/// Log a message.
pub fn log(severity: Severity, source: &str, message: &str) -> KernelResult<u64> {
    with_state(|state| {
        if severity < state.min_keep {
            state.dropped += 1;
            return Ok(0);
        }

        let id = state.next_id;
        state.next_id += 1;
        state.total_logged += 1;

        state.entries.push(LogEntry {
            id,
            severity,
            source: String::from(source),
            message: String::from(message),
            timestamp_ns: crate::hpet::elapsed_ns(),
            pid: 0,
        });

        // Evict oldest when full.
        while state.entries.len() > MAX_ENTRIES {
            state.entries.remove(0);
        }

        Ok(id)
    })
}

/// Log with PID.
pub fn log_with_pid(severity: Severity, source: &str, message: &str, pid: u32) -> KernelResult<u64> {
    with_state(|state| {
        if severity < state.min_keep {
            state.dropped += 1;
            return Ok(0);
        }

        let id = state.next_id;
        state.next_id += 1;
        state.total_logged += 1;

        state.entries.push(LogEntry {
            id, severity,
            source: String::from(source),
            message: String::from(message),
            timestamp_ns: crate::hpet::elapsed_ns(),
            pid,
        });

        while state.entries.len() > MAX_ENTRIES {
            state.entries.remove(0);
        }

        Ok(id)
    })
}

/// Get the most recent N entries.
pub fn tail(count: usize) -> Vec<LogEntry> {
    let guard = STATE.lock();
    guard.as_ref().map_or(Vec::new(), |s| {
        let start = if s.entries.len() > count { s.entries.len() - count } else { 0 };
        s.entries[start..].to_vec()
    })
}

/// Query entries with a filter.
pub fn query(filter: &LogFilter) -> Vec<LogEntry> {
    let guard = STATE.lock();
    guard.as_ref().map_or(Vec::new(), |s| {
        let limit = filter.limit.unwrap_or(100);
        s.entries.iter()
            .filter(|e| {
                if let Some(min) = filter.min_severity {
                    if e.severity < min { return false; }
                }
                if let Some(ref src) = filter.source {
                    if !e.source.contains(src.as_str()) { return false; }
                }
                if let Some(ref msg) = filter.message {
                    if !e.message.contains(msg.as_str()) { return false; }
                }
                if let Some(after) = filter.after_ns {
                    if e.timestamp_ns < after { return false; }
                }
                true
            })
            .take(limit)
            .cloned()
            .collect()
    })
}

/// Count entries by severity.
pub fn count_by_severity() -> Vec<(Severity, usize)> {
    let guard = STATE.lock();
    guard.as_ref().map_or(Vec::new(), |s| {
        let sevs = [
            Severity::Debug, Severity::Info, Severity::Notice, Severity::Warning,
            Severity::Error, Severity::Critical, Severity::Alert, Severity::Emergency,
        ];
        sevs.iter().map(|sev| {
            let count = s.entries.iter().filter(|e| e.severity == *sev).count();
            (*sev, count)
        }).filter(|(_, c)| *c > 0).collect()
    })
}

/// Set minimum severity to keep.
pub fn set_min_severity(severity: Severity) -> KernelResult<()> {
    with_state(|state| { state.min_keep = severity; Ok(()) })
}

/// Clear all entries.
pub fn clear() -> KernelResult<usize> {
    with_state(|state| {
        let count = state.entries.len();
        state.entries.clear();
        Ok(count)
    })
}

/// Statistics: (entry_count, total_logged, dropped, error_count, critical_count, ops).
pub fn stats() -> (usize, u64, u64, usize, usize, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let errors = s.entries.iter().filter(|e| e.severity >= Severity::Error).count();
            let crits = s.entries.iter().filter(|e| e.severity >= Severity::Critical).count();
            (s.entries.len(), s.total_logged, s.dropped, errors, crits, s.ops)
        }
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("syslog::self_test() — running tests...");
    init_defaults();

    // 1: Boot message exists.
    let entries = tail(10);
    assert!(!entries.is_empty());
    crate::serial_println!("  [1/11] boot message: OK");

    // 2: Log info.
    let id = log(Severity::Info, "test", "Hello from self-test").expect("log info");
    assert!(id > 0);
    crate::serial_println!("  [2/11] log info: OK");

    // 3: Log error.
    log(Severity::Error, "test", "Something went wrong").expect("log error");
    log(Severity::Warning, "scheduler", "High CPU usage").expect("log warn");
    crate::serial_println!("  [3/11] log error/warn: OK");

    // 4: Tail.
    let recent = tail(2);
    assert_eq!(recent.len(), 2);
    crate::serial_println!("  [4/11] tail: OK");

    // 5: Query by severity.
    let filter = LogFilter { min_severity: Some(Severity::Error), ..Default::default() };
    let results = query(&filter);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].severity, Severity::Error);
    crate::serial_println!("  [5/11] query severity: OK");

    // 6: Query by source.
    let filter = LogFilter { source: Some(String::from("scheduler")), ..Default::default() };
    let results = query(&filter);
    assert_eq!(results.len(), 1);
    crate::serial_println!("  [6/11] query source: OK");

    // 7: Query by message.
    let filter = LogFilter { message: Some(String::from("Hello")), ..Default::default() };
    let results = query(&filter);
    assert_eq!(results.len(), 1);
    crate::serial_println!("  [7/11] query message: OK");

    // 8: Count by severity.
    let counts = count_by_severity();
    assert!(!counts.is_empty());
    crate::serial_println!("  [8/11] count by severity: OK");

    // 9: Log with PID.
    log_with_pid(Severity::Info, "app", "App started", 1234).expect("log pid");
    let recent = tail(1);
    assert_eq!(recent[0].pid, 1234);
    crate::serial_println!("  [9/11] log with PID: OK");

    // 10: Set min severity.
    set_min_severity(Severity::Warning).expect("set min");
    log(Severity::Debug, "test", "This should be dropped").expect("log debug");
    let (_, _, dropped, _, _, _) = stats();
    assert!(dropped >= 1);
    crate::serial_println!("  [10/11] min severity filter: OK");

    // 11: Stats.
    let (count, total, dropped, errors, crits, ops) = stats();
    assert!(count >= 4);
    assert!(total >= 5);
    assert!(errors >= 1);
    assert!(ops > 0);
    let _ = (dropped, crits);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("syslog::self_test() — all 11 tests passed");
}
