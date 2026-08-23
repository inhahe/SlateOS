//! Storage Sense — smart automated storage cleanup.
//!
//! Automatically frees disk space by cleaning temporary files, old downloads,
//! recycle bin contents, and system caches based on configurable policies.
//!
//! ## Architecture
//!
//! ```text
//! Disk space low or scheduled run
//!   → storagesense::run_cleanup() → free space
//!   → storagesense::estimate_savings() → preview
//!
//! Configuration
//!   → storagesense::set_policy(policy)
//!   → storagesense::set_schedule(interval)
//!
//! Integration:
//!   → storageclean (manual cleanup)
//!   → trash (recycle bin)
//!   → thumbcache (thumbnail cache)
//!   → cache (system caches)
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

/// Cleanup category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupCategory {
    TempFiles,
    RecycleBin,
    Downloads,
    ThumbnailCache,
    SystemCache,
    LogFiles,
    BrowserCache,
    PackageCache,
    OldUpdates,
}

impl CleanupCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::TempFiles => "Temporary Files",
            Self::RecycleBin => "Recycle Bin",
            Self::Downloads => "Downloads",
            Self::ThumbnailCache => "Thumbnail Cache",
            Self::SystemCache => "System Cache",
            Self::LogFiles => "Log Files",
            Self::BrowserCache => "Browser Cache",
            Self::PackageCache => "Package Cache",
            Self::OldUpdates => "Old Updates",
        }
    }
}

/// Cleanup schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schedule {
    /// Never run automatically.
    Manual,
    /// Run daily.
    Daily,
    /// Run weekly.
    Weekly,
    /// Run monthly.
    Monthly,
    /// Run when disk space is low.
    OnLowSpace,
}

impl Schedule {
    pub fn label(self) -> &'static str {
        match self {
            Self::Manual => "Manual",
            Self::Daily => "Daily",
            Self::Weekly => "Weekly",
            Self::Monthly => "Monthly",
            Self::OnLowSpace => "On Low Space",
        }
    }

    /// How long to wait between runs, in nanoseconds.
    ///
    /// `None` means the schedule is not time-based at all, which covers two
    /// unlike cases on purpose: `Manual` never comes due by itself, and
    /// `OnLowSpace` comes due on a condition rather than a clock. Neither has
    /// an interval to compare against, and [`due_reason`] distinguishes them.
    ///
    /// `Monthly` is a flat 30 days. There is no calendar here — the kernel has
    /// an uptime counter, not a date — so "monthly" can only mean an interval,
    /// and 30 days is the least surprising one to pick.
    #[must_use]
    pub fn interval_ns(self) -> Option<u64> {
        const DAY_NS: u64 = 24 * 3_600 * 1_000_000_000;
        match self {
            Self::Daily => Some(DAY_NS),
            Self::Weekly => Some(7 * DAY_NS),
            Self::Monthly => Some(30 * DAY_NS),
            Self::Manual | Self::OnLowSpace => None,
        }
    }
}

/// Why a cleanup run is currently due.
///
/// Returned by [`due_reason`]. Distinguishing the reasons matters because they
/// have different remedies: an elapsed interval is satisfied by running once,
/// whereas low space may still be low afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DueReason {
    /// The schedule is time-based and cleanup has never run.
    NeverRun,
    /// The schedule's interval has elapsed since the last run.
    IntervalElapsed {
        /// Nanoseconds since the last run.
        since_last_ns: u64,
        /// The interval that has been exceeded.
        interval_ns: u64,
    },
    /// The schedule is `OnLowSpace` and free space is below the threshold.
    LowSpace {
        /// Free space on `/`, in MiB.
        free_mb: u64,
        /// The configured threshold, in MiB.
        threshold_mb: u32,
    },
}

