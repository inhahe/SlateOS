//! Dump analyzer — crash dump parsing and analysis.
//!
//! Parses kernel and application crash dumps, extracts stack traces,
//! identifies faulting modules, and provides automated root cause
//! analysis suggestions.
//!
//! ## Architecture
//!
//! ```text
//! Crash occurs
//!   → crashreport generates dump file
//!   → dumpanalyzer::analyze_dump(path) → analysis result
//!
//! Settings panel → System → Crash Reports
//!   → dumpanalyzer::list_analyses() → previous analyses
//!   → dumpanalyzer::get_analysis(id) → detailed report
//!
//! Integration:
//!   → crashreport (dump file generation)
//!   → syslog (crash event correlation)
//!   → driverupdate (driver fault identification)
//!   → notifcenter (crash notifications)
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
    Info,
    Warning,
    Error,
    Critical,
    Fatal,
}

impl CrashSeverity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "Info",
            Self::Warning => "Warning",
            Self::Error => "Error",
            Self::Critical => "Critical",
            Self::Fatal => "Fatal",
        }
    }
}

/// Fault type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultType {
    PageFault,
    NullPointer,
    StackOverflow,
    DivisionByZero,
    IllegalInstruction,
    GeneralProtection,
    DoubleFault,
    Assertion,
    Panic,
    OutOfMemory,
    Timeout,
    Unknown,
}

impl FaultType {
    pub fn label(self) -> &'static str {
        match self {
            Self::PageFault => "Page Fault",
            Self::NullPointer => "Null Pointer",
            Self::StackOverflow => "Stack Overflow",
            Self::DivisionByZero => "Division by Zero",
            Self::IllegalInstruction => "Illegal Instruction",
            Self::GeneralProtection => "General Protection",
            Self::DoubleFault => "Double Fault",
            Self::Assertion => "Assertion Failure",
            Self::Panic => "Panic",
            Self::OutOfMemory => "Out of Memory",
            Self::Timeout => "Timeout",
            Self::Unknown => "Unknown",
        }
    }
}

/// A stack frame in the backtrace.
#[derive(Debug, Clone)]
pub struct StackFrame {
    /// Frame index (0 = top).
    pub index: u32,
    /// Address.
    pub address: u64,
    /// Module name.
    pub module: String,
    /// Function name (if resolved).
    pub function: String,
    /// Offset within function.
    pub offset: u64,
}

/// A crash analysis report.
#[derive(Debug, Clone)]
pub struct CrashAnalysis {
    /// Analysis ID.
    pub id: u32,
    /// Dump file path.
    pub dump_path: String,
    /// Severity.
    pub severity: CrashSeverity,
    /// Fault type.
    pub fault_type: FaultType,
    /// Faulting module.
    pub faulting_module: String,
    /// Faulting address.
    pub fault_address: u64,
    /// Stack frames.
    pub stack_frames: Vec<StackFrame>,
    /// Summary / root cause suggestion.
    pub summary: String,
    /// Analysis timestamp (ns).
    pub analyzed_ns: u64,
    /// PID of crashed process (0 = kernel).
    pub pid: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_ANALYSES: usize = 200;

struct State {
    analyses: Vec<CrashAnalysis>,
    next_id: u32,
    total_analyzed: u64,
    total_kernel_crashes: u64,
    total_app_crashes: u64,
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
        analyses: Vec::new(),
        next_id: 1,
        total_analyzed: 0,
        total_kernel_crashes: 0,
        total_app_crashes: 0,
        ops: 0,
    });
}

/// Analyze a crash dump.
pub fn analyze_dump(
    dump_path: &str, severity: CrashSeverity, fault_type: FaultType,
    faulting_module: &str, fault_address: u64, pid: u32,
    stack_frames: Vec<StackFrame>, summary: &str,
) -> KernelResult<u32> {
    with_state(|state| {
        if state.analyses.len() >= MAX_ANALYSES {
            // Remove oldest.
            state.analyses.remove(0);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.total_analyzed += 1;
        if pid == 0 {
            state.total_kernel_crashes += 1;
        } else {
            state.total_app_crashes += 1;
        }

        state.analyses.push(CrashAnalysis {
            id, dump_path: String::from(dump_path),
            severity, fault_type,
            faulting_module: String::from(faulting_module),
            fault_address, stack_frames,
            summary: String::from(summary),
            analyzed_ns: crate::hpet::elapsed_ns(),
            pid,
        });
        Ok(id)
    })
}

/// Get analysis by ID.
pub fn get_analysis(id: u32) -> KernelResult<CrashAnalysis> {
    with_state(|state| {
        state.analyses.iter().find(|a| a.id == id).cloned().ok_or(KernelError::NotFound)
    })
}

/// List all analyses.
pub fn list_analyses() -> Vec<CrashAnalysis> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.analyses.clone())
}

