//! Filesystem health monitoring and diagnostics.
//!
//! Aggregates health signals from multiple filesystem subsystems
//! (journal, cache, quota, integrity, reclaim, tmpwatch) into a
//! unified health report.  Detects anomalies and provides actionable
//! recommendations.
//!
//! ## Design Reference
//!
//! design.txt line 885: comprehensive system information explorer
//! including mounted drives, capacity, and free space.
//!
//! ## Architecture
//!
//! ```text
//! health::check()
//!   ├── check_mounts()      — verify mounted filesystems respond
//!   ├── check_space()       — warn on low disk space
//!   ├── check_journal()     — verify journal is healthy
//!   ├── check_cache()       — verify cache hit rates
//!   ├── check_quotas()      — detect over-quota users
//!   ├── check_integrity()   — check for known violations
//!   ├── check_handles()     — detect handle leaks
//!   ├── check_trash()       — check recycle bin size
//!   ├── check_tmpwatch()    — report temp cleanup
//!   └── check_reclaim()     — report reclaim activity
//! → HealthReport with overall status + per-check details
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::{vec, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::KernelResult;
use crate::fs::Vfs;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Overall health status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    /// Everything is fine.
    Healthy,
    /// Some non-critical issues detected.
    Warning,
    /// Critical issues detected.
    Critical,
}

impl HealthStatus {
    /// Return the name of this status.
    pub fn name(self) -> &'static str {
        match self {
            Self::Healthy => "HEALTHY",
            Self::Warning => "WARNING",
            Self::Critical => "CRITICAL",
        }
    }

    /// Merge two statuses, keeping the worse one.
    fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Critical, _) | (_, Self::Critical) => Self::Critical,
            (Self::Warning, _) | (_, Self::Warning) => Self::Warning,
            _ => Self::Healthy,
        }
    }
}

/// A single health check result.
#[derive(Debug, Clone)]
pub struct CheckResult {
    /// Name of the check.
    pub name: String,
    /// Status of this check.
    pub status: HealthStatus,
    /// Human-readable message.
    pub message: String,
    /// Optional recommendation.
    pub recommendation: Option<String>,
}

/// Complete health report.
#[derive(Debug, Clone)]
pub struct HealthReport {
    /// Overall status (worst of all checks).
    pub status: HealthStatus,
    /// Individual check results.
    pub checks: Vec<CheckResult>,
    /// Total checks run.
    pub total_checks: usize,
    /// Number of healthy checks.
    pub healthy: usize,
    /// Number of warnings.
    pub warnings: usize,
    /// Number of critical issues.
    pub critical: usize,
}

// ---------------------------------------------------------------------------
// Global stats
// ---------------------------------------------------------------------------

static CHECKS_RUN: AtomicU64 = AtomicU64::new(0);
static LAST_REPORT: Mutex<Option<HealthReport>> = Mutex::new(None);

/// Get the number of health checks run.
pub fn checks_run() -> u64 {
    CHECKS_RUN.load(Ordering::Relaxed)
}

/// Get the last health report.
pub fn last_report() -> Option<HealthReport> {
    LAST_REPORT.lock().clone()
}

// ---------------------------------------------------------------------------
// Health check runner
// ---------------------------------------------------------------------------

/// Run all health checks and return a report.
pub fn check() -> KernelResult<HealthReport> {
    let checks = vec![
        check_mounts(),
        check_space(),
        check_journal(),
        check_cache(),
        check_handles(),
        check_quotas(),
        check_integrity(),
        check_trash(),
        check_tmpwatch(),
        check_reclaim(),
    ];

    // Compute overall status.
    let mut overall = HealthStatus::Healthy;
    let mut healthy = 0usize;
    let mut warnings = 0usize;
    let mut critical = 0usize;

    for c in &checks {
        match c.status {
            HealthStatus::Healthy => healthy += 1,
            HealthStatus::Warning => warnings += 1,
            HealthStatus::Critical => critical += 1,
        }
        overall = overall.merge(c.status);
    }

    let report = HealthReport {
        status: overall,
        total_checks: checks.len(),
        healthy,
        warnings,
        critical,
        checks,
    };

    CHECKS_RUN.fetch_add(1, Ordering::Relaxed);
    *LAST_REPORT.lock() = Some(report.clone());

    serial_println!(
        "[health] Check complete: {} (H:{} W:{} C:{})",
        overall.name(),
        healthy,
        warnings,
        critical,
    );

    Ok(report)
}

// ---------------------------------------------------------------------------
// Individual checks
// ---------------------------------------------------------------------------

