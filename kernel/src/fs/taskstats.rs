//! Task Statistics — per-task comprehensive accounting.
//!
//! Tracks per-task CPU time, memory usage, I/O bytes,
//! scheduling delays, and page faults. Provides the kernel
//! equivalent of Linux's taskstats interface.
//!
//! ## Architecture
//!
//! ```text
//! Task statistics
//!   → taskstats::update_cpu(pid, ns) → record CPU time
//!   → taskstats::update_io(pid, read, write) → record I/O
//!   → taskstats::update_delay(pid, type, ns) → record delay
//!   → taskstats::get(pid) → full task accounting
//!
//! Integration:
//!   → procstat (process statistics)
//!   → schedclass (scheduler class)
//!   → pftrack (page fault tracking)
//!   → memcg (memory cgroup)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Delay accounting type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelayType {
    CpuWait,
    BlockIo,
    SwapIn,
    FreePage,
    Thrashing,
    Compaction,
}

impl DelayType {
    pub fn label(self) -> &'static str {
        match self {
            Self::CpuWait => "cpu_wait",
            Self::BlockIo => "blkio",
            Self::SwapIn => "swapin",
            Self::FreePage => "freepg",
            Self::Thrashing => "thrash",
            Self::Compaction => "compact",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::CpuWait => 0,
            Self::BlockIo => 1,
            Self::SwapIn => 2,
            Self::FreePage => 3,
            Self::Thrashing => 4,
            Self::Compaction => 5,
        }
    }
}

/// Per-task accounting data.
#[derive(Debug, Clone)]
pub struct TaskAccounting {
    pub pid: u32,
    pub name: String,
    pub cpu_time_ns: u64,
    pub user_time_ns: u64,
    pub sys_time_ns: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub read_syscalls: u64,
    pub write_syscalls: u64,
    pub rss_pages: u64,
    pub vm_pages: u64,
    pub minor_faults: u64,
    pub major_faults: u64,
    pub voluntary_switches: u64,
    pub involuntary_switches: u64,
    pub delays_ns: [u64; 6],
    pub delay_counts: [u64; 6],
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_TASKS: usize = 512;

struct State {
    tasks: Vec<TaskAccounting>,
    total_cpu_ns: u64,
    total_io_bytes: u64,
    total_delays_ns: u64,
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
            TaskAccounting {
                pid: 1, name: String::from("init"), cpu_time_ns: 5_000_000_000,
                user_time_ns: 2_000_000_000, sys_time_ns: 3_000_000_000,
                read_bytes: 1_073_741_824, write_bytes: 536_870_912,
                read_syscalls: 100000, write_syscalls: 50000,
                rss_pages: 1024, vm_pages: 4096,
                minor_faults: 50000, major_faults: 100,
                voluntary_switches: 200000, involuntary_switches: 10000,
                delays_ns: [1_000_000_000, 500_000_000, 100_000_000, 50_000_000, 0, 0],
                delay_counts: [10000, 5000, 1000, 500, 0, 0],
            },
            TaskAccounting {
                pid: 100, name: String::from("shell"), cpu_time_ns: 1_000_000_000,
                user_time_ns: 800_000_000, sys_time_ns: 200_000_000,
                read_bytes: 134_217_728, write_bytes: 67_108_864,
                read_syscalls: 20000, write_syscalls: 10000,
                rss_pages: 512, vm_pages: 2048,
                minor_faults: 10000, major_faults: 20,
                voluntary_switches: 50000, involuntary_switches: 2000,
                delays_ns: [200_000_000, 100_000_000, 0, 0, 0, 0],
                delay_counts: [2000, 1000, 0, 0, 0, 0],
            },
        ],
        total_cpu_ns: 6_000_000_000,
        total_io_bytes: 1_811_939_328,
        total_delays_ns: 1_950_000_000,
        ops: 0,
    });
}

/// Register a new task.
pub fn register(pid: u32, name: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.tasks.iter().any(|t| t.pid == pid) { return Err(KernelError::AlreadyExists); }
        if state.tasks.len() >= MAX_TASKS { return Err(KernelError::ResourceExhausted); }
        state.tasks.push(TaskAccounting {
            pid, name: String::from(name), cpu_time_ns: 0,
            user_time_ns: 0, sys_time_ns: 0,
            read_bytes: 0, write_bytes: 0,
            read_syscalls: 0, write_syscalls: 0,
            rss_pages: 0, vm_pages: 0,
            minor_faults: 0, major_faults: 0,
            voluntary_switches: 0, involuntary_switches: 0,
            delays_ns: [0; 6], delay_counts: [0; 6],
        });
        Ok(())
    })
}

