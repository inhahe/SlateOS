//! Migration Statistics — process/thread CPU migration monitoring.
//!
//! Tracks cross-CPU task migrations, migration latency,
//! NUMA-crossing migrations, and per-CPU migration balance.
//! Essential for diagnosing scheduler locality issues.
//!
//! ## Architecture
//!
//! ```text
//! Migration monitoring
//!   → migstat::record(pid, from, to) → track migration
//!   → migstat::record_numa_cross(pid) → NUMA boundary crossing
//!   → migstat::per_cpu() → per-CPU migration counts
//!   → migstat::hot_tasks(n) → most-migrated tasks
//!
//! Integration:
//!   → schedclass (scheduler class)
//!   → numastat (NUMA statistics)
//!   → cpustat (CPU utilization)
//!   → taskstats (per-task accounting)
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

/// Migration reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigReason {
    LoadBalance,
    AffinityChange,
    HotUnplug,
    WakeAffine,
    CacheCold,
    Forced,
}

impl MigReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::LoadBalance => "load_balance",
            Self::AffinityChange => "affinity",
            Self::HotUnplug => "hot_unplug",
            Self::WakeAffine => "wake_affine",
            Self::CacheCold => "cache_cold",
            Self::Forced => "forced",
        }
    }
}

/// Per-CPU migration counters.
#[derive(Debug, Clone)]
pub struct CpuMigStats {
    pub cpu_id: u32,
    pub migrations_in: u64,
    pub migrations_out: u64,
    pub numa_crosses: u64,
}

/// Per-task migration info.
#[derive(Debug, Clone)]
pub struct TaskMigInfo {
    pub pid: u32,
    pub name: String,
    pub migrations: u64,
    pub numa_crosses: u64,
    pub last_cpu: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_CPUS: usize = 64;
const MAX_TASKS: usize = 256;

struct State {
    cpus: Vec<CpuMigStats>,
    tasks: Vec<TaskMigInfo>,
    total_migrations: u64,
    total_numa_crosses: u64,
    reason_counts: [u64; 6],
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

fn reason_index(r: MigReason) -> usize {
    match r {
        MigReason::LoadBalance => 0,
        MigReason::AffinityChange => 1,
        MigReason::HotUnplug => 2,
        MigReason::WakeAffine => 3,
        MigReason::CacheCold => 4,
        MigReason::Forced => 5,
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialise an **empty** migration-statistics table.
///
/// Seeds NO per-CPU rows, NO task rows, and zero totals.  Real migration
/// accounting is wired through [`register_cpu`] (one zero-counter row per
/// online CPU, populated by the scheduler at bring-up), [`register_task`], and
/// [`record`]; until those are called the tables are genuinely empty, so the
/// `/proc/migstat` file and the `migstat` kshell command report zeros rather
/// than fabricated numbers — the kernel's hard "never invent data in procfs"
/// rule.
///
/// NOTE: this previously seeded four fictional per-CPU rows (cpu0..3 with
/// migrations_in/out 300_000–500_000, numa_crosses 8_000–12_000) and two
/// fictional tasks (pid 1 "init" migrations 1000; pid 100 "shell" migrations
/// 50_000) plus invented aggregate totals (total_migrations 1_600_000,
/// total_numa_crosses 39_000, and pre-filled reason_counts), which
/// `/proc/migstat` then displayed as if they were real scheduler migration
/// measurements.  That demo data was removed; the self-test now builds its own
/// fixtures explicitly via the real API (see [`self_test`]).  The scheduler is
/// expected to call [`register_cpu`] per online CPU and [`record`] on every
/// task migration.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        cpus: Vec::new(),
        tasks: Vec::new(),
        total_migrations: 0,
        total_numa_crosses: 0,
        reason_counts: [0; 6],
        ops: 0,
    });
}

/// Register a CPU for migration tracking.
///
/// The scheduler calls this once per online CPU at bring-up so the per-CPU
/// migration table reflects the real topology with zeroed counters.  Without a
/// registered row, [`record`] silently skips the per-CPU update for that CPU id
/// (the task-level and aggregate counters still advance).
pub fn register_cpu(cpu_id: u32) -> KernelResult<()> {
    with_state(|state| {
        if state.cpus.iter().any(|c| c.cpu_id == cpu_id) { return Err(KernelError::AlreadyExists); }
        if state.cpus.len() >= MAX_CPUS { return Err(KernelError::ResourceExhausted); }
        state.cpus.push(CpuMigStats {
            cpu_id, migrations_in: 0, migrations_out: 0, numa_crosses: 0,
        });
        Ok(())
    })
}

/// Record a task migration.
pub fn record(pid: u32, from_cpu: u32, to_cpu: u32, reason: MigReason, is_numa: bool) -> KernelResult<()> {
    with_state(|state| {
        // Update CPU counters.
        if let Some(c) = state.cpus.iter_mut().find(|c| c.cpu_id == from_cpu) {
            c.migrations_out += 1;
            if is_numa { c.numa_crosses += 1; }
        }
        if let Some(c) = state.cpus.iter_mut().find(|c| c.cpu_id == to_cpu) {
            c.migrations_in += 1;
        }
        // Update task counters.
        if let Some(t) = state.tasks.iter_mut().find(|t| t.pid == pid) {
            t.migrations += 1;
            t.last_cpu = to_cpu;
            if is_numa { t.numa_crosses += 1; }
        }
        state.total_migrations += 1;
        if is_numa { state.total_numa_crosses += 1; }
        state.reason_counts[reason_index(reason)] += 1;
        Ok(())
    })
}

/// Register a task for migration tracking.
pub fn register_task(pid: u32, name: &str, cpu: u32) -> KernelResult<()> {
    with_state(|state| {
        if state.tasks.iter().any(|t| t.pid == pid) { return Err(KernelError::AlreadyExists); }
        if state.tasks.len() >= MAX_TASKS { return Err(KernelError::ResourceExhausted); }
        state.tasks.push(TaskMigInfo {
            pid, name: String::from(name), migrations: 0, numa_crosses: 0, last_cpu: cpu,
        });
        Ok(())
    })
}

/// Per-CPU migration stats.
pub fn per_cpu() -> Vec<CpuMigStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.cpus.clone())
}