/// Check that filesystems are mounted and responding.
fn check_mounts() -> CheckResult {
    let mounts = Vfs::mounts();

    if mounts.is_empty() {
        CheckResult {
            name: String::from("Mounts"),
            status: HealthStatus::Critical,
            message: String::from("No filesystems mounted"),
            recommendation: Some(String::from("Mount a root filesystem")),
        }
    } else {
        // Verify root is accessible.
        let root_ok = Vfs::readdir("/").is_ok();
        if root_ok {
            CheckResult {
                name: String::from("Mounts"),
                status: HealthStatus::Healthy,
                message: alloc::format!("{} filesystem(s) mounted, root accessible", mounts.len()),
                recommendation: None,
            }
        } else {
            CheckResult {
                name: String::from("Mounts"),
                status: HealthStatus::Critical,
                message: String::from("Root filesystem not accessible"),
                recommendation: Some(String::from("Check root mount configuration")),
            }
        }
    }
}

/// Check disk space usage.
fn check_space() -> CheckResult {
    match Vfs::statvfs("/") {
        Ok(info) => {
            let total = info.total_blocks.saturating_mul(info.block_size);
            let free = info.free_blocks.saturating_mul(info.block_size);

            if total == 0 {
                return CheckResult {
                    name: String::from("Disk Space"),
                    status: HealthStatus::Warning,
                    message: String::from("Unable to determine disk capacity"),
                    recommendation: None,
                };
            }

            let used_pct = ((total.saturating_sub(free)) * 100) / total;

            if used_pct >= 95 {
                CheckResult {
                    name: String::from("Disk Space"),
                    status: HealthStatus::Critical,
                    message: alloc::format!("{}% used ({} free of {})",
                        used_pct,
                        crate::fs::usage::format_size(free),
                        crate::fs::usage::format_size(total),
                    ),
                    recommendation: Some(String::from("Run 'reclaim' to free space or delete unused files")),
                }
            } else if used_pct >= 85 {
                CheckResult {
                    name: String::from("Disk Space"),
                    status: HealthStatus::Warning,
                    message: alloc::format!("{}% used ({} free of {})",
                        used_pct,
                        crate::fs::usage::format_size(free),
                        crate::fs::usage::format_size(total),
                    ),
                    recommendation: Some(String::from("Consider freeing disk space")),
                }
            } else {
                CheckResult {
                    name: String::from("Disk Space"),
                    status: HealthStatus::Healthy,
                    message: alloc::format!("{}% used ({} free of {})",
                        used_pct,
                        crate::fs::usage::format_size(free),
                        crate::fs::usage::format_size(total),
                    ),
                    recommendation: None,
                }
            }
        }
        Err(_) => CheckResult {
            name: String::from("Disk Space"),
            status: HealthStatus::Warning,
            message: String::from("Unable to query filesystem info"),
            recommendation: None,
        },
    }
}

/// Check journal health.
fn check_journal() -> CheckResult {
    // journal::stats() returns (entry_count: usize, max_seq: u64).
    let (entry_count, max_seq) = crate::fs::journal::stats();

    if entry_count == 0 && max_seq == 0 {
        CheckResult {
            name: String::from("Journal"),
            status: HealthStatus::Healthy,
            message: String::from("Journal idle (no operations logged)"),
            recommendation: None,
        }
    } else if entry_count > 8000 {
        // Journal is getting large (near its capacity of ~10K).
        CheckResult {
            name: String::from("Journal"),
            status: HealthStatus::Warning,
            message: alloc::format!("{} entries (seq {})", entry_count, max_seq),
            recommendation: Some(String::from("Journal near capacity; old entries being evicted")),
        }
    } else {
        CheckResult {
            name: String::from("Journal"),
            status: HealthStatus::Healthy,
            message: alloc::format!("{} entries (seq {})", entry_count, max_seq),
            recommendation: None,
        }
    }
}

/// Check buffer cache performance.
fn check_cache() -> CheckResult {
    let stats = crate::fs::cache::stats();

    let total_ops = stats.hits.saturating_add(stats.misses);
    if total_ops == 0 {
        return CheckResult {
            name: String::from("Buffer Cache"),
            status: HealthStatus::Healthy,
            message: String::from("No cache operations yet"),
            recommendation: None,
        };
    }

    let hit_rate = (stats.hits * 100) / total_ops;

    if hit_rate < 50 && total_ops > 100 {
        CheckResult {
            name: String::from("Buffer Cache"),
            status: HealthStatus::Warning,
            message: alloc::format!("{}% hit rate ({} hits / {} total, {} dirty)",
                hit_rate, stats.hits, total_ops, stats.entries_dirty),
            recommendation: Some(String::from("Low cache hit rate; consider increasing cache size")),
        }
    } else {
        CheckResult {
            name: String::from("Buffer Cache"),
            status: HealthStatus::Healthy,
            message: alloc::format!("{}% hit rate ({} hits / {} total, {} dirty)",
                hit_rate, stats.hits, total_ops, stats.entries_dirty),
            recommendation: None,
        }
    }
}

/// Check for file handle leaks.
fn check_handles() -> CheckResult {
    // handle module provides open_count(), not a stats() struct.
    let open = crate::fs::handle::open_count();

    if open > 1000 {
        CheckResult {
            name: String::from("File Handles"),
            status: HealthStatus::Warning,
            message: alloc::format!("{} open handles", open),
            recommendation: Some(String::from("High number of open file handles; check for leaks")),
        }
    } else {
        CheckResult {
            name: String::from("File Handles"),
            status: HealthStatus::Healthy,
            message: alloc::format!("{} open handle(s)", open),
            recommendation: None,
        }
    }
}

