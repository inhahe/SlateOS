//! System Maintenance — scheduled maintenance task management.
//!
//! Schedules and tracks system maintenance tasks: disk defrag, index rebuilds,
//! update checks, integrity scans, and performance tuning at optimal times.
//!
//! ## Architecture
//!
//! ```text
//! Idle period or schedule
//!   → sysmaint::run_pending() → execute due tasks
//!   → sysmaint::check_schedule() → identify tasks needing run
//!
//! Integration:
//!   → fstrim (disk trimming)
//!   → findex (index rebuild)
//!   → updatemgr (update checks)
//!   → health (integrity scans)
//!   → storagesense (storage cleanup)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Maintenance task type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskType {
    DiskTrim,
    IndexRebuild,
    UpdateCheck,
    IntegrityScan,
    CacheCleanup,
    LogRotation,
    TempCleanup,
    PerformanceTune,
    SecurityScan,
    BackupVerify,
}

impl TaskType {
    pub fn label(self) -> &'static str {
        match self {
            Self::DiskTrim => "Disk Trim",
            Self::IndexRebuild => "Index Rebuild",
            Self::UpdateCheck => "Update Check",
            Self::IntegrityScan => "Integrity Scan",
            Self::CacheCleanup => "Cache Cleanup",
            Self::LogRotation => "Log Rotation",
            Self::TempCleanup => "Temp Cleanup",
            Self::PerformanceTune => "Performance Tune",
            Self::SecurityScan => "Security Scan",
            Self::BackupVerify => "Backup Verify",
        }
    }
}

/// Task run status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Idle,
    Scheduled,
    Running,
    Completed,
    Failed,
    Skipped,
}

impl TaskStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Scheduled => "Scheduled",
            Self::Running => "Running",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
            Self::Skipped => "Skipped",
        }
    }
}

/// A maintenance task configuration.
#[derive(Debug, Clone)]
pub struct MaintTask {
    pub id: u32,
    pub task_type: TaskType,
    pub enabled: bool,
    /// Interval in hours between runs.
    pub interval_hours: u32,
    /// Only run when system is idle.
    pub idle_only: bool,
    pub status: TaskStatus,
    pub last_run_ns: u64,
    pub last_duration_ms: u64,
    pub run_count: u64,
    pub fail_count: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    tasks: Vec<MaintTask>,
    next_id: u32,
    /// Whether maintenance window is currently active.
    window_active: bool,
    total_runs: u64,
    total_failures: u64,
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
    let mut id = 1u32;
    let mut tasks = Vec::new();
    let defaults = [
        (TaskType::DiskTrim, 168, true),       // Weekly
        (TaskType::IndexRebuild, 24, true),     // Daily
        (TaskType::UpdateCheck, 12, false),     // Twice daily
        (TaskType::IntegrityScan, 720, true),   // Monthly
        (TaskType::CacheCleanup, 168, true),    // Weekly
        (TaskType::LogRotation, 168, true),     // Weekly
        (TaskType::TempCleanup, 24, true),      // Daily
        (TaskType::PerformanceTune, 720, true), // Monthly
        (TaskType::SecurityScan, 168, true),    // Weekly
        (TaskType::BackupVerify, 720, true),    // Monthly
    ];
    for (tt, interval, idle) in &defaults {
        tasks.push(MaintTask {
            id: { let i = id; id += 1; i },
            task_type: *tt,
            enabled: true,
            interval_hours: *interval,
            idle_only: *idle,
            status: TaskStatus::Idle,
            last_run_ns: 0,
            last_duration_ms: 0,
            run_count: 0,
            fail_count: 0,
        });
    }
    *guard = Some(State {
        tasks,
        next_id: id,
        window_active: false,
        total_runs: 0,
        total_failures: 0,
        ops: 0,
    });
}

/// Check which tasks are due.
pub fn check_schedule() -> KernelResult<Vec<u32>> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let mut due = Vec::new();
        for task in &state.tasks {
            if !task.enabled { continue; }
            let interval_ns = (task.interval_hours as u64) * 3600 * 1_000_000_000;
            let elapsed = now.saturating_sub(task.last_run_ns);
            if elapsed >= interval_ns {
                due.push(task.id);
            }
        }
        Ok(due)
    })
}

/// Run a specific task.
pub fn run_task(task_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let task = state.tasks.iter_mut().find(|t| t.id == task_id)
            .ok_or(KernelError::NotFound)?;
        task.status = TaskStatus::Running;
        // Simulate task completion.
        task.last_run_ns = now;
        task.last_duration_ms = 100; // Simulated duration.
        task.run_count += 1;
        task.status = TaskStatus::Completed;
        state.total_runs += 1;
        Ok(())
    })
}

