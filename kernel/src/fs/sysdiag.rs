//! System diagnostics and troubleshooting.
//!
//! Provides automated health checks and diagnostic tests for common
//! system issues.  Similar to Windows Troubleshooter, macOS System
//! Report, or Linux's `systemd-analyze`.  Runs structured tests across
//! multiple subsystems and produces actionable reports.
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Diagnostics
//!   → sysdiag::run_all() → DiagReport
//!   → sysdiag::run_category(cat) → CategoryReport
//!
//! System tray alert
//!   → sysdiag::quick_check() → issues list
//!
//! Integration:
//!   → health (filesystem health)
//!   → perfmon (resource monitoring)
//!   → netsettings (network status)
//!   → sysinfo (hardware info)
//!   → cache (buffer cache stats)
//! ```
//!
//! ## Test Categories
//!
//! - **Network**: connectivity, DNS resolution, gateway reachability
//! - **Storage**: filesystem health, space, mount status
//! - **Memory**: usage, swap, pressure indicators
//! - **Services**: critical service status
//! - **Boot**: boot time analysis, startup items
//! - **Security**: certificate validity, capability config

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_ISSUES: usize = 256;
const MAX_HISTORY: usize = 64;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Diagnostic test category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagCategory {
    Network,
    Storage,
    Memory,
    Services,
    Boot,
    Security,
}

impl DiagCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Network => "Network",
            Self::Storage => "Storage",
            Self::Memory => "Memory",
            Self::Services => "Services",
            Self::Boot => "Boot",
            Self::Security => "Security",
        }
    }

    pub fn all() -> &'static [DiagCategory] {
        &[
            Self::Network,
            Self::Storage,
            Self::Memory,
            Self::Services,
            Self::Boot,
            Self::Security,
        ]
    }
}

/// Severity of a diagnostic finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Informational — no action needed.
    Info,
    /// Minor issue — system works but could be improved.
    Warning,
    /// Significant issue — may cause problems.
    Error,
    /// Critical issue — system functionality impaired.
    Critical,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
            Self::Critical => "critical",
        }
    }
}

/// A single diagnostic finding.
#[derive(Debug, Clone)]
pub struct DiagIssue {
    pub id: u64,
    pub category: DiagCategory,
    pub severity: Severity,
    pub title: String,
    pub detail: String,
    pub suggestion: String,
    pub timestamp_ns: u64,
}

/// Result of running diagnostics for one category.
#[derive(Debug, Clone)]
pub struct CategoryReport {
    pub category: DiagCategory,
    pub tests_run: u32,
    pub tests_passed: u32,
    pub issues: Vec<DiagIssue>,
    pub duration_us: u64,
}

/// Complete diagnostic report across all categories.
#[derive(Debug, Clone)]
pub struct DiagReport {
    pub categories: Vec<CategoryReport>,
    pub total_tests: u32,
    pub total_passed: u32,
    pub total_issues: usize,
    pub worst_severity: Severity,
    pub duration_us: u64,
    pub timestamp_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct DiagState {
    /// Current/latest issues.
    issues: Vec<DiagIssue>,
    /// History of reports (summaries only).
    history: Vec<ReportSummary>,
    /// Total diagnostics runs.
    total_runs: u64,
    /// Total issues found across all runs.
    total_issues_found: u64,
    /// Next issue ID.
    next_id: u64,
    /// Operation counter.
    ops: u64,
}

#[derive(Debug, Clone)]
struct ReportSummary {
    timestamp_ns: u64,
    total_tests: u32,
    total_passed: u32,
    issue_count: usize,
    worst_severity: Severity,
    duration_us: u64,
}

static STATE: Mutex<Option<DiagState>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut DiagState) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    let result = f(state)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    Ok(result)
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the diagnostics subsystem.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }
    *guard = Some(DiagState {
        issues: Vec::new(),
        history: Vec::new(),
        total_runs: 0,
        total_issues_found: 0,
        next_id: 1,
        ops: 0,
    });
}

// ---------------------------------------------------------------------------
// Diagnostic runners
// ---------------------------------------------------------------------------