/// Policy for a cleanup category.
#[derive(Debug, Clone)]
pub struct CleanupPolicy {
    pub category: CleanupCategory,
    pub enabled: bool,
    /// Max age in days before cleanup (0 = always clean).
    pub max_age_days: u32,
    /// Estimated bytes that can be freed.
    pub estimated_bytes: u64,
    /// Bytes actually freed in last run.
    pub last_freed_bytes: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    policies: Vec<CleanupPolicy>,
    schedule: Schedule,
    /// Threshold in MB for low-space trigger.
    low_space_threshold_mb: u32,
    total_runs: u64,
    total_bytes_freed: u64,
    /// When cleanup last ran (ns since boot), or `None` if it never has.
    ///
    /// Not a `u64` with `0` meaning "never": this clock counts nanoseconds
    /// since boot and starts *at* zero, so `0` is a legal instant. Under a
    /// sentinel encoding a run that landed at uptime 0 would read back as
    /// never having happened, and [`due_reason`] would declare cleanup
    /// immediately due again — deleting the same files a second time on the
    /// one boot where the first pass had only just finished.
    last_run_ns: Option<u64>,
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

fn default_policies() -> Vec<CleanupPolicy> {
    alloc::vec![
        CleanupPolicy {
            category: CleanupCategory::TempFiles,
            enabled: true,
            max_age_days: 7,
            estimated_bytes: 50_000_000,
            last_freed_bytes: 0
        },
        CleanupPolicy {
            category: CleanupCategory::RecycleBin,
            enabled: true,
            max_age_days: 30,
            estimated_bytes: 200_000_000,
            last_freed_bytes: 0
        },
        CleanupPolicy {
            category: CleanupCategory::Downloads,
            enabled: false,
            max_age_days: 60,
            estimated_bytes: 500_000_000,
            last_freed_bytes: 0
        },
        CleanupPolicy {
            category: CleanupCategory::ThumbnailCache,
            enabled: true,
            max_age_days: 14,
            estimated_bytes: 100_000_000,
            last_freed_bytes: 0
        },
        CleanupPolicy {
            category: CleanupCategory::SystemCache,
            enabled: true,
            max_age_days: 30,
            estimated_bytes: 300_000_000,
            last_freed_bytes: 0
        },
        CleanupPolicy {
            category: CleanupCategory::LogFiles,
            enabled: true,
            max_age_days: 30,
            estimated_bytes: 50_000_000,
            last_freed_bytes: 0
        },
        CleanupPolicy {
            category: CleanupCategory::BrowserCache,
            enabled: false,
            max_age_days: 14,
            estimated_bytes: 200_000_000,
            last_freed_bytes: 0
        },
        CleanupPolicy {
            category: CleanupCategory::PackageCache,
            enabled: true,
            max_age_days: 60,
            estimated_bytes: 400_000_000,
            last_freed_bytes: 0
        },
        CleanupPolicy {
            category: CleanupCategory::OldUpdates,
            enabled: true,
            max_age_days: 90,
            estimated_bytes: 1_000_000_000,
            last_freed_bytes: 0
        },
    ]
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
        policies: default_policies(),
        schedule: Schedule::Weekly,
        low_space_threshold_mb: 1024,
        total_runs: 0,
        total_bytes_freed: 0,
        last_run_ns: None,
        ops: 0,
    });
}

/// Estimate total saveable bytes from enabled categories.
pub fn estimate_savings() -> KernelResult<u64> {
    with_state(|state| {
        let total: u64 = state
            .policies
            .iter()
            .filter(|p| p.enabled)
            .map(|p| p.estimated_bytes)
            .sum();
        Ok(total)
    })
}

/// Run cleanup on all enabled categories.
pub fn run_cleanup() -> KernelResult<u64> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let mut total_freed: u64 = 0;
        for policy in state.policies.iter_mut() {
            if !policy.enabled {
                continue;
            }
            // Simulate cleanup: free the estimated amount.
            let freed = policy.estimated_bytes;
            policy.last_freed_bytes = freed;
            total_freed = total_freed.saturating_add(freed);
        }
        state.total_runs += 1;
        state.total_bytes_freed = state.total_bytes_freed.saturating_add(total_freed);
        state.last_run_ns = Some(now);
        Ok(total_freed)
    })
}

/// Run cleanup for a specific category.
pub fn run_category(category: CleanupCategory) -> KernelResult<u64> {
    with_state(|state| {
        let policy = state
            .policies
            .iter_mut()
            .find(|p| p.category == category)
            .ok_or(KernelError::NotFound)?;
        let freed = policy.estimated_bytes;
        policy.last_freed_bytes = freed;
        state.total_bytes_freed = state.total_bytes_freed.saturating_add(freed);
        state.total_runs += 1;
        state.last_run_ns = Some(crate::hpet::elapsed_ns());
        Ok(freed)
    })
}

/// Set schedule.
pub fn set_schedule(schedule: Schedule) -> KernelResult<()> {
    with_state(|state| {
        state.schedule = schedule;
        Ok(())
    })
}

/// Enable/disable a category.
pub fn set_category_enabled(category: CleanupCategory, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let policy = state
            .policies
            .iter_mut()
            .find(|p| p.category == category)
            .ok_or(KernelError::NotFound)?;
        policy.enabled = enabled;
        Ok(())
    })
}

