//! Process Stats — per-process resource statistics.
//!
//! Tracks CPU time, memory usage, I/O bytes, page faults, and
//! context switches per process. Provides both instantaneous and
//! cumulative views.
//!
//! ## Architecture
//!
//! ```text
//! Process statistics
//!   → procstat::get(pid) → process stats
//!   → procstat::update(pid, ...) → update stats
//!   → procstat::top_cpu(n) → top N by CPU
//!   → procstat::top_mem(n) → top N by memory
//!
//! Integration:
//!   → taskmon (task monitor)
//!   → perfmon (performance monitor)
//!   → prochistory (process history)
//!   → oomkiller (OOM scoring)
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

/// Process state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcState {
    Running,
    Sleeping,
    Waiting,
    Stopped,
    Zombie,
}

impl ProcState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "R",
            Self::Sleeping => "S",
            Self::Waiting => "D",
            Self::Stopped => "T",
            Self::Zombie => "Z",
        }
    }
}

/// Per-process statistics.
#[derive(Debug, Clone)]
pub struct ProcessStats {
    pub pid: u32,
    pub name: String,
    pub state: ProcState,
    pub cpu_time_us: u64,
    pub user_time_us: u64,
    pub sys_time_us: u64,
    pub memory_bytes: u64,
    pub rss_pages: u64,
    pub io_read_bytes: u64,
    pub io_write_bytes: u64,
    pub page_faults: u64,
    pub ctx_switches: u64,
    pub threads: u32,
    pub started_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_PROCESSES: usize = 1024;

struct State {
    processes: Vec<ProcessStats>,
    total_updates: u64,
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

/// Initialise an **empty** per-process statistics table.
///
/// Seeds NO processes and zero counters.  Real per-process accounting is wired
/// through [`register`] (one row per live process the scheduler/loader creates)
/// and the `update_cpu`/`update_memory` functions; until those are called the
/// table is genuinely empty, so `/proc/procstat` and the `procstat` kshell
/// command report zeros rather than fabricated numbers — the kernel's hard
/// "never invent data in procfs" rule.
///
/// NOTE: this previously seeded three fictional processes ("init" pid 1: cpu
/// 5ms / mem 8KiB; "sshd" pid 100: cpu 15ms / mem 32KiB / 2 threads; "browser"
/// pid 200: cpu 500ms / mem 512KiB / 128 rss pages / 8 threads / 1MiB io read),
/// which `/proc/procstat` (and the `top_cpu`/`top_mem` views) then displayed as
/// if they were real measured per-process resource usage.  That demo data was
/// removed; the self-test now builds its own fixtures explicitly via the real
/// API (see [`self_test`]).  The process layer is expected to call [`register`]
/// when a process is created, the update functions as it runs, and
/// [`unregister`] when it exits.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        processes: Vec::new(),
        total_updates: 0,
        ops: 0,
    });
}

/// Get stats for a process.
pub fn get_process(pid: u32) -> Option<ProcessStats> {
    STATE.lock().as_ref().and_then(|s| s.processes.iter().find(|p| p.pid == pid).cloned())
}

/// List all processes.
pub fn list_processes() -> Vec<ProcessStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.processes.clone())
}

/// Register a new process.
pub fn register(pid: u32, name: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.processes.len() >= MAX_PROCESSES { return Err(KernelError::ResourceExhausted); }
        if state.processes.iter().any(|p| p.pid == pid) { return Err(KernelError::AlreadyExists); }
        let now = crate::hpet::elapsed_ns();
        state.processes.push(ProcessStats {
            pid, name: String::from(name), state: ProcState::Running,
            cpu_time_us: 0, user_time_us: 0, sys_time_us: 0,
            memory_bytes: 0, rss_pages: 0, io_read_bytes: 0,
            io_write_bytes: 0, page_faults: 0, ctx_switches: 0,
            threads: 1, started_ns: now,
        });
        Ok(())
    })
}

