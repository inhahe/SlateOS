//! Process History — process execution history tracking.
//!
//! Records process start/stop events, tracks execution duration,
//! exit codes, and provides searchable history of all processes.
//!
//! ## Architecture
//!
//! ```text
//! Process lifecycle
//!   → prochistory::record_start(name, pid) → log start
//!   → prochistory::record_exit(pid, code) → log exit + duration
//!   → prochistory::search(query) → find processes
//!
//! Integration:
//!   → perfmon (performance monitoring)
//!   → taskmon (task monitoring)
//!   → crashreport (crash reporting)
//!   → audit (security audit)
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

/// Process exit reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    Normal,
    Crashed,
    Killed,
    Timeout,
    OutOfMemory,
    Unknown,
}

impl ExitReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Crashed => "Crashed",
            Self::Killed => "Killed",
            Self::Timeout => "Timeout",
            Self::OutOfMemory => "OOM",
            Self::Unknown => "Unknown",
        }
    }
}

/// A process history entry.
#[derive(Debug, Clone)]
pub struct ProcessRecord {
    pub entry_id: u32,
    pub pid: u32,
    pub name: String,
    pub args: String,
    pub start_ns: u64,
    pub end_ns: Option<u64>,
    pub exit_code: Option<i32>,
    pub exit_reason: Option<ExitReason>,
    pub peak_memory_kb: u64,
}

impl ProcessRecord {
    pub fn duration_ms(&self) -> Option<u64> {
        self.end_ns.map(|end| (end.saturating_sub(self.start_ns)) / 1_000_000)
    }

    pub fn is_running(&self) -> bool {
        self.end_ns.is_none()
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_HISTORY: usize = 1000;

struct State {
    history: Vec<ProcessRecord>,
    next_id: u32,
    total_started: u64,
    total_exited: u64,
    total_crashed: u64,
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
        history: Vec::new(),
        next_id: 1,
        total_started: 0,
        total_exited: 0,
        total_crashed: 0,
        ops: 0,
    });
}

/// Record a process start.
pub fn record_start(name: &str, pid: u32, args: &str) -> KernelResult<u32> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        if state.history.len() >= MAX_HISTORY {
            state.history.remove(0);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.total_started += 1;
        state.history.push(ProcessRecord {
            entry_id: id, pid, name: String::from(name),
            args: String::from(args), start_ns: now,
            end_ns: None, exit_code: None, exit_reason: None,
            peak_memory_kb: 0,
        });
        Ok(id)
    })
}

/// Record a process exit.
pub fn record_exit(pid: u32, exit_code: i32, reason: ExitReason, peak_memory_kb: u64) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        // Find the most recent running entry for this PID.
        let entry = state.history.iter_mut().rev()
            .find(|e| e.pid == pid && e.end_ns.is_none())
            .ok_or(KernelError::NotFound)?;
        entry.end_ns = Some(now);
        entry.exit_code = Some(exit_code);
        entry.exit_reason = Some(reason);
        entry.peak_memory_kb = peak_memory_kb;
        state.total_exited += 1;
        if reason == ExitReason::Crashed || reason == ExitReason::OutOfMemory {
            state.total_crashed += 1;
        }
        Ok(())
    })
}

/// Search history by process name.
pub fn search(query: &str) -> Vec<ProcessRecord> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let q = query.to_lowercase();
        s.history.iter()
            .filter(|e| e.name.to_lowercase().contains(&q))
            .cloned()
            .collect()
    })
}

/// Get recent history.
pub fn recent(max: usize) -> Vec<ProcessRecord> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut h = s.history.clone();
        h.reverse();
        h.truncate(max);
        h
    })
}

/// Get running processes from history.
pub fn running() -> Vec<ProcessRecord> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.history.iter().filter(|e| e.is_running()).cloned().collect()
    })
}

/// Get crashed processes.
pub fn crashed(max: usize) -> Vec<ProcessRecord> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut crashes: Vec<_> = s.history.iter()
            .filter(|e| matches!(e.exit_reason, Some(ExitReason::Crashed) | Some(ExitReason::OutOfMemory)))
            .cloned()
            .collect();
        crashes.reverse();
        crashes.truncate(max);
        crashes
    })
}

/// Statistics: (history_size, total_started, total_exited, total_crashed, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.history.len(), s.total_started, s.total_exited, s.total_crashed, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("prochistory::self_test() — running tests...");
    init_defaults();

    // 1: Empty initially.
    assert!(recent(10).is_empty());
    crate::serial_println!("  [1/8] empty: OK");

    // 2: Record start.
    let _id1 = record_start("browser", 100, "--new-window").expect("start1");
    let _id2 = record_start("editor", 101, "file.txt").expect("start2");
    assert_eq!(recent(10).len(), 2);
    crate::serial_println!("  [2/8] start: OK");

    // 3: Running processes.
    let r = running();
    assert_eq!(r.len(), 2);
    crate::serial_println!("  [3/8] running: OK");

    // 4: Record normal exit.
    record_exit(100, 0, ExitReason::Normal, 150_000).expect("exit1");
    assert_eq!(running().len(), 1);
    let hist = recent(10);
    let browser = hist.iter().find(|e| e.name == "browser").expect("browser");
    assert_eq!(browser.exit_code, Some(0));
    assert!(browser.peak_memory_kb > 0);
    crate::serial_println!("  [4/8] normal exit: OK");

    // 5: Record crash.
    record_start("crashy", 102, "").expect("start3");
    record_exit(102, -1, ExitReason::Crashed, 50_000).expect("crash");
    let crashes = crashed(10);
    assert_eq!(crashes.len(), 1);
    assert_eq!(crashes[0].name, "crashy");
    crate::serial_println!("  [5/8] crash: OK");

    // 6: Search.
    let results = search("browser");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "browser");
    crate::serial_println!("  [6/8] search: OK");

    // 7: Duration.
    let hist = recent(10);
    let browser = hist.iter().find(|e| e.name == "browser").expect("browser2");
    assert!(browser.duration_ms().is_some());
    crate::serial_println!("  [7/8] duration: OK");

    // 8: Stats.
    let (size, started, exited, crashed_count, ops) = stats();
    assert_eq!(size, 3);
    assert_eq!(started, 3);
    assert_eq!(exited, 2);
    assert_eq!(crashed_count, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("prochistory::self_test() — all 8 tests passed");
}