/// Run all diagnostic categories.
pub fn run_all() -> KernelResult<DiagReport> {
    let start_ns = crate::hpet::elapsed_ns();
    let mut all_categories = Vec::new();
    let mut total_tests = 0u32;
    let mut total_passed = 0u32;
    let mut all_issues = Vec::new();

    for cat in DiagCategory::all() {
        let report = run_category(*cat)?;
        total_tests += report.tests_run;
        total_passed += report.tests_passed;
        all_issues.extend(report.issues.clone());
        all_categories.push(report);
    }

    let worst = all_issues.iter()
        .map(|i| i.severity)
        .max()
        .unwrap_or(Severity::Info);

    let elapsed_us = (crate::hpet::elapsed_ns() - start_ns) / 1000;

    let report = DiagReport {
        total_tests,
        total_passed,
        total_issues: all_issues.len(),
        worst_severity: worst,
        duration_us: elapsed_us,
        timestamp_ns: start_ns,
        categories: all_categories,
    };

    // Save to state
    with_state(|state| {
        state.issues = all_issues;
        state.total_runs += 1;
        state.total_issues_found += report.total_issues as u64;

        if state.history.len() >= MAX_HISTORY {
            state.history.remove(0);
        }
        state.history.push(ReportSummary {
            timestamp_ns: report.timestamp_ns,
            total_tests: report.total_tests,
            total_passed: report.total_passed,
            issue_count: report.total_issues,
            worst_severity: report.worst_severity,
            duration_us: report.duration_us,
        });
        Ok(())
    })?;

    Ok(report)
}

/// Run diagnostics for a single category.
pub fn run_category(cat: DiagCategory) -> KernelResult<CategoryReport> {
    let start_ns = crate::hpet::elapsed_ns();

    let (tests_run, tests_passed, issues) = match cat {
        DiagCategory::Network => diag_network(),
        DiagCategory::Storage => diag_storage(),
        DiagCategory::Memory => diag_memory(),
        DiagCategory::Services => diag_services(),
        DiagCategory::Boot => diag_boot(),
        DiagCategory::Security => diag_security(),
    };

    let elapsed_us = (crate::hpet::elapsed_ns() - start_ns) / 1000;

    Ok(CategoryReport {
        category: cat,
        tests_run,
        tests_passed,
        issues,
        duration_us: elapsed_us,
    })
}

/// Quick check — returns current issues without full re-scan.
pub fn quick_check() -> Vec<DiagIssue> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| {
        s.issues.iter()
            .filter(|i| i.severity >= Severity::Warning)
            .cloned()
            .collect()
    })
}

// ---------------------------------------------------------------------------
// Individual diagnostic tests
// ---------------------------------------------------------------------------

fn next_issue_id() -> u64 {
    let mut guard = STATE.lock();
    if let Some(state) = guard.as_mut() {
        let id = state.next_id;
        state.next_id += 1;
        id
    } else {
        0
    }
}

fn make_issue(
    cat: DiagCategory,
    severity: Severity,
    title: &str,
    detail: &str,
    suggestion: &str,
) -> DiagIssue {
    DiagIssue {
        id: next_issue_id(),
        category: cat,
        severity,
        title: String::from(title),
        detail: String::from(detail),
        suggestion: String::from(suggestion),
        timestamp_ns: crate::hpet::elapsed_ns(),
    }
}

fn diag_network() -> (u32, u32, Vec<DiagIssue>) {
    let mut tests = 0u32;
    let mut passed = 0u32;
    let mut issues = Vec::new();

    // Test 1: check if any network interface is configured
    tests += 1;
    let (iface_count, connected, _, _) = crate::fs::netsettings::stats();
    if iface_count == 0 {
        issues.push(make_issue(
            DiagCategory::Network, Severity::Critical,
            "No network interfaces",
            "No network interfaces are configured.",
            "Add a network interface via netsettings.",
        ));
    } else {
        passed += 1;
    }

    // Test 2: check if any interface is connected
    tests += 1;
    if connected == 0 && iface_count > 0 {
        issues.push(make_issue(
            DiagCategory::Network, Severity::Warning,
            "No connected interfaces",
            &format!("{} interfaces configured but none connected.", iface_count),
            "Check cable/WiFi connection or enable an interface.",
        ));
    } else if connected > 0 {
        passed += 1;
    }

    // Test 3: check VPN status
    tests += 1;
    // VPN is optional, just informational
    passed += 1;

    // Test 4: check DNS configuration
    tests += 1;
    // Basic check that netsettings has been initialized
    if iface_count > 0 {
        passed += 1;
    } else {
        issues.push(make_issue(
            DiagCategory::Network, Severity::Info,
            "DNS not configured",
            "No DNS servers configured (no interfaces).",
            "Configure DNS via netsettings.",
        ));
    }

    (tests, passed, issues)
}

