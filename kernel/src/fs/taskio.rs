//! Per-Task I/O Statistics — process-level I/O accounting.
//!
//! Tracks per-task read/write bytes, syscall counts, cancelled
//! writes, and I/O wait time. Essential for identifying I/O-heavy
//! processes and resource accounting.
//!
//! ## Architecture
//!
//! ```text
//! Per-task I/O monitoring
//!   → taskio::register(pid) → register task
//!   → taskio::record_read(pid, bytes) → read accounting
//!   → taskio::record_write(pid, bytes) → write accounting
//!   → taskio::record_cancelled(pid, bytes) → cancelled write
//!   → taskio::per_task() → per-task stats
//!
//! Integration:
//!   → taskstats (task statistics)
//!   → procstat (process stats)
//!   → diskstat (disk stats)
//!   → iolatency (I/O latency)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Per-task I/O stats.
#[derive(Debug, Clone)]
pub struct TaskIoStats {
    pub pid: u32,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub read_syscalls: u64,
    pub write_syscalls: u64,
    pub cancelled_write_bytes: u64,
    pub io_wait_ns: u64,
    pub page_faults_io: u64,  // Major page faults (disk I/O)
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_TASKS: usize = 1024;

struct State {
    tasks: Vec<TaskIoStats>,
    total_read_bytes: u64,
    total_write_bytes: u64,
    total_cancelled: u64,
    total_io_wait_ns: u64,
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

/// Initialise an **empty** per-task I/O table.
///
/// Seeds NO tasks and zero counters.  Real per-task accounting is wired through
/// [`register`] (one row per process the scheduler/process layer tracks) and the
/// `record_read`/`record_write`/`record_cancelled`/`record_io_wait`/
/// `record_page_fault_io` functions; until those are called the table is
/// genuinely empty, so `/proc/taskio` and the `taskio` kshell command report
/// zeros rather than fabricated numbers — the kernel's hard "never invent data in
/// procfs" rule.
///
/// NOTE: this previously seeded three fictional tasks (pid 1: 500MB read / 200MB
/// write / 1M read syscalls / 500k write syscalls / 10MB cancelled / 50s io_wait
/// / 50k major faults; pid 100: 2GB read / 1GB write / 5M+3M syscalls / 50MB
/// cancelled / 200s io_wait / 200k faults; pid 200: 100MB read / 50MB write /
/// 200k+100k syscalls / 1MB cancelled / 10s io_wait / 10k faults) plus invented
/// aggregate totals (total_read_bytes 2.6GB, total_write_bytes 1.25GB,
/// total_cancelled 61MB, total_io_wait_ns 260s), which `/proc/taskio` (and the
/// `per_task` view) then displayed as if they were real measured per-process I/O.
/// That demo data was removed; the self-test now builds its own fixtures
/// explicitly via the real API (see [`self_test`]).  The process layer is
/// expected to call [`register`] when a task starts and the record functions on
/// every I/O event.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        tasks: Vec::new(),
        total_read_bytes: 0,
        total_write_bytes: 0,
        total_cancelled: 0,
        total_io_wait_ns: 0,
        ops: 0,
    });
}

/// Register a task.
pub fn register(pid: u32) -> KernelResult<()> {
    with_state(|state| {
        if state.tasks.len() >= MAX_TASKS { return Err(KernelError::ResourceExhausted); }
        if state.tasks.iter().any(|t| t.pid == pid) { return Err(KernelError::AlreadyExists); }
        state.tasks.push(TaskIoStats {
            pid, read_bytes: 0, write_bytes: 0, read_syscalls: 0,
            write_syscalls: 0, cancelled_write_bytes: 0, io_wait_ns: 0,
            page_faults_io: 0,
        });
        Ok(())
    })
}

/// Unregister a task.
pub fn unregister(pid: u32) -> KernelResult<()> {
    with_state(|state| {
        let idx = state.tasks.iter().position(|t| t.pid == pid)
            .ok_or(KernelError::NotFound)?;
        state.tasks.remove(idx);
        Ok(())
    })
}

/// Record a read.
pub fn record_read(pid: u32, bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        let t = state.tasks.iter_mut().find(|t| t.pid == pid)
            .ok_or(KernelError::NotFound)?;
        t.read_bytes += bytes;
        t.read_syscalls += 1;
        state.total_read_bytes += bytes;
        Ok(())
    })
}

/// Record a write.
pub fn record_write(pid: u32, bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        let t = state.tasks.iter_mut().find(|t| t.pid == pid)
            .ok_or(KernelError::NotFound)?;
        t.write_bytes += bytes;
        t.write_syscalls += 1;
        state.total_write_bytes += bytes;
        Ok(())
    })
}

