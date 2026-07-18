//! Crash reporting — capture, store, and manage diagnostic crash reports.
//!
//! When a process crashes (segfault, unhandled exception, panic, etc.)
//! the crash reporter captures diagnostic information including stack
//! trace, register state, loaded modules, and user description.
//!
//! ## Architecture
//!
//! ```text
//! Exception handler / panic hook
//!   → crashreport::submit_report(pid, signal, info)
//!
//! Settings panel → Privacy → Crash Reports
//!   → crashreport::set_auto_submit() / list_reports()
//!
//! Integration:
//!   → procfs (process info)
//!   → notifcenter (crash notification)
//!   → sysdiag (system diagnostics attachment)
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

/// Crash severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashSeverity {
    /// Fatal — process terminated.
    Fatal,
    /// Non-fatal — process continued but was unstable.
    NonFatal,
    /// Hang — process became unresponsive.
    Hang,
    /// Kernel panic (if we managed to capture anything).
    KernelPanic,
}

impl CrashSeverity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Fatal => "Fatal",
            Self::NonFatal => "Non-fatal",
            Self::Hang => "Hang",
            Self::KernelPanic => "Kernel Panic",
        }
    }
}

/// Crash signal/reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashSignal {
    Segfault,
    BusError,
    IllegalInstruction,
    FloatingPoint,
    Abort,
    StackOverflow,
    OutOfMemory,
    Panic,
    Hang,
    Other,
}

impl CrashSignal {
    pub fn label(self) -> &'static str {
        match self {
            Self::Segfault => "SIGSEGV",
            Self::BusError => "SIGBUS",
            Self::IllegalInstruction => "SIGILL",
            Self::FloatingPoint => "SIGFPE",
            Self::Abort => "SIGABRT",
            Self::StackOverflow => "Stack Overflow",
            Self::OutOfMemory => "OOM",
            Self::Panic => "Panic",
            Self::Hang => "Hang",
            Self::Other => "Other",
        }
    }
}

/// Report submission status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportStatus {
    /// Captured locally, not submitted.
    Local,
    /// Queued for submission.
    Queued,
    /// Submitted to crash server.
    Submitted,
    /// Submission failed.
    Failed,
}

impl ReportStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Local => "Local",
            Self::Queued => "Queued",
            Self::Submitted => "Submitted",
            Self::Failed => "Failed",
        }
    }
}

/// A captured crash report.
#[derive(Debug, Clone)]
pub struct CrashReport {
    /// Unique report ID.
    pub id: u32,
    /// Process ID that crashed.
    pub pid: u32,
    /// Process name / executable.
    pub process_name: String,
    /// Crash signal.
    pub signal: CrashSignal,
    /// Severity.
    pub severity: CrashSeverity,
    /// Faulting address (if applicable).
    pub fault_address: u64,
    /// Instruction pointer at crash.
    pub instruction_pointer: u64,
    /// Stack trace (addresses).
    pub stack_trace: Vec<u64>,
    /// Loaded module names.
    pub modules: Vec<String>,
    /// Timestamp (ns since boot).
    pub timestamp_ns: u64,
    /// User-provided description.
    pub description: String,
    /// Submission status.
    pub status: ReportStatus,
    /// Application version string.
    pub app_version: String,
    /// OS version at time of crash.
    pub os_version: String,
}

/// Crash reporter configuration.
#[derive(Debug, Clone)]
pub struct CrashConfig {
    /// Whether crash reporting is enabled.
    pub enabled: bool,
    /// Auto-submit reports (privacy-sensitive).
    pub auto_submit: bool,
    /// Include stack traces.
    pub include_stack: bool,
    /// Include loaded modules list.
    pub include_modules: bool,
    /// Maximum reports to retain.
    pub max_reports: usize,
    /// Send non-fatal crash reports too.
    pub report_non_fatal: bool,
}

