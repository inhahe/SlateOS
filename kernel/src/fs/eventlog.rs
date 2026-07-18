//! Event Log — centralized system event logging and querying.
//!
//! Provides a structured event log for kernel events, driver messages,
//! service notifications, and system diagnostics with severity-based
//! filtering and source-based querying.
//!
//! ## Architecture
//!
//! ```text
//! Event logging
//!   → eventlog::log(severity, source, message) → record event
//!   → eventlog::query(filter) → retrieve matching events
//!   → eventlog::clear(source) → remove events
//!
//! Integration:
//!   → syslog (system logging)
//!   → crashreport (crash reports)
//!   → sysdiag (system diagnostics)
//!   → audit (audit trail)
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

/// Event severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Debug,
    Info,
    Warning,
    Error,
    Critical,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warning => "WARN",
            Self::Error => "ERROR",
            Self::Critical => "CRIT",
        }
    }
}

/// Event category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventCategory {
    System,
    Security,
    Application,
    Hardware,
    Network,
    Storage,
    Driver,
    Service,
}

impl EventCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Security => "Security",
            Self::Application => "Application",
            Self::Hardware => "Hardware",
            Self::Network => "Network",
            Self::Storage => "Storage",
            Self::Driver => "Driver",
            Self::Service => "Service",
        }
    }
}

/// A single event log entry.
#[derive(Debug, Clone)]
pub struct EventEntry {
    pub id: u64,
    pub severity: Severity,
    pub category: EventCategory,
    pub source: String,
    pub message: String,
    pub timestamp_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_EVENTS: usize = 5000;

struct State {
    events: Vec<EventEntry>,
    next_id: u64,
    total_logged: u64,
    total_cleared: u64,
    total_queries: u64,
    counts_by_severity: [u64; 5], // debug, info, warn, error, critical.
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

fn severity_index(s: Severity) -> usize {
    match s {
        Severity::Debug => 0,
        Severity::Info => 1,
        Severity::Warning => 2,
        Severity::Error => 3,
        Severity::Critical => 4,
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialise an **empty** event log.
///
/// Seeds NO events and zero counters.  Entries appear only when a real
/// subsystem calls [`log_event`]; until that wiring exists, `/proc/eventlog`
/// and the `eventlog` kshell command report an empty log rather than
/// fabricated entries — the kernel's hard "never invent data in procfs" rule.
///
/// (Previously this seeded two FABRICATED entries — an Info/System/"kernel"
/// "System boot completed" and "Event log initialized", both stamped with the
/// current time — plus a fabricated `total_logged` of 2 and a
/// `counts_by_severity` of `[0, 2, 0, 0, 0]`, which `/proc/eventlog` and the
/// query/recent views then displayed as if they were real logged events.  No
/// subsystem calls [`log_event`]: the kernel's REAL system event log is the
/// separate `crate::eventlog` module behind `/proc/sysevents`, so this
/// `fs::eventlog` is an entirely unwired parallel tracker.  The self-test now
/// builds its own fixtures via the real API — see [`self_test`].)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        events: Vec::new(),
        next_id: 1,
        total_logged: 0,
        total_cleared: 0,
        total_queries: 0,
        counts_by_severity: [0; 5],
        ops: 0,
    });
}

/// Log an event.
pub fn log_event(severity: Severity, category: EventCategory, source: &str, message: &str) -> KernelResult<u64> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        if state.events.len() >= MAX_EVENTS {
            state.events.remove(0);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.events.push(EventEntry {
            id, severity, category, source: String::from(source),
            message: String::from(message), timestamp_ns: now,
        });
        state.total_logged += 1;
        state.counts_by_severity[severity_index(severity)] += 1;
        Ok(id)
    })
}

/// Query events by severity (minimum level).
pub fn query_by_severity(min_severity: Severity, max_results: usize) -> Vec<EventEntry> {
    let mut guard = STATE.lock();
    if let Some(state) = guard.as_mut() {
        state.ops += 1;
        state.total_queries += 1;
        let mut results: Vec<EventEntry> = state.events.iter()
            .filter(|e| e.severity >= min_severity)
            .cloned()
            .collect();
        results.reverse();
        results.truncate(max_results);
        results
    } else {
        Vec::new()
    }
}

/// Query events by source.
pub fn query_by_source(source: &str, max_results: usize) -> Vec<EventEntry> {
    let mut guard = STATE.lock();
    if let Some(state) = guard.as_mut() {
        state.ops += 1;
        state.total_queries += 1;
        let src_lower = source.to_lowercase();
        let mut results: Vec<EventEntry> = state.events.iter()
            .filter(|e| e.source.to_lowercase().contains(&src_lower))
            .cloned()
            .collect();
        results.reverse();
        results.truncate(max_results);
        results
    } else {
        Vec::new()
    }
}

