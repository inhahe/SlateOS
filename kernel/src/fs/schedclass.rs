//! Scheduler Class — scheduling policy and class tracking.
//!
//! Tracks scheduler classes (RT, CFS/EEVDF, idle, deadline),
//! per-class task counts, context switches, and time slices.
//! Essential for diagnosing scheduling latency and policy issues.
//!
//! ## Architecture
//!
//! ```text
//! Scheduler class tracking
//!   → schedclass::register_task(pid, class) → assign class
//!   → schedclass::record_switch(from, to) → track context switch
//!   → schedclass::record_slice(pid, ns) → track time slice
//!   → schedclass::class_stats() → per-class statistics
//!
//! Integration:
//!   → procstat (process statistics)
//!   → cpuidle (CPU idle stats)
//!   → perfmon (performance monitor)
//!   → tracemon (trace monitor)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Scheduler class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedClass {
    RealTime,
    Deadline,
    Normal,
    Batch,
    Idle,
}

impl SchedClass {
    pub fn label(self) -> &'static str {
        match self {
            Self::RealTime => "RT",
            Self::Deadline => "DL",
            Self::Normal => "NORMAL",
            Self::Batch => "BATCH",
            Self::Idle => "IDLE",
        }
    }
}

/// Per-class statistics.
#[derive(Debug, Clone)]
pub struct ClassStats {
    pub class: SchedClass,
    pub task_count: u32,
    pub context_switches: u64,
    pub total_runtime_ns: u64,
    pub total_slices: u64,
    pub avg_slice_ns: u64,
    pub migrations: u64,
}

/// Per-task scheduling info.
#[derive(Debug, Clone)]
pub struct TaskSchedInfo {
    pub pid: u32,
    pub class: SchedClass,
    pub priority: i32,
    pub nice: i32,
    pub runtime_ns: u64,
    pub switches: u64,
    pub migrations: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_TASKS: usize = 512;

struct State {
    tasks: Vec<TaskSchedInfo>,
    class_stats: Vec<ClassStats>,
    total_switches: u64,
    total_migrations: u64,
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

/// Initialise the scheduler-class statistics state.
///
/// Starts with no tracked tasks and all per-class counters at zero. The
/// five `class_stats` rows (RealTime, Deadline, Normal, Batch, Idle) are a
/// fixed scheduler-class taxonomy, so they are always present — but with
/// zeroed task counts, context switches, runtime, slices, and migrations.
/// The `/proc/schedclass` generator and the `schedclass` kshell command
/// surface this table (and `list_tasks` / `class_stats`) as if it reflects
/// the real set of scheduled tasks, so seeding it with phantom tasks and
/// invented switch/runtime/migration totals would be fabricated procfs
/// data. Tasks are registered through [`register_task`] and the counters
/// advance only through real [`record_switch`] / [`record_slice`] /
/// [`record_migration`] calls.
///
/// (Previously this seeded three fictional tasks — pid 0 Idle (50s runtime,
/// 100k switches), pid 1 Normal (10s runtime, 500k switches, 1000
/// migrations), pid 2 RealTime (1s runtime, 200k switches, 50 migrations) —
/// with matching invented per-class stats and totals of 800000 switches /
/// 1050 migrations.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        tasks: Vec::new(),
        class_stats: alloc::vec![
            ClassStats { class: SchedClass::RealTime, task_count: 0, context_switches: 0, total_runtime_ns: 0, total_slices: 0, avg_slice_ns: 0, migrations: 0 },
            ClassStats { class: SchedClass::Deadline, task_count: 0, context_switches: 0, total_runtime_ns: 0, total_slices: 0, avg_slice_ns: 0, migrations: 0 },
            ClassStats { class: SchedClass::Normal, task_count: 0, context_switches: 0, total_runtime_ns: 0, total_slices: 0, avg_slice_ns: 0, migrations: 0 },
            ClassStats { class: SchedClass::Batch, task_count: 0, context_switches: 0, total_runtime_ns: 0, total_slices: 0, avg_slice_ns: 0, migrations: 0 },
            ClassStats { class: SchedClass::Idle, task_count: 0, context_switches: 0, total_runtime_ns: 0, total_slices: 0, avg_slice_ns: 0, migrations: 0 },
        ],
        total_switches: 0,
        total_migrations: 0,
        ops: 0,
    });
}

/// Register or update a task's scheduling class.
pub fn register_task(pid: u32, class: SchedClass, priority: i32, nice: i32) -> KernelResult<()> {
    with_state(|state| {
        if let Some(t) = state.tasks.iter_mut().find(|t| t.pid == pid) {
            // Update existing — adjust class stats.
            if let Some(cs) = state.class_stats.iter_mut().find(|c| c.class == t.class) {
                cs.task_count = cs.task_count.saturating_sub(1);
            }
            t.class = class;
            t.priority = priority;
            t.nice = nice;
        } else {
            if state.tasks.len() >= MAX_TASKS { return Err(KernelError::ResourceExhausted); }
            state.tasks.push(TaskSchedInfo {
                pid, class, priority, nice, runtime_ns: 0, switches: 0, migrations: 0,
            });
        }
        if let Some(cs) = state.class_stats.iter_mut().find(|c| c.class == class) {
            cs.task_count += 1;
        }
        Ok(())
    })
}

