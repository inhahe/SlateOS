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

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        tasks: alloc::vec![
            TaskIoStats { pid: 1, read_bytes: 500_000_000, write_bytes: 200_000_000, read_syscalls: 1_000_000, write_syscalls: 500_000, cancelled_write_bytes: 10_000_000, io_wait_ns: 50_000_000_000, page_faults_io: 50_000 },
            TaskIoStats { pid: 100, read_bytes: 2_000_000_000, write_bytes: 1_000_000_000, read_syscalls: 5_000_000, write_syscalls: 3_000_000, cancelled_write_bytes: 50_000_000, io_wait_ns: 200_000_000_000, page_faults_io: 200_000 },
            TaskIoStats { pid: 200, read_bytes: 100_000_000, write_bytes: 50_000_000, read_syscalls: 200_000, write_syscalls: 100_000, cancelled_write_bytes: 1_000_000, io_wait_ns: 10_000_000_000, page_faults_io: 10_000 },
        ],
        total_read_bytes: 2_600_000_000,
        total_write_bytes: 1_250_000_000,
        total_cancelled: 61_000_000,
        total_io_wait_ns: 260_000_000_000,
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_task().len(), 3);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Register.
    register(500).expect("register");
    assert_eq!(per_task().len(), 4);
    assert!(register(500).is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Read.
    record_read(500, 4096).expect("read");
    let t = per_task().iter().find(|t| t.pid == 500).cloned().unwrap();
    assert_eq!(t.read_bytes, 4096);
    assert_eq!(t.read_syscalls, 1);
    crate::serial_println!("  [3/8] read: OK");

    // 4: Write.
    record_write(500, 8192).expect("write");
    let t = per_task().iter().find(|t| t.pid == 500).cloned().unwrap();
    assert_eq!(t.write_bytes, 8192);
    assert_eq!(t.write_syscalls, 1);
    crate::serial_println!("  [4/8] write: OK");

    // 5: Cancelled.
    record_cancelled(500, 1024).expect("cancel");
    let t = per_task().iter().find(|t| t.pid == 500).cloned().unwrap();
    assert_eq!(t.cancelled_write_bytes, 1024);
    crate::serial_println!("  [5/8] cancelled: OK");

    // 6: IO wait.
    record_io_wait(500, 5000).expect("wait");
    let t = per_task().iter().find(|t| t.pid == 500).cloned().unwrap();
    assert_eq!(t.io_wait_ns, 5000);
    crate::serial_println!("  [6/8] io wait: OK");

    // 7: Unregister.
    unregister(500).expect("unregister");
    assert_eq!(per_task().len(), 3);
    assert!(unregister(500).is_err());
    crate::serial_println!("  [7/8] unregister: OK");

    // 8: Stats.
    let (tasks, rb, wb, cancelled, _io_wait, ops) = stats();
    assert_eq!(tasks, 3);
    assert!(rb > 2_600_000_000);
    assert!(wb > 1_250_000_000);
    assert!(cancelled > 61_000_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("taskio::self_test() — all 8 tests passed");
}