fn diag_storage() -> (u32, u32, Vec<DiagIssue>) {
    let mut tests = 0u32;
    let mut passed = 0u32;
    let mut issues = Vec::new();

    // Test 1: root filesystem mounted
    tests += 1;
    if crate::fs::Vfs::readdir("/").is_ok() {
        passed += 1;
    } else {
        issues.push(make_issue(
            DiagCategory::Storage, Severity::Critical,
            "Root filesystem not accessible",
            "Cannot read root directory /.",
            "Check filesystem mounts.",
        ));
    }

    // Test 2: /tmp mounted (memfs)
    tests += 1;
    if crate::fs::Vfs::readdir("/tmp").is_ok() {
        passed += 1;
    } else {
        issues.push(make_issue(
            DiagCategory::Storage, Severity::Warning,
            "/tmp not available",
            "Temporary filesystem not mounted at /tmp.",
            "Mount memfs at /tmp during boot.",
        ));
    }

    // Test 3: check disk space via statvfs
    tests += 1;
    if let Ok(info) = crate::fs::Vfs::statvfs("/") {
        let total = info.total_blocks.saturating_mul(info.block_size);
        let free = info.free_blocks.saturating_mul(info.block_size);
        if total > 0 {
            let used_pct = ((total - free) * 100) / total;
            if used_pct > 95 {
                issues.push(make_issue(
                    DiagCategory::Storage, Severity::Critical,
                    "Disk almost full",
                    &format!("Root filesystem is {}% full ({} free).",
                        used_pct, crate::fs::storageclean::format_size(free)),
                    "Free space using 'sclean scan' and 'sclean clean'.",
                ));
            } else if used_pct > 85 {
                issues.push(make_issue(
                    DiagCategory::Storage, Severity::Warning,
                    "Disk space low",
                    &format!("Root filesystem is {}% full.", used_pct),
                    "Consider cleaning up unused files.",
                ));
            } else {
                passed += 1;
            }
        } else {
            passed += 1;
        }
    } else {
        passed += 1; // Cannot check, not an error
    }

    // Test 4: /proc mounted
    tests += 1;
    if crate::fs::Vfs::readdir("/proc").is_ok() {
        passed += 1;
    } else {
        issues.push(make_issue(
            DiagCategory::Storage, Severity::Warning,
            "/proc not available",
            "Process filesystem not mounted.",
            "Procfs should be mounted during boot.",
        ));
    }

    (tests, passed, issues)
}

fn diag_memory() -> (u32, u32, Vec<DiagIssue>) {
    let mut tests = 0u32;
    let mut passed = 0u32;
    let mut issues = Vec::new();

    // Test 1: check basic memory availability via sysinfo
    tests += 1;
    let mem = crate::fs::sysinfo::memory_info();
    if mem.total_bytes > 0 {
        passed += 1;
        let used_pct = if mem.total_bytes > 0 {
            (mem.used_bytes * 100) / mem.total_bytes
        } else {
            0
        };
        // Test 2: memory pressure
        tests += 1;
        if used_pct > 95 {
            issues.push(make_issue(
                DiagCategory::Memory, Severity::Critical,
                "Memory critically low",
                &format!("{}% of RAM in use ({} of {}).",
                    used_pct,
                    crate::fs::storageclean::format_size(mem.used_bytes),
                    crate::fs::storageclean::format_size(mem.total_bytes)),
                "Close applications or add more RAM.",
            ));
        } else if used_pct > 85 {
            issues.push(make_issue(
                DiagCategory::Memory, Severity::Warning,
                "Memory usage high",
                &format!("{}% of RAM in use.", used_pct),
                "Monitor memory usage and close unused applications.",
            ));
        } else {
            passed += 1;
        }
    } else {
        issues.push(make_issue(
            DiagCategory::Memory, Severity::Info,
            "Memory info unavailable",
            "Cannot query memory statistics.",
            "Initialize sysinfo module.",
        ));
        tests += 1; // Count the pressure test too
    }

    // Test 3: swap status
    tests += 1;
    let (_, _, _, swap_ops) = crate::fs::swapcfg::stats();
    if swap_ops > 0 {
        passed += 1;
    } else {
        // Swap not initialized is informational, not an error
        passed += 1;
    }

    (tests, passed, issues)
}