/// Set max age for a category.
pub fn set_max_age(category: CleanupCategory, days: u32) -> KernelResult<()> {
    with_state(|state| {
        let policy = state
            .policies
            .iter_mut()
            .find(|p| p.category == category)
            .ok_or(KernelError::NotFound)?;
        policy.max_age_days = days;
        Ok(())
    })
}

/// Set low space threshold in MB.
pub fn set_low_space_threshold(mb: u32) -> KernelResult<()> {
    with_state(|state| {
        state.low_space_threshold_mb = mb.clamp(100, 50_000);
        Ok(())
    })
}

/// List policies.
pub fn list_policies() -> Vec<CleanupPolicy> {
    STATE
        .lock()
        .as_ref()
        .map_or(Vec::new(), |s| s.policies.clone())
}

/// Get current schedule.
pub fn get_schedule() -> Schedule {
    STATE
        .lock()
        .as_ref()
        .map_or(Schedule::Manual, |s| s.schedule)
}

/// When cleanup last ran, in nanoseconds since boot; `None` if it never has.
#[must_use]
pub fn last_run_ns() -> Option<u64> {
    STATE.lock().as_ref().and_then(|s| s.last_run_ns)
}

/// The configured low-space threshold, in MiB.
#[must_use]
pub fn low_space_threshold_mb() -> u32 {
    STATE
        .lock()
        .as_ref()
        .map_or(0, |s| s.low_space_threshold_mb)
}

/// Whether a cleanup run is currently due, and why.
///
/// This is the function that makes the schedule mean something. Before it,
/// `schedule`, `low_space_threshold_mb` and `last_run_ns` were all recorded,
/// two of them settable from the shell, and *none* of them ever consulted: a
/// user could set `storagesense schedule daily`, see "Daily" echoed back
/// forever, and cleanup would still only ever run when they asked for it by
/// hand. Dead configuration is worse than absent configuration, because it
/// looks like a promise.
///
/// The disk query is deliberately outside the lock. `Vfs::statvfs` descends
/// into a filesystem driver and may take VFS locks of its own, and holding the
/// Storage Sense lock across that would invert against every path that calls in
/// here while holding one. The values read under the lock are copied out first.
///
/// A failed `statvfs` yields `None` rather than a default: "cannot tell how
/// much space is free" must not be reported as "space is low", because the
/// remedy for low space is to delete the user's files.
#[must_use]
pub fn due_reason() -> Option<DueReason> {
    let (schedule, last_run, threshold_mb) = {
        let guard = STATE.lock();
        let state = guard.as_ref()?;
        (
            state.schedule,
            state.last_run_ns,
            state.low_space_threshold_mb,
        )
    };

    if schedule == Schedule::OnLowSpace {
        // Lock released above; statvfs may take VFS locks.
        let info = crate::fs::vfs::Vfs::statvfs("/").ok()?;
        let free_mb = info
            .free_blocks
            .saturating_mul(info.block_size)
            .checked_div(1024 * 1024)?;
        return (free_mb < u64::from(threshold_mb)).then_some(DueReason::LowSpace {
            free_mb,
            threshold_mb,
        });
    }

    let interval_ns = schedule.interval_ns()?;
    let Some(last) = last_run else {
        return Some(DueReason::NeverRun);
    };
    let since_last_ns = crate::hpet::elapsed_ns().saturating_sub(last);
    (since_last_ns >= interval_ns).then_some(DueReason::IntervalElapsed {
        since_last_ns,
        interval_ns,
    })
}

/// Format bytes to human-readable string.
///
/// See [`crate::bytesize`]. Storage Sense and the disk-cleanup tool report on
/// the same directories, and before this they disagreed both on the digits and
/// on the unit names.
pub fn format_bytes(bytes: u64) -> String {
    crate::bytesize::iec(bytes)
}

