//! Auto Fix — automated problem detection and repair.
//!
//! Scans for common system issues (orphaned files, broken shortcuts,
//! registry inconsistencies, corrupted caches) and offers automatic repair.
//!
//! ## Architecture
//!
//! ```text
//! Scheduled or manual scan
//!   → autofix::scan() → list of detected issues
//!   → autofix::fix(issue_id) → attempt repair
//!   → autofix::fix_all() → repair all fixable issues
//!
//! Integration:
//!   → sysdiag (system diagnostics)
//!   → health (filesystem health)
//!   → storageclean (cleanup)
//!   → crashreport (crash recovery)
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

/// Issue severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
    Critical,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "Info",
            Self::Warning => "Warning",
            Self::Error => "Error",
            Self::Critical => "Critical",
        }
    }
}

/// Issue category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueCategory {
    OrphanedFiles,
    BrokenShortcuts,
    CorruptedCache,
    MissingDependency,
    DiskErrors,
    PermissionErrors,
    ConfigInconsistency,
    TempFileAccumulation,
    ServiceFailure,
    DriverIssue,
}

impl IssueCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::OrphanedFiles => "Orphaned Files",
            Self::BrokenShortcuts => "Broken Shortcuts",
            Self::CorruptedCache => "Corrupted Cache",
            Self::MissingDependency => "Missing Dependency",
            Self::DiskErrors => "Disk Errors",
            Self::PermissionErrors => "Permission Errors",
            Self::ConfigInconsistency => "Config Inconsistency",
            Self::TempFileAccumulation => "Temp File Accumulation",
            Self::ServiceFailure => "Service Failure",
            Self::DriverIssue => "Driver Issue",
        }
    }
}

/// Fix status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixStatus {
    Detected,
    FixAvailable,
    Fixed,
    CannotFix,
    Ignored,
}

impl FixStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Detected => "Detected",
            Self::FixAvailable => "Fix Available",
            Self::Fixed => "Fixed",
            Self::CannotFix => "Cannot Fix",
            Self::Ignored => "Ignored",
        }
    }
}

/// A detected issue.
#[derive(Debug, Clone)]
pub struct Issue {
    pub id: u32,
    pub category: IssueCategory,
    pub severity: Severity,
    pub description: String,
    pub status: FixStatus,
    pub detected_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_ISSUES: usize = 200;

struct State {
    issues: Vec<Issue>,
    next_id: u32,
    total_scans: u64,
    total_fixes: u64,
    total_ignored: u64,
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
        issues: Vec::new(),
        next_id: 1,
        total_scans: 0,
        total_fixes: 0,
        total_ignored: 0,
        ops: 0,
    });
}

/// Run a system scan. Returns number of issues found.
pub fn scan() -> KernelResult<usize> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        state.total_scans += 1;
        // Simulate finding common issues.
        let simulated = [
            (IssueCategory::TempFileAccumulation, Severity::Info, "Temporary files older than 7 days"),
            (IssueCategory::CorruptedCache, Severity::Warning, "Thumbnail cache inconsistency detected"),
            (IssueCategory::BrokenShortcuts, Severity::Info, "2 shortcuts point to missing targets"),
        ];
        let mut found = 0;
        for (cat, sev, desc) in &simulated {
            // Don't re-add if same category already detected and not fixed.
            if state.issues.iter().any(|i| i.category == *cat && i.status == FixStatus::Detected) {
                continue;
            }
            if state.issues.len() >= MAX_ISSUES { break; }
            let id = state.next_id;
            state.next_id += 1;
            state.issues.push(Issue {
                id, category: *cat, severity: *sev,
                description: String::from(*desc),
                status: FixStatus::FixAvailable,
                detected_ns: now,
            });
            found += 1;
        }
        Ok(found)
    })
}