/// Update CPU time.
pub fn update_cpu(pid: u32, user_ns: u64, sys_ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let t = state.tasks.iter_mut().find(|t| t.pid == pid)
            .ok_or(KernelError::NotFound)?;
        t.user_time_ns += user_ns;
        t.sys_time_ns += sys_ns;
        t.cpu_time_ns += user_ns + sys_ns;
        state.total_cpu_ns += user_ns + sys_ns;
        Ok(())
    })
}

/// Update I/O bytes.
pub fn update_io(pid: u32, read_bytes: u64, write_bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        let t = state.tasks.iter_mut().find(|t| t.pid == pid)
            .ok_or(KernelError::NotFound)?;
        t.read_bytes += read_bytes;
        t.write_bytes += write_bytes;
        if read_bytes > 0 { t.read_syscalls += 1; }
        if write_bytes > 0 { t.write_syscalls += 1; }
        state.total_io_bytes += read_bytes + write_bytes;
        Ok(())
    })
}

/// Record a delay.
pub fn update_delay(pid: u32, delay_type: DelayType, ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let t = state.tasks.iter_mut().find(|t| t.pid == pid)
            .ok_or(KernelError::NotFound)?;
        let idx = delay_type.index();
        t.delays_ns[idx] += ns;
        t.delay_counts[idx] += 1;
        state.total_delays_ns += ns;
        Ok(())
    })
}

/// Get task accounting.
pub fn get(pid: u32) -> Option<TaskAccounting> {
    STATE.lock().as_ref().and_then(|s| s.tasks.iter().find(|t| t.pid == pid).cloned())
}

/// List all tasks.
pub fn list_tasks() -> Vec<TaskAccounting> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.tasks.clone())
}

/// Top CPU consumers.
pub fn top_cpu(n: usize) -> Vec<TaskAccounting> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut sorted = s.tasks.clone();
        sorted.sort_by(|a, b| b.cpu_time_ns.cmp(&a.cpu_time_ns));
        sorted.truncate(n);
        sorted
    })
}

/// Statistics: (task_count, total_cpu_ns, total_io_bytes, total_delays_ns, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.tasks.len(), s.total_cpu_ns, s.total_io_bytes, s.total_delays_ns, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("taskstats::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(list_tasks().len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Register.
    register(200, "test_task").expect("register");
    assert_eq!(list_tasks().len(), 3);
    assert!(register(200, "dup").is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Update CPU.
    update_cpu(200, 1_000_000, 500_000).expect("cpu");
    let t = get(200).expect("get");
    assert_eq!(t.cpu_time_ns, 1_500_000);
    assert_eq!(t.user_time_ns, 1_000_000);
    crate::serial_println!("  [3/8] cpu: OK");

    // 4: Update I/O.
    update_io(200, 4096, 8192).expect("io");
    let t = get(200).expect("get2");
    assert_eq!(t.read_bytes, 4096);
    assert_eq!(t.write_bytes, 8192);
    crate::serial_println!("  [4/8] io: OK");

    // 5: Delay accounting.
    update_delay(200, DelayType::CpuWait, 100_000).expect("delay");
    let t = get(200).expect("get3");
    assert_eq!(t.delays_ns[0], 100_000);
    assert_eq!(t.delay_counts[0], 1);
    crate::serial_println!("  [5/8] delay: OK");

    // 6: Top CPU.
    let top = top_cpu(2);
    assert_eq!(top.len(), 2);
    assert!(top[0].cpu_time_ns >= top[1].cpu_time_ns);
    crate::serial_println!("  [6/8] top cpu: OK");

    // 7: Not found.
    assert!(update_cpu(999, 0, 0).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (tasks, cpu, io, delays, ops) = stats();
    assert_eq!(tasks, 3);
    assert!(cpu > 6_000_000_000);
    assert!(io > 1_811_939_328);
    assert!(delays > 1_950_000_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("taskstats::self_test() — all 8 tests passed");
}