/// Remove a process (exited).
pub fn unregister(pid: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.processes.len();
        state.processes.retain(|p| p.pid != pid);
        if state.processes.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Update CPU time.
pub fn update_cpu(pid: u32, user_us: u64, sys_us: u64) -> KernelResult<()> {
    with_state(|state| {
        let p = state.processes.iter_mut().find(|p| p.pid == pid).ok_or(KernelError::NotFound)?;
        p.user_time_us += user_us;
        p.sys_time_us += sys_us;
        p.cpu_time_us = p.user_time_us + p.sys_time_us;
        state.total_updates += 1;
        Ok(())
    })
}

/// Update memory.
pub fn update_memory(pid: u32, bytes: u64, rss_pages: u64) -> KernelResult<()> {
    with_state(|state| {
        let p = state.processes.iter_mut().find(|p| p.pid == pid).ok_or(KernelError::NotFound)?;
        p.memory_bytes = bytes;
        p.rss_pages = rss_pages;
        state.total_updates += 1;
        Ok(())
    })
}

/// Top N by CPU time.
pub fn top_cpu(n: usize) -> Vec<ProcessStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut sorted = s.processes.clone();
        sorted.sort_by_key(|e| core::cmp::Reverse(e.cpu_time_us));
        sorted.truncate(n);
        sorted
    })
}

/// Top N by memory.
pub fn top_mem(n: usize) -> Vec<ProcessStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut sorted = s.processes.clone();
        sorted.sort_by_key(|e| core::cmp::Reverse(e.memory_bytes));
        sorted.truncate(n);
        sorted
    })
}

/// Statistics: (process_count, total_updates, ops).
pub fn stats() -> (usize, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.processes.len(), s.total_updates, s.ops),
        None => (0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("procstat::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real API,
    // so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/procstat must never surface).
    // Resetting first clears any residue from a prior `procstat test` run so the
    // totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated processes or updates.
    assert_eq!(list_processes().len(), 0);
    let (c0, u0, _o0) = stats();
    assert_eq!((c0, u0), (0, 0));
    assert!(get_process(1).is_none());
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register processes — zeroed counters, Running state; dup pid fails.
    register(100, "alpha").expect("reg1");
    register(200, "beta").expect("reg2");
    assert!(register(100, "dup").is_err());
    assert_eq!(list_processes().len(), 2);
    let p = get_process(100).expect("get");
    assert_eq!(p.name, "alpha");
    assert_eq!(p.state, ProcState::Running);
    assert_eq!((p.cpu_time_us, p.memory_bytes), (0, 0));
    crate::serial_println!("  [2/8] register: OK");

    // 3: Update CPU — user/sys accumulate, cpu_time is their sum.
    update_cpu(100, 1000, 500).expect("cpu");
    update_cpu(100, 200, 100).expect("cpu2");
    let p = get_process(100).expect("get2");
    assert_eq!(p.user_time_us, 1200);
    assert_eq!(p.sys_time_us, 600);
    assert_eq!(p.cpu_time_us, 1800);
    crate::serial_println!("  [3/8] cpu: OK");

    // 4: Update memory sets bytes + rss exactly.
    update_memory(100, 65536, 16).expect("mem");
    let p = get_process(100).expect("get3");
    assert_eq!(p.memory_bytes, 65536);
    assert_eq!(p.rss_pages, 16);
    crate::serial_println!("  [4/8] memory: OK");

    // 5: Give beta more CPU + memory so ordering is deterministic.
    update_cpu(200, 9000, 1000).expect("cpu beta");   // cpu_time 10000 > 1800
    update_memory(200, 1_000_000, 256).expect("mem beta");
    crate::serial_println!("  [5/8] second process: OK");

    // 6: Top CPU ranks beta (10000us) above alpha (1800us).
    let top = top_cpu(2);
    assert_eq!(top.len(), 2);
    assert_eq!(top[0].pid, 200);
    assert_eq!(top[1].pid, 100);
    crate::serial_println!("  [6/8] top_cpu: OK");

    // 7: Top memory ranks beta (1MB) above alpha (64KB); unknown pid → NotFound.
    let top = top_mem(2);
    assert_eq!(top[0].pid, 200);
    assert!(update_cpu(999, 1, 1).is_err());
    assert!(unregister(999).is_err());
    crate::serial_println!("  [7/8] top_mem + not found: OK");

    // 8: Unregister removes a process; counts exact.
    unregister(100).expect("unreg");
    assert_eq!(list_processes().len(), 1);
    let (count, updates, ops) = stats();
    assert_eq!(count, 1);
    assert_eq!(updates, 5); // 2 cpu + 1 mem (alpha) + 1 cpu + 1 mem (beta)
    assert!(ops > 0);
    crate::serial_println!("  [8/8] unregister+stats: OK");

    // Leave NO residue: reset to the uninitialised state so a diagnostic run
    // never leaves fixtures resident in the live /proc/procstat table.
    *STATE.lock() = None;

    crate::serial_println!("procstat::self_test() — all 8 tests passed");
}