/// Fix a specific issue.
pub fn fix(issue_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let issue = state.issues.iter_mut().find(|i| i.id == issue_id)
            .ok_or(KernelError::NotFound)?;
        match issue.status {
            FixStatus::FixAvailable | FixStatus::Detected => {
                issue.status = FixStatus::Fixed;
                state.total_fixes += 1;
                Ok(())
            }
            FixStatus::Fixed => Ok(()), // Already fixed.
            FixStatus::CannotFix => Err(KernelError::NotSupported),
            FixStatus::Ignored => {
                issue.status = FixStatus::Fixed;
                state.total_fixes += 1;
                Ok(())
            }
        }
    })
}

/// Fix all fixable issues.
pub fn fix_all() -> KernelResult<usize> {
    with_state(|state| {
        let mut fixed = 0;
        for issue in state.issues.iter_mut() {
            if issue.status == FixStatus::FixAvailable || issue.status == FixStatus::Detected {
                issue.status = FixStatus::Fixed;
                state.total_fixes += 1;
                fixed += 1;
            }
        }
        Ok(fixed)
    })
}

/// Ignore an issue.
pub fn ignore(issue_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let issue = state.issues.iter_mut().find(|i| i.id == issue_id)
            .ok_or(KernelError::NotFound)?;
        issue.status = FixStatus::Ignored;
        state.total_ignored += 1;
        Ok(())
    })
}

/// Clear fixed/ignored issues.
pub fn clear_resolved() -> KernelResult<usize> {
    with_state(|state| {
        let before = state.issues.len();
        state.issues.retain(|i| i.status != FixStatus::Fixed && i.status != FixStatus::Ignored);
        Ok(before - state.issues.len())
    })
}

/// List issues, optionally filtered by status.
pub fn list_issues(status_filter: Option<FixStatus>) -> Vec<Issue> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        match status_filter {
            Some(f) => s.issues.iter().filter(|i| i.status == f).cloned().collect(),
            None => s.issues.clone(),
        }
    })
}

/// Statistics: (issue_count, total_scans, total_fixes, total_ignored, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.issues.len(), s.total_scans, s.total_fixes, s.total_ignored, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("autofix::self_test() — running tests...");
    init_defaults();

    // 1: No issues initially.
    assert_eq!(list_issues(None).len(), 0);
    crate::serial_println!("  [1/8] no issues: OK");

    // 2: Scan finds issues.
    let found = scan().expect("scan");
    assert_eq!(found, 3);
    assert_eq!(list_issues(None).len(), 3);
    crate::serial_println!("  [2/8] scan: OK");

    // 3: Fix one issue.
    let issues = list_issues(None);
    fix(issues[0].id).expect("fix");
    let fixed = list_issues(Some(FixStatus::Fixed));
    assert_eq!(fixed.len(), 1);
    crate::serial_println!("  [3/8] fix one: OK");

    // 4: Ignore one issue.
    ignore(issues[1].id).expect("ignore");
    let ignored = list_issues(Some(FixStatus::Ignored));
    assert_eq!(ignored.len(), 1);
    crate::serial_println!("  [4/8] ignore: OK");

    // 5: Fix all remaining.
    let count = fix_all().expect("fix_all");
    assert_eq!(count, 1); // Only 1 remaining unfixed.
    crate::serial_println!("  [5/8] fix all: OK");

    // 6: Clear resolved.
    let cleared = clear_resolved().expect("clear");
    assert_eq!(cleared, 3);
    assert_eq!(list_issues(None).len(), 0);
    crate::serial_println!("  [6/8] clear: OK");

    // 7: Re-scan doesn't duplicate.
    let found = scan().expect("scan2");
    assert_eq!(found, 3);
    let found2 = scan().expect("scan3");
    assert_eq!(found2, 0); // Already detected.
    crate::serial_println!("  [7/8] no duplicates: OK");

    // 8: Stats.
    let (issues, scans, fixes, ignored, ops) = stats();
    assert_eq!(issues, 3);
    assert!(scans >= 3);
    assert!(fixes >= 2);
    assert_eq!(ignored, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("autofix::self_test() — all 8 tests passed");
}
