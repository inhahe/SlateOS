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

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

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

    /// How long to wait between runs, in nanoseconds.
    ///
    /// `None` means the frequency never comes due on its own — `Manual`
    /// schedules run only when the operator asks. That is a genuinely
    /// different state from "due after a very long time", so it is not
    /// expressible as a large interval and callers must handle it.
    ///
    /// `Monthly` is a flat 30 days. There is no calendar here — the kernel
    /// has an uptime counter, not a date — so a month is a fixed span rather
    /// than "the same day-of-month next month".
    #[must_use]
    pub fn interval_ns(self) -> Option<u64> {
        const HOUR_NS: u64 = 3_600 * 1_000_000_000;
        match self {
            Self::Hourly => Some(HOUR_NS),
            Self::Daily => Some(24 * HOUR_NS),
            Self::Weekly => Some(7 * 24 * HOUR_NS),
            Self::Monthly => Some(30 * 24 * HOUR_NS),
            Self::Manual => None,
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
    /// Uptime at which this schedule last ran, or `None` if it never has.
    ///
    /// This is deliberately not a `u64` with `0` meaning "never": `0` is a
    /// *legal* uptime — the first nanoseconds after boot — so the sentinel
    /// and a real timestamp are indistinguishable. With the old encoding a
    /// never-run schedule looked like one that ran at boot, which made
    /// [`due_schedules`] report it as "ran recently, not due" for a full
    /// interval after every reboot: the backup that had never happened was
    /// the one the scheduler was most confident it could skip.
    pub last_run_ns: Option<u64>,
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
    if guard.is_some() {
        return;
    }
    *guard = Some(State {
        schedules: alloc::vec![BackupSchedule {
            id: 1,
            name: String::from("Daily Home"),
            source_path: String::from("/home"),
            destination: String::from("/backup/daily"),
            backup_type: BackupType::Incremental,
            frequency: BackupFrequency::Daily,
            retention_count: 30,
            enabled: true,
            last_run_ns: None,
            run_count: 0,
        },],
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
pub fn create_schedule(
    name: &str,
    source: &str,
    dest: &str,
    btype: BackupType,
    freq: BackupFrequency,
    retention: u32,
) -> KernelResult<u32> {
    with_state(|state| {
        if state.schedules.len() >= MAX_SCHEDULES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.schedules.push(BackupSchedule {
            id,
            name: String::from(name),
            source_path: String::from(source),
            destination: String::from(dest),
            backup_type: btype,
            frequency: freq,
            retention_count: retention,
            enabled: true,
            last_run_ns: None,
            run_count: 0,
        });
        Ok(id)
    })
}

/// Delete a schedule.
pub fn delete_schedule(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.schedules.len();
        state.schedules.retain(|s| s.id != id);
        if state.schedules.len() == before {
            return Err(KernelError::NotFound);
        }
        Ok(())
    })
}

/// Enable/disable a schedule.
pub fn set_enabled(id: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let sched = state
            .schedules
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        sched.enabled = enabled;
        Ok(())
    })
}