/// Run all due tasks.
pub fn run_pending() -> KernelResult<usize> {
    // Get due task IDs first to avoid borrow issues.
    let due = check_schedule()?;
    let mut ran = 0;
    for id in &due {
        if run_task(*id).is_ok() {
            ran += 1;
        }
    }
    Ok(ran)
}

/// Set task enabled.
pub fn set_enabled(task_id: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let task = state.tasks.iter_mut().find(|t| t.id == task_id)
            .ok_or(KernelError::NotFound)?;
        task.enabled = enabled;
        Ok(())
    })
}

/// Set task interval.
pub fn set_interval(task_id: u32, hours: u32) -> KernelResult<()> {
    with_state(|state| {
        let task = state.tasks.iter_mut().find(|t| t.id == task_id)
            .ok_or(KernelError::NotFound)?;
        task.interval_hours = hours.clamp(1, 8760);
        Ok(())
    })
}

/// Set idle-only preference.
pub fn set_idle_only(task_id: u32, idle_only: bool) -> KernelResult<()> {
    with_state(|state| {
        let task = state.tasks.iter_mut().find(|t| t.id == task_id)
            .ok_or(KernelError::NotFound)?;
        task.idle_only = idle_only;
        Ok(())
    })
}

/// Start/stop maintenance window.
pub fn set_window(active: bool) -> KernelResult<()> {
    with_state(|state| {
        state.window_active = active;
        Ok(())
    })
}

/// List all tasks.
pub fn list_tasks() -> Vec<MaintTask> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.tasks.clone())
}

/// Statistics: (task_count, total_runs, total_failures, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.tasks.len(), s.total_runs, s.total_failures, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("sysmaint::self_test() — running tests...");
    // Start from a clean default schedule so the assertions below are exact.
    // This self_test runs tasks (run_task / run_pending), which bumps run_count
    // and marks tasks Completed; without resetting afterwards it would leave
    // every task looking as though maintenance had actually executed, which
    // `sysmaint show` would then report as real run history.
    *STATE.lock() = None;
    init_defaults();

    // 1: Default tasks — the seeded schedule is legitimate config (default
    //    intervals), but every activity counter starts honestly zeroed.
    let tasks = list_tasks();
    assert_eq!(tasks.len(), 10);
    assert_eq!(tasks[0].task_type, TaskType::DiskTrim);
    for t in &tasks {
        assert_eq!(t.status, TaskStatus::Idle);
        assert_eq!((t.last_run_ns, t.last_duration_ms, t.run_count, t.fail_count), (0, 0, 0, 0));
    }
    let (_, runs0, fails0, _) = stats();
    assert_eq!((runs0, fails0), (0, 0));
    crate::serial_println!("  [1/8] default tasks: OK");

    // 2: All tasks are due (never run).
    let due = check_schedule().expect("check");
    assert_eq!(due.len(), 10);
    crate::serial_println!("  [2/8] all due: OK");

    // 3: Run a task.
    run_task(1).expect("run");
    let tasks = list_tasks();
    assert_eq!(tasks[0].status, TaskStatus::Completed);
    assert_eq!(tasks[0].run_count, 1);
    crate::serial_println!("  [3/8] run task: OK");

    // 4: Task no longer due.
    let due = check_schedule().expect("check2");
    assert_eq!(due.len(), 9); // Task 1 just ran.
    crate::serial_println!("  [4/8] not due after run: OK");

    // 5: Run pending.
    let ran = run_pending().expect("pending");
    assert_eq!(ran, 9);
    crate::serial_println!("  [5/8] run pending: OK");

    // 6: Disable task.
    set_enabled(1, false).expect("disable");
    let tasks = list_tasks();
    assert!(!tasks[0].enabled);
    crate::serial_println!("  [6/8] disable: OK");

    // 7: Set interval.
    set_interval(2, 48).expect("interval");
    let tasks = list_tasks();
    assert_eq!(tasks[1].interval_hours, 48);
    crate::serial_println!("  [7/8] interval: OK");

    // 8: Stats.
    let (count, runs, failures, ops) = stats();
    assert_eq!(count, 10);
    assert!(runs >= 10);
    assert_eq!(failures, 0);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave the live state as a clean default schedule (not the fixture state
    // with tasks marked run), so `sysmaint show` afterwards reports the honest
    // default config with zeroed run counts rather than fabricated activity.
    *STATE.lock() = None;
    init_defaults();
    crate::serial_println!("sysmaint::self_test() — all 8 tests passed");
}