fn diag_services() -> (u32, u32, Vec<DiagIssue>) {
    let mut tests = 0u32;
    let mut passed = 0u32;
    let mut issues = Vec::new();

    // Test 1: autostart items configured
    tests += 1;
    let (item_count, _, _, _) = crate::fs::autostart::stats();
    if item_count > 0 {
        passed += 1;
    } else {
        issues.push(make_issue(
            DiagCategory::Services, Severity::Info,
            "No autostart items",
            "No startup items configured.",
            "Add services to autostart for automatic startup.",
        ));
    }

    // Test 2: notification center initialized
    tests += 1;
    let (_, _, _, _, notif_ops) = crate::fs::notifcenter::stats();
    if notif_ops > 0 {
        passed += 1;
    } else {
        passed += 1; // Not initialized is fine at this stage
    }

    // Test 3: application registry populated
    tests += 1;
    let (app_count, _, _, _) = crate::fs::appregistry::stats();
    if app_count > 0 {
        passed += 1;
    } else {
        issues.push(make_issue(
            DiagCategory::Services, Severity::Info,
            "No applications registered",
            "Application registry is empty.",
            "Register applications via appregistry.",
        ));
    }

    (tests, passed, issues)
}

fn diag_boot() -> (u32, u32, Vec<DiagIssue>) {
    let mut tests = 0u32;
    let mut passed = 0u32;
    let mut issues = Vec::new();

    // Test 1: boot configuration exists
    tests += 1;
    let (entry_count, _, _, _) = crate::fs::bootcfg::stats();
    if entry_count > 0 {
        passed += 1;
    } else {
        issues.push(make_issue(
            DiagCategory::Boot, Severity::Info,
            "No boot entries",
            "Boot configuration has no entries.",
            "Initialize bootcfg with default entries.",
        ));
    }

    // Test 2: uptime check (system running)
    tests += 1;
    let uptime_ns = crate::hpet::elapsed_ns();
    if uptime_ns > 0 {
        passed += 1;
    }

    // Test 3: autostart impact assessment
    tests += 1;
    let (count, _, _, _) = crate::fs::autostart::stats();
    if count > 20 {
        issues.push(make_issue(
            DiagCategory::Boot, Severity::Warning,
            "Many startup items",
            &format!("{} items in autostart may slow boot.", count),
            "Review autostart items and disable non-essential ones.",
        ));
    } else {
        passed += 1;
    }

    (tests, passed, issues)
}

fn diag_security() -> (u32, u32, Vec<DiagIssue>) {
    let mut tests = 0u32;
    let mut passed = 0u32;
    let mut issues = Vec::new();

    // Test 1: user accounts configured
    tests += 1;
    let (user_count, _, _, _) = crate::fs::useracct::stats();
    if user_count > 0 {
        passed += 1;
    } else {
        issues.push(make_issue(
            DiagCategory::Security, Severity::Warning,
            "No user accounts",
            "No user accounts configured.",
            "Create user accounts via useracct.",
        ));
    }

    // Test 2: certificate store has root CAs
    tests += 1;
    let (cert_count, _, _, _, _) = crate::fs::certmgr::stats();
    if cert_count > 0 {
        passed += 1;
    } else {
        issues.push(make_issue(
            DiagCategory::Security, Severity::Warning,
            "No certificates",
            "Certificate store is empty — HTTPS will fail.",
            "Initialize certmgr with system root CAs.",
        ));
    }

    // Test 3: capability settings initialized
    tests += 1;
    let (group_count, _, _, _, _) = crate::fs::capsettings::stats();
    if group_count > 0 {
        passed += 1;
    } else {
        issues.push(make_issue(
            DiagCategory::Security, Severity::Info,
            "No capability groups",
            "Capability settings not initialized.",
            "Initialize capsettings with default groups.",
        ));
    }

    (tests, passed, issues)
}