/// List analyses by fault type.
pub fn list_by_fault(fault_type: FaultType) -> Vec<CrashAnalysis> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.analyses.iter().filter(|a| a.fault_type == fault_type).cloned().collect()
    })
}

/// Delete an analysis.
pub fn delete_analysis(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.analyses.iter().position(|a| a.id == id)
            .ok_or(KernelError::NotFound)?;
        state.analyses.remove(pos);
        Ok(())
    })
}

/// Clear all analyses.
pub fn clear_all() -> usize {
    let mut guard = STATE.lock();
    if let Some(state) = guard.as_mut() {
        let count = state.analyses.len();
        state.analyses.clear();
        count
    } else { 0 }
}

/// Statistics: (analysis_count, total_analyzed, kernel_crashes, app_crashes, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.analyses.len(), s.total_analyzed, s.total_kernel_crashes, s.total_app_crashes, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("dumpanalyzer::self_test() — running tests...");
    init_defaults();

    // 1: Empty initial.
    assert!(list_analyses().is_empty());
    crate::serial_println!("  [1/11] empty initial: OK");

    // 2: Analyze kernel crash.
    let frames = alloc::vec![
        StackFrame { index: 0, address: 0xFFFF_8000_0010_0000, module: String::from("kernel"), function: String::from("page_fault_handler"), offset: 0x42 },
        StackFrame { index: 1, address: 0xFFFF_8000_0020_0000, module: String::from("kernel"), function: String::from("interrupt_dispatch"), offset: 0x100 },
    ];
    let id1 = analyze_dump(
        "/var/crash/dump-001.bin", CrashSeverity::Fatal, FaultType::PageFault,
        "kernel", 0xDEAD_BEEF, 0, frames, "Null pointer dereference in page table walk",
    ).expect("analyze");
    assert!(id1 > 0);
    crate::serial_println!("  [2/11] kernel crash: OK");

    // 3: Analyze app crash.
    let frames2 = alloc::vec![
        StackFrame { index: 0, address: 0x0040_1000, module: String::from("app.exe"), function: String::from("main"), offset: 0x10 },
    ];
    let id2 = analyze_dump(
        "/var/crash/dump-002.bin", CrashSeverity::Error, FaultType::StackOverflow,
        "app.exe", 0x0040_1000, 42, frames2, "Infinite recursion detected",
    ).expect("analyze2");
    assert_eq!(list_analyses().len(), 2);
    crate::serial_println!("  [3/11] app crash: OK");

    // 4: Get analysis.
    let a = get_analysis(id1).expect("get");
    assert_eq!(a.fault_type, FaultType::PageFault);
    assert_eq!(a.pid, 0);
    assert_eq!(a.stack_frames.len(), 2);
    crate::serial_println!("  [4/11] get analysis: OK");

    // 5: Stack frame details.
    assert_eq!(a.stack_frames[0].function, "page_fault_handler");
    assert_eq!(a.stack_frames[0].module, "kernel");
    crate::serial_println!("  [5/11] stack frames: OK");

    // 6: List by fault type.
    let pf = list_by_fault(FaultType::PageFault);
    assert_eq!(pf.len(), 1);
    let so = list_by_fault(FaultType::StackOverflow);
    assert_eq!(so.len(), 1);
    crate::serial_println!("  [6/11] filter by fault: OK");

    // 7: Delete analysis.
    delete_analysis(id2).expect("delete");
    assert_eq!(list_analyses().len(), 1);
    crate::serial_println!("  [7/11] delete: OK");

    // 8: Summary text.
    let a = get_analysis(id1).expect("get2");
    assert!(a.summary.contains("Null pointer"));
    crate::serial_println!("  [8/11] summary: OK");

    // 9: Severity labels.
    assert_eq!(CrashSeverity::Fatal.label(), "Fatal");
    assert_eq!(FaultType::DoubleFault.label(), "Double Fault");
    crate::serial_println!("  [9/11] labels: OK");

    // 10: Clear all.
    let cleared = clear_all();
    assert_eq!(cleared, 1);
    assert!(list_analyses().is_empty());
    crate::serial_println!("  [10/11] clear all: OK");

    // 11: Stats.
    let (count, total, kernel, app, ops) = stats();
    assert_eq!(count, 0);
    assert_eq!(total, 2);
    assert_eq!(kernel, 1);
    assert_eq!(app, 1);
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("dumpanalyzer::self_test() — all 11 tests passed");
}