/// Query events by category.
pub fn query_by_category(category: EventCategory, max_results: usize) -> Vec<EventEntry> {
    let mut guard = STATE.lock();
    if let Some(state) = guard.as_mut() {
        state.ops += 1;
        state.total_queries += 1;
        let mut results: Vec<EventEntry> = state.events.iter()
            .filter(|e| e.category == category)
            .cloned()
            .collect();
        results.reverse();
        results.truncate(max_results);
        results
    } else {
        Vec::new()
    }
}

/// Get recent events.
pub fn recent(max_results: usize) -> Vec<EventEntry> {
    let mut guard = STATE.lock();
    if let Some(state) = guard.as_mut() {
        state.ops += 1;
        state.total_queries += 1;
        let mut results = state.events.clone();
        results.reverse();
        results.truncate(max_results);
        results
    } else {
        Vec::new()
    }
}

/// Clear events from a specific source.
pub fn clear_source(source: &str) -> KernelResult<usize> {
    with_state(|state| {
        let before = state.events.len();
        state.events.retain(|e| e.source != source);
        let cleared = before - state.events.len();
        state.total_cleared += cleared as u64;
        Ok(cleared)
    })
}

/// Clear all events.
pub fn clear_all() -> KernelResult<usize> {
    with_state(|state| {
        let count = state.events.len();
        state.events.clear();
        state.total_cleared += count as u64;
        Ok(count)
    })
}

/// Get event count.
pub fn count() -> usize {
    STATE.lock().as_ref().map_or(0, |s| s.events.len())
}

/// Get severity counts: [debug, info, warn, error, critical].
pub fn severity_counts() -> [u64; 5] {
    STATE.lock().as_ref().map_or([0; 5], |s| s.counts_by_severity)
}

/// Statistics: (event_count, total_logged, total_cleared, total_queries, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.events.len(), s.total_logged, s.total_cleared, s.total_queries, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("eventlog::self_test() — running tests...");
    // Start from a clean slate so the fixtures built below can never leak into
    // the live /proc/eventlog registry (this module is not boot-wired, so the
    // natural state is uninitialised — `eventlog test` must leave it that way).
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no fabricated entries.
    assert_eq!(count(), 0);
    let (c0, l0, cl0, q0, _o0) = stats();
    assert_eq!((c0, l0, cl0, q0), (0, 0, 0, 0));
    assert_eq!(severity_counts(), [0; 5]);
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Log events — counters rise from zero.
    log_event(Severity::Warning, EventCategory::Hardware, "disk", "Disk temperature high").expect("log1");
    log_event(Severity::Error, EventCategory::Network, "eth0", "Link down").expect("log2");
    log_event(Severity::Debug, EventCategory::Application, "app1", "Debug trace").expect("log3");
    assert_eq!(count(), 3);
    assert_eq!(severity_counts(), [1, 0, 1, 1, 0]); // debug, info, warn, error, crit.
    crate::serial_println!("  [2/8] log: OK");

    // 3: Query by severity — warning + error are >= Warning, debug is not.
    let warnings = query_by_severity(Severity::Warning, 100);
    assert_eq!(warnings.len(), 2);
    crate::serial_println!("  [3/8] query severity: OK");

    // 4: Query by source.
    let disk_events = query_by_source("disk", 100);
    assert_eq!(disk_events.len(), 1);
    assert_eq!(disk_events[0].source, "disk");
    crate::serial_println!("  [4/8] query source: OK");

    // 5: Query by category.
    let net_events = query_by_category(EventCategory::Network, 100);
    assert_eq!(net_events.len(), 1);
    crate::serial_println!("  [5/8] query category: OK");

    // 6: Recent — most recent first (app1 was logged last).
    let rec = recent(2);
    assert_eq!(rec.len(), 2);
    assert_eq!(rec[0].source, "app1");
    crate::serial_println!("  [6/8] recent: OK");

    // 7: Clear source — removes exactly the eth0 event.
    let cleared = clear_source("eth0").expect("clear");
    assert_eq!(cleared, 1);
    assert_eq!(count(), 2);
    crate::serial_println!("  [7/8] clear source: OK");

    // 8: Stats — exact totals (2 events, 3 logged, 1 cleared, 4 queries).
    let (evt_count, logged, cleared_total, queries, ops) = stats();
    assert_eq!(evt_count, 2);
    assert_eq!(logged, 3);
    assert_eq!(cleared_total, 1);
    assert_eq!(queries, 4);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Reset so the test leaves no fixtures behind in /proc/eventlog.
    *STATE.lock() = None;

    crate::serial_println!("eventlog::self_test() — all 8 tests passed");
}
