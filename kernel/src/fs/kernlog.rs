//! Kernel Log — kernel message ring buffer.
//!
//! Maintains a ring buffer of kernel log messages (like dmesg),
//! with severity levels, timestamps, and source tracking.
//!
//! ## Architecture
//!
//! ```text
//! Kernel logging
//!   → kernlog::log(level, source, msg) → add message
//!   → kernlog::read(from_seq) → read new messages
//!   → kernlog::dmesg() → dump all messages
//!
//! Integration:
//!   → syslog (system logging)
//!   → eventlog (event logging)
//!   → crashreport (crash reporting)
//!   → sysdiag (diagnostics)
//! ```

#![allow(dead_code)]

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Log level/severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Emergency = 0,
    Alert = 1,
    Critical = 2,
    Error = 3,
    Warning = 4,
    Notice = 5,
    Info = 6,
    Debug = 7,
}

impl LogLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Emergency => "EMERG",
            Self::Alert => "ALERT",
            Self::Critical => "CRIT",
            Self::Error => "ERR",
            Self::Warning => "WARN",
            Self::Notice => "NOTICE",
            Self::Info => "INFO",
            Self::Debug => "DEBUG",
        }
    }
}

/// A kernel log entry.
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub seq: u64,
    pub timestamp_ns: u64,
    pub level: LogLevel,
    pub source: String,
    pub message: String,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const RING_SIZE: usize = 4096;

struct State {
    ring: Vec<LogEntry>,
    next_seq: u64,
    total_logged: u64,
    total_dropped: u64,
    min_level: LogLevel,
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
    if guard.is_some() {
        return;
    }
    let now = crate::hpet::elapsed_ns();
    let mut state = State {
        ring: Vec::with_capacity(256),
        next_seq: 1,
        total_logged: 0,
        total_dropped: 0,
        min_level: LogLevel::Debug,
        ops: 0,
    };
    // Boot message.
    state.ring.push(LogEntry {
        seq: 0,
        timestamp_ns: now,
        level: LogLevel::Info,
        source: String::from("kernel"),
        message: String::from("Kernel log initialized"),
    });
    state.total_logged = 1;
    *guard = Some(state);
}

/// Log a message.
pub fn log(level: LogLevel, source: &str, message: &str) -> KernelResult<u64> {
    with_state(|state| {
        if (level as u8) > (state.min_level as u8) {
            return Ok(0); // Filtered.
        }
        let now = crate::hpet::elapsed_ns();
        let seq = state.next_seq;
        state.next_seq += 1;
        if state.ring.len() >= RING_SIZE {
            state.ring.remove(0);
            state.total_dropped += 1;
        }
        state.ring.push(LogEntry {
            seq,
            timestamp_ns: now,
            level,
            source: String::from(source),
            message: String::from(message),
        });
        state.total_logged += 1;
        Ok(seq)
    })
}

/// Read messages from a sequence number.
pub fn read_from(from_seq: u64) -> Vec<LogEntry> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.ring
            .iter()
            .filter(|e| e.seq >= from_seq)
            .cloned()
            .collect()
    })
}

/// Get all messages (like dmesg).
pub fn dmesg() -> Vec<LogEntry> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.ring.clone())
}

/// Get last N messages.
pub fn tail(n: usize) -> Vec<LogEntry> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let start = s.ring.len().saturating_sub(n);
        s.ring[start..].to_vec()
    })
}

/// Filter by level.
pub fn filter_level(level: LogLevel) -> Vec<LogEntry> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.ring
            .iter()
            .filter(|e| e.level == level)
            .cloned()
            .collect()
    })
}

/// Filter by source.
pub fn filter_source(source: &str) -> Vec<LogEntry> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.ring
            .iter()
            .filter(|e| e.source == source)
            .cloned()
            .collect()
    })
}

/// Set minimum log level.
pub fn set_min_level(level: LogLevel) -> KernelResult<()> {
    with_state(|state| {
        state.min_level = level;
        Ok(())
    })
}

/// Clear all messages.
pub fn clear() -> KernelResult<()> {
    with_state(|state| {
        state.ring.clear();
        Ok(())
    })
}

/// Current message count.
pub fn count() -> usize {
    STATE.lock().as_ref().map_or(0, |s| s.ring.len())
}