/// Record cancelled write bytes.
pub fn record_cancelled(pid: u32, bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        let t = state.tasks.iter_mut().find(|t| t.pid == pid)
            .ok_or(KernelError::NotFound)?;
        t.cancelled_write_bytes += bytes;
        state.total_cancelled += bytes;
        Ok(())
    })
}

/// Record I/O wait time.
pub fn record_io_wait(pid: u32, ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let t = state.tasks.iter_mut().find(|t| t.pid == pid)
            .ok_or(KernelError::NotFound)?;
        t.io_wait_ns += ns;
        state.total_io_wait_ns += ns;
        Ok(())
    })
}

/// Record a major page fault.
pub fn record_page_fault_io(pid: u32) -> KernelResult<()> {
    with_state(|state| {
        let t = state.tasks.iter_mut().find(|t| t.pid == pid)
            .ok_or(KernelError::NotFound)?;
        t.page_faults_io += 1;
        Ok(())
    })
}

/// Per-task stats.
pub fn per_task() -> Vec<TaskIoStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.tasks.clone())
}

/// Statistics: (task_count, total_read_bytes, total_write_bytes, total_cancelled, total_io_wait_ns, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.tasks.len(), s.total_read_bytes, s.total_write_bytes, s.total_cancelled, s.total_io_wait_ns, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("taskio::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real API,
    // so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/taskio must never surface).  Resetting
    // first clears any residue from a prior `taskio test` run so the totals
    // asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated tasks or counters; record on an
    // unregistered pid fails.
    assert_eq!(per_task().len(), 0);
    let (c0, rb0, wb0, can0, iow0, _o0) = stats();
    assert_eq!((c0, rb0, wb0, can0, iow0), (0, 0, 0, 0, 0));
    assert!(record_read(500, 1).is_err()); // no phantom task exists yet
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register — zeroed counters; dup fails.
    register(500).expect("register");
    let t = per_task().into_iter().find(|t| t.pid == 500).expect("find");
    assert_eq!((t.read_bytes, t.write_bytes, t.io_wait_ns), (0, 0, 0));
    assert!(register(500).is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Read — bytes accumulate, syscall count rises once per call.
    record_read(500, 4096).expect("read");
    record_read(500, 4096).expect("read2");
    let t = per_task().into_iter().find(|t| t.pid == 500).expect("find");
    assert_eq!(t.read_bytes, 8192);
    assert_eq!(t.read_syscalls, 2);
    crate::serial_println!("  [3/8] read: OK");

    // 4: Write — bytes + syscall count.
    record_write(500, 8192).expect("write");
    let t = per_task().into_iter().find(|t| t.pid == 500).expect("find");
    assert_eq!(t.write_bytes, 8192);
    assert_eq!(t.write_syscalls, 1);
    crate::serial_println!("  [4/8] write: OK");

    // 5: Cancelled + io_wait + major fault — independent counters.
    record_cancelled(500, 1024).expect("cancel");
    record_io_wait(500, 5000).expect("wait");
    record_page_fault_io(500).expect("fault");
    let t = per_task().into_iter().find(|t| t.pid == 500).expect("find");
    assert_eq!(t.cancelled_write_bytes, 1024);
    assert_eq!(t.io_wait_ns, 5000);
    assert_eq!(t.page_faults_io, 1);
    crate::serial_println!("  [5/8] cancelled/wait/fault: OK");

    // 6: Unregister — row removed; second unregister fails; record then fails.
    unregister(500).expect("unregister");
    assert_eq!(per_task().len(), 0);
    assert!(unregister(500).is_err());
    assert!(record_read(500, 1).is_err());
    crate::serial_println!("  [6/8] unregister: OK");

    // 7: Unknown task → NotFound on every record path.
    assert!(record_read(999, 1).is_err());
    assert!(record_write(999, 1).is_err());
    assert!(record_cancelled(999, 1).is_err());
    assert!(record_io_wait(999, 1).is_err());
    assert!(record_page_fault_io(999).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Aggregate totals persist across unregister: 8192 read, 8192 write,
    // 1024 cancelled, 5000 io_wait (per-task rows are gone but totals are
    // monotonic lifetime counters).
    let (tasks, rb, wb, cancelled, io_wait, ops) = stats();
    assert_eq!(tasks, 0);
    assert_eq!(rb, 8192);
    assert_eq!(wb, 8192);
    assert_eq!(cancelled, 1024);
    assert_eq!(io_wait, 5000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: reset to the uninitialised state so a diagnostic run
    // never leaves fixtures resident in the live /proc/taskio table.
    *STATE.lock() = None;

    crate::serial_println!("taskio::self_test() — all 8 tests passed");
}
