//! Backup Scheduler — automated backup scheduling and management.
//!
//! Manages scheduled backup jobs with configurable frequency,
//! retention policies, and destination targets.
//!
//! ## Architecture
//!
//! ```text
//! Backup scheduling
//!   → backupsched::create_schedule(params) → new backup job
//!   → backupsched::run_now(schedule_id) → trigger immediate backup
//!   → backupsched::get_history(schedule_id) → past runs
//!
//! Integration:
//!   → backup (backup operations)
//!   → systemimage (system snapshots)
//!   → dirsync (directory sync)
//!   → tasksched (task scheduler)
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

/// Backup frequency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupFrequency {
    Hourly,
    Daily,
    Weekly,
    Monthly,
    Manual,
}

impl BackupFrequency {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hourly => "Hourly",
            Self::Daily => "Daily",
            Self::Weekly => "Weekly",
            Self::Monthly => "Monthly",
            Self::Manual => "Manual",
        }
    }
}

/// Backup type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupType {
    Full,
    Incremental,
    Differential,
    Mirror,
}

impl BackupType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "Full",
            Self::Incremental => "Incremental",
            Self::Differential => "Differential",
            Self::Mirror => "Mirror",
        }
    }
}

/// Run result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunResult {
    Success,
    PartialSuccess,
    Failed,
    Skipped,
    Cancelled,
}

impl RunResult {
    pub fn label(self) -> &'static str {
        match self {
            Self::Success => "Success",
            Self::PartialSuccess => "Partial",
            Self::Failed => "Failed",
            Self::Skipped => "Skipped",
            Self::Cancelled => "Cancelled",
        }
    }
}

/// A backup run record.
#[derive(Debug, Clone)]
pub struct BackupRun {
    pub schedule_id: u32,
    pub result: RunResult,
    pub bytes_backed: u64,
    pub files_count: u64,
    pub started_ns: u64,
    pub duration_ms: u64,
}

/// A backup schedule.
#[derive(Debug, Clone)]
pub struct BackupSchedule {
    pub id: u32,
    pub name: String,
    pub source_path: String,
    pub destination: String,
    pub backup_type: BackupType,
    pub frequency: BackupFrequency,
    pub retention_count: u32,
    pub enabled: bool,
    pub last_run_ns: u64,
    pub run_count: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_SCHEDULES: usize = 50;
const MAX_HISTORY: usize = 500;

struct State {
    schedules: Vec<BackupSchedule>,
    history: Vec<BackupRun>,
    next_id: u32,
    total_runs: u64,
    total_successful: u64,
    total_failed: u64,
    total_bytes_backed: u64,
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
        schedules: alloc::vec![
            BackupSchedule {
                id: 1, name: String::from("Daily Home"),
                source_path: String::from("/home"), destination: String::from("/backup/daily"),
                backup_type: BackupType::Incremental, frequency: BackupFrequency::Daily,
                retention_count: 30, enabled: true, last_run_ns: 0, run_count: 0,
            },
        ],
        history: Vec::new(),
        next_id: 2,
        total_runs: 0,
        total_successful: 0,
        total_failed: 0,
        total_bytes_backed: 0,
        ops: 0,
    });
}

/// Create a new backup schedule.
pub fn create_schedule(name: &str, source: &str, dest: &str, btype: BackupType, freq: BackupFrequency, retention: u32) -> KernelResult<u32> {
    with_state(|state| {
        if state.schedules.len() >= MAX_SCHEDULES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.schedules.push(BackupSchedule {
            id, name: String::from(name),
            source_path: String::from(source), destination: String::from(dest),
            backup_type: btype, frequency: freq, retention_count: retention,
            enabled: true, last_run_ns: 0, run_count: 0,
        });
        Ok(id)
    })
}

/// Delete a schedule.
pub fn delete_schedule(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.schedules.len();
        state.schedules.retain(|s| s.id != id);
        if state.schedules.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Enable/disable a schedule.
pub fn set_enabled(id: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let sched = state.schedules.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        sched.enabled = enabled;
        Ok(())
    })
}

/// Run a backup now (simulate).
pub fn run_now(id: u32, result: RunResult, bytes: u64, files: u64) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let sched = state.schedules.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        sched.last_run_ns = now;
        sched.run_count += 1;

        if state.history.len() >= MAX_HISTORY { state.history.remove(0); }
        state.history.push(BackupRun {
            schedule_id: id, result, bytes_backed: bytes,
            files_count: files, started_ns: now, duration_ms: 0,
        });

        state.total_runs += 1;
        match result {
            RunResult::Success | RunResult::PartialSuccess => {
                state.total_successful += 1;
                state.total_bytes_backed += bytes;
            }
            RunResult::Failed => state.total_failed += 1,
            _ => {}
        }
        Ok(())
    })
}

/// Get run history for a schedule.
pub fn get_history(schedule_id: u32, max: usize) -> Vec<BackupRun> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut runs: Vec<BackupRun> = s.history.iter()
            .filter(|r| r.schedule_id == schedule_id)
            .cloned()
            .collect();
        runs.reverse();
        runs.truncate(max);
        runs
    })
}

/// List all schedules.
pub fn list_schedules() -> Vec<BackupSchedule> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.schedules.clone())
}

/// Statistics: (schedule_count, history_size, total_runs, total_successful, total_failed, total_bytes, ops).
pub fn stats() -> (usize, usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.schedules.len(), s.history.len(), s.total_runs, s.total_successful, s.total_failed, s.total_bytes_backed, s.ops),
        None => (0, 0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("backupsched::self_test() — running tests...");
    init_defaults();

    // 1: Default schedule.
    assert_eq!(list_schedules().len(), 1);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Create schedule.
    let id = create_schedule("Weekly Docs", "/documents", "/backup/weekly", BackupType::Full, BackupFrequency::Weekly, 8).expect("create");
    assert_eq!(list_schedules().len(), 2);
    crate::serial_println!("  [2/8] create: OK");

    // 3: Run backup.
    run_now(id, RunResult::Success, 500_000_000, 1500).expect("run");
    let hist = get_history(id, 10);
    assert_eq!(hist.len(), 1);
    assert_eq!(hist[0].result, RunResult::Success);
    crate::serial_println!("  [3/8] run: OK");

    // 4: Multiple runs.
    run_now(1, RunResult::Success, 100_000_000, 200).expect("run2");
    run_now(1, RunResult::Failed, 0, 0).expect("run3");
    let hist = get_history(1, 10);
    assert_eq!(hist.len(), 2);
    crate::serial_println!("  [4/8] multiple runs: OK");

    // 5: Disable.
    set_enabled(id, false).expect("disable");
    let scheds = list_schedules();
    let s = scheds.iter().find(|s| s.id == id).expect("find");
    assert!(!s.enabled);
    crate::serial_println!("  [5/8] disable: OK");

    // 6: Enable.
    set_enabled(id, true).expect("enable");
    crate::serial_println!("  [6/8] enable: OK");

    // 7: Delete.
    delete_schedule(id).expect("delete");
    assert_eq!(list_schedules().len(), 1);
    crate::serial_println!("  [7/8] delete: OK");

    // 8: Stats.
    let (scheds, hist, runs, success, failed, bytes, ops) = stats();
    assert_eq!(scheds, 1);
    assert_eq!(hist, 3);
    assert_eq!(runs, 3);
    assert_eq!(success, 2);
    assert_eq!(failed, 1);
    assert!(bytes > 0);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("backupsched::self_test() — all 8 tests passed");
}
