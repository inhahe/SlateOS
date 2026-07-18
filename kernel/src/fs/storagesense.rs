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

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

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
    last_run_ns: u64,
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
        CleanupPolicy { category: CleanupCategory::TempFiles, enabled: true, max_age_days: 7, estimated_bytes: 50_000_000, last_freed_bytes: 0 },
        CleanupPolicy { category: CleanupCategory::RecycleBin, enabled: true, max_age_days: 30, estimated_bytes: 200_000_000, last_freed_bytes: 0 },
        CleanupPolicy { category: CleanupCategory::Downloads, enabled: false, max_age_days: 60, estimated_bytes: 500_000_000, last_freed_bytes: 0 },
        CleanupPolicy { category: CleanupCategory::ThumbnailCache, enabled: true, max_age_days: 14, estimated_bytes: 100_000_000, last_freed_bytes: 0 },
        CleanupPolicy { category: CleanupCategory::SystemCache, enabled: true, max_age_days: 30, estimated_bytes: 300_000_000, last_freed_bytes: 0 },
        CleanupPolicy { category: CleanupCategory::LogFiles, enabled: true, max_age_days: 30, estimated_bytes: 50_000_000, last_freed_bytes: 0 },
        CleanupPolicy { category: CleanupCategory::BrowserCache, enabled: false, max_age_days: 14, estimated_bytes: 200_000_000, last_freed_bytes: 0 },
        CleanupPolicy { category: CleanupCategory::PackageCache, enabled: true, max_age_days: 60, estimated_bytes: 400_000_000, last_freed_bytes: 0 },
        CleanupPolicy { category: CleanupCategory::OldUpdates, enabled: true, max_age_days: 90, estimated_bytes: 1_000_000_000, last_freed_bytes: 0 },
    ]
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        policies: default_policies(),
        schedule: Schedule::Weekly,
        low_space_threshold_mb: 1024,
        total_runs: 0,
        total_bytes_freed: 0,
        last_run_ns: 0,
        ops: 0,
    });
}

/// Estimate total saveable bytes from enabled categories.
pub fn estimate_savings() -> KernelResult<u64> {
    with_state(|state| {
        let total: u64 = state.policies.iter()
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
            if !policy.enabled { continue; }
            // Simulate cleanup: free the estimated amount.
            let freed = policy.estimated_bytes;
            policy.last_freed_bytes = freed;
            total_freed = total_freed.saturating_add(freed);
        }
        state.total_runs += 1;
        state.total_bytes_freed = state.total_bytes_freed.saturating_add(total_freed);
        state.last_run_ns = now;
        Ok(total_freed)
    })
}

/// Run cleanup for a specific category.
pub fn run_category(category: CleanupCategory) -> KernelResult<u64> {
    with_state(|state| {
        let policy = state.policies.iter_mut().find(|p| p.category == category)
            .ok_or(KernelError::NotFound)?;
        let freed = policy.estimated_bytes;
        policy.last_freed_bytes = freed;
        state.total_bytes_freed = state.total_bytes_freed.saturating_add(freed);
        state.total_runs += 1;
        state.last_run_ns = crate::hpet::elapsed_ns();
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
        let policy = state.policies.iter_mut().find(|p| p.category == category)
            .ok_or(KernelError::NotFound)?;
        policy.enabled = enabled;
        Ok(())
    })
}

/// Set max age for a category.
pub fn set_max_age(category: CleanupCategory, days: u32) -> KernelResult<()> {
    with_state(|state| {
        let policy = state.policies.iter_mut().find(|p| p.category == category)
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
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.policies.clone())
}

/// Get current schedule.
pub fn get_schedule() -> Schedule {
    STATE.lock().as_ref().map_or(Schedule::Manual, |s| s.schedule)
}

/// Format bytes to human-readable string.
pub fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{}.{} GB", bytes / 1_073_741_824, (bytes % 1_073_741_824) / 107_374_183)
    } else if bytes >= 1_048_576 {
        format!("{}.{} MB", bytes / 1_048_576, (bytes % 1_048_576) / 104_858)
    } else if bytes >= 1_024 {
        format!("{} KB", bytes / 1_024)
    } else {
        format!("{} B", bytes)
    }
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

pub fn self_test() {
    crate::serial_println!("storagesense::self_test() — running tests...");
    init_defaults();

    // 1: Default policies.
    let policies = list_policies();
    assert_eq!(policies.len(), 9);
    assert!(policies[0].enabled); // TempFiles
    assert!(!policies[2].enabled); // Downloads disabled by default
    crate::serial_println!("  [1/8] default policies: OK");

    // 2: Estimate savings.
    let est = estimate_savings().expect("estimate");
    assert!(est > 0);
    crate::serial_println!("  [2/8] estimate: OK ({} bytes)", est);

    // 3: Run cleanup.
    let freed = run_cleanup().expect("cleanup");
    assert!(freed > 0);
    crate::serial_println!("  [3/8] cleanup: OK ({} freed)", format_bytes(freed));

    // 4: Category cleanup.
    let freed = run_category(CleanupCategory::TempFiles).expect("cat");
    assert_eq!(freed, 50_000_000);
    crate::serial_println!("  [4/8] category cleanup: OK");

    // 5: Set schedule.
    set_schedule(Schedule::Daily).expect("sched");
    assert_eq!(get_schedule(), Schedule::Daily);
    crate::serial_println!("  [5/8] schedule: OK");

    // 6: Enable/disable category.
    set_category_enabled(CleanupCategory::Downloads, true).expect("enable");
    let policies = list_policies();
    assert!(policies[2].enabled);
    crate::serial_println!("  [6/8] category toggle: OK");

    // 7: Format bytes.
    assert_eq!(format_bytes(1_073_741_824), "1.0 GB");
    assert_eq!(format_bytes(5_242_880), "5.0 MB");
    crate::serial_println!("  [7/8] format bytes: OK");

    // 8: Stats.
    let (policies, runs, freed, ops) = stats();
    assert_eq!(policies, 9);
    assert!(runs >= 2);
    assert!(freed > 0);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("storagesense::self_test() — all 8 tests passed");
}
