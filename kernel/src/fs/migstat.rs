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

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        cpus: alloc::vec![
            CpuMigStats { cpu_id: 0, migrations_in: 500_000, migrations_out: 450_000, numa_crosses: 10_000 },
            CpuMigStats { cpu_id: 1, migrations_in: 450_000, migrations_out: 500_000, numa_crosses: 12_000 },
            CpuMigStats { cpu_id: 2, migrations_in: 300_000, migrations_out: 350_000, numa_crosses: 8_000 },
            CpuMigStats { cpu_id: 3, migrations_in: 350_000, migrations_out: 300_000, numa_crosses: 9_000 },
        ],
        tasks: alloc::vec![
            TaskMigInfo { pid: 1, name: String::from("init"), migrations: 1000, numa_crosses: 50, last_cpu: 0 },
            TaskMigInfo { pid: 100, name: String::from("shell"), migrations: 50_000, numa_crosses: 2000, last_cpu: 1 },
        ],
        total_migrations: 1_600_000,
        total_numa_crosses: 39_000,
        reason_counts: [800_000, 200_000, 5_000, 400_000, 150_000, 45_000],
        ops: 0,
    });
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
        sorted.sort_by(|a, b| b.migrations.cmp(&a.migrations));
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_cpu().len(), 4);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Register task.
    register_task(200, "test_task", 0).expect("register");
    assert!(register_task(200, "dup", 0).is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Record migration.
    record(200, 0, 1, MigReason::LoadBalance, false).expect("migrate");
    let t = hot_tasks(10).iter().find(|t| t.pid == 200).cloned().unwrap();
    assert_eq!(t.migrations, 1);
    assert_eq!(t.last_cpu, 1);
    crate::serial_println!("  [3/8] migration: OK");

    // 4: NUMA crossing.
    record(200, 1, 2, MigReason::WakeAffine, true).expect("numa");
    let t = hot_tasks(10).iter().find(|t| t.pid == 200).cloned().unwrap();
    assert_eq!(t.numa_crosses, 1);
    crate::serial_println!("  [4/8] numa cross: OK");

    // 5: Per-CPU stats.
    let cpus = per_cpu();
    assert!(cpus[0].migrations_out > 450_000);
    assert!(cpus[1].migrations_in > 450_000);
    crate::serial_println!("  [5/8] per cpu: OK");

    // 6: Reason stats.
    let reasons = reason_stats();
    assert!(reasons[0].1 > 800_000); // LoadBalance.
    crate::serial_println!("  [6/8] reasons: OK");

    // 7: Hot tasks.
    let top = hot_tasks(2);
    assert!(top.len() >= 2);
    assert!(top[0].migrations >= top[1].migrations);
    crate::serial_println!("  [7/8] hot tasks: OK");

    // 8: Stats.
    let (cpus, tasks, migs, numa, ops) = stats();
    assert_eq!(cpus, 4);
    assert!(tasks >= 3);
    assert!(migs > 1_600_000);
    assert!(numa > 39_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("migstat::self_test() — all 8 tests passed");
}