/// Run a backup now (simulate).
pub fn run_now(id: u32, result: RunResult, bytes: u64, files: u64) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let sched = state
            .schedules
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        sched.last_run_ns = Some(now);
        sched.run_count += 1;

        if state.history.len() >= MAX_HISTORY {
            state.history.remove(0);
        }
        state.history.push(BackupRun {
            schedule_id: id,
            result,
            bytes_backed: bytes,
            files_count: files,
            started_ns: now,
            duration_ms: 0,
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
        let mut runs: Vec<BackupRun> = s
            .history
            .iter()
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
    STATE
        .lock()
        .as_ref()
        .map_or(Vec::new(), |s| s.schedules.clone())
}

/// Ids of the enabled schedules whose configured frequency has come due.
///
/// A schedule is due when it is enabled, its frequency has an interval at all
/// (`Manual` never comes due by itself), and either it has never run or at
/// least one interval has elapsed since it last did.
///
/// Until this existed, [`BackupSchedule::frequency`] was recorded, displayed,
/// and never acted on: every schedule was effectively `Manual` regardless of
/// what the operator set, because nothing in the tree ever compared a
/// frequency against a clock.
#[must_use]
pub fn due_schedules() -> Vec<u32> {
    let now_ns = crate::hpet::elapsed_ns();
    let guard = STATE.lock();
    let Some(state) = guard.as_ref() else {
        return Vec::new();
    };
    state
        .schedules
        .iter()
        .filter(|s| {
            if !s.enabled {
                return false;
            }
            let Some(interval_ns) = s.frequency.interval_ns() else {
                return false;
            };
            match s.last_run_ns {
                // Never run: due now. This is the case the `Option` exists
                // for — the old `0` sentinel made a never-run schedule look
                // like one that ran at uptime zero, so it stayed "not due"
                // for a whole interval after each boot.
                None => true,
                Some(last) => now_ns.saturating_sub(last) >= interval_ns,
            }
        })
        .map(|s| s.id)
        .collect()
}

/// Statistics: (schedule_count, history_size, total_runs, total_successful, total_failed, total_bytes, ops).
pub fn stats() -> (usize, usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (
            s.schedules.len(),
            s.history.len(),
            s.total_runs,
            s.total_successful,
            s.total_failed,
            s.total_bytes_backed,
            s.ops,
        ),
        None => (0, 0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run the module's self-test suite against a table of its own.
///
/// The suite mutates module state and asserts exact contents, and it used to
/// do that to the *live* table -- which, since it is also a kernel-shell
/// subcommand, changed or destroyed whatever the user had here and then
/// reported success.  The live state is moved aside for the duration and put
/// back afterwards; `crate::fs::selftest` records why this shape rather than
/// the alternatives.
///
/// The pristine value is `None` rather than a table: this module initialises
/// lazily, and `None` is exactly what a fresh boot holds.
pub fn self_test() {
    // `OPS` is a lock-free mirror of `state.ops`, which lives *inside* the
    // table. `with_pristine` restores the table and so restores `state.ops`,
    // but it cannot know about the mirror -- leave it and the two disagree
    // permanently, with `<module> stats` reporting the suite's activity as
    // the user's.
    let saved_ops = OPS.load(Ordering::Relaxed);
    crate::fs::selftest::with_pristine(&STATE, None, self_test_inner);
    OPS.store(saved_ops, Ordering::Relaxed);
}

fn self_test_inner() {
    crate::serial_println!("backupsched::self_test() — running tests...");
    init_defaults();

    // 1: Default schedule.
    assert_eq!(list_schedules().len(), 1);
    let seeded = list_schedules();
    assert_eq!(
        seeded[0].last_run_ns, None,
        "a seeded schedule has never run; `0` is a legal uptime, not `never`"
    );
    assert!(
        due_schedules().contains(&1),
        "an enabled schedule that has never run is due immediately"
    );
    crate::serial_println!("  [1/10] defaults: OK");

    // 2: Create schedule.
    let id = create_schedule(
        "Weekly Docs",
        "/documents",
        "/backup/weekly",
        BackupType::Full,
        BackupFrequency::Weekly,
        8,
    )
    .expect("create");
    assert_eq!(list_schedules().len(), 2);
    crate::serial_println!("  [2/10] create: OK");

    // 3: Run backup.
    run_now(id, RunResult::Success, 500_000_000, 1500).expect("run");
    let hist = get_history(id, 10);
    assert_eq!(hist.len(), 1);
    assert_eq!(hist[0].result, RunResult::Success);
    crate::serial_println!("  [3/10] run: OK");

    // 4: Multiple runs.
    run_now(1, RunResult::Success, 100_000_000, 200).expect("run2");
    run_now(1, RunResult::Failed, 0, 0).expect("run3");
    let hist = get_history(1, 10);
    assert_eq!(hist.len(), 2);
    crate::serial_println!("  [4/10] multiple runs: OK");

    // 5: Disable.
    set_enabled(id, false).expect("disable");
    let scheds = list_schedules();
    let s = scheds.iter().find(|s| s.id == id).expect("find");
    assert!(!s.enabled);
    crate::serial_println!("  [5/10] disable: OK");

    // 6: Enable.
    set_enabled(id, true).expect("enable");
    crate::serial_println!("  [6/10] enable: OK");

    // 7: Delete.
    delete_schedule(id).expect("delete");
    assert_eq!(list_schedules().len(), 1);
    crate::serial_println!("  [7/10] delete: OK");

    // 8: Stats.
    let (scheds, hist, runs, success, failed, bytes, ops) = stats();
    assert_eq!(scheds, 1);
    assert_eq!(hist, 3);
    assert_eq!(runs, 3);
    assert_eq!(success, 2);
    assert_eq!(failed, 1);
    assert!(bytes > 0);
    assert!(ops > 0);
    crate::serial_println!("  [8/10] stats: OK");

    // 9: Running a schedule clears its due-ness for one interval.
    //
    // Schedule 1 is Daily and was run in test 4, so unless this machine has
    // been up for a day it is not due again. The test asserts the transition
    // rather than a fixed answer: never-run was due in test 1, and the same
    // schedule is not due now.
    assert!(
        list_schedules()[0].last_run_ns.is_some(),
        "run_now must record when it ran"
    );
    assert!(
        !due_schedules().contains(&1),
        "a Daily schedule run moments ago is not due again yet"
    );
    crate::serial_println!("  [9/10] run clears due: OK");

    // 10: Frequency actually gates. A Manual schedule never comes due on its
    // own, and a disabled one never comes due at all -- both regardless of
    // having never run, which is the condition that makes every other
    // schedule due.
    let manual = create_schedule(
        "On Demand",
        "/srv",
        "/backup/manual",
        BackupType::Mirror,
        BackupFrequency::Manual,
        1,
    )
    .expect("create manual");
    assert_eq!(BackupFrequency::Manual.interval_ns(), None);
    assert!(
        !due_schedules().contains(&manual),
        "a Manual schedule is never due on its own, even having never run"
    );

    let hourly = create_schedule(
        "Hourly Var",
        "/var",
        "/backup/hourly",
        BackupType::Incremental,
        BackupFrequency::Hourly,
        4,
    )
    .expect("create hourly");
    assert!(
        due_schedules().contains(&hourly),
        "an enabled Hourly schedule that has never run is due"
    );
    set_enabled(hourly, false).expect("disable hourly");
    assert!(
        !due_schedules().contains(&hourly),
        "a disabled schedule is never due"
    );

    delete_schedule(manual).expect("cleanup manual");
    delete_schedule(hourly).expect("cleanup hourly");
    assert_eq!(list_schedules().len(), 1, "test 10 must clean up after itself");
    crate::serial_println!("  [10/10] frequency gating: OK");

    crate::serial_println!("backupsched::self_test() — all 10 tests passed");
}