/// Record a context switch.
pub fn record_switch(pid: u32) -> KernelResult<()> {
    with_state(|state| {
        if let Some(t) = state.tasks.iter_mut().find(|t| t.pid == pid) {
            t.switches += 1;
            if let Some(cs) = state.class_stats.iter_mut().find(|c| c.class == t.class) {
                cs.context_switches += 1;
            }
        }
        state.total_switches += 1;
        Ok(())
    })
}

/// Record time slice usage.
pub fn record_slice(pid: u32, ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let t = state.tasks.iter_mut().find(|t| t.pid == pid)
            .ok_or(KernelError::NotFound)?;
        t.runtime_ns += ns;
        if let Some(cs) = state.class_stats.iter_mut().find(|c| c.class == t.class) {
            cs.total_runtime_ns += ns;
            cs.total_slices += 1;
            if cs.total_slices > 0 {
                cs.avg_slice_ns = cs.total_runtime_ns / cs.total_slices;
            }
        }
        Ok(())
    })
}

/// Record a task migration between CPUs.
pub fn record_migration(pid: u32) -> KernelResult<()> {
    with_state(|state| {
        if let Some(t) = state.tasks.iter_mut().find(|t| t.pid == pid) {
            t.migrations += 1;
            if let Some(cs) = state.class_stats.iter_mut().find(|c| c.class == t.class) {
                cs.migrations += 1;
            }
        }
        state.total_migrations += 1;
        Ok(())
    })
}

/// Get per-class statistics.
pub fn class_stats() -> Vec<ClassStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.class_stats.clone())
}

/// Get task scheduling info.
pub fn task_info(pid: u32) -> Option<TaskSchedInfo> {
    STATE.lock().as_ref().and_then(|s| s.tasks.iter().find(|t| t.pid == pid).cloned())
}

/// List all tracked tasks.
pub fn list_tasks() -> Vec<TaskSchedInfo> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.tasks.clone())
}

/// Statistics: (task_count, class_count, total_switches, total_migrations, ops).
pub fn stats() -> (usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.tasks.len(), s.class_stats.len(), s.total_switches, s.total_migrations, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("schedclass::self_test() — running tests...");
    // Start from a clean, empty state so the assertions below are exact and
    // no fixtures leak into the live scheduler-class tables afterwards.
    *STATE.lock() = None;
    init_defaults();

    // Helper: fetch a class row by class.
    fn class_of(class: SchedClass) -> ClassStats {
        class_stats().into_iter().find(|c| c.class == class).expect("class row")
    }

    // 1: Empty defaults — no tasks, five zeroed class rows, zero totals.
    assert_eq!(list_tasks().len(), 0);
    assert_eq!(class_stats().len(), 5);
    for c in class_stats() {
        assert_eq!(c.task_count, 0);
        assert_eq!(c.context_switches, 0);
        assert_eq!(c.total_runtime_ns, 0);
        assert_eq!(c.total_slices, 0);
        assert_eq!(c.migrations, 0);
    }
    let (t0, c0, sw0, mig0, _) = stats();
    assert_eq!((t0, c0, sw0, mig0), (0, 5, 0, 0));
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Register a task — appears in the task list and bumps its class count.
    register_task(100, SchedClass::Normal, 120, 0).expect("register");
    assert_eq!(list_tasks().len(), 1);
    assert_eq!(class_of(SchedClass::Normal).task_count, 1);
    crate::serial_println!("  [2/8] register: OK");

    // 3: Context switch — task + class + global counters advance by one.
    record_switch(100).expect("switch");
    assert_eq!(task_info(100).expect("t").switches, 1);
    assert_eq!(class_of(SchedClass::Normal).context_switches, 1);
    assert_eq!(stats().2, 1); // total_switches
    crate::serial_println!("  [3/8] switch: OK");

    // 4: Time slice — runtime and avg-slice accounting are exact.
    record_slice(100, 10_000).expect("slice");
    let t = task_info(100).expect("info");
    assert_eq!(t.runtime_ns, 10_000);
    let n = class_of(SchedClass::Normal);
    assert_eq!(n.total_runtime_ns, 10_000);
    assert_eq!(n.total_slices, 1);
    assert_eq!(n.avg_slice_ns, 10_000);
    crate::serial_println!("  [4/8] slice: OK");

    // 5: Migration — task, class, and global migration counters advance.
    record_migration(100).expect("migrate");
    assert_eq!(task_info(100).expect("info2").migrations, 1);
    assert_eq!(class_of(SchedClass::Normal).migrations, 1);
    assert_eq!(stats().3, 1); // total_migrations
    crate::serial_println!("  [5/8] migration: OK");

    // 6: Class change — task moves Normal→RealTime; class task_counts follow.
    register_task(100, SchedClass::RealTime, 50, 0).expect("reclass");
    assert_eq!(task_info(100).expect("info3").class, SchedClass::RealTime);
    assert_eq!(class_of(SchedClass::Normal).task_count, 0);
    assert_eq!(class_of(SchedClass::RealTime).task_count, 1);
    crate::serial_println!("  [6/8] class change: OK");

    // 7: Recording a slice for an unknown pid is NotFound (no phantom rows).
    assert!(record_slice(9999, 100).is_err());
    assert_eq!(list_tasks().len(), 1);
    crate::serial_println!("  [7/8] unknown pid: OK");

    // 8: Final stats reflect only the real activity above.
    let (tasks, classes, switches, migrations, ops) = stats();
    assert_eq!(tasks, 1);
    assert_eq!(classes, 5);
    assert_eq!(switches, 1);
    assert_eq!(migrations, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("schedclass::self_test() — all 8 tests passed");
}