impl Default for CrashConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_submit: false,
            include_stack: true,
            include_modules: true,
            max_reports: 100,
            report_non_fatal: false,
        }
    }
}

const MAX_REPORTS: usize = 500;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    reports: Vec<CrashReport>,
    config: CrashConfig,
    next_id: u32,
    total_crashes: u64,
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

/// Initialise crash reporter.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }
    *guard = Some(State {
        reports: Vec::new(),
        config: CrashConfig::default(),
        next_id: 1,
        total_crashes: 0,
        ops: 0,
    });
}

/// Submit a crash report.
pub fn submit_report(
    pid: u32,
    process_name: &str,
    signal: CrashSignal,
    severity: CrashSeverity,
    fault_address: u64,
    instruction_pointer: u64,
    stack_trace: &[u64],
    modules: &[&str],
) -> KernelResult<u32> {
    with_state(|state| {
        if !state.config.enabled {
            return Err(KernelError::NotSupported);
        }
        if severity == CrashSeverity::NonFatal && !state.config.report_non_fatal {
            return Err(KernelError::NotSupported);
        }

        let id = state.next_id;
        state.next_id += 1;
        state.total_crashes += 1;

        let now = crate::hpet::elapsed_ns();

        let report = CrashReport {
            id,
            pid,
            process_name: String::from(process_name),
            signal,
            severity,
            fault_address,
            instruction_pointer,
            stack_trace: if state.config.include_stack {
                stack_trace.to_vec()
            } else {
                Vec::new()
            },
            modules: if state.config.include_modules {
                modules.iter().map(|m| String::from(*m)).collect()
            } else {
                Vec::new()
            },
            timestamp_ns: now,
            description: String::new(),
            status: if state.config.auto_submit { ReportStatus::Queued } else { ReportStatus::Local },
            app_version: String::new(),
            os_version: String::from("0.1.0"),
        };

        state.reports.push(report);

        // Trim old reports if over limit.
        let max = state.config.max_reports.min(MAX_REPORTS);
        while state.reports.len() > max {
            state.reports.remove(0);
        }

        Ok(id)
    })
}

/// Add a user description to a report.
pub fn set_description(id: u32, description: &str) -> KernelResult<()> {
    with_state(|state| {
        let report = state.reports.iter_mut().find(|r| r.id == id)
            .ok_or(KernelError::NotFound)?;
        report.description = String::from(description);
        Ok(())
    })
}

/// Delete a crash report.
pub fn delete_report(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.reports.iter().position(|r| r.id == id)
            .ok_or(KernelError::NotFound)?;
        state.reports.remove(pos);
        Ok(())
    })
}

/// Clear all reports.
pub fn clear_reports() -> KernelResult<usize> {
    with_state(|state| {
        let count = state.reports.len();
        state.reports.clear();
        Ok(count)
    })
}

/// Get a specific report.
pub fn get_report(id: u32) -> KernelResult<CrashReport> {
    with_state(|state| {
        state.reports.iter().find(|r| r.id == id)
            .cloned()
            .ok_or(KernelError::NotFound)
    })
}

/// List all reports (newest first).
pub fn list_reports() -> Vec<CrashReport> {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let mut reports = s.reports.clone();
            reports.reverse();
            reports
        }
        None => Vec::new(),
    }
}

/// Count reports by severity.
pub fn count_by_severity() -> (u32, u32, u32, u32) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let fatal = s.reports.iter().filter(|r| r.severity == CrashSeverity::Fatal).count() as u32;
            let non_fatal = s.reports.iter().filter(|r| r.severity == CrashSeverity::NonFatal).count() as u32;
            let hang = s.reports.iter().filter(|r| r.severity == CrashSeverity::Hang).count() as u32;
            let kernel = s.reports.iter().filter(|r| r.severity == CrashSeverity::KernelPanic).count() as u32;
            (fatal, non_fatal, hang, kernel)
        }
        None => (0, 0, 0, 0),
    }
}