/// Statistics: (policy_count, total_runs, total_bytes_freed, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.policies.len(), s.total_runs, s.total_bytes_freed, s.ops),
        None => (0, 0, 0, 0),
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
    crate::serial_println!("storagesense::self_test() — running tests...");
    init_defaults();

    // 1: Default policies, and the pristine "never run" state.
    let policies = list_policies();
    assert_eq!(policies.len(), 9);
    assert!(policies[0].enabled); // TempFiles
    assert!(!policies[2].enabled); // Downloads disabled by default
    // `0` is a legal uptime, so a never-run schedule must say `None` rather
    // than borrow an instant that means something else.
    assert_eq!(
        last_run_ns(),
        None,
        "pristine state has never run cleanup; `0` is a legal uptime, not `never`"
    );
    assert_eq!(get_schedule(), Schedule::Weekly);
    assert_eq!(
        due_reason(),
        Some(DueReason::NeverRun),
        "a time-based schedule that has never run is due immediately"
    );
    crate::serial_println!("  [1/11] default policies + never-run: OK");

    // 2: Estimate savings.
    let est = estimate_savings().expect("estimate");
    assert!(est > 0);
    crate::serial_println!("  [2/11] estimate: OK ({} bytes)", est);

    // 3: Run cleanup.
    let freed = run_cleanup().expect("cleanup");
    assert!(freed > 0);
    crate::serial_println!("  [3/11] cleanup: OK ({} freed)", format_bytes(freed));

    // 4: Category cleanup.
    let freed = run_category(CleanupCategory::TempFiles).expect("cat");
    assert_eq!(freed, 50_000_000);
    crate::serial_println!("  [4/11] category cleanup: OK");

    // 5: Set schedule.
    set_schedule(Schedule::Daily).expect("sched");
    assert_eq!(get_schedule(), Schedule::Daily);
    crate::serial_println!("  [5/11] schedule: OK");

    // 6: Enable/disable category.
    set_category_enabled(CleanupCategory::Downloads, true).expect("enable");
    let policies = list_policies();
    assert!(policies[2].enabled);
    crate::serial_println!("  [6/11] category toggle: OK");

    // 7: Format bytes.  `GiB`/`MiB` since the move onto `bytesize`: these are
    // 1024-based quantities and calling them GB/MB overstated the prefix by
    // ~7% per unit, which is the precise confusion the IEC names exist to end.
    assert_eq!(format_bytes(1_073_741_824), "1.0 GiB");
    assert_eq!(format_bytes(5_242_880), "5.0 MiB");
    crate::serial_println!("  [7/11] format bytes: OK");

    // 8: Stats.
    let (policies, runs, freed, ops) = stats();
    assert_eq!(policies, 9);
    assert!(runs >= 2);
    assert!(freed > 0);
    assert!(ops > 0);
    crate::serial_println!("  [8/11] stats: OK");

    // 9: A run clears the due state.  Tests 3 and 4 ran cleanup, so the
    // timestamp is now recorded and no interval has elapsed since.
    let last = last_run_ns().expect("a run above must have recorded its time");
    assert!(
        due_reason().is_none(),
        "a daily schedule that just ran is not due again"
    );
    set_schedule(Schedule::Monthly).expect("sched");
    assert!(
        due_reason().is_none(),
        "lengthening the interval cannot make a just-run schedule due"
    );
    crate::serial_println!("  [9/11] run clears due: OK (last run at {} ns)", last);

    // 10: The two non-time-based schedules.  `Manual` never comes due on its
    // own — that is the whole meaning of the setting, and the bug this suite
    // exists to prevent is a scheduler that ignores it and cleans anyway.
    set_schedule(Schedule::Manual).expect("sched");
    assert_eq!(Schedule::Manual.interval_ns(), None);
    assert!(
        due_reason().is_none(),
        "Manual must never come due by itself, even long after the last run"
    );
    // `OnLowSpace` is also intervalless, but for the opposite reason: it is
    // condition-driven, not never-driven.  Its due-ness depends on the disk, so
    // assert only what is deterministic — that it consults space, not a clock,
    // and so is never `NeverRun` or `IntervalElapsed`.
    assert_eq!(Schedule::OnLowSpace.interval_ns(), None);
    set_schedule(Schedule::OnLowSpace).expect("sched");
    assert!(
        matches!(due_reason(), None | Some(DueReason::LowSpace { .. })),
        "OnLowSpace must decide on free space, never on elapsed time"
    );
    assert_eq!(low_space_threshold_mb(), 1024);
    crate::serial_println!("  [10/11] Manual and OnLowSpace: OK");

    // 11: Intervals are ordered and distinct.  A `Monthly` that silently
    // equalled `Daily` would look correct in every other test here.
    let day = Schedule::Daily
        .interval_ns()
        .expect("daily has an interval");
    let week = Schedule::Weekly
        .interval_ns()
        .expect("weekly has an interval");
    let month = Schedule::Monthly
        .interval_ns()
        .expect("monthly has an interval");
    assert_eq!(day, 86_400 * 1_000_000_000);
    assert_eq!(week, 7 * day);
    assert_eq!(month, 30 * day);
    assert!(day < week && week < month);
    crate::serial_println!("  [11/11] schedule intervals: OK");

    crate::serial_println!("storagesense::self_test() — all 11 tests passed");
}