/// Check quota status.
fn check_quotas() -> CheckResult {
    let stats = crate::fs::quota::stats();

    if stats.over_hard > 0 {
        CheckResult {
            name: String::from("Quotas"),
            status: HealthStatus::Critical,
            message: alloc::format!("{} user(s) over hard quota limit", stats.over_hard),
            recommendation: Some(String::from("Users over hard quota cannot write; free space or increase limits")),
        }
    } else if stats.over_soft > 0 {
        CheckResult {
            name: String::from("Quotas"),
            status: HealthStatus::Warning,
            message: alloc::format!("{} user(s) over soft quota limit", stats.over_soft),
            recommendation: Some(String::from("Users approaching quota limits")),
        }
    } else {
        CheckResult {
            name: String::from("Quotas"),
            status: HealthStatus::Healthy,
            message: alloc::format!("{} quota(s) configured, all within limits", stats.entries),
            recommendation: None,
        }
    }
}

/// Check integrity baseline status.
fn check_integrity() -> CheckResult {
    let stats = crate::fs::integrity::stats();

    if stats.baseline_entries > 0 {
        CheckResult {
            name: String::from("Integrity"),
            status: HealthStatus::Healthy,
            message: alloc::format!("{} file(s) baselined ({} baselines, {} verifies)",
                stats.baseline_entries, stats.baseline_count, stats.verify_count),
            recommendation: None,
        }
    } else {
        CheckResult {
            name: String::from("Integrity"),
            status: HealthStatus::Healthy,
            message: String::from("No integrity baselines configured"),
            recommendation: None,
        }
    }
}

/// Check trash bin status.
fn check_trash() -> CheckResult {
    match crate::fs::trash::list() {
        Ok(items) => {
            let count = items.len();
            if count > 1000 {
                CheckResult {
                    name: String::from("Trash"),
                    status: HealthStatus::Warning,
                    message: alloc::format!("{} items in trash", count),
                    recommendation: Some(String::from("Consider emptying the trash to free space")),
                }
            } else {
                CheckResult {
                    name: String::from("Trash"),
                    status: HealthStatus::Healthy,
                    message: alloc::format!("{} item(s) in trash", count),
                    recommendation: None,
                }
            }
        }
        Err(_) => CheckResult {
            name: String::from("Trash"),
            status: HealthStatus::Healthy,
            message: String::from("Trash not initialized"),
            recommendation: None,
        },
    }
}

/// Check temporary file cleanup status.
fn check_tmpwatch() -> CheckResult {
    let stats = crate::fs::tmpwatch::stats();

    CheckResult {
        name: String::from("Tmpwatch"),
        status: HealthStatus::Healthy,
        message: alloc::format!("{} runs, {} files cleaned, {} freed",
            stats.runs, stats.total_files_removed,
            crate::fs::usage::format_size(stats.total_bytes_freed)),
        recommendation: None,
    }
}

/// Check reclaim daemon status.
fn check_reclaim() -> CheckResult {
    let stats = crate::fs::reclaim::stats();

    CheckResult {
        name: String::from("Reclaim"),
        status: HealthStatus::Healthy,
        message: alloc::format!("{} runs, {} reclaimed",
            stats.trigger_count,
            crate::fs::usage::format_size(stats.total_bytes_freed)),
        recommendation: None,
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[health] Running self-test...");

    test_check_all();
    test_status_merge();
    test_report_counts();
    test_stats();

    serial_println!("[health] Self-test passed (4 tests).");
    Ok(())
}

fn test_check_all() {
    // Run a full health check — should not panic.
    let report = check().expect("health check should succeed");
    assert!(!report.checks.is_empty(), "should have check results");
    assert_eq!(report.total_checks, report.checks.len());

    serial_println!("[health]   check all: ok (status={})", report.status.name());
}

fn test_status_merge() {
    assert_eq!(HealthStatus::Healthy.merge(HealthStatus::Healthy), HealthStatus::Healthy);
    assert_eq!(HealthStatus::Healthy.merge(HealthStatus::Warning), HealthStatus::Warning);
    assert_eq!(HealthStatus::Warning.merge(HealthStatus::Critical), HealthStatus::Critical);
    assert_eq!(HealthStatus::Critical.merge(HealthStatus::Healthy), HealthStatus::Critical);

    serial_println!("[health]   status merge: ok");
}

fn test_report_counts() {
    let report = check().expect("check");
    assert_eq!(
        report.healthy + report.warnings + report.critical,
        report.total_checks,
        "counts should sum to total"
    );

    serial_println!("[health]   report counts: ok");
}

fn test_stats() {
    let count = checks_run();
    assert!(count >= 2, "should have run at least 2 checks");

    let report = last_report();
    assert!(report.is_some(), "should have cached report");

    serial_println!("[health]   stats: ok");
}