/// Set auto-submit for crash reports.
pub fn set_auto_submit(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.auto_submit = enabled;
        Ok(())
    })
}

/// Enable or disable crash reporting entirely.
pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.enabled = enabled;
        Ok(())
    })
}

/// Set whether to include non-fatal crashes.
pub fn set_report_non_fatal(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.report_non_fatal = enabled;
        Ok(())
    })
}

/// Statistics: (report_count, total_crashes, fatal_count, enabled, ops).
pub fn stats() -> (usize, u64, u32, bool, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let fatal = s.reports.iter().filter(|r| r.severity == CrashSeverity::Fatal).count() as u32;
            (s.reports.len(), s.total_crashes, fatal, s.config.enabled, s.ops)
        }
        None => (0, 0, 0, false, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("crashreport::self_test() — running tests...");

    init_defaults();

    // Test 1: Submit a fatal crash report.
    let id1 = submit_report(
        100, "test_app", CrashSignal::Segfault, CrashSeverity::Fatal,
        0xDEAD_BEEF, 0x0040_1000, &[0x0040_1000, 0x0040_0F00, 0x0040_0E00],
        &["libc.so", "libm.so"],
    ).expect("submit report");
    assert!(id1 > 0);
    crate::serial_println!("  [1/11] submit report: OK");

    // Test 2: Get report.
    let r = get_report(id1).expect("get report");
    assert_eq!(r.pid, 100);
    assert_eq!(r.signal, CrashSignal::Segfault);
    assert_eq!(r.stack_trace.len(), 3);
    crate::serial_println!("  [2/11] get report: OK");

    // Test 3: Set description.
    set_description(id1, "Crashed while opening file").expect("set desc");
    let r = get_report(id1).expect("get after desc");
    assert_eq!(r.description, "Crashed while opening file");
    crate::serial_println!("  [3/11] set description: OK");

    // Test 4: Submit another report.
    let id2 = submit_report(
        200, "browser", CrashSignal::Abort, CrashSeverity::Fatal,
        0, 0x0050_2000, &[0x0050_2000], &["libweb.so"],
    ).expect("submit report 2");
    crate::serial_println!("  [4/11] submit second report: OK");

    // Test 5: List reports (newest first).
    let reports = list_reports();
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].id, id2);
    crate::serial_println!("  [5/11] list reports: OK");

    // Test 6: Count by severity.
    let (fatal, _, _, _) = count_by_severity();
    assert_eq!(fatal, 2);
    crate::serial_println!("  [6/11] count by severity: OK");

    // Test 7: Delete a report.
    delete_report(id1).expect("delete");
    let reports = list_reports();
    assert_eq!(reports.len(), 1);
    crate::serial_println!("  [7/11] delete report: OK");

    // Test 8: Non-fatal reports are suppressed by default.
    let result = submit_report(
        300, "editor", CrashSignal::FloatingPoint, CrashSeverity::NonFatal,
        0, 0, &[], &[],
    );
    assert!(result.is_err());
    crate::serial_println!("  [8/11] non-fatal suppression: OK");

    // Test 9: Enable non-fatal reporting.
    set_report_non_fatal(true).expect("enable non-fatal");
    let id3 = submit_report(
        300, "editor", CrashSignal::FloatingPoint, CrashSeverity::NonFatal,
        0, 0, &[], &[],
    ).expect("submit non-fatal");
    assert!(id3 > 0);
    crate::serial_println!("  [9/11] non-fatal enabled: OK");

    // Test 10: Clear all.
    let cleared = clear_reports().expect("clear");
    assert_eq!(cleared, 2);
    let reports = list_reports();
    assert!(reports.is_empty());
    crate::serial_println!("  [10/11] clear reports: OK");

    // Test 11: Stats.
    let (count, total, _fatal, enabled, ops) = stats();
    assert_eq!(count, 0);
    assert!(total >= 3);
    assert!(enabled);
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("crashreport::self_test() — all 11 tests passed");
}