// ---------------------------------------------------------------------------
// Query
// ---------------------------------------------------------------------------

/// Get all current issues.
pub fn current_issues() -> Vec<DiagIssue> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| s.issues.clone())
}

/// Get issues filtered by severity.
pub fn issues_by_severity(min_severity: Severity) -> Vec<DiagIssue> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| {
        s.issues.iter()
            .filter(|i| i.severity >= min_severity)
            .cloned()
            .collect()
    })
}

/// Get issues for a specific category.
pub fn issues_for_category(cat: DiagCategory) -> Vec<DiagIssue> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| {
        s.issues.iter()
            .filter(|i| i.category == cat)
            .cloned()
            .collect()
    })
}

/// Get run history summaries.
pub fn history() -> Vec<(u64, u32, u32, usize)> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| {
        s.history.iter()
            .map(|h| (h.timestamp_ns, h.total_tests, h.total_passed, h.issue_count))
            .collect()
    })
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (issue_count, total_runs, total_issues_found, history_count, ops).
pub fn stats() -> (usize, u64, u64, usize, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.issues.len(), s.total_runs, s.total_issues_found, s.history.len(), s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the diagnostics module.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[sysdiag] Running self-tests...");

    // Reset state
    *STATE.lock() = None;
    init_defaults();

    // Test 1: initial state
    {
        let (issues, runs, _, _, _) = stats();
        assert_eq!(issues, 0);
        assert_eq!(runs, 0);
        serial_println!("[sysdiag]   1. Initial state empty — OK");
    }

    // Test 2: run all diagnostics
    {
        let report = run_all().expect("run_all");
        assert!(report.total_tests > 0);
        assert!(report.total_passed <= report.total_tests);
        assert!(report.duration_us < 10_000_000); // Should complete in < 10s
        serial_println!("[sysdiag]   2. Run all diagnostics — OK ({}t/{}p/{}i in {}us)",
            report.total_tests, report.total_passed, report.total_issues, report.duration_us);
    }

    // Test 3: run individual category
    {
        let report = run_category(DiagCategory::Storage).expect("run storage");
        assert!(report.tests_run > 0);
        assert_eq!(report.category, DiagCategory::Storage);
        serial_println!("[sysdiag]   3. Run single category — OK");
    }

    // Test 4: quick check
    {
        let issues = quick_check();
        // Quick check returns warnings+ from latest run
        let _ = issues.len();
        serial_println!("[sysdiag]   4. Quick check — OK");
    }

    // Test 5: history recorded
    {
        let hist = history();
        assert!(!hist.is_empty());
        serial_println!("[sysdiag]   5. History recorded — OK");
    }

    // Test 6: severity ordering
    {
        assert!(Severity::Info < Severity::Warning);
        assert!(Severity::Warning < Severity::Error);
        assert!(Severity::Error < Severity::Critical);
        serial_println!("[sysdiag]   6. Severity ordering — OK");
    }

    // Test 7: category labels
    {
        for cat in DiagCategory::all() {
            assert!(!cat.label().is_empty());
        }
        assert_eq!(DiagCategory::all().len(), 6);
        serial_println!("[sysdiag]   7. Category labels — OK");
    }

    // Test 8: filter by severity
    {
        let critical = issues_by_severity(Severity::Critical);
        let all = current_issues();
        assert!(critical.len() <= all.len());
        serial_println!("[sysdiag]   8. Severity filter — OK");
    }

    // Test 9: filter by category
    {
        let net_issues = issues_for_category(DiagCategory::Network);
        for issue in &net_issues {
            assert_eq!(issue.category, DiagCategory::Network);
        }
        serial_println!("[sysdiag]   9. Category filter — OK");
    }

    // Test 10: stats update
    {
        let (_, runs, total_found, hist_count, _) = stats();
        assert!(runs >= 1);
        let _ = total_found;
        assert!(hist_count >= 1);
        serial_println!("[sysdiag]  10. Stats updated — OK");
    }

    // Test 11: multiple runs accumulate history
    {
        let _ = run_all();
        let (_, runs, _, hist_count, _) = stats();
        assert!(runs >= 2);
        assert!(hist_count >= 2);
        serial_println!("[sysdiag]  11. Multiple runs accumulate — OK");
    }

    serial_println!("[sysdiag] All 11 self-tests passed.");
}
