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

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    let now = crate::hpet::elapsed_ns();
    *guard = Some(State {
        processes: alloc::vec![
            ProcessStats {
                pid: 1, name: String::from("init"), state: ProcState::Sleeping,
                cpu_time_us: 5000, user_time_us: 2000, sys_time_us: 3000,
                memory_bytes: 8192, rss_pages: 2, io_read_bytes: 4096,
                io_write_bytes: 1024, page_faults: 10, ctx_switches: 50,
                threads: 1, started_ns: now,
            },
            ProcessStats {
                pid: 100, name: String::from("sshd"), state: ProcState::Sleeping,
                cpu_time_us: 15000, user_time_us: 8000, sys_time_us: 7000,
                memory_bytes: 32768, rss_pages: 8, io_read_bytes: 65536,
                io_write_bytes: 16384, page_faults: 25, ctx_switches: 200,
                threads: 2, started_ns: now,
            },
            ProcessStats {
                pid: 200, name: String::from("browser"), state: ProcState::Running,
                cpu_time_us: 500000, user_time_us: 400000, sys_time_us: 100000,
                memory_bytes: 524288, rss_pages: 128, io_read_bytes: 1048576,
                io_write_bytes: 262144, page_faults: 500, ctx_switches: 5000,
                threads: 8, started_ns: now,
            },
        ],
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
        sorted.sort_by(|a, b| b.cpu_time_us.cmp(&a.cpu_time_us));
        sorted.truncate(n);
        sorted
    })
}

/// Top N by memory.
pub fn top_mem(n: usize) -> Vec<ProcessStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut sorted = s.processes.clone();
        sorted.sort_by(|a, b| b.memory_bytes.cmp(&a.memory_bytes));
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(list_processes().len(), 3);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Get process.
    let p = get_process(200).expect("get");
    assert_eq!(p.name, "browser");
    assert_eq!(p.state, ProcState::Running);
    crate::serial_println!("  [2/8] get: OK");

    // 3: Register.
    register(500, "test_app").expect("reg");
    assert_eq!(list_processes().len(), 4);
    assert!(register(500, "dup").is_err());
    crate::serial_println!("  [3/8] register: OK");

    // 4: Update CPU.
    update_cpu(500, 1000, 500).expect("cpu");
    let p = get_process(500).expect("get2");
    assert_eq!(p.cpu_time_us, 1500);
    assert_eq!(p.user_time_us, 1000);
    crate::serial_println!("  [4/8] cpu: OK");

    // 5: Update memory.
    update_memory(500, 65536, 16).expect("mem");
    let p = get_process(500).expect("get3");
    assert_eq!(p.memory_bytes, 65536);
    crate::serial_println!("  [5/8] memory: OK");

    // 6: Top CPU.
    let top = top_cpu(2);
    assert_eq!(top.len(), 2);
    assert_eq!(top[0].pid, 200); // browser has most CPU time.
    crate::serial_println!("  [6/8] top_cpu: OK");

    // 7: Top memory.
    let top = top_mem(2);
    assert_eq!(top.len(), 2);
    assert_eq!(top[0].pid, 200); // browser has most memory.
    crate::serial_println!("  [7/8] top_mem: OK");

    // 8: Unregister + stats.
    unregister(500).expect("unreg");
    assert_eq!(list_processes().len(), 3);
    let (count, updates, ops) = stats();
    assert_eq!(count, 3);
    assert!(updates >= 2);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] unregister+stats: OK");

    crate::serial_println!("procstat::self_test() — all 8 tests passed");
}