/// Statistics: (message_count, total_logged, total_dropped, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.ring.len(), s.total_logged, s.total_dropped, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run the module's self-test suite against a table of its own.
///
/// The suite mutates module state and asserts exact contents, and it used to
/// do that to the *live* table -- which, since it is also a kernel-shell
/// subcommand, changed or destroyed whatever the user had here and then
/// reported success.  The live state is moved aside for the duration and put
/// back afterwards; `crate::fs::selftest` records why this shape rather than
/// the alternatives.
///
/// The pristine value is `None` rather than a table: this module initialises
/// lazily, and `None` is exactly what a fresh boot holds.
pub fn self_test() {
    // `OPS` is a lock-free mirror of `state.ops`, which lives *inside* the
    // table. `with_pristine` restores the table and so restores `state.ops`,
    // but it cannot know about the mirror -- leave it and the two disagree
    // permanently, with `<module> stats` reporting the suite's activity as
    // the user's.
    let saved_ops = OPS.load(Ordering::Relaxed);
    crate::fs::selftest::with_pristine(&STATE, None, self_test_inner);
    OPS.store(saved_ops, Ordering::Relaxed);
}

fn self_test_inner() {
    crate::serial_println!("kernlog::self_test() — running tests...");
    init_defaults();

    // 1: Boot message.
    assert_eq!(count(), 1);
    let msgs = dmesg();
    assert_eq!(msgs[0].source, "kernel");
    crate::serial_println!("  [1/8] boot message: OK");

    // 2: Log message.
    let seq = log(LogLevel::Info, "test", "Hello from test").expect("log");
    assert!(seq > 0);
    assert_eq!(count(), 2);
    crate::serial_println!("  [2/8] log: OK");

    // 3: Multiple levels.
    log(LogLevel::Warning, "fs", "Disk space low").expect("warn");
    log(LogLevel::Error, "net", "Connection refused").expect("err");
    log(LogLevel::Debug, "sched", "Task switched").expect("dbg");
    assert_eq!(count(), 5);
    crate::serial_println!("  [3/8] levels: OK");

    // 4: Read from seq.
    //
    // `seq` is 1 — the first sequence `log` hands out, since `next_seq` starts
    // at 1 while the boot message is seq 0 — so this is every entry bar the
    // boot message: exactly the four the suite logged. Stated exactly rather
    // than as `>= 4`, which is true of any ring that has ever been written to
    // and so says nothing about `read_from`'s filtering.
    let recent = read_from(seq);
    assert_eq!(recent.len(), 4, "everything after the boot message");
    crate::serial_println!("  [4/8] read from: OK");

    // 5: Tail.
    let last2 = tail(2);
    assert_eq!(last2.len(), 2);
    crate::serial_println!("  [5/8] tail: OK");

    // 6: Filter by level.
    let warnings = filter_level(LogLevel::Warning);
    assert_eq!(warnings.len(), 1);
    let errors = filter_level(LogLevel::Error);
    assert_eq!(errors.len(), 1);
    crate::serial_println!("  [6/8] filter level: OK");

    // 7: Filter by source.
    let net_msgs = filter_source("net");
    assert_eq!(net_msgs.len(), 1);
    crate::serial_println!("  [7/8] filter source: OK");

    // 8: Stats.
    //
    // `total_logged` is 5, not 6: `init_defaults` pushes the boot message and
    // sets the counter to 1, and the suite logs four more. This asserted
    // `>= 6` and failed the first time it was ever run — the boot message got
    // counted twice, which nothing caught because on the live table the suite
    // ran against before `58117e72c` the counter was already far past 6.
    //
    // `total_dropped` is 0 and is worth saying so rather than discarding:
    // `log` only drops once the ring reaches RING_SIZE (4096), so a non-zero
    // count here would mean the ring is not the size it claims. That is the
    // one thing this test can tell us that `count()` cannot.
    let (msg_count, total, dropped, ops) = stats();
    assert_eq!(msg_count, 5);
    assert_eq!(total, 5, "the boot message plus the four the suite logged");
    assert_eq!(dropped, 0, "five entries cannot overflow a 4096-slot ring");
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("kernlog::self_test() — all 8 tests passed");
}