/// Most-migrated tasks.
pub fn hot_tasks(n: usize) -> Vec<TaskMigInfo> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut sorted = s.tasks.clone();
        sorted.sort_by_key(|e| core::cmp::Reverse(e.migrations));
        sorted.truncate(n);
        sorted
    })
}

/// Reason breakdown.
pub fn reason_stats() -> [(MigReason, u64); 6] {
    let guard = STATE.lock();
    let counts = guard.as_ref().map_or([0u64; 6], |s| s.reason_counts);
    [
        (MigReason::LoadBalance, counts[0]),
        (MigReason::AffinityChange, counts[1]),
        (MigReason::HotUnplug, counts[2]),
        (MigReason::WakeAffine, counts[3]),
        (MigReason::CacheCold, counts[4]),
        (MigReason::Forced, counts[5]),
    ]
}

/// Statistics: (cpu_count, task_count, total_migrations, total_numa_crosses, ops).
pub fn stats() -> (usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.cpus.len(), s.tasks.len(), s.total_migrations, s.total_numa_crosses, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("migstat::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real
    // API, so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/migstat must never surface).
    // Resetting first clears any residue from a prior `migstat test` run so the
    // totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated CPUs, tasks, or totals.
    assert_eq!(per_cpu().len(), 0);
    assert_eq!(hot_tasks(10).len(), 0);
    let (c0, t0, m0, n0, _o0) = stats();
    assert_eq!((c0, t0, m0, n0), (0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register CPUs (zeroed) and a task; duplicates fail.
    register_cpu(0).expect("cpu0");
    register_cpu(1).expect("cpu1");
    register_cpu(2).expect("cpu2");
    assert!(register_cpu(0).is_err());
    register_task(200, "test_task", 0).expect("register");
    assert!(register_task(200, "dup", 0).is_err());
    assert_eq!(per_cpu().len(), 3);
    crate::serial_println!("  [2/8] register: OK");

    // 3: Record migration (exact, from zero).
    record(200, 0, 1, MigReason::LoadBalance, false).expect("migrate");
    let t = hot_tasks(10).iter().find(|t| t.pid == 200).cloned().expect("task");
    assert_eq!(t.migrations, 1);
    assert_eq!(t.last_cpu, 1);
    crate::serial_println!("  [3/8] migration: OK");

    // 4: NUMA crossing bumps the task's numa counter exactly.
    record(200, 1, 2, MigReason::WakeAffine, true).expect("numa");
    let t = hot_tasks(10).iter().find(|t| t.pid == 200).cloned().expect("task");
    assert_eq!(t.numa_crosses, 1);
    assert_eq!(t.migrations, 2);
    crate::serial_println!("  [4/8] numa cross: OK");

    // 5: Per-CPU counters reflect exactly the two migrations above.
    //    Migration 1: out cpu0, in cpu1. Migration 2: out cpu1 (numa), in cpu2.
    let cpu0 = per_cpu().iter().find(|c| c.cpu_id == 0).cloned().expect("cpu0");
    let cpu1 = per_cpu().iter().find(|c| c.cpu_id == 1).cloned().expect("cpu1");
    let cpu2 = per_cpu().iter().find(|c| c.cpu_id == 2).cloned().expect("cpu2");
    assert_eq!(cpu0.migrations_out, 1);
    assert_eq!(cpu1.migrations_in, 1);
    assert_eq!(cpu1.migrations_out, 1);
    assert_eq!(cpu1.numa_crosses, 1);
    assert_eq!(cpu2.migrations_in, 1);
    crate::serial_println!("  [5/8] per cpu: OK");

    // 6: Reason breakdown reflects exactly one LoadBalance + one WakeAffine.
    let reasons = reason_stats();
    assert_eq!(reasons[0].1, 1); // LoadBalance
    assert_eq!(reasons[3].1, 1); // WakeAffine
    assert_eq!(reasons[1].1, 0); // AffinityChange untouched
    crate::serial_println!("  [6/8] reasons: OK");

    // 7: Hot tasks sorted by migration count; a second task ranks below.
    register_task(201, "quiet_task", 3).expect("register2");
    let top = hot_tasks(2);
    assert_eq!(top.len(), 2);
    assert_eq!(top[0].pid, 200); // 2 migrations
    assert!(top[0].migrations >= top[1].migrations);
    crate::serial_println!("  [7/8] hot tasks: OK");

    // 8: Aggregate totals equal the exact sums of the operations above.
    let (cpus, tasks, migs, numa, ops) = stats();
    assert_eq!(cpus, 3);
    assert_eq!(tasks, 2); // pid 200 + pid 201
    assert_eq!(migs, 2); // two record() calls
    assert_eq!(numa, 1); // one numa crossing
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: a diagnostic self-test must not populate the live
    // /proc/migstat table with its fixtures.  Reset to the uninitialised state
    // so production reads report an empty table until the scheduler wires real
    // accounting.
    *STATE.lock() = None;

    crate::serial_println!("migstat::self_test() — all 8 tests passed");
}
